//! The comment-directive extension point: structured directives read
//! from comment trivia — today, suppressions ("extinguish every
//! diagnostic family on this scope").
//!
//! Owned by this crate per the design: the registry input lives with
//! the consuming layer, implementations are registered at the
//! composition root, `celerrate_plugin` re-exports the vocabulary.
//! The vocabulary (what a directive *is*) belongs to this trait; the
//! written tag table (what `@phpstan-ignore-line` *means*) is
//! bridge-internal, like the tag precedence table (design section 4).
//! Scopes are symbolic — a provider is a pure function of the comment
//! and cannot see positions; `suppressed_ranges` resolves them.
//! Identifiers travel with a directive but are not yet matched here:
//! identifier-level matching lives in `suppression_directives`' filter
//! computation (task 2 of the part-5 plan resolves the long-standing
//! reservation).

use std::sync::Arc;

use celerrate_db::SourceFile;
use celerrate_source::{LineColumn, LineIndex, TextRange, TextSize};
use celerrate_syntax::{SyntaxKind, SyntaxToken};

use crate::plugin::PluginIdentity;

/// The comment shapes a provider may be handed.
///
/// This type is `#[non_exhaustive]` — new shapes may be added in future
/// versions. Use the constructor provided by this crate for portable
/// construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CommentKind {
    /// `//` and `#` comments.
    Line,
    /// `/* ... */` comments.
    Block,
    /// `/** ... */` docblocks.
    Docblock,
}

/// Where a directive applies, relative to the comment that carries it.
///
/// This type is `#[non_exhaustive]` — new scopes may be added in future
/// versions. Use the constructor provided by this crate for portable
/// construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DirectiveScope {
    /// The whole line(s) the comment covers — a trailing comment
    /// covers the code before it on the same line.
    CurrentLine,
    /// The whole line after the comment's last line.
    NextLine,
    /// Both of the above: the fixed over-suppression resolution of a
    /// placement-dependent directive (PHPStan 1.11's bare
    /// `@phpstan-ignore`).
    CurrentAndNextLine,
    /// The whole span of the node the comment annotates (a docblock's
    /// Psalm scope). Falls back to [`Self::CurrentAndNextLine`] when
    /// no annotated node exists: over-suppressed, never dropped.
    AnnotatedDeclaration,
    /// The line(s) the comment trails when code precedes it on the
    /// comment's first line; the next line when the comment stands
    /// alone. The native directive's placement-dependent scope,
    /// resolved where the token and its line context are visible
    /// (`resolve_scope`), never in a provider - a provider is a pure
    /// function of the comment and cannot see position.
    TrailingOrNextLine,
}

/// One written identifier of a suppression directive and its fate.
/// Foreign fates come from the bridge's correspondence table; codes
/// travel as plain strings and are interned downstream through
/// `celerrate_diagnostics::find_identifier` (design section 8: the
/// facade grows no identifier vocabulary for this).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SuppressionIdentifier {
    /// A foreign identifier the correspondence table maps: it
    /// suppresses exactly these Celerrate codes.
    Mapped { written: String, codes: Vec<String> },
    /// A foreign identifier that explicitly names the whole scope
    /// (`@psalm-suppress all`): an entry, not a fallback accident.
    ScopeWide { written: String },
    /// A foreign identifier with no Celerrate correspondence: the
    /// directive falls back to scope-wide suppression, honoring the
    /// user's existing decision (the #58 triage policy).
    Unmapped { written: String },
    /// A native `CEL####` identifier, written form kept verbatim. An
    /// unknown one suppresses nothing (never widens) and is reported
    /// by CEL0041.
    Native { written: String },
}

impl SuppressionIdentifier {
    pub fn mapped(written: String, codes: Vec<String>) -> Self {
        Self::Mapped { written, codes }
    }

    pub fn scope_wide(written: String) -> Self {
        Self::ScopeWide { written }
    }

    pub fn unmapped(written: String) -> Self {
        Self::Unmapped { written }
    }

    pub fn native(written: String) -> Self {
        Self::Native { written }
    }

    /// The identifier as the user wrote it.
    pub fn written(&self) -> &str {
        match self {
            Self::Mapped { written, .. }
            | Self::ScopeWide { written }
            | Self::Unmapped { written }
            | Self::Native { written } => written,
        }
    }
}

