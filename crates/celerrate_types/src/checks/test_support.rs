//! Shared test-only fixture for the checks family: the same
//! `Fixture`/`fixture`/`handle_of` idiom task 2's `mod.rs` test module
//! built, lifted here so `receivers.rs` (and the task 4-6 walkers)
//! share one builder rather than each re-deriving it — the crate's
//! existing `test_support`/`minimal_stub_index` pattern
//! (`inheritance/test_support.rs`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{AstId, BodyQuery, UseTables, body_ir, item_tree};
use celerrate_source::FileId;
use celerrate_stubs::{StubIndex, StubIndexInput};

use super::CheckContext;
use crate::inference::{BodyOwner, InferenceContext, body_owner, inferred_body_types};

pub(crate) struct Fixture {
    pub(crate) db: TestDatabase,
    pub(crate) handles: Vec<SourceFile>,
    pub(crate) files: AnalyzedFileSet,
    pub(crate) stubs: StubIndexInput,
    pub(crate) configuration: ProjectConfiguration,
}

/// The default fixture: the crate's minimal realistic stub surface
/// (`inheritance::test_support::minimal_stub_index`), the same default
/// every other module's fixture carries.
pub(crate) fn fixture(sources: &[&str]) -> Fixture {
    fixture_with_stubs(
        sources,
        crate::inheritance::test_support::minimal_stub_index(),
    )
}

/// A fixture over a caller-supplied stub index, for the synthetic stub
/// surfaces a test needs (decision 7's `UnitEnum`/`BackedEnum`, a
/// single named class) that the default minimal index does not carry.
pub(crate) fn fixture_with_stubs(sources: &[&str], stub_index: StubIndex) -> Fixture {
    let db = TestDatabase::default();
    let handles: Vec<SourceFile> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let files = AnalyzedFileSet::new(&db, handles.clone());
    let stubs = StubIndexInput::builder(stub_index)
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
        handles,
        files,
        stubs,
        configuration,
    }
}

pub(crate) fn handle_of(fixture: &Fixture, index: usize) -> SourceFile {
    fixture.handles[index]
}

/// A `CheckContext` for the declaration numbered `body_index` in file
/// 0, built the same way `body_typed_verdicts` builds one. Every test
/// fixture that calls this is built to have a body there, so a missing
/// IR or inferred body is an unconditional test-only panic, not a
/// production path.
pub(crate) fn context_for(fixture: &Fixture, body_index: u32) -> CheckContext<'_, '_> {
    let file = handle_of(fixture, 0);
    let db = &fixture.db;
    let body = BodyQuery::new(
        db,
        AstId {
            file: FileId::new(0),
            index: body_index,
        },
    );
    let ir = body_ir(db, file, body)
        .as_ref()
        .expect("the fixture body has an IR");
    let inferred = inferred_body_types(
        db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        file,
        body,
        InferenceContext::new(db, None),
    )
    .as_ref()
    .expect("the fixture body types");
    let owner = body_owner(db, file, body).as_ref();
    let namespace = match owner {
        Some(BodyOwner::Function(function)) => function.namespace.clone(),
        Some(BodyOwner::Method { namespace, .. }) => namespace.clone(),
        None => String::new(),
    };
    CheckContext {
        db,
        files: fixture.files,
        stubs: fixture.stubs,
        configuration: fixture.configuration,
        file,
        body: body.ast_id(db),
        ir,
        inferred,
        owner,
        tables: UseTables::for_namespace(item_tree(db, file), &namespace),
        namespace,
    }
}
