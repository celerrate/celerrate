//! `preg_match`: the return literals and the pattern-derived
//! `$matches` shape. The group scanner is lexical —
//! delimiters, escapes, character classes, non-capturing markers,
//! the three named-group spellings — not a regex parser:
//! alternation-aware optionality is a recorded debt, and a
//! conditional group (`(?(1)a|b)`) counts one spurious group, as
//! does a leading `]` inside a character class (`[]()]`), which is
//! literal in PCRE but ends the class early for the scanner and
//! lets the following `(` count as a group.
//!
//! Known limitations of the pattern scanner: all three items above
//! are sound-but-imprecise, recorded and unresolved:
//! alternation-aware group optionality (a group inside `a|b` is
//! unconditionally required by the scanner, though only one branch's
//! groups actually populate at runtime), the conditional-group
//! over-count, and the leading-`]` character-class miscount. A fourth,
//! named here for the same reason: an undecided or non-zero
//! `PREG_OFFSET_CAPTURE`-family flags argument answers the
//! conservative `array<int|string, mixed>` value shape
//! (`preg_match_matches` below) rather than threading the pattern's
//! named groups through the offset-tuple shape — sound, imprecise,
//! unmeasured against the corpus.

use celerrate_plugin::{ShapeField, ShapeKey, TypeContext, TypeId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PatternGroup {
    Numbered,
    Named(String),
}

pub(crate) fn preg_match_return<'db>(context: TypeContext<'db>) -> TypeId<'db> {
    context.union([
        context.int_literal(0),
        context.int_literal(1),
        context.bool_literal(false),
    ])
}

pub(crate) fn preg_match_matches<'db>(
    context: TypeContext<'db>,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    if arguments.len() < 2 {
        return None;
    }
    let int_or_string = context.union([context.int(), context.string()]);
    let flags = arguments.get(3);
    let flags_decided_zero = match flags {
        None => true,
        Some(flags) => context.int_literal_value(*flags) == Some(0),
    };
    if !flags_decided_zero {
        // PREG_OFFSET_CAPTURE and friends change the value shape.
        return Some(context.array(int_or_string, context.mixed()));
    }
    let groups = arguments
        .first()
        .and_then(|pattern| context.string_literal_value(*pattern))
        .and_then(|pattern| pattern_groups(&pattern));
    let Some(groups) = groups else {
        return Some(context.array(int_or_string, context.string()));
    };
    let mut fields = vec![ShapeField {
        key: ShapeKey::Integer(0),
        optional: true,
        value: context.string(),
    }];
    for (position, group) in groups.iter().enumerate() {
        let number = position as i64 + 1;
        if let PatternGroup::Named(name) = group {
            fields.push(ShapeField {
                key: ShapeKey::String(name.clone()),
                optional: true,
                value: context.string(),
            });
        }
        fields.push(ShapeField {
            key: ShapeKey::Integer(number),
            optional: true,
            value: context.string(),
        });
    }
    Some(context.shape(fields))
}

/// The capturing groups of a PCRE pattern, in order. `None` when
/// the pattern has no readable delimited body.
pub(crate) fn pattern_groups(pattern: &str) -> Option<Vec<PatternGroup>> {
    let mut characters = pattern.chars();
    let opening = characters.next()?;
    let closing = match opening {
        '(' => ')',
        '{' => '}',
        '[' => ']',
        '<' => '>',
        delimiter
            if !delimiter.is_alphanumeric() && delimiter != '\\' && !delimiter.is_whitespace() =>
        {
            delimiter
        }
        _ => return None,
    };
    let rest: String = characters.collect();
    let body_end = rest.rfind(closing)?;
    let body = rest.get(..body_end)?;
    let mut groups = Vec::new();
    let mut cursor = body.chars().peekable();
    while let Some(character) = cursor.next() {
        match character {
            '\\' => {
                cursor.next();
            }
            '[' => {
                // A character class: escapes still hide, `]` ends it.
                while let Some(inner) = cursor.next() {
                    match inner {
                        '\\' => {
                            cursor.next();
                        }
                        ']' => break,
                        _ => {}
                    }
                }
            }
            '(' => {
                if cursor.peek() != Some(&'?') {
                    groups.push(PatternGroup::Numbered);
                    continue;
                }
                cursor.next(); // the '?'
                match cursor.peek() {
                    Some('P') => {
                        cursor.next();
                        if cursor.peek() == Some(&'<') {
                            cursor.next();
                            groups.push(named_group(&mut cursor, '>'));
                        }
                    }
                    Some('<') => {
                        cursor.next();
                        // `(?<name>` captures; `(?<=` / `(?<!` do not.
                        match cursor.peek() {
                            Some('=') | Some('!') => {}
                            _ => groups.push(named_group(&mut cursor, '>')),
                        }
                    }
                    Some('\'') => {
                        cursor.next();
                        groups.push(named_group(&mut cursor, '\''));
                    }
                    _ => {} // (?:, (?=, (?!, modifiers: non-capturing
                }
            }
            _ => {}
        }
    }
    Some(groups)
}

