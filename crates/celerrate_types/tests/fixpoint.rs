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
use celerrate_semantics::{AstId, BodyQuery};
use celerrate_source::FileId;
use celerrate_stdlib_provider::StdlibProvider;
use celerrate_stubs::{StubIndex, StubIndexInput};
use celerrate_types::{
    DynamicTypeProviderRegistration, DynamicTypeProviderRegistry, FunctionQuery, MethodQuery,
    inferred_body_types, inferred_function_return, inferred_method_return, typed_file_verdicts,
};

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
    // Deliberately empty (issue #36's fixed decision 3): this suite pins
    // fixpoint budgets, where a stub surface adds resolution noise
    // without observing stub behaviour, and a separate compilation unit
    // cannot reach `pub(crate)` test support anyway.
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

/// The display of one method's inferred return, resolved through
/// `inferred_method_return` — the method-cycle analog of [`return_of`]
/// (task 6's second cycle-recovered query, extended here to the same
/// determinism harness the free-function fixpoint already pins).
fn method_return_display(fixture: &Fixture, class_key: &str, method_name: &str) -> String {
    inferred_method_return(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        MethodQuery::new(&fixture.db, class_key.to_owned(), method_name.to_owned()),
    )
    .display(&fixture.db)
}

/// The fixture with the real embedded stub blob (so provider-claimed
/// functions like `json_decode` and `preg_match` resolve against a
/// real declared signature) plus `StdlibProvider` registered through
/// `DynamicTypeProviderRegistry` (task 6's registration idiom,
/// `tests/by_reference.rs`'s own `fixture`, duplicated here: no
/// shared test-support module spans this crate's integration-test
/// binaries — `invalidation_scope.rs`'s own `executions_of` doc notes
/// the same constraint). Task 12's determinism and invalidation pins
/// need the real provider wired up, not the empty stub index the
/// fixpoint suite otherwise uses to keep resolution noise out.
fn fixture_with_embedded_stubs_and_stdlib_provider(sources: &[&str]) -> Fixture {
    let db = TestDatabase::default();
    let handles: Vec<SourceFile> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let files = AnalyzedFileSet::new(&db, handles.clone());
    let stubs = StubIndexInput::builder(celerrate_stubs::embedded_stub_index().unwrap())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    let _ = DynamicTypeProviderRegistry::builder(vec![DynamicTypeProviderRegistration {
        identity: celerrate_stdlib_provider::descriptor().identity,
        provider: Arc::new(StdlibProvider::new()),
    }])
    .durability(salsa::Durability::HIGH)
    .new(&db);
    Fixture {
        db,
        files,
        stubs,
        configuration,
        handles,
    }
}

/// The body of the declaration numbered `index` in file 0 (mirrors
/// `inference.rs`'s own private `body_query` test helper, unreachable
/// from this external integration-test binary).
fn body_query(fixture: &Fixture, index: u32) -> BodyQuery<'_> {
    BodyQuery::new(
        &fixture.db,
        AstId {
            file: FileId::new(0),
            index,
        },
    )
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

