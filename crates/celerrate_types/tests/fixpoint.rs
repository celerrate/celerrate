//! Fixpoint determinism fixtures (design section 10, harness 3): the
//! same mutual-recursion cluster queried from every entry point and
//! across thread counts answers identically, and an edit landing
//! mid-fixpoint unwinds cleanly with no provisional value served.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_source::FileId;
use celerrate_stubs::{StubIndex, StubIndexInput};
use celerrate_types::{FunctionQuery, inferred_function_return};

struct Fixture {
    db: TestDatabase,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    handles: Vec<SourceFile>,
}

fn fixture(sources: &[&str]) -> Fixture {
    let db = TestDatabase::default();
    let handles: Vec<SourceFile> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let files = AnalyzedFileSet::new(&db, handles.clone());
    let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    Fixture {
        db,
        files,
        stubs,
        configuration,
        handles,
    }
}

/// The fixture with a non-blocking `BlockingProvider` registered for
/// `block`: barriers of one party never block, so a fresh database
/// resolves `block()` through the same provider claim the cancellation
/// fixture registers — keeping the post-edit comparison byte-identical.
fn fixture_with(sources: &[&str]) -> Fixture {
    let fixture = fixture(sources);
    let _ = celerrate_types::DynamicTypeProviderRegistry::builder(vec![
        celerrate_types::DynamicTypeProviderRegistration {
            identity: celerrate_semantics::PluginIdentity {
                name: "blocking".to_owned(),
                version: "0.0.0".to_owned(),
                configuration: String::new(),
            },
            provider: Arc::new(BlockingProvider::inert()),
        },
    ])
    .durability(salsa::Durability::HIGH)
    .new(&fixture.db);
    fixture
}

fn return_of(fixture: &Fixture, key: &str) -> String {
    inferred_function_return(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        FunctionQuery::new(&fixture.db, key.to_owned()),
    )
    .display(&fixture.db)
}

const MUTUAL: &str = "<?php
function a(bool $c) { if ($c) { return b($c); } return 1; }
function b(bool $c) { if ($c) { return a($c); } return 'x'; }";

#[test]
fn direct_recursion_converges() {
    let fixture = fixture(&[
        "<?php function down(int $n) { if ($n > 0) { return down($n - 1); } return 0; }",
    ]);
    assert_eq!(return_of(&fixture, "down"), "0");
}

#[test]
fn baseless_mutual_recursion_is_never() {
    let fixture = fixture(&["<?php function a() { return b(); } function b() { return a(); }"]);
    assert_eq!(return_of(&fixture, "a"), "never");
    assert_eq!(return_of(&fixture, "b"), "never");
}

#[test]
fn every_entry_point_converges_to_the_same_fixpoint() {
    // Entry a-then-b.
    let first = fixture(&[MUTUAL]);
    let a_first = (return_of(&first, "a"), return_of(&first, "b"));
    // Entry b-then-a, a fresh database.
    let second = fixture(&[MUTUAL]);
    let b_first_b = return_of(&second, "b");
    let b_first_a = return_of(&second, "a");
    assert_eq!(a_first.0, b_first_a);
    assert_eq!(a_first.1, b_first_b);
    assert_eq!(a_first.0, a_first.1, "the cluster shares one fixpoint");
}