/// Who a directive belongs to. The distinction is load-bearing for the
/// empty-identifier case: a bare foreign directive suppresses the
/// whole scope, a bare native one suppresses nothing (identifiers are
/// mandatory by design), and only native directives are subject to the
/// CEL0041/CEL0042 reporting rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DirectiveOrigin {
    Foreign,
    Native,
}

/// One structured directive a comment carries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CommentDirective {
    /// Extinguish diagnostics on the scope, filtered by the
    /// identifiers under the origin's policy (`filter_of` below is the
    /// single implementation of that policy).
    #[non_exhaustive]
    Suppress {
        scope: DirectiveScope,
        origin: DirectiveOrigin,
        identifiers: Vec<SuppressionIdentifier>,
    },
}

impl CommentDirective {
    /// Constructor for cross-crate construction: literal construction
    /// is closed by `#[non_exhaustive]`.
    pub fn suppress(
        scope: DirectiveScope,
        origin: DirectiveOrigin,
        identifiers: Vec<SuppressionIdentifier>,
    ) -> Self {
        Self::Suppress {
            scope,
            origin,
            identifiers,
        }
    }
}

/// A provider translates one comment into the directives it carries.
/// Implementations must be deterministic pure functions of their
/// arguments: no interior state, no environment reads (the
/// byte-identical harness is the mechanical detector).
pub trait CommentDirectiveProvider: Send + Sync {
    fn directives(&self, kind: CommentKind, text: &str) -> Vec<CommentDirective>;
}

/// One registration: the implementation travels with its identity.
#[derive(Clone)]
pub struct CommentDirectiveRegistration {
    pub identity: PluginIdentity,
    pub provider: Arc<dyn CommentDirectiveProvider>,
}

impl std::fmt::Debug for CommentDirectiveRegistration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommentDirectiveRegistration")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// The registry: set once per process at the composition root, in the
/// high-durability tier, and never mutated — reading it therefore
/// never invalidates. Databases that register nothing (every test
/// database by default) take the no-plugin path. Providers are
/// consulted in registered order; contributions concatenate in that
/// order — suppression is a union, so the result is independent of
/// thread timing by construction.
#[salsa::input(singleton)]
pub struct CommentDirectiveRegistry {
    #[returns(ref)]
    pub registrations: Vec<CommentDirectiveRegistration>,
}

/// The file's suppressed ranges: every comment handed to every
/// registered provider, the symbolic scopes resolved against the line
/// index, sorted and deduplicated. An own-tree read for strictly-local
/// output — the syntax-gating precedent. `Eq`-comparable: a comment
/// edit that leaves the directive set unchanged backdates, and
/// dependents never re-run.
#[salsa::tracked(returns(ref))]
pub fn suppressed_ranges(db: &dyn salsa::Database, file: SourceFile) -> Vec<TextRange> {
    let Some(registry) = CommentDirectiveRegistry::try_get(db) else {
        return Vec::new();
    };
    let registrations = registry.registrations(db);
    if registrations.is_empty() {
        return Vec::new();
    }
    let root = celerrate_db::parse(db, file).tree();
    let index = celerrate_db::line_index(db, file);
    let text_end = root.text_range().end();
    let mut ranges = Vec::new();
    for element in root.descendants_with_tokens() {
        let Some(token) = element.as_token() else {
            continue;
        };
        let Some(kind) = comment_kind(token.kind()) else {
            continue;
        };
        for registration in registrations {
            for directive in registration.provider.directives(kind, token.text()) {
                match directive {
                    CommentDirective::Suppress { scope, .. } => {
                        if let Some(range) = resolve_scope(scope, token, index, text_end) {
                            ranges.push(range);
                        }
                    }
                }
            }
        }
    }
    ranges.sort_by_key(|range| (range.start(), range.end()));
    ranges.dedup();
    ranges
}