/// The method-cycle counterpart of
/// `every_entry_point_converges_to_the_same_fixpoint` (design section
/// 10, harness 3, extended to method cycles per decision 15): the same
/// two-class mutual-recursion cluster, queried in two orders -- ping
/// then pong, and pong then ping, both method-first -- over fresh
/// databases answers identically regardless of entry order.
/// Entry-point independence for the method fixpoint is already pinned
/// at the unit level (task 6's
/// `a_mutual_method_cluster_converges_the_same_from_either_entry_point`,
/// a single-class cluster asking `left`/`right` on the same class);
/// this fixture is a genuine two-class cluster. The comparison is
/// pairwise by key, mirroring `every_entry_point_converges_to_the_same_fixpoint`
/// above, not sorted: a sorted comparison would still pass under an
/// entry-order-dependent swap between `ping` and `pong`, since the two
/// orders would then carry the same multiset of values, only relabeled.
#[test]
fn a_method_cycle_answers_identically_from_every_entry_point() {
    // The same two-class mutual-recursion cluster, queried in two
    // orders -- ping then pong, and pong then ping, both method-first
    // -- over fresh databases: identical answers regardless of order.
    let source = r#"<?php
namespace App;
class Left {
    public function ping(Right $right, bool $stop) {
        if ($stop) { return 1; }
        return $right->pong($this, $stop);
    }
}
class Right {
    public function pong(Left $left, bool $stop) {
        if ($stop) { return 'one'; }
        return $left->ping($this, $stop);
    }
}
"#;
    let first = fixture(&[source]);
    let first_ping = method_return_display(&first, "app\\left", "ping");
    let first_pong = method_return_display(&first, "app\\right", "pong");
    let second = fixture(&[source]);
    let second_pong = method_return_display(&second, "app\\right", "pong");
    let second_ping = method_return_display(&second, "app\\left", "ping");
    assert_eq!(first_ping, second_ping);
    assert_eq!(first_pong, second_pong);
    assert_eq!(first_ping, first_pong, "the cluster shares one fixpoint");
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

/// The method-cycle counterpart of `thread_fan_out_answers_identically`
/// (design section 10, harness 3, extended to method cycles per
/// decision 15): the same two-class mutual-recursion cluster, warmed
/// once, answers identically to every one of several threads fanning
/// out over snapshots of the warmed database.
#[test]
fn thread_fan_out_answers_identically_for_a_method_cluster() {
    let source = r#"<?php
namespace App;
class Left {
    public function ping(Right $right, bool $stop) {
        if ($stop) { return 1; }
        return $right->pong($this, $stop);
    }
}
class Right {
    public function pong(Left $left, bool $stop) {
        if ($stop) { return 'one'; }
        return $left->ping($this, $stop);
    }
}
"#;
    let fixture = fixture(&[source]);
    // Warm the fixpoint once, then fan out over snapshots.
    let expected = (
        method_return_display(&fixture, "app\\left", "ping"),
        method_return_display(&fixture, "app\\right", "pong"),
    );
    let results: Vec<(String, String)> = std::thread::scope(|scope| {
        (0..4)
            .map(|_| {
                let db = fixture.db.clone();
                let files = fixture.files;
                let stubs = fixture.stubs;
                let configuration = fixture.configuration;
                scope.spawn(move || {
                    let of = |class: &str, method: &str| {
                        inferred_method_return(
                            &db,
                            files,
                            stubs,
                            configuration,
                            MethodQuery::new(&db, class.to_owned(), method.to_owned()),
                        )
                        .display(&db)
                    };
                    (of("app\\left", "ping"), of("app\\right", "pong"))
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
        _site: &celerrate_types::InvocationSite<'db, '_>,
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

/// The method-cycle counterpart of
/// `an_edit_mid_fixpoint_unwinds_cleanly_and_serves_no_provisional_value`
/// (design section 10, harness 3, extended to method cycles per
/// decision 15): identical scaffolding, only the queried cluster
/// changes — a self-recursive method reaching the same blocking
/// provider instead of a self-recursive function.
#[test]
fn an_edit_mid_method_fixpoint_unwinds_cleanly_and_serves_no_provisional_value() {
    let source = "<?php
    namespace App;
    class Entry {
        public function run(int $n) {
            if ($n > 0) { return $this->run($n - 1); }
            return block();
        }
    }";
    let edited = "<?php
    namespace App;
    class Entry {
        public function run(int $n) {
            if ($n > 0) { return $this->run($n - 1); }
            return block() ?? 'edited';
        }
    }";

    // Same rationale as the free-function version above: the fixture's
    // own handle is dropped before the edit lands, so the setter handle
    // reaches sole ownership and carries the post-edit demand.
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
    drop(db);

    let worker = std::thread::spawn(move || {
        salsa::Cancelled::catch(std::panic::AssertUnwindSafe(|| {
            inferred_method_return(
                &worker_db,
                files,
                stubs,
                configuration,
                MethodQuery::new(&worker_db, "app\\entry".to_owned(), "run".to_owned()),
            )
            .display(&worker_db)
        }))
    });

    // The worker is inside the provider, mid-fixpoint.
    entered.wait();
    let setter = std::thread::spawn(move || {
        use salsa::Setter as _;
        file.set_bytes(&mut setter_db)
            .to(edited.as_bytes().to_vec());
        setter_db
    });
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

    let unwound = worker.join().unwrap();
    assert!(unwound.is_err(), "the fixpoint unwound with Cancelled");
    let setter_db = setter.join().unwrap();

    // No provisional value: a fresh demand on the post-edit database
    // answers the post-edit fixpoint, byte-identical to a from-scratch
    // database that resolves `block()` through an equivalent claim.
    let after = inferred_method_return(
        &setter_db,
        files,
        stubs,
        configuration,
        MethodQuery::new(&setter_db, "app\\entry".to_owned(), "run".to_owned()),
    )
    .display(&setter_db);
    let fresh = fixture_with(&[edited]);
    assert_eq!(after, method_return_display(&fresh, "app\\entry", "run"));
}

/// A body exercising every provider channel this plan built: the
/// array-family return channel (`array_map`), the computation-dependent
/// return channel (`json_decode`), and the by-reference channel
/// (`preg_match`'s `$matches`).
const DETERMINISM_SOURCE: &str = r#"<?php
function consume(string $json, string $subject): void {
    $mapped = array_map(fn (int $n): string => (string) $n, [1, 2]);
    $decoded = json_decode($json, true);
    if (preg_match('/(?<year>\d+)/', $subject, $matches) === 1) {
        $inside = $matches;
    }
}
"#;

/// Task 12's determinism pin (decision 16): a body exercising every
/// provider channel this plan built types identically across two
/// fresh, unrelated databases. Interner handles may differ across
/// databases (each database owns its own `salsa` interner), so the
/// comparison renders through `display` rather than comparing `TypeId`
/// values directly — the same accommodation `return_of`/
/// `method_return_display` above already make.
#[test]
fn provider_answers_are_identical_across_fresh_databases() {
    // Interner handles may differ across databases; displays must
    // not.
    let render = || {
        let f = fixture_with_embedded_stubs_and_stdlib_provider(&[DETERMINISM_SOURCE]);
        let file = f.handles[0];
        let inferred = inferred_body_types(
            &f.db,
            f.files,
            f.stubs,
            f.configuration,
            file,
            body_query(&f, 0),
        )
        .as_ref()
        .unwrap();
        inferred
            .expression_types
            .iter()
            .map(|of| of.display(&f.db))
            .collect::<Vec<String>>()
    };
    assert_eq!(render(), render());
}

/// Task 12's fall-through pin (decision 16): every registered
/// `StdlibProvider` handler answers `None` for arguments that are
/// themselves `mixed` (`array_map`'s callback and subject, `current`'s
/// subject), so the declared tier's answer stands and no provider edge
/// is ever counted — the residual instrument's `provider_edges` field
/// (untested since task 10 introduced it, per its own doc comment)
/// stays honest at zero rather than over-counting a claim that never
/// actually contributed a type.
#[test]
fn a_claim_never_reached_leaves_the_body_on_the_declared_tier() {
    let f = fixture_with_embedded_stubs_and_stdlib_provider(&[r#"<?php
function consume(mixed $anything): void {
    $mapped = array_map($anything, $anything);
    $slice = current($anything);
}
"#]);
    let file = f.handles[0];
    let inferred = inferred_body_types(
        &f.db,
        f.files,
        f.stubs,
        f.configuration,
        file,
        body_query(&f, 0),
    )
    .as_ref()
    .unwrap();
    assert_eq!(inferred.edge_counts.provider_edges, 0);
}

/// Task 13's closing determinism pin: the typed checks layer over a
/// body exercising the unknown-method and nullability families
/// answers identically across two fresh, unrelated databases. The
/// rendering layer moved up into the rule framework's typed-body
/// phase, so this pin compares the verdict aggregate this crate still
/// owns, one layer down: the same determinism proof over range-free
/// records, which carry no interner handle at all (each database owns
/// its own `salsa` interner) and so compare directly across two
/// databases. The rendered form's own determinism is pinned by the
/// rules-side tests, and the thread-count byte-identity over the full
/// product is the corpus/equivalence harness's job (extended in task
/// 10); this pin is the single-database, single-thread baseline the
/// checks layer itself owns.
#[test]
fn typed_verdicts_are_identical_across_fresh_databases() {
    let compute = || {
        let f = fixture(&[r#"<?php
class A { public function shared(): void {} }
class B {}
function f(A|B $either, ?A $nullable): void {
    $either->nowhere();
    $nullable->shared();
}
"#]);
        typed_file_verdicts(&f.db, f.files, f.stubs, f.configuration, f.handles[0]).clone()
    };
    let first = compute();
    assert!(
        !first.verdicts.is_empty(),
        "the fixture must fire, or the comparison below proves nothing",
    );
    assert_eq!(first, compute());
}
