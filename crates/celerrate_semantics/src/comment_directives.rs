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

use std::sync::Arc;

use celerrate_db::SourceFile;
use celerrate_source::{LineColumn, LineIndex, TextRange, TextSize};
use celerrate_syntax::{SyntaxKind, SyntaxToken};

use crate::plugin::PluginIdentity;

/// The comment shapes a provider may be handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentKind {
    /// `//` and `#` comments.
    Line,
    /// `/* ... */` comments.
    Block,
    /// `/** ... */` docblocks.
    Docblock,
}

/// Where a directive applies, relative to the comment that carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
}

/// One structured directive a comment carries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CommentDirective {
    /// Extinguish every diagnostic family on the scope. The
    /// identifiers are the foreign diagnostic names the written form
    /// carried (`@phpstan-ignore method.notFound`), carried for the
    /// rule framework's identifier-level correspondence, never matched
    /// here (design section 5).
    Suppress {
        scope: DirectiveScope,
        identifiers: Vec<String>,
    },
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
                directives.push(CommentDirective::Suppress {
                    scope: DirectiveScope::CurrentLine,
                    identifiers: vec!["fake.identifier".to_owned()],
                });
            }
            for (marker, scope) in [
                ("@line", DirectiveScope::CurrentLine),
                ("@next", DirectiveScope::NextLine),
                ("@both", DirectiveScope::CurrentAndNextLine),
                ("@declaration", DirectiveScope::AnnotatedDeclaration),
            ] {
                if text.contains(marker) {
                    directives.push(CommentDirective::Suppress {
                        scope,
                        identifiers: Vec::new(),
                    });
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
            vec![CommentDirective::Suppress {
                scope: DirectiveScope::CurrentLine,
                identifiers: vec!["fake.identifier".to_owned()],
            }],
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