/// Whether a diagnostic anchored at `offset` falls in a suppressed
/// range. Matching is by the diagnostic's start — the location the
/// report names — end-exclusive, except at the very end of the file:
/// a diagnostic anchored exactly at the text's end (an
/// unexpected-end-of-file parse error) belongs to the last line and
/// must be suppressible from it, or the suppression under-suppresses
/// (design section 5's rule, in the one place every consumer shares).
pub fn is_suppressed(suppressed: &[TextRange], offset: TextSize, text_end: TextSize) -> bool {
    suppressed.iter().any(|range| {
        offset >= range.start()
            && (offset < range.end() || (offset == range.end() && range.end() == text_end))
    })
}

/// The trivia kinds a provider may read.
fn comment_kind(kind: SyntaxKind) -> Option<CommentKind> {
    match kind {
        SyntaxKind::LineComment => Some(CommentKind::Line),
        SyntaxKind::BlockComment => Some(CommentKind::Block),
        SyntaxKind::DocComment => Some(CommentKind::Docblock),
        _ => None,
    }
}

/// Resolves a symbolic scope to concrete offsets. `None` means the
/// scope names nothing that exists (a next-line directive on the last
/// line): nothing to suppress, nothing to under-suppress.
fn resolve_scope(
    scope: DirectiveScope,
    token: &SyntaxToken,
    index: &LineIndex,
    text_end: TextSize,
) -> Option<TextRange> {
    let comment = token.text_range();
    let first_line = index.line_column(comment.start()).line;
    let last_line = index.line_column(comment.end()).line;
    match scope {
        DirectiveScope::CurrentLine => whole_lines(index, text_end, first_line, last_line),
        DirectiveScope::NextLine => {
            let next = last_line.checked_add(1)?;
            whole_lines(index, text_end, next, next)
        }
        DirectiveScope::CurrentAndNextLine => {
            whole_lines(index, text_end, first_line, last_line.saturating_add(1))
        }
        DirectiveScope::AnnotatedDeclaration => annotated_node_range(token)
            .or_else(|| whole_lines(index, text_end, first_line, last_line.saturating_add(1))),
        DirectiveScope::TrailingOrNextLine => {
            if code_adjacent_on_line(token, index) {
                whole_lines(index, text_end, first_line, last_line)
            } else {
                // The comment stands alone: the next line. When that
                // line does not exist (the directive sits on the
                // file's last line), the scope degenerates to the
                // empty end-of-file range: the directive survives
                // resolution - the reporting rules must see it - and,
                // through the end-of-file exception, covers exactly
                // the diagnostics anchored at the text's end, the same
                // coverage the empty final line of a newline-terminated
                // file gets (decision 6 of the part-5 plan).
                let next_line = last_line
                    .checked_add(1)
                    .and_then(|next| whole_lines(index, text_end, next, next));
                Some(next_line.unwrap_or_else(|| TextRange::empty(text_end)))
            }
        }
    }
}

/// The covering range of the whole lines `first..=last`, newline
/// included. `None` when `first` does not exist; a `last` past the
/// file's end clamps to the end of text.
fn whole_lines(index: &LineIndex, text_end: TextSize, first: u32, last: u32) -> Option<TextRange> {
    let start = index.offset(LineColumn {
        line: first,
        column: 0,
    })?;
    let end = last
        .checked_add(1)
        .and_then(|below| {
            index.offset(LineColumn {
                line: below,
                column: 0,
            })
        })
        .unwrap_or(text_end);
    Some(TextRange::new(start, end.max(start)))
}

/// Whether any non-trivia token shares a line with `token`: before it
/// on the token's first line, or after it on the token's last line (a
/// block comment can lead its statement) - the placement question
/// `TrailingOrNextLine` resolves on. Comment trivia does not count as
/// code (a neighboring comment leaves the directive alone on its
/// line); anything else, the `<?php` open tag included, does.
fn code_adjacent_on_line(token: &SyntaxToken, index: &LineIndex) -> bool {
    let first_line = index.line_column(token.text_range().start()).line;
    let Some(line_start) = index.offset(LineColumn {
        line: first_line,
        column: 0,
    }) else {
        return false;
    };
    let mut current = token.prev_token();
    while let Some(previous) = current {
        if previous.text_range().end() <= line_start {
            break;
        }
        if !is_comment_or_whitespace(previous.kind()) {
            return true;
        }
        current = previous.prev_token();
    }
    let last_line = index.line_column(token.text_range().end()).line;
    let mut current = token.next_token();
    while let Some(next) = current {
        if index.line_column(next.text_range().start()).line > last_line {
            break;
        }
        if !is_comment_or_whitespace(next.kind()) {
            return true;
        }
        current = next.next_token();
    }
    false
}

