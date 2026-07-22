//! The suppression-directive recognizer: the bridge's implementation
//! of the comment-directive extension point.
//!
//! # The mapping table (bridge-internal, design section 5)
//!
//! | Written form                    | Comment kind | Directive                        |
//! |---------------------------------|--------------|----------------------------------|
//! | `@phpstan-ignore-line`          | any          | suppress, current line           |
//! | `@phpstan-ignore-next-line`     | any          | suppress, next line              |
//! | `@phpstan-ignore <identifiers>` | any          | suppress, current and next line  |
//! | `@psalm-suppress <identifiers>` | docblock     | suppress, annotated declaration  |
//! | `@psalm-suppress <identifiers>` | line, block  | suppress, current and next line  |
//!
//! PHPStan 1.11's bare `@phpstan-ignore` targets its own line when the
//! comment trails code and the next line otherwise; covering both
//! lines is the superset that under-suppresses neither placement
//! (design section 5: over-suppression, never under-suppression). A
//! docblock-attached `@psalm-suppress` maps to the annotated
//! declaration's whole span — its Psalm scope, not the docblock's own
//! line where no diagnostic ever fires. Identifiers are marked through
//! the correspondence table (design section 8): the bridge marks each
//! written identifier as mapped to its Celerrate codes, explicitly
//! scope-wide, or unmapped; the matcher downstream (`celerrate_semantics`)
//! is where that mark turns into a filter. Malformed content yields
//! fewer identifiers or no directive, never an error - no docblock
//! diagnostics. Only the tag's own line is read for identifiers; a
//! list that wraps onto a continuation line widens the directive to
//! its whole scope rather than honoring the prefix that fitted, so
//! wrapping over-suppresses and never under-suppresses.

use celerrate_plugin::{
    CommentDirective, CommentDirectiveProvider, CommentKind, DirectiveOrigin, DirectiveScope,
    SuppressionIdentifier,
};

use crate::correspondence::{Dialect, ForeignMapping, foreign_mapping};
use crate::syntax::PhpdocBridge;

const PHPSTAN_IGNORE: &str = "@phpstan-ignore";
const PSALM_SUPPRESS: &str = "@psalm-suppress";

impl CommentDirectiveProvider for PhpdocBridge {
    fn directives(&self, kind: CommentKind, text: &str) -> Vec<CommentDirective> {
        comment_directives(kind, text)
    }
}

/// Every directive one comment carries, in written order. Total over
/// arbitrary input.
pub fn comment_directives(kind: CommentKind, text: &str) -> Vec<CommentDirective> {
    let mut directives = Vec::new();
    let mut rest = text;
    while let Some(position) = rest.find('@') {
        let Some(tail) = rest.get(position..) else {
            break;
        };
        if let Some(after) = tail.strip_prefix(PHPSTAN_IGNORE) {
            directives.extend(phpstan_directive(after));
        } else if let Some(after) = tail.strip_prefix(PSALM_SUPPRESS) {
            directives.extend(psalm_directive(kind, after));
        }
        // `@` is ASCII: one past it is always a character boundary.
        rest = rest.get(position + 1..).unwrap_or("");
    }
    directives
}

/// Classifies what follows `@phpstan-ignore`, longest suffix first —
/// `-next-line` before `-line` before the bare identifier-bearing form.
fn phpstan_directive(after_tag: &str) -> Option<CommentDirective> {
    if let Some(rest) = after_tag.strip_prefix("-next-line") {
        ends_word(rest).then(|| suppress(DirectiveScope::NextLine, Vec::new()))
    } else if let Some(rest) = after_tag.strip_prefix("-line") {
        ends_word(rest).then(|| suppress(DirectiveScope::CurrentLine, Vec::new()))
    } else if ends_word(after_tag) {
        Some(suppress(
            DirectiveScope::CurrentAndNextLine,
            marked_identifiers(Dialect::Phpstan, after_tag),
        ))
    } else {
        None
    }
}

