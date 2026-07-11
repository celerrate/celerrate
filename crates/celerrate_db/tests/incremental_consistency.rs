//! The incremental correctness harness, skeleton form: after any edit
//! sequence, the incremental result must be byte-for-byte identical to
//! a from-scratch analysis (parent spec, section 9, tier 3).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use celerrate_db::testing::assert_incremental_consistency;

#[test]
fn edit_sequences_match_from_scratch_analysis() {
    assert_incremental_consistency(
        &[b"<?php echo 1;", b"<?php function f() { return 2; }"],
        &[
            (0, b"<?php echo 10;"),
            (1, b"<?php function f() { return"),
            (0, b"<?php echo ;"),
            (1, b"<?php function f() { return 2; }"),
        ],
    );
}

#[test]
fn degenerate_bytes_stay_consistent() {
    assert_incremental_consistency(
        &[b"\xFF\xFE<?php echo \xFF;"],
        &[(0, b"<?php"), (0, b"\xEF\xBB\xBF<?php echo 1;")],
    );
}