/// The trivia kinds that do not count as code for placement.
fn is_comment_or_whitespace(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Whitespace
            | SyntaxKind::LineComment
            | SyntaxKind::BlockComment
            | SyntaxKind::DocComment
    )
}

/// The node a docblock annotates: the exact inverse of
/// `celerrate_syntax::ast::docblock_token` — the next sibling element
/// past whitespace, when it is a node. Anything else (an orphan
/// docblock at the end of the file, a token neighbor) answers `None`
/// and the caller falls back to the line-based scope.
fn annotated_node_range(token: &SyntaxToken) -> Option<TextRange> {
    let mut current = token.next_sibling_or_token();
    while let Some(element) = current {
        if let Some(node) = element.as_node() {
            return Some(node.text_range());
        }
        let next = element.as_token()?;
        if next.kind() != SyntaxKind::Whitespace {
            return None;
        }
        current = element.next_sibling_or_token();
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use celerrate_db::testing::TestDatabase;
    use celerrate_source::{FileId, TextSize};
    use salsa::Setter as _;

    #[derive(Debug)]
    struct FakeProvider;

    impl CommentDirectiveProvider for FakeProvider {
        fn directives(&self, kind: CommentKind, text: &str) -> Vec<CommentDirective> {
            let mut directives = Vec::new();
            if text.contains("@fake") && kind == CommentKind::Line {
                directives.push(CommentDirective::suppress(
                    DirectiveScope::CurrentLine,
                    DirectiveOrigin::Foreign,
                    vec![SuppressionIdentifier::unmapped(
                        "fake.identifier".to_owned(),
                    )],
                ));
            }
            for (marker, scope) in [
                ("@line", DirectiveScope::CurrentLine),
                ("@next", DirectiveScope::NextLine),
                ("@both", DirectiveScope::CurrentAndNextLine),
                ("@declaration", DirectiveScope::AnnotatedDeclaration),
                ("@trailing", DirectiveScope::TrailingOrNextLine),
            ] {
                if text.contains(marker) {
                    directives.push(CommentDirective::suppress(
                        scope,
                        DirectiveOrigin::Foreign,
                        Vec::new(),
                    ));
                }
            }
            directives
        }
    }

    fn identity(name: &str) -> crate::PluginIdentity {
        crate::PluginIdentity {
            name: name.to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        }
    }

    fn fixture(source: &str) -> (TestDatabase, celerrate_db::SourceFile) {
        let db = TestDatabase::default();
        let _ = CommentDirectiveRegistry::builder(vec![CommentDirectiveRegistration {
            identity: identity("fake"),
            provider: std::sync::Arc::new(FakeProvider),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&db);
        let file = celerrate_db::SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
        (db, file)
    }

    fn offset_of(source: &str, needle: &str) -> TextSize {
        TextSize::from(u32::try_from(source.find(needle).unwrap()).unwrap())
    }

    fn suppressed_at(
        db: &TestDatabase,
        file: celerrate_db::SourceFile,
        source: &str,
        needle: &str,
    ) -> bool {
        is_suppressed(
            suppressed_ranges(db, file),
            offset_of(source, needle),
            TextSize::of(source),
        )
    }

    #[test]
    fn an_unset_registry_is_the_no_plugin_path() {
        let db = TestDatabase::default();
        assert!(CommentDirectiveRegistry::try_get(&db).is_none());
    }

    #[test]
    fn a_registered_provider_answers_through_the_registry() {
        let db = TestDatabase::default();
        let _ = CommentDirectiveRegistry::builder(vec![CommentDirectiveRegistration {
            identity: identity("fake"),
            provider: std::sync::Arc::new(FakeProvider),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&db);

        let registry = CommentDirectiveRegistry::try_get(&db).unwrap();
        let registrations = registry.registrations(&db);
        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0].identity.name, "fake");
        assert_eq!(
            registrations[0]
                .provider
                .directives(CommentKind::Line, "// @fake"),
            vec![CommentDirective::suppress(
                DirectiveScope::CurrentLine,
                DirectiveOrigin::Foreign,
                vec![SuppressionIdentifier::unmapped(
                    "fake.identifier".to_owned()
                )],
            )],
        );
        assert!(
            registrations[0]
                .provider
                .directives(CommentKind::Docblock, "/** @fake */")
                .is_empty(),
            "the fake only answers line comments: the kind travels",
        );
        assert!(
            registrations[0]
                .provider
                .directives(CommentKind::Line, "// plain prose")
                .is_empty(),
        );
    }

    #[test]
    fn without_a_registry_nothing_is_suppressed() {
        let db = TestDatabase::default();
        let source = "<?php\n$x = 1; // @line\n";
        let file = celerrate_db::SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
        assert!(suppressed_ranges(&db, file).is_empty());
    }

    #[test]
    fn a_current_line_directive_covers_its_whole_line_and_only_it() {
        let source = "<?php\n$x = 1; // @line\n$y = 2;\n";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "$x"));
        assert!(!suppressed_at(&db, file, source, "$y"));
        assert!(!suppressed_at(&db, file, source, "<?php"));
    }

    #[test]
    fn a_current_line_directive_in_a_multi_line_comment_covers_all_its_lines() {
        let source = "<?php\n$before = 1;\n/* @line\n   spans two lines */\n$after = 1;\n";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "/* @line"));
        assert!(suppressed_at(&db, file, source, "spans two"));
        assert!(!suppressed_at(&db, file, source, "$before"));
        assert!(!suppressed_at(&db, file, source, "$after"));
    }

    #[test]
    fn a_next_line_directive_covers_the_line_below_and_only_it() {
        let source = "<?php\n// @next\n$x = 1;\n$y = 2;\n";
        let (db, file) = fixture(source);
        assert!(!suppressed_at(&db, file, source, "// @next"));
        assert!(suppressed_at(&db, file, source, "$x"));
        assert!(!suppressed_at(&db, file, source, "$y"));
    }

    #[test]
    fn a_both_lines_directive_covers_its_line_and_the_next() {
        let source = "<?php\n$x = 1; // @both\n$y = 2;\n$z = 3;\n";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "$x"));
        assert!(suppressed_at(&db, file, source, "$y"));
        assert!(!suppressed_at(&db, file, source, "$z"));
    }

    #[test]
    fn a_docblock_directive_covers_the_annotated_declaration_whole() {
        let source = "<?php\n/** @declaration */\nclass Service {\n    public function boot() { $inside = 1; }\n}\n$outside = 1;\n";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "$inside"));
        assert!(!suppressed_at(&db, file, source, "$outside"));
    }

    #[test]
    fn an_orphan_docblock_falls_back_to_its_own_and_the_next_line() {
        let source = "<?php\n$x = 1;\n/** @declaration */";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "@declaration"));
        assert!(!suppressed_at(&db, file, source, "$x"));
    }

    #[test]
    fn a_next_line_directive_on_the_last_line_suppresses_nothing() {
        let source = "<?php\n$x = 1; // @next";
        let (db, file) = fixture(source);
        assert!(suppressed_ranges(&db, file).is_empty());
    }

    #[test]
    fn an_end_of_file_anchor_is_suppressible_from_the_last_line() {
        // Decision 5's exception: a diagnostic anchored exactly at the
        // text's end (an unexpected-end-of-file parse error) belongs
        // to the last line.
        let source = "<?php\n$x = 1; // @line";
        let (db, file) = fixture(source);
        assert!(is_suppressed(
            suppressed_ranges(&db, file),
            TextSize::of(source),
            TextSize::of(source),
        ));
    }

    #[test]
    fn the_suppress_constructor_is_field_faithful() {
        let directive = CommentDirective::suppress(
            DirectiveScope::NextLine,
            DirectiveOrigin::Foreign,
            vec![SuppressionIdentifier::unmapped(
                "method.notFound".to_owned(),
            )],
        );
        assert_eq!(
            directive,
            CommentDirective::Suppress {
                scope: DirectiveScope::NextLine,
                origin: DirectiveOrigin::Foreign,
                identifiers: vec![SuppressionIdentifier::Unmapped {
                    written: "method.notFound".to_owned(),
                }],
            },
        );
    }

    #[test]
    fn each_identifier_variant_answers_its_written_form() {
        for (identifier, expected) in [
            (
                SuppressionIdentifier::mapped("a.b".to_owned(), vec!["CEL0030".to_owned()]),
                "a.b",
            ),
            (SuppressionIdentifier::scope_wide("all".to_owned()), "all"),
            (SuppressionIdentifier::unmapped("a.b".to_owned()), "a.b"),
            (
                SuppressionIdentifier::native("CEL0030".to_owned()),
                "CEL0030",
            ),
        ] {
            assert_eq!(identifier.written(), expected);
        }
    }

    #[test]
    fn a_trailing_directive_behind_code_covers_its_own_line() {
        let source = "<?php\n$x = 1; // @trailing\n$y = 2;\n";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "$x"));
        assert!(!suppressed_at(&db, file, source, "$y"));
    }

    #[test]
    fn a_trailing_directive_alone_on_its_line_covers_the_next_line_only() {
        let source = "<?php\n// @trailing\n$x = 1;\n$y = 2;\n";
        let (db, file) = fixture(source);
        assert!(!suppressed_at(&db, file, source, "// @trailing"));
        assert!(suppressed_at(&db, file, source, "$x"));
        assert!(!suppressed_at(&db, file, source, "$y"));
    }

    #[test]
    fn a_preceding_comment_is_not_code_for_placement_resolution() {
        // Only trivia precedes the directive on its line: it stands alone
        // and targets the next line.
        let source = "<?php\n/* note */ // @trailing\n$x = 1;\n$y = 2;\n";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "$x"));
        assert!(!suppressed_at(&db, file, source, "$y"));
    }

    #[test]
    fn an_open_tag_counts_as_code_for_placement_resolution() {
        let source = "<?php // @trailing\n$x = 1;\n";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "<?php"));
        assert!(!suppressed_at(&db, file, source, "$x"));
    }

    #[test]
    fn code_following_the_comment_on_its_line_makes_it_trailing() {
        // A block comment can lead its statement: code after the comment
        // on the comment's last line is adjacency too (decision 6).
        let source = "<?php\n/* @trailing */ $x = 1;\n$y = 2;\n";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "$x"));
        assert!(!suppressed_at(&db, file, source, "$y"));
    }

    #[test]
    fn a_trailing_directive_alone_on_the_last_line_still_resolves() {
        // The next line does not exist: the scope degenerates to the empty
        // end-of-file range (decision 6). Nothing on an ordinary line is
        // suppressed, but the directive survives resolution - task 8's
        // reporting rules must see it - and through the end-of-file
        // exception it covers exactly the end-of-file position, the same
        // coverage the empty final line of a newline-terminated file gets.
        let source = "<?php\n$x = 1;\n// @trailing";
        let (db, file) = fixture(source);
        assert!(!suppressed_at(&db, file, source, "$x"));
        let end = TextSize::of(source);
        let ranges = suppressed_ranges(&db, file);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges.first().copied(), Some(TextRange::empty(end)));
    }

    #[test]
    fn identical_resolved_ranges_deduplicate() {
        let source = "<?php\n$x = 1; // @line @fake\n";
        let (db, file) = fixture(source);
        assert_eq!(suppressed_ranges(&db, file).len(), 1);
    }

    #[salsa::tracked]
    fn suppression_count(db: &dyn salsa::Database, file: celerrate_db::SourceFile) -> usize {
        suppressed_ranges(db, file).len()
    }

    #[test]
    fn a_prose_comment_edit_backdates_the_suppression_set() {
        let source = "<?php\n$x = 1; // @line\n// prose\n";
        let (mut db, file) = fixture(source);
        assert_eq!(suppression_count(&db, file), 1);
        db.take_executed();

        file.set_bytes(&mut db)
            .to(b"<?php\n$x = 1; // @line\n// edited prose\n".to_vec());
        assert_eq!(suppression_count(&db, file), 1);

        let executed = db.take_executed();
        assert!(
            executed
                .iter()
                .any(|query| query.contains("suppressed_ranges")),
            "the own-tree read re-runs on any edit: {executed:?}",
        );
        assert!(
            !executed
                .iter()
                .any(|query| query.contains("suppression_count")),
            "an identical set backdates: the consumer never re-ran: {executed:?}",
        );
    }
}
