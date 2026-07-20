//! Arbitrary source text and arbitrary edit sets through
//! `celerrate_edit::apply`. Invariants: `apply` never panics, an `Ok`
//! result never contains a silently resolved overlap, and the patched
//! text agrees with one-at-a-time back-to-front splicing.

#![no_main]

use arbitrary::Arbitrary;
use celerrate_source::{FileId, TextEdit, TextRange, TextSize};
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct Input {
    source: String,
    edits: Vec<(u16, u16, String)>,
}

fuzz_target!(|input: Input| {
    let edits: Vec<TextEdit> = input
        .edits
        .iter()
        .map(|(first, second, replacement)| {
            // `TextRange::new` requires start <= end; arbitrary pairs
            // are normalized, everything else is apply's problem.
            let low = u32::from(*first.min(second));
            let high = u32::from(*first.max(second));
            TextEdit {
                file: FileId::new(0),
                range: TextRange::new(TextSize::from(low), TextSize::from(high)),
                replacement: replacement.clone(),
            }
        })
        .collect();
    let Ok(patched) = celerrate_edit::apply(&input.source, &edits) else {
        // Refusal is always a legal outcome; the properties constrain
        // what `apply` accepts, not what it rejects.
        return;
    };
    let mut sorted = edits;
    sorted.sort();
    for pair in sorted.windows(2) {
        if let [first, second] = pair {
            assert!(
                first.range.end() <= second.range.start(),
                "apply silently resolved an overlap: {first:?} / {second:?}",
            );
        }
    }
    let mut expected = input.source.clone();
    for edit in sorted.iter().rev() {
        let start = usize::from(edit.range.start());
        let end = usize::from(edit.range.end());
        let (Some(head), Some(tail)) = (expected.get(..start), expected.get(end..)) else {
            panic!("apply accepted an edit its oracle cannot splice: {edit:?}");
        };
        expected = format!("{head}{}{tail}", edit.replacement);
    }
    assert_eq!(
        patched, expected,
        "apply disagrees with one-at-a-time splicing",
    );
});
