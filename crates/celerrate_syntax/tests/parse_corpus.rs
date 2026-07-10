//! Snapshot corpus for the parser: every `tests/parse_corpus/*.php`
//! file is parsed and snapshotted as an indented tree plus diagnostics.
//! The corpus grows with the grammar; each plan adds the files its
//! rules cover. The lossless invariant is asserted on every file.
#![allow(clippy::expect_used)]

mod support;

#[test]
fn parse_corpus() {
    insta::glob!("parse_corpus/*.php", |path| {
        let source = std::fs::read_to_string(path).expect("corpus file is readable");
        insta::assert_snapshot!(support::render_parse(&source));
    });
}
