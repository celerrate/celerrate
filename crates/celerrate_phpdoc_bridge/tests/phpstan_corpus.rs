//! The pinned-reference coverage statement: every
//! `TypeParserTest` input from the pinned phpstan/phpdoc-parser gets
//! a parse verdict, pinned in a committed snapshot. The snapshot's
//! header is the published coverage number; the gate is regression
//! against the committed file, re-blessed deliberately with
//! `CELERRATE_BLESS=1`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fmt::Write as _;

use celerrate_phpdoc_bridge::parse_type_expression_text;

const CASES: &str = include_str!("phpstan_corpus/cases.txt");
const VERDICTS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/phpstan_corpus/verdicts.txt"
);

/// Undoes `xtask/src/phpdoc_corpus.rs`'s `escape` (kept in mirror by
/// the round-trip nature of the snapshot itself).
fn unescape(line: &str) -> String {
    let mut value = String::new();
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            value.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => value.push('\n'),
            Some('r') => value.push('\r'),
            Some('t') => value.push('\t'),
            Some('\\') => value.push('\\'),
            Some(other) => {
                value.push('\\');
                value.push(other);
            }
            None => value.push('\\'),
        }
    }
    value
}

fn escape(case: &str) -> String {
    case.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[test]
fn the_pinned_reference_coverage_statement_is_current() {
    let declared: usize = CASES
        .lines()
        .find_map(|line| line.strip_prefix("# cases = "))
        .expect("the case file carries its count header")
        .parse()
        .unwrap();
    // Only header lines are filtered: an empty line is a legitimate
    // case (the upstream corpus includes an empty-input case).
    let cases: Vec<String> = CASES
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(unescape)
        .collect();
    assert_eq!(
        cases.len(),
        declared,
        "the case list and its header disagree"
    );
    let mut parsed = 0usize;
    let mut body = String::new();
    for case in &cases {
        let verdict = if parse_type_expression_text(case).is_some() {
            parsed += 1;
            "ok"
        } else {
            "rejected"
        };
        let _ = writeln!(body, "{verdict}: {}", escape(case));
    }
    let percentage = parsed * 100 / cases.len().max(1);
    let rendered = format!(
        "# The pinned-reference coverage statement (type-engine design, section 5).\n\
         # {parsed} of {} TypeParserTest inputs parse ({percentage}%).\n\
         # The corpus deliberately includes invalid inputs (upstream\n\
         # expects a ParserException on them): they count as rejected.\n\
         {body}",
        cases.len(),
    );
    if std::env::var_os("CELERRATE_BLESS").is_some() {
        std::fs::write(VERDICTS_PATH, &rendered).unwrap();
    }
    let committed = std::fs::read_to_string(VERDICTS_PATH).unwrap();
    assert_eq!(
        committed, rendered,
        "coverage drifted: re-bless with CELERRATE_BLESS=1 and review the diff",
    );
    assert!(
        percentage >= 50,
        "under half the pinned reference parses ({percentage}%): investigate before shipping",
    );
}
