//! The provider's end-to-end proof: a real inference fixture with
//! `StdlibProvider` registered through `DynamicTypeProviderRegistry`
//! (mirroring `celerrate_types`' `tests/fixpoint.rs` registry-building
//! idiom), demonstrating that the answer flows through the actual call
//! boundary — `inferred_body_types`'s provider-return edge — and not
//! merely through the handler's own unit shape.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{AstId, BodyQuery};
use celerrate_source::FileId;
use celerrate_stdlib_provider::StdlibProvider;
use celerrate_stubs::{StubIndex, StubIndexInput};
use celerrate_types::{
    DynamicTypeProviderRegistration, DynamicTypeProviderRegistry, inferred_body_types,
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
    // Deliberately empty, mirroring `fixpoint.rs`'s fixture: this
    // suite exercises the provider seam, not stub resolution.
    let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
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

#[test]
fn current_over_a_call_site_array_literal_resolves_through_the_provider_edge() {
    let fixture = fixture(&["<?php function f() { return current([1, 'a']); }"]);
    let file = fixture.handles.first().unwrap();
    let body = BodyQuery::new(
        &fixture.db,
        AstId {
            file: FileId::new(0),
            index: 0,
        },
    );
    let inferred = inferred_body_types(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        *file,
        body,
    )
    .as_ref()
    .unwrap();
    // Canonical union order is structural rank, not argument order
    // (`ordering.rs`: `Bool` rank 3, `Int` rank 4, `String` rank 6),
    // so the display is `false|1|'a'`, not `1|'a'|false` as an
    // argument-order reading might suggest.
    assert_eq!(inferred.return_type.display(&fixture.db), "false|1|'a'");
    assert_eq!(inferred.edge_counts.provider_edges, 1);
}