fn psalm_directive(kind: CommentKind, after_tag: &str) -> Option<CommentDirective> {
    if !ends_word(after_tag) {
        return None;
    }
    let scope = match kind {
        CommentKind::Docblock => DirectiveScope::AnnotatedDeclaration,
        CommentKind::Line | CommentKind::Block => DirectiveScope::CurrentAndNextLine,
        // A comment kind this bridge does not know yet: the both-lines
        // superset, the same over-suppression-never-under-suppression
        // resolution the bare form uses (design section 5).
        _ => DirectiveScope::CurrentAndNextLine,
    };
    Some(suppress(
        scope,
        marked_identifiers(Dialect::Psalm, after_tag),
    ))
}

/// One written identifier, marked through the correspondence table
/// (design section 8): mapped with its code strings, explicitly
/// scope-wide, or unmapped. This resolves the long-standing "carried,
/// never matched" reservation: the bridge marks, the matcher
/// downstream matches.
fn foreign_identifier(dialect: Dialect, written: String) -> SuppressionIdentifier {
    match foreign_mapping(dialect, &written) {
        ForeignMapping::Codes(codes) => SuppressionIdentifier::mapped(
            written,
            codes.iter().map(|code| (*code).to_owned()).collect(),
        ),
        ForeignMapping::ScopeWide => SuppressionIdentifier::scope_wide(written),
        ForeignMapping::Unmapped => SuppressionIdentifier::unmapped(written),
    }
}

fn suppress(scope: DirectiveScope, identifiers: Vec<SuppressionIdentifier>) -> CommentDirective {
    CommentDirective::suppress(scope, DirectiveOrigin::Foreign, identifiers)
}

/// The written form of the synthetic identifier appended when a foreign
/// identifier list wraps past the tag's own line. It is deliberately
/// unspellable as a real identifier of either dialect, and it exists
/// for one effect: `filter_of` downstream turns any `Unmapped` entry
/// into a whole-scope filter, so a directive whose list continues out
/// of the parser's reach widens instead of quietly protecting only the
/// identifiers that fit on the first line. Widening is the accepted
/// failure direction; the narrowing it replaces was a silent
/// regression. If a verbose channel ever echoes a directive's written
/// identifiers, this reads as the reason the directive widened.
const WRAPPED_LIST_CONTINUES: &str = "<identifier list continues on the next line>";

/// Every identifier a bare tag carries, marked through the
/// correspondence table, plus the synthetic widening identifier when
/// the written list runs past the tag's own line.
fn marked_identifiers(dialect: Dialect, after_tag: &str) -> Vec<SuppressionIdentifier> {
    let (written, continues) = identifier_list(after_tag);
    let mut identifiers: Vec<SuppressionIdentifier> = written
        .into_iter()
        .map(|written| foreign_identifier(dialect, written))
        .collect();
    if continues {
        identifiers.push(SuppressionIdentifier::unmapped(
            WRAPPED_LIST_CONTINUES.to_owned(),
        ));
    }
    identifiers
}

/// A tag ends at a word boundary: the end of the comment, whitespace,
/// or a closing `*/`. `@phpstan-ignored` is prose, not a directive.
fn ends_word(after: &str) -> bool {
    after.is_empty()
        || after.starts_with(|character: char| character.is_whitespace())
        || after.starts_with("*/")
}

