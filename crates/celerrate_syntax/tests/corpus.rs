//! Snapshot corpus: every `tests/corpus/*.php` file is lexed and
//! snapshotted as a `kind @ start..end "text"` listing plus diagnostics.
//! The lossless invariant is asserted on every file.
#![allow(clippy::expect_used, clippy::indexing_slicing)]

use std::fmt::Write as _;

use celerrate_syntax::lex;

fn render(source: &str) -> String {
    let (tokens, diagnostics) = lex(source);
    let mut output = String::new();
    let mut offset = 0usize;
    for token in &tokens {
        let end = offset + usize::from(token.length);
        let text = &source[offset..end];
        let _ = writeln!(output, "{:?} @ {offset}..{end} {text:?}", token.kind);
        offset = end;
    }
    assert_eq!(offset, source.len(), "the token stream must be lossless");
    if !diagnostics.is_empty() {
        let _ = writeln!(output, "---");
        for diagnostic in &diagnostics {
            let _ = writeln!(
                output,
                "{:?} @ {}..{}",
                diagnostic.kind,
                u32::from(diagnostic.range.start()),
                u32::from(diagnostic.range.end()),
            );
        }
    }
    output
}

#[test]
fn corpus() {
    insta::glob!("corpus/*.php", |path| {
        let source = std::fs::read_to_string(path).expect("corpus file is readable");
        insta::assert_snapshot!(render(&source));
    });
}
