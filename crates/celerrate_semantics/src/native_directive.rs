//! Celerrate's own suppression directive: `@celerrate-ignore CEL0030,
//! CEL0031 (reason)` in a line comment, a block comment, or a
//! docblock. Identifiers are mandatory - there is no blanket form, so
//! the tool's own directive cannot dig a new #58-class hole by
//! construction (a bare directive parses, suppresses nothing, and is
//! CEL0042's subject). The optional parenthesized trailer is a reason,
//! excluded from identifier parsing. Docblock placement keeps the
//! annotated declaration's scope; everywhere else the scope is
//! placement-resolved (`DirectiveScope::TrailingOrNextLine`).
//!
//! The small parsing helpers (`ends_word`, `identifiers_of`) mirror the
//! bridge's rather than being shared with it. Sharing would in fact be
//! easy to wire - this crate sits below the bridge in the DAG, so a
//! shared helper would flow downward, not against the grain - but there
//! is no reason to: the two grammars are free to diverge, this one
//! being Celerrate's own to evolve on its own schedule, and the shared
//! surface would be about twenty lines of parsing helper that the two
//! dialects have no obligation to keep identical. Malformed content
//! yields fewer identifiers or no directive, never an error.

use crate::comment_directives::{
    CommentDirective, CommentDirectiveProvider, CommentKind, DirectiveOrigin, DirectiveScope,
    SuppressionIdentifier,
};

/// The written tag.
const NATIVE_DIRECTIVE_TAG: &str = "@celerrate-ignore";

/// The core provider, registered unconditionally at the composition
/// root under the reserved core identity.
#[derive(Debug, Default)]
pub struct NativeDirectiveProvider;

impl CommentDirectiveProvider for NativeDirectiveProvider {
    fn directives(&self, kind: CommentKind, text: &str) -> Vec<CommentDirective> {
        native_directives(kind, text)
    }
}

/// Every native directive one comment carries, in written order. Total
/// over arbitrary input.
pub fn native_directives(kind: CommentKind, text: &str) -> Vec<CommentDirective> {
    let mut directives = Vec::new();
    let mut rest = text;
    while let Some(position) = rest.find('@') {
        let Some(tail) = rest.get(position..) else {
            break;
        };
        if let Some(after) = tail.strip_prefix(NATIVE_DIRECTIVE_TAG)
            && ends_word(after)
        {
            let scope = match kind {
                CommentKind::Docblock => DirectiveScope::AnnotatedDeclaration,
                _ => DirectiveScope::TrailingOrNextLine,
            };
            directives.push(CommentDirective::suppress(
                scope,
                DirectiveOrigin::Native,
                identifiers_of(after)
                    .into_iter()
                    .map(SuppressionIdentifier::native)
                    .collect(),
            ));
        }
        // `@` is ASCII: one past it is always a character boundary.
        rest = rest.get(position + 1..).unwrap_or("");
    }
    directives
}

/// A tag ends at a word boundary: the end of the comment, whitespace,
/// or a closing `*/`. `@celerrate-ignored` is prose, not a directive.
fn ends_word(after: &str) -> bool {
    after.is_empty()
        || after.starts_with(|character: char| character.is_whitespace())
        || after.starts_with("*/")
}

/// The identifier list after the tag: the rest of that line, the
/// parenthesized reason trailer dropped, the closing `*/` dropped,
/// comma-separated, trimmed of whitespace and docblock decoration.
fn identifiers_of(after_tag: &str) -> Vec<String> {
    let mut line = after_tag.lines().next().unwrap_or("");
    if let Some((before, _)) = line.split_once("*/") {
        line = before;
    }
    if let Some((before, _)) = line.split_once('(') {
        line = before;
    }
    line.split(',')
        .map(|identifier| identifier.trim().trim_matches('*').trim())
        .filter(|identifier| !identifier.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::comment_directives::{CommentKind, DirectiveOrigin, DirectiveScope};

    fn native(scope: DirectiveScope, identifiers: &[&str]) -> CommentDirective {
        CommentDirective::suppress(
            scope,
            DirectiveOrigin::Native,
            identifiers
                .iter()
                .map(|written| SuppressionIdentifier::native((*written).to_owned()))
                .collect(),
        )
    }

    #[test]
    fn a_line_comment_directive_is_placement_resolved() {
        assert_eq!(
            native_directives(CommentKind::Line, "// @celerrate-ignore CEL0030, CEL0031"),
            vec![native(
                DirectiveScope::TrailingOrNextLine,
                &["CEL0030", "CEL0031"],
            )],
        );
    }

    #[test]
    fn a_docblock_directive_keeps_the_declaration_scope() {
        assert_eq!(
            native_directives(
                CommentKind::Docblock,
                "/**\n * @celerrate-ignore CEL0030\n */",
            ),
            vec![native(DirectiveScope::AnnotatedDeclaration, &["CEL0030"])],
        );
    }

    #[test]
    fn the_reason_trailer_is_excluded_from_identifier_parsing() {
        assert_eq!(
            native_directives(
                CommentKind::Line,
                "// @celerrate-ignore CEL0030, CEL0031 (nullable receiver from the legacy adapter)",
            ),
            vec![native(
                DirectiveScope::TrailingOrNextLine,
                &["CEL0030", "CEL0031"],
            )],
        );
    }

    #[test]
    fn a_bare_directive_still_parses_with_no_identifiers() {
        // No blanket form: the empty identifier list suppresses
        // nothing (filter_of answers Only(empty)); the directive still
        // exists so CEL0042 can report it.
        assert_eq!(
            native_directives(CommentKind::Line, "// @celerrate-ignore"),
            vec![native(DirectiveScope::TrailingOrNextLine, &[])],
        );
    }

    #[test]
    fn the_tag_must_end_at_a_word_boundary() {
        assert!(native_directives(CommentKind::Line, "// @celerrate-ignored").is_empty());
        assert!(native_directives(CommentKind::Line, "// @celerrate-ignores CEL0030").is_empty());
    }

    #[test]
    fn plain_prose_carries_nothing() {
        assert!(native_directives(CommentKind::Line, "// a plain remark").is_empty());
        assert!(native_directives(CommentKind::Docblock, "/** @param int $x */").is_empty());
    }

    #[test]
    fn adversarial_inputs_never_panic() {
        let inputs = [
            "@celerrate-ignore",
            "@celerrate-ignore-",
            "@celerrate-ignore ((((((",
            "@celerrate-ignore ,,,,,",
            "/* @celerrate-ignore */ trailing",
            "@celerrate-ignore \u{0} \u{7f} é漢字",
            "@@@@celerrate-ignore@celerrate-ignore",
            "@",
            "",
        ];
        for input in inputs {
            for kind in [CommentKind::Line, CommentKind::Block, CommentKind::Docblock] {
                let _ = native_directives(kind, input);
            }
        }
    }
}