/// The identifier list after a bare tag, and whether the written list
/// continues past the tag's own line. Only that line is read: a
/// parenthesized trailer is dropped (`@phpstan-ignore method.notFound
/// (nullable receiver)`), the closing `*/` is dropped, and the rest is
/// comma-separated and trimmed of whitespace and docblock decoration.
/// A line left dangling on a comma is the wrapped-list case: the
/// remaining identifiers sit on a continuation line this function never
/// sees, so the caller widens rather than honoring only the prefix.
/// Malformed content yields fewer identifiers, never a lost directive.
fn identifier_list(after_tag: &str) -> (Vec<String>, bool) {
    let mut line = after_tag.lines().next().unwrap_or("");
    if let Some((before, _)) = line.split_once("*/") {
        line = before;
    }
    if let Some((before, _)) = line.split_once('(') {
        line = before;
    }
    let continues = line.trim_end().ends_with(',');
    let identifiers = line
        .split(',')
        .map(|identifier| identifier.trim().trim_matches('*').trim())
        .filter(|identifier| !identifier.is_empty())
        .map(str::to_owned)
        .collect();
    (identifiers, continues)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn mapped(written: &str, codes: &[&str]) -> SuppressionIdentifier {
        SuppressionIdentifier::mapped(
            written.to_owned(),
            codes.iter().map(|code| (*code).to_owned()).collect(),
        )
    }

    #[test]
    fn ignore_line_suppresses_the_current_line_in_any_comment_kind() {
        for (kind, text) in [
            (CommentKind::Line, "// @phpstan-ignore-line"),
            (CommentKind::Line, "# @phpstan-ignore-line"),
            (CommentKind::Block, "/* @phpstan-ignore-line */"),
            (CommentKind::Docblock, "/** @phpstan-ignore-line */"),
        ] {
            assert_eq!(
                comment_directives(kind, text),
                vec![suppress(DirectiveScope::CurrentLine, vec![])],
                "{text}",
            );
        }
    }

    #[test]
    fn ignore_next_line_suppresses_the_next_line_and_is_not_read_as_the_bare_form() {
        assert_eq!(
            comment_directives(CommentKind::Line, "// @phpstan-ignore-next-line"),
            vec![suppress(DirectiveScope::NextLine, vec![])],
        );
    }

    #[test]
    fn the_bare_form_carries_identifiers_and_covers_both_placements() {
        assert_eq!(
            comment_directives(
                CommentKind::Line,
                "// @phpstan-ignore method.notFound, property.notFound (nullable receiver)",
            ),
            vec![suppress(
                DirectiveScope::CurrentAndNextLine,
                vec![
                    mapped("method.notFound", &["CEL0030"]),
                    mapped("property.notFound", &["CEL0031"]),
                ],
            )],
        );
        assert_eq!(
            comment_directives(CommentKind::Line, "// @phpstan-ignore"),
            vec![suppress(DirectiveScope::CurrentAndNextLine, vec![])],
        );
    }

    #[test]
    fn psalm_suppress_in_a_docblock_targets_the_annotated_declaration() {
        assert_eq!(
            comment_directives(
                CommentKind::Docblock,
                "/**\n * @psalm-suppress PossiblyNullReference, InvalidArgument\n */",
            ),
            vec![suppress(
                DirectiveScope::AnnotatedDeclaration,
                vec![
                    mapped("PossiblyNullReference", &["CEL0034"]),
                    mapped("InvalidArgument", &["CEL0035"]),
                ],
            )],
        );
    }

    #[test]
    fn psalm_suppress_in_an_ordinary_comment_covers_both_lines() {
        assert_eq!(
            comment_directives(CommentKind::Block, "/* @psalm-suppress InvalidArgument */"),
            vec![suppress(
                DirectiveScope::CurrentAndNextLine,
                vec![mapped("InvalidArgument", &["CEL0035"])],
            )],
        );
    }

    #[test]
    fn psalm_suppress_all_marks_the_identifier_scope_wide() {
        assert_eq!(
            comment_directives(CommentKind::Block, "/* @psalm-suppress all */"),
            vec![suppress(
                DirectiveScope::CurrentAndNextLine,
                vec![SuppressionIdentifier::scope_wide("all".to_owned())],
            )],
        );
    }

    #[test]
    fn a_docblock_may_carry_several_directives() {
        assert_eq!(
            comment_directives(
                CommentKind::Docblock,
                "/**\n * @psalm-suppress UndefinedClass\n * @phpstan-ignore-next-line\n */",
            ),
            vec![
                suppress(
                    DirectiveScope::AnnotatedDeclaration,
                    vec![mapped("UndefinedClass", &["CEL0018", "CEL0021", "CEL0022"])],
                ),
                suppress(DirectiveScope::NextLine, vec![]),
            ],
        );
    }

    #[test]
    fn a_wrapped_identifier_list_widens_to_the_whole_scope() {
        // Only the tag's line is read, so `UndefinedFunction` is never
        // matched. Honoring `UndefinedClass` alone would protect fewer
        // codes than written, which is under-suppression; the dangling
        // comma is the signal that turns the directive scope-wide.
        assert_eq!(
            comment_directives(
                CommentKind::Docblock,
                "/**\n * @psalm-suppress UndefinedClass,\n * UndefinedFunction\n */",
            ),
            vec![suppress(
                DirectiveScope::AnnotatedDeclaration,
                vec![
                    mapped("UndefinedClass", &["CEL0018", "CEL0021", "CEL0022"]),
                    SuppressionIdentifier::unmapped(WRAPPED_LIST_CONTINUES.to_owned()),
                ],
            )],
        );
    }

    #[test]
    fn a_wrapped_list_is_detected_past_the_reason_trailer_and_the_terminator() {
        for text in [
            "// @phpstan-ignore class.notFound, (a reason)",
            "/* @phpstan-ignore class.notFound, */",
        ] {
            assert_eq!(
                comment_directives(CommentKind::Line, text),
                vec![suppress(
                    DirectiveScope::CurrentAndNextLine,
                    vec![
                        mapped("class.notFound", &["CEL0018", "CEL0021", "CEL0022"]),
                        SuppressionIdentifier::unmapped(WRAPPED_LIST_CONTINUES.to_owned()),
                    ],
                )],
                "{text}",
            );
        }
    }

    #[test]
    fn a_complete_list_never_widens() {
        assert_eq!(
            comment_directives(
                CommentKind::Docblock,
                "/**\n * @psalm-suppress UndefinedClass, UndefinedFunction\n */",
            ),
            vec![suppress(
                DirectiveScope::AnnotatedDeclaration,
                vec![
                    mapped("UndefinedClass", &["CEL0018", "CEL0021", "CEL0022"]),
                    mapped("UndefinedFunction", &["CEL0019", "CEL0021", "CEL0022"]),
                ],
            )],
        );
    }

    #[test]
    fn a_tag_must_end_at_a_word_boundary() {
        // Prose that merely embeds the letters is not a directive.
        assert!(comment_directives(CommentKind::Line, "// @phpstan-ignored").is_empty());
        assert!(comment_directives(CommentKind::Line, "// @phpstan-ignore-linear").is_empty());
        assert!(comment_directives(CommentKind::Line, "// @psalm-suppressive").is_empty());
    }

    #[test]
    fn plain_prose_carries_nothing() {
        assert!(comment_directives(CommentKind::Line, "// a plain remark").is_empty());
        assert!(comment_directives(CommentKind::Docblock, "/** @param int $x */").is_empty());
    }

    #[test]
    fn adversarial_inputs_never_panic() {
        let inputs = [
            "@phpstan-ignore",
            "@phpstan-ignore-",
            "@phpstan-ignore-next-line@phpstan-ignore-line",
            "@@@@phpstan-ignore@psalm-suppress",
            "// @phpstan-ignore ((((((",
            "// @phpstan-ignore ,,,,,",
            "/* @psalm-suppress */ trailing",
            "@psalm-suppress \u{0} \u{7f} é漢字",
            "@",
            "",
            "*/ @phpstan-ignore-line /*",
        ];
        for input in inputs {
            for kind in [CommentKind::Line, CommentKind::Block, CommentKind::Docblock] {
                let _ = comment_directives(kind, input);
            }
        }
    }
}
