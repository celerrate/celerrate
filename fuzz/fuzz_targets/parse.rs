//! Arbitrary bytes through `SourceText::from_bytes` then the full
//! parsing pipeline. Invariants: no panic anywhere, the tree is
//! lossless, and parsing terminates (libFuzzer's timeout catches
//! hangs — guaranteed progress is the property under test).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = celerrate_source::SourceText::from_bytes(data) else {
        return;
    };
    let parse = celerrate_syntax::parse(source.text());
    assert_eq!(
        parse.tree().text().to_string(),
        source.text(),
        "the tree must be lossless"
    );
});
