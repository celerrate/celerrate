//! Arbitrary bytes through `SourceText::from_bytes` then the lexer.
//! Invariants: no panic anywhere, the token stream is lossless, and
//! lexing terminates (libFuzzer's timeout catches hangs).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = celerrate_source::SourceText::from_bytes(data) else {
        return;
    };
    let (tokens, _diagnostics) = celerrate_syntax::lex(source.text());
    let total: usize = tokens
        .iter()
        .map(|token| usize::from(token.length))
        .sum();
    assert_eq!(total, source.text().len(), "the token stream must be lossless");
});
