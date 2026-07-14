//! The incremental correctness harness, grown to resolution: edit
//! sequences replayed over one incremental database, with the item
//! tree, the numbering, the symbol table, and name resolution asserted
//! identical to a from-scratch analysis after every edit.

#![allow(clippy::unwrap_used)]

use celerrate_db::testing::{
    assert_incremental_consistency_with, assert_incremental_consistency_with_context,
};
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{
    AstId, BodyQuery, SymbolResolution, SymbolSources, SymbolSpace, UseTables, ast_id_map, body_ir,
    item_tree, resolve_name, semantic_diagnostics, source_symbol_table,
};
use celerrate_stubs::{StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind};

fn assert_semantic_consistency(initial: &[&[u8]], edits: &[(usize, &[u8])]) {
    assert_incremental_consistency_with(
        initial,
        edits,
        &|incremental, file, from_scratch, fresh_file, index| {
            assert_eq!(
                item_tree(incremental, file),
                item_tree(from_scratch, fresh_file),
                "item tree diverged for file {index}",
            );
            assert_eq!(
                ast_id_map(incremental, file),
                ast_id_map(from_scratch, fresh_file),
                "declaration numbering diverged for file {index}",
            );
        },
    );
}

#[test]
fn body_signature_and_namespace_edits_replay_consistently() {
    assert_semantic_consistency(
        &[b"<?php namespace App; use Lib\\Helper; class Service { public function run() { return 1; } }"],
        &[
            (0, b"<?php namespace App; use Lib\\Helper; class Service { public function run() { return 2; } }"),
            (0, b"<?php namespace App; use Lib\\Helper; class Renamed { public function run() { return 2; } }"),
            (0, b"<?php namespace Core; class Renamed extends Base {}"),
        ],
    );
}

#[test]
fn declaration_churn_replays_consistently() {
    assert_semantic_consistency(
        &[b"<?php function keep() {}"],
        &[
            (0, b"<?php function keep() {} function added() {}"),
            (
                0,
                b"<?php if (!function_exists('keep')) { function keep() {} }",
            ),
            (
                0,
                b"<?php use Foo\\{Bar, function baz}; const A = 1, B = 2;",
            ),
            (0, b"<?php"),
        ],
    );
}

#[test]
fn malformed_intermediate_states_replay_consistently() {
    // Mid-typing states: the boundary must stay consistent over
    // whatever the error-resilient parser recovers.
    assert_semantic_consistency(
        &[b"<?php class Complete {}"],
        &[
            (0, b"<?php class Broken {"),
            (0, b"<?php class Broken { use "),
            (0, b"<?php class Fixed { use Helper; }"),
        ],
    );
}

#[test]
fn multiple_files_replay_independently() {
    assert_semantic_consistency(
        &[
            b"<?php namespace A; class One {}",
            b"<?php namespace B; class Two {}",
        ],
        &[
            (
                0,
                b"<?php namespace A; class One { public function m() {} }",
            ),
            (1, b"<?php namespace B; class Two {} class Three {}"),
            (0, b"<?php namespace A;"),
        ],
    );
}

type ResolutionContext = (AnalyzedFileSet, StubIndexInput, ProjectConfiguration);

fn resolution_context(
    db: &celerrate_db::testing::TestDatabase,
    files: &[SourceFile],
) -> ResolutionContext {
    (
        AnalyzedFileSet::new(db, files.to_vec()),
        StubIndexInput::builder(StubIndex::from_symbols(vec![StubSymbol {
            name: "strlen".to_owned(),
            kind: StubSymbolKind::Function,
            availability: StubAvailability::ALWAYS,
        }]))
        .durability(salsa::Durability::HIGH)
        .new(db),
        ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(db),
    )
}

/// Every inheritance name of every file, resolved: the resolution
/// traffic a real check would produce, in deterministic order.
fn resolution_answers(
    db: &celerrate_db::testing::TestDatabase,
    context: &ResolutionContext,
) -> Vec<(String, Option<SymbolResolution>)> {
    let (files, stubs, configuration) = *context;
    let sources = SymbolSources {
        files,
        stubs,
        configuration,
    };
    let mut answers = Vec::new();
    for &file in files.files(db) {
        let tree = celerrate_semantics::item_tree(db, file);
        for declaration in &tree.declarations {
            let tables = UseTables::for_namespace(tree, &declaration.namespace);
            for name in declaration
                .extends
                .iter()
                .chain(&declaration.implements)
                .chain(&declaration.trait_uses)
            {
                answers.push((
                    name.clone(),
                    resolve_name(
                        db,
                        sources,
                        &declaration.namespace,
                        &tables,
                        name,
                        SymbolSpace::ClassLike,
                    ),
                ));
            }
        }
    }
    answers
}