fn named_group(
    cursor: &mut std::iter::Peekable<std::str::Chars<'_>>,
    terminator: char,
) -> PatternGroup {
    let mut name = String::new();
    while let Some(&character) = cursor.peek() {
        cursor.next();
        if character == terminator {
            break;
        }
        name.push(character);
    }
    PatternGroup::Named(name)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_plugin::{ShapeKey, TypeId};
    use celerrate_types::testing_type_context;

    use super::PatternGroup;

    #[test]
    fn the_scanner_counts_capturing_groups() {
        assert_eq!(
            super::pattern_groups("/(a)(b)/").unwrap(),
            vec![PatternGroup::Numbered, PatternGroup::Numbered],
        );
    }

    #[test]
    fn non_capturing_and_lookaround_groups_do_not_count() {
        assert_eq!(
            super::pattern_groups("/(?:a)(?=b)(?!c)(?<=d)(?<!e)/").unwrap(),
            vec![],
        );
    }

    #[test]
    fn named_groups_carry_their_names_in_all_three_spellings() {
        assert_eq!(
            super::pattern_groups("/(?P<year>\\d+)-(?<month>\\d+)-(?'day'\\d+)/").unwrap(),
            vec![
                PatternGroup::Named("year".to_owned()),
                PatternGroup::Named("month".to_owned()),
                PatternGroup::Named("day".to_owned()),
            ],
        );
    }

    #[test]
    fn escapes_and_character_classes_hide_their_parentheses() {
        assert_eq!(
            super::pattern_groups("/\\((a)[)(]/").unwrap(),
            vec![PatternGroup::Numbered],
        );
    }

    #[test]
    fn bracket_style_delimiters_pair() {
        assert_eq!(
            super::pattern_groups("{(a)}").unwrap(),
            vec![PatternGroup::Numbered],
        );
    }

    #[test]
    fn a_degenerate_pattern_answers_none_never_panics() {
        for pattern in ["", "/", "x", "((((", "/\\", "[", "/(?P<"] {
            // None or a best-effort group list — the only hard
            // requirement is: no panic, and None on patterns without a
            // readable body.
            let _ = super::pattern_groups(pattern);
        }
        assert!(super::pattern_groups("").is_none());
        assert!(super::pattern_groups("x").is_none());
    }

    /// The scanner's hostile-input sweep, modelled on `norm.rs`'s
    /// `every_norm_alphabet_soup_is_parsed_or_rejected_without_panicking`
    /// and `written.rs`'s
    /// `every_ascii_soup_is_parsed_or_rejected_without_panicking`
    /// (`celerrate_stubs::compiler::refinement_source` holds the same
    /// contract for its own text scanner): three-byte soups over an
    /// alphabet drawn from the scanner's own grammar — delimiters
    /// (identical and paired), escapes, character-class brackets, group
    /// markers, and the named-group spellings. `pattern_groups` must
    /// never panic on any combination, whatever it answers.
    #[test]
    fn every_pattern_alphabet_soup_is_scanned_or_rejected_without_panicking() {
        let alphabet = [
            b'/', b'{', b'}', b'(', b')', b'[', b']', b'<', b'>', b'?', b':', b'=', b'!', b'P',
            b'\'', b'\\', b'a', b'1',
        ];
        for &a in &alphabet {
            for &b in &alphabet {
                for &c in &alphabet {
                    let text: String = [a, b, c].iter().map(|&byte| byte as char).collect();
                    let _ = super::pattern_groups(&text);
                }
            }
        }
    }

    /// No recursion in the scanner (an iterative cursor, unlike the
    /// norm parser's nested-atom recursion), so a stack-overflow guard
    /// is not needed the way `norm.rs`'s
    /// `deeply_nested_input_answers_none_instead_of_overflowing_the_stack`
    /// needs one — but a pathological width (a huge run of unmatched
    /// `(`) and multi-byte UTF-8 content interleaved with delimiters
    /// must still complete without panicking: `rest.get(..body_end)`
    /// slices at a byte index `rfind` guarantees lands on a char
    /// boundary, but only a real run proves it under load-bearing
    /// multi-byte input, not just the scanner's own reasoning.
    #[test]
    fn pathological_width_and_multi_byte_input_never_panics() {
        let deeply_nested = format!("/{}/", "(".repeat(100_000));
        assert!(super::pattern_groups(&deeply_nested).is_some());
        for pattern in [
            "/(café)(?<naïve>é+)/",
            "/(?P<日本語>.)(?'emoji'🎉)/",
            "\u{0}\u{1}\u{2}",
            "/[é\\]a](b)/",
        ] {
            let _ = super::pattern_groups(pattern);
        }
    }

    #[test]
    fn matches_shape_is_all_optional_with_both_key_spellings() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let pattern = TypeId::string_literal(&db, "/(?<year>\\d+)-(\\d+)/");
        let answer = super::preg_match_matches(context, &[pattern, TypeId::string(&db)]).unwrap();
        let fields = answer.shape_fields(&db).unwrap();
        // {0?: string, year?: string, 1?: string, 2?: string}: group 0,
        // the named group under both its name and its number, the
        // second group under its number. Every field optional, every
        // value string.
        assert!(fields.iter().all(|field| field.optional));
        assert!(
            fields
                .iter()
                .all(|field| field.value == TypeId::string(&db)),
        );
        assert_eq!(fields.len(), 4);
        let keys: Vec<ShapeKey> = fields.iter().map(|field| field.key.clone()).collect();
        assert!(keys.contains(&ShapeKey::Integer(0)));
        assert!(keys.contains(&ShapeKey::Integer(1)));
        assert!(keys.contains(&ShapeKey::Integer(2)));
        assert!(keys.contains(&ShapeKey::String("year".to_owned())));
    }

    #[test]
    fn a_flags_argument_or_unknown_pattern_falls_back_conservatively() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let int_or_string = TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]);
        // Unknown pattern: values are still strings.
        assert_eq!(
            super::preg_match_matches(context, &[TypeId::string(&db), TypeId::string(&db)])
                .unwrap(),
            TypeId::array(&db, int_or_string, TypeId::string(&db)),
        );
        // A non-zero-literal flags argument: values are opaque.
        let pattern = TypeId::string_literal(&db, "/(a)/");
        assert_eq!(
            super::preg_match_matches(
                context,
                &[
                    pattern,
                    TypeId::string(&db),
                    TypeId::mixed(&db), // the $matches slot
                    TypeId::int(&db),   // unknown flags
                ],
            )
            .unwrap(),
            TypeId::array(&db, int_or_string, TypeId::mixed(&db)),
        );
    }

    #[test]
    fn the_return_is_zero_one_or_false() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        assert_eq!(
            super::preg_match_return(context),
            TypeId::union(
                &db,
                [
                    TypeId::int_literal(&db, 0),
                    TypeId::int_literal(&db, 1),
                    TypeId::bool_literal(&db, false),
                ],
            ),
        );
    }
}
