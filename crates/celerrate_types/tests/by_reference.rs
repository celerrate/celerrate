//! Task 9's by-reference channel, end to end: `preg_match`'s declared
//! signature (the real embedded phpstorm-stubs blob) gives `$matches`
//! the general write-back first; `StdlibProvider`'s
//! `by_reference_types` then overrides it with the pattern-derived,
//! all-optional shape.
//!
//! An integration test file, not a `#[cfg(test)]` module inside this
//! crate: see `Cargo.toml`'s comment on the `celerrate_stdlib_provider`
//! dev-dependency for why embedding it in `src/inference.rs` does not
//! compile. This file links the plain compiled rlib of this crate, the
//! same way `tests/fixpoint.rs` and
//! `celerrate_stdlib_provider/tests/end_to_end.rs` already do.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{AstId, BodyQuery};
use celerrate_source::FileId;
use celerrate_stdlib_provider::StdlibProvider;
use celerrate_stubs::StubIndexInput;
use celerrate_types::{
    DynamicTypeProviderRegistration, DynamicTypeProviderRegistry, FunctionQuery,
    inferred_body_types, inferred_function_return,
};

struct Fixture {
    db: TestDatabase,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    handles: Vec<SourceFile>,
}

/// The real embedded stub blob (so `preg_match` exists and its
/// `$matches` parameter is declared by-reference) plus `StdlibProvider`
/// registered through `DynamicTypeProviderRegistry`, mirroring
/// `tests/fixpoint.rs`'s `fixture_with` idiom.
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

/// The provider's by-reference contribution overrides the declared
/// write-back at the real call boundary: the display shows the
/// pattern-derived, all-optional shape (`year` present, no plain
/// `array<...>` write-back left over from the declared signature).
#[test]
fn preg_match_refines_matches_through_the_by_reference_channel() {
    let f = fixture(&[r#"<?php
function consume(string $subject) {
    if (preg_match('/(?<year>\d+)/', $subject, $matches) === 1) {
        return $matches;
    }
    return null;
}
"#]);
    let inferred = inferred_function_return(
        &f.db,
        f.files,
        f.stubs,
        f.configuration,
        FunctionQuery::new(&f.db, "consume".to_owned()),
    );
    let display = inferred.display(&f.db);
    assert!(display.contains("year"), "{display}");
    assert!(!display.contains("array<"), "{display}");
}

/// A spread argument stops the by-reference application
/// (`apply_provider_by_reference` mirrors `apply_by_reference`'s spread
/// rule): the contribution's index has no positional argument to land
/// on, so binding is skipped rather than panicking. The load-bearing
/// assertion is completion without a panic; task 10 re-covers the same
/// body with a recording assertion once `stub_calls` exists.
#[test]
fn a_spread_argument_stops_the_by_reference_application() {
    let f = fixture(&[r#"<?php
function consume(array $arguments): void {
    preg_match(...$arguments);
}
"#]);
    let file = *f.handles.first().unwrap();
    let body = BodyQuery::new(
        &f.db,
        AstId {
            file: FileId::new(0),
            index: 0,
        },
    );
    let inferred = inferred_body_types(&f.db, f.files, f.stubs, f.configuration, file, body)
        .as_ref()
        .unwrap();
    assert!(!inferred.expression_types.is_empty());
}

/// A spread landing exactly at the `$matches` position, with a
/// contribution the provider DID compute (unlike the previous test,
/// where `preg_match_matches` bails before any contribution exists):
/// `apply_provider_by_reference`'s guard —
/// `arguments.iter().take(*index + 1).any(|argument| argument.spread)`
/// — must skip the bind rather than mis-attributing the pattern-derived
/// shape to `$rest`, an array of trailing arguments that has nothing to
/// do with `$matches`. Without the guard this would bind the shape onto
/// `$rest` and the assertion below would fail.
#[test]
fn a_spread_at_the_matches_position_is_not_mistaken_for_matches() {
    let f = fixture(&[r#"<?php
function consume(string $subject, array $rest) {
    preg_match('/(?<year>\d+)/', $subject, ...$rest);
    return $rest;
}
"#]);
    let inferred = inferred_function_return(
        &f.db,
        f.files,
        f.stubs,
        f.configuration,
        FunctionQuery::new(&f.db, "consume".to_owned()),
    );
    let display = inferred.display(&f.db);
    // `$rest` keeps its declared `array` type: the pattern-derived
    // shape (which would show `year`) was never bound onto it.
    assert!(!display.contains("year"), "{display}");
}