#[test]
fn thread_fan_out_answers_identically() {
    let fixture = fixture(&[MUTUAL]);
    // Warm the fixpoint once, then fan out over snapshots.
    let expected = (return_of(&fixture, "a"), return_of(&fixture, "b"));
    let results: Vec<(String, String)> = std::thread::scope(|scope| {
        (0..4)
            .map(|_| {
                let db = fixture.db.clone();
                let files = fixture.files;
                let stubs = fixture.stubs;
                let configuration = fixture.configuration;
                scope.spawn(move || {
                    let of = |key: &str| {
                        inferred_function_return(
                            &db,
                            files,
                            stubs,
                            configuration,
                            FunctionQuery::new(&db, key.to_owned()),
                        )
                        .display(&db)
                    };
                    (of("a"), of("b"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    for result in results {
        assert_eq!(result, expected);
    }
}

#[test]
fn a_growing_recursion_terminates_deterministically() {
    let source = "<?php function grow() { return [grow()]; }";
    let first = fixture(&[source]);
    let second = fixture(&[source]);
    // The caps and the budget guarantee termination; determinism is
    // the contract, the exact widened form is not.
    assert_eq!(return_of(&first, "grow"), return_of(&second, "grow"));
}

/// A provider that rendezvouses with the test exactly once: the first
/// invocation signals the test may write (`entered`) then waits until
/// the write is pending (`released`); every later invocation returns
/// immediately. The one-shot arming is load-bearing — the fixpoint
/// re-walks the body once per iteration, so `block()` is called more
/// than once, and a barrier that blocked on every call would deadlock
/// the second time (the test drives each barrier a single time). After
/// the rendezvous the worker keeps iterating, and the next salsa fetch
/// observes the cancellation flag and unwinds. Its value contribution
/// is deterministic (always `None`).
#[derive(Debug)]
struct BlockingProvider {
    armed: AtomicBool,
    entered: Arc<Barrier>,
    released: Arc<Barrier>,
}

impl BlockingProvider {
    fn armed(entered: Arc<Barrier>, released: Arc<Barrier>) -> Self {
        Self {
            armed: AtomicBool::new(true),
            entered,
            released,
        }
    }

    /// A provider that never blocks: barriers of one party trip
    /// immediately, and the arming flag starts disarmed. Used by the
    /// from-scratch comparison database, which resolves `block()`
    /// through the same claim without any rendezvous.
    fn inert() -> Self {
        Self {
            armed: AtomicBool::new(false),
            entered: Arc::new(Barrier::new(1)),
            released: Arc::new(Barrier::new(1)),
        }
    }
}

impl celerrate_types::DynamicTypeProvider for BlockingProvider {
    fn claims(&self) -> Vec<celerrate_types::SymbolClaim> {
        vec![celerrate_types::SymbolClaim::Function {
            key: "block".to_owned(),
        }]
    }

    fn return_type<'db>(
        &self,
        _db: &'db dyn salsa::Database,
        _invocation: &celerrate_types::Invocation<'db>,
    ) -> Option<celerrate_types::TypeId<'db>> {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.entered.wait();
            self.released.wait();
        }
        None
    }
}

#[test]
fn an_edit_mid_fixpoint_unwinds_cleanly_and_serves_no_provisional_value() {
    let source = "<?php
    function entry(int $n) { if ($n > 0) { return entry($n - 1); } return block(); }";
    let edited = "<?php
    function entry(int $n) { if ($n > 0) { return entry($n - 1); } return block() ?? 'edited'; }";

    // The fixture owns the sole database handle; take its parts apart so
    // the handle itself can be dropped before the edit lands. Editing an
    // input in salsa 0.27 bumps the revision through `cancel_others`
    // (`salsa-0.27.2/src/storage.rs`): it raises the cancellation flag
    // immediately, then blocks until it is the *sole* owner — every other
    // database clone must be dropped, not merely idle. The setter handle
    // is the one that survives, so it carries the post-edit demand.
    let Fixture {
        db,
        files,
        stubs,
        configuration,
        handles,
    } = fixture(&[source]);
    let file = handles[0];

    let entered = Arc::new(Barrier::new(2));
    let released = Arc::new(Barrier::new(2));
    let _ = celerrate_types::DynamicTypeProviderRegistry::builder(vec![
        celerrate_types::DynamicTypeProviderRegistration {
            identity: celerrate_semantics::PluginIdentity {
                name: "blocking".to_owned(),
                version: "0.0.0".to_owned(),
                configuration: String::new(),
            },
            provider: Arc::new(BlockingProvider::armed(entered.clone(), released.clone())),
        },
    ])
    .durability(salsa::Durability::HIGH)
    .new(&db);

    let worker_db = db.clone();
    let probe_db = db.clone();
    let mut setter_db = db.clone();
    // Drop the fixture's own handle now: only the worker, probe, and
    // setter handles remain, and all but the setter will be released
    // before the edit bumps the revision.
    drop(db);

    let worker = std::thread::spawn(move || {
        salsa::Cancelled::catch(std::panic::AssertUnwindSafe(|| {
            inferred_function_return(
                &worker_db,
                files,
                stubs,
                configuration,
                FunctionQuery::new(&worker_db, "entry".to_owned()),
            )
            .display(&worker_db)
        }))
    });

    // The worker is inside the provider, mid-fixpoint.
    entered.wait();
    // The pending edit cancels every in-flight snapshot: `set_bytes`
    // raises the flag at once, then blocks until it is the sole owner.
    // It hands the surviving handle back for the post-edit demand.
    let setter = std::thread::spawn(move || {
        use salsa::Setter as _;
        file.set_bytes(&mut setter_db)
            .to(edited.as_bytes().to_vec());
        setter_db
    });
    // Confirm the cancellation flag is set by catching it ourselves, then
    // drop the probe handle (so the setter can reach sole ownership) and
    // release the provider so the worker can observe the cancellation.
    loop {
        let probed = salsa::Cancelled::catch(std::panic::AssertUnwindSafe(|| {
            let _ = celerrate_db::parse(&probe_db, file);
        }));
        if probed.is_err() {
            break;
        }
        std::thread::yield_now();
    }
    drop(probe_db);
    released.wait();

    // The worker's next fetch after the provider returns observes the
    // flag and unwinds; joining it drops the worker handle, leaving the
    // setter as the sole owner so its edit lands.
    let unwound = worker.join().unwrap();
    assert!(unwound.is_err(), "the fixpoint unwound with Cancelled");
    let setter_db = setter.join().unwrap();

    // No provisional value: a fresh demand on the post-edit database
    // answers the post-edit fixpoint, byte-identical to a from-scratch
    // database that resolves `block()` through an equivalent claim.
    let after = inferred_function_return(
        &setter_db,
        files,
        stubs,
        configuration,
        FunctionQuery::new(&setter_db, "entry".to_owned()),
    )
    .display(&setter_db);
    let fresh = fixture_with(&[edited]);
    assert_eq!(after, return_of(&fresh, "entry"));
}