#[test]
fn resolution_matches_a_from_scratch_analysis_after_every_edit() {
    assert_incremental_consistency_with_context(
        &[
            b"<?php namespace App; use Lib\\Helper; class Consumer extends Helper implements Contract {}",
            b"<?php namespace Lib; class Helper {}",
            b"<?php namespace App; interface Contract {}",
        ],
        &[
            // A body edit: nothing observable changes.
            (1, b"<?php namespace Lib; class Helper { public function noop() {} }"),
            // The referenced declaration disappears.
            (1, b"<?php namespace Lib;"),
            // It returns under a different spelling: class lookups are
            // case-insensitive, so it resolves again.
            (1, b"<?php namespace Lib; class HELPER {}"),
            // The import is re-aliased: the reference now misses it.
            (0, b"<?php namespace App; use Lib\\Helper as Aid; class Consumer extends Helper implements Contract {}"),
            // A new file-set-neutral edit: an unrelated declaration.
            (2, b"<?php namespace App; interface Contract {} interface Extra {}"),
        ],
        &resolution_context,
        &|incremental, context, from_scratch, fresh_context| {
            assert_eq!(
                source_symbol_table(incremental, context.0),
                source_symbol_table(from_scratch, fresh_context.0),
                "the symbol tables diverged",
            );
            assert_eq!(
                resolution_answers(incremental, context),
                resolution_answers(from_scratch, fresh_context),
                "the resolution answers diverged",
            );
        },
    );
}

#[test]
fn semantic_diagnostics_match_from_scratch_analysis() {
    let initial: &[&[u8]] = &[
        b"<?php namespace App; use Lib\\Helper; $x = new Helper(); missing();",
        b"<?php namespace Lib; class Helper {}",
    ];
    let edits: &[(usize, &[u8])] = &[
        // The reference becomes unknown: the declaration disappears.
        (1, b"<?php namespace Lib;"),
        // It comes back, and a gated construct appears in the consumer.
        (1, b"<?php namespace Lib; class Helper {}"),
        (
            0,
            b"<?php namespace App; use Lib\\Helper; $x = new Helper(); readonly class C {}",
        ),
        // Degenerate bytes stay consistent.
        (0, b"<?php class"),
    ];
    assert_incremental_consistency_with_context(
        initial,
        edits,
        &|db, files| {
            (
                AnalyzedFileSet::new(db, files.to_vec()),
                StubIndexInput::builder(StubIndex::default())
                    .durability(salsa::Durability::HIGH)
                    .new(db),
                ProjectConfiguration::builder(PhpVersionRange::new(
                    PhpVersion::new(8, 1),
                    PhpVersion::new(8, 5),
                ))
                .durability(salsa::Durability::MEDIUM)
                .new(db),
                files.to_vec(),
            )
        },
        &|incremental_db, incremental, scratch_db, scratch| {
            let (incremental_set, incremental_stubs, incremental_configuration, incremental_files) =
                incremental;
            let (scratch_set, scratch_stubs, scratch_configuration, scratch_files) = scratch;
            for (incremental_file, scratch_file) in
                incremental_files.iter().zip(scratch_files.iter())
            {
                assert_eq!(
                    semantic_diagnostics(
                        incremental_db,
                        *incremental_file,
                        *incremental_set,
                        *incremental_stubs,
                        *incremental_configuration,
                    ),
                    semantic_diagnostics(
                        scratch_db,
                        *scratch_file,
                        *scratch_set,
                        *scratch_stubs,
                        *scratch_configuration,
                    ),
                );
            }
        },
    );
}

/// Body IR consistency: every numbered declaration's lowering (bodied
/// or not) must be byte-identical to a from-scratch database's.
fn assert_body_consistency(initial: &[&[u8]], edits: &[(usize, &[u8])]) {
    assert_incremental_consistency_with(
        initial,
        edits,
        &|incremental, file, from_scratch, fresh_file, index| {
            let count = ast_id_map(incremental, file).len();
            assert_eq!(
                count,
                ast_id_map(from_scratch, fresh_file).len(),
                "numbering diverged for file {index}",
            );
            for declaration in 0..u32::try_from(count).unwrap() {
                let body = BodyQuery::new(
                    incremental,
                    AstId {
                        file: file.file_id(incremental),
                        index: declaration,
                    },
                );
                let fresh_body = BodyQuery::new(
                    from_scratch,
                    AstId {
                        file: fresh_file.file_id(from_scratch),
                        index: declaration,
                    },
                );
                assert_eq!(
                    body_ir(incremental, file, body),
                    body_ir(from_scratch, fresh_file, fresh_body),
                    "body IR diverged for file {index} declaration {declaration}",
                );
            }
        },
    );
}

#[test]
fn body_lowerings_replay_consistently() {
    assert_body_consistency(
        &[b"<?php class A { public function m() { return $this->x?->y(); } } function f() { $g = fn () => 1; }"],
        &[
            (0, b"<?php class A { public function m() { return $this->x?->y(); } } function f() { $g = fn () => 2; }"),
            (0, b"<?php class A { public function m() { /** @var Y $y */ return ($this->x?->y)(); } } function f() { $g = fn () => 2; }"),
            (0, b"<?php class B {} class A { public function m() { return new class { function n() { return 1; } }; } } function f() {}"),
            (0, b"<?php function f() { if (true) { foreach ([1, ...$r] as $k => &$v) { yield $k => $v; } } }"),
            (0, b"<?php function f() { match ($x) { 1, 2 => strlen(...), default => [, $b] = $p } ; }"),
            (0, b"<?php function f() { $x = "),
        ],
    );
}
