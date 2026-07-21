//! Shared test-only harness for the core rule modules: the salsa
//! inputs a phase query reads, the core-rule registration the
//! composition root performs, and one drive helper per phase.
//!
//! Lifted here rather than copied into each rule module's test module:
//! every migrated family needs the same fixture, and a per-module copy
//! would re-derive the same forty lines once per family. The crate's
//! own precedent is `celerrate_types`' `checks::test_support` and
//! `inheritance::test_support`.
//!
//! The fixture is phase-agnostic on purpose, so a later family's drive
//! helper (the typed-body phase) sits beside the semantic ones and
//! reads the very same inputs.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_diagnostics::Diagnostic;
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::PluginIdentity;
use celerrate_source::FileId;
use celerrate_stubs::{StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind};

use crate::metadata::Tier;
use crate::phases::{semantic_phase_diagnostics, typed_body_phase_diagnostics};
use crate::registry::{RuleRegistration, RuleRegistry};

/// A stub symbol available in every PHP version.
pub(crate) fn stub(name: &str, kind: StubSymbolKind) -> StubSymbol {
    stub_with(name, kind, StubAvailability::ALWAYS)
}

/// A stub symbol carrying an explicit availability window, for the
/// tests that exercise the version-gating stances.
pub(crate) fn stub_with(
    name: &str,
    kind: StubSymbolKind,
    availability: StubAvailability,
) -> StubSymbol {
    StubSymbol {
        name: name.to_owned(),
        kind,
        availability,
    }
}

/// The supported range every default-range drive helper uses: the
/// whole of the currently supported PHP versions, so nothing is gated
/// unless a test asks for it.
pub(crate) fn default_range() -> PhpVersionRange {
    PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5))
}

/// The salsa inputs of one drive: the analyzed file set, the compiled
/// stub surface, and the project configuration, with the first source's
/// handle singled out as the checked file.
pub(crate) struct Fixture {
    pub(crate) db: TestDatabase,
    pub(crate) file: SourceFile,
    pub(crate) files: AnalyzedFileSet,
    pub(crate) stubs: StubIndexInput,
    pub(crate) configuration: ProjectConfiguration,
}

/// A fixture whose registry is populated from [`crate::rules::core_rules`]
/// under the reserved core identity, exactly as the composition root's
/// `register_core_rules` composes it: a drive through this fixture
/// exercises the framework path the CLI serves.
pub(crate) fn registered_fixture(
    sources: &[&str],
    stub_symbols: Vec<StubSymbol>,
    range: PhpVersionRange,
) -> Fixture {
    let db = TestDatabase::default();
    let handles: Vec<SourceFile> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let file = *handles.first().unwrap();
    let files = AnalyzedFileSet::new(&db, handles);
    let stubs = StubIndexInput::builder(StubIndex::from_symbols(stub_symbols))
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(range)
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
    let identity = PluginIdentity {
        name: crate::CORE_IDENTITY_NAME.to_owned(),
        version: "test".to_owned(),
        configuration: String::new(),
    };
    let registrations = crate::rules::core_rules()
        .into_iter()
        .map(|(metadata, implementation)| RuleRegistration {
            identity: identity.clone(),
            active: metadata.tier == Tier::Default,
            metadata,
            implementation,
        })
        .collect();
    let _ = RuleRegistry::builder(registrations)
        .durability(salsa::Durability::HIGH)
        .new(&db);
    Fixture {
        db,
        file,
        files,
        stubs,
        configuration,
    }
}

/// The semantic phase drained for the FIRST source, with the given
/// stubs and the given supported range.
pub(crate) fn semantic_diagnostics_in_range(
    sources: &[&str],
    stub_symbols: Vec<StubSymbol>,
    range: PhpVersionRange,
) -> Vec<Diagnostic> {
    let fixture = registered_fixture(sources, stub_symbols, range);
    semantic_phase_diagnostics(
        &fixture.db,
        fixture.file,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
    )
    .clone()
}

/// The semantic phase drained over [`default_range`], for the tests
/// that do not exercise version gating.
pub(crate) fn semantic_diagnostics(
    sources: &[&str],
    stub_symbols: Vec<StubSymbol>,
) -> Vec<Diagnostic> {
    semantic_diagnostics_in_range(sources, stub_symbols, default_range())
}

/// The typed-body phase drained for a fixture's checked file.
///
/// Split from [`typed_body_diagnostics`] so a caller can keep the
/// fixture it drove: a test that drives another query over the same
/// database, or reads the fixture's own handles, cannot do so through a
/// helper that builds its fixture and drops it.
pub(crate) fn typed_body_diagnostics_of(fixture: &Fixture) -> Vec<Diagnostic> {
    typed_body_phase_diagnostics(
        &fixture.db,
        fixture.file,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
    )
    .clone()
}

/// The typed-body phase drained for the FIRST source, over
/// [`default_range`] and the empty stub surface.
///
/// No stub argument, unlike the semantic helpers: every typed-family
/// fixture declares its own classes and functions, so the argument
/// would be `vec![]` at every call site. A test that does need a stub
/// composes [`registered_fixture`] with [`typed_body_diagnostics_of`]
/// instead.
pub(crate) fn typed_body_diagnostics(sources: &[&str]) -> Vec<Diagnostic> {
    typed_body_diagnostics_of(&registered_fixture(sources, vec![], default_range()))
}
