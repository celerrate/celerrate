//! The incremental correctness harness, grown to the boundary: edit
//! sequences replayed over one incremental database, with the item
//! tree and the numbering asserted identical to a from-scratch
//! analysis after every edit.

#![allow(clippy::unwrap_used)]

use celerrate_db::testing::assert_incremental_consistency_with;
use celerrate_semantics::{ast_id_map, item_tree};

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
