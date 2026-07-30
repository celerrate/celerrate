//! The comment-directive extension point: structured directives read
//! from comment trivia — today, suppressions ("extinguish every
//! diagnostic family on this scope").
//!
//! Owned by this crate: the registry input lives with
//! the consuming layer, implementations are registered at the
//! composition root, `celerrate_plugin` re-exports the vocabulary.
//! The vocabulary (what a directive *is*) belongs to this trait; the
//! written tag table (what `@phpstan-ignore-line` *means*) is
//! bridge-internal, like the tag precedence table.
//! Scopes are symbolic — a provider is a pure function of the comment
//! and cannot see positions; `suppression_directives` resolves them.
//! Identifier-level correspondence is resolved here too: `filter_of`
//! is the single implementation of the correspondence policy, closing
//! the reservation this module used to carry.

use std::sync::Arc;

use celerrate_db::SourceFile;
use celerrate_diagnostics::DiagnosticId;
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
/// `celerrate_diagnostics::find_identifier` (the facade grows no
/// identifier vocabulary for this).
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

/// What a directive's identifier list resolved to: the matcher input,
/// a filter per range, `All` or `Only(sorted codes)`; co-location
/// merges by union semantically, because a diagnostic is suppressed
/// exactly when any directive admits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuppressionFilter {
    /// Every diagnostic family on the scope.
    All,
    /// Exactly these identifiers, sorted and deduplicated (binary
    /// search relies on the order).
    Only(Vec<DiagnosticId>),
}

/// One directive, resolved against the file: where it sits (the
/// carrying comment token - where CEL0041/CEL0042 anchor), what it
/// covers, what it admits, and what the reporting rules need to speak
/// about it. The reason trailer is deliberately not carried: its only
/// consumer (a verbose widened-directive channel) is not part of the
/// product surface yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDirective {
    pub anchor: TextRange,
    pub scope: TextRange,
    pub filter: SuppressionFilter,
    /// The written identifiers, verbatim, in written order.
    pub identifiers: Vec<String>,
    pub origin: DirectiveOrigin,
}

impl ResolvedDirective {
    /// Whether this directive admits (suppresses) a diagnostic of `id`
    /// anchored at `offset`. Position matching is by the diagnostic's
    /// start, end-exclusive, except at the very end of the file: a
    /// diagnostic anchored exactly at the text's end (an
    /// unexpected-end-of-file parse error) belongs to the last line
    /// and must be suppressible from it (the rule `is_suppressed`
    /// carried, preserved verbatim).
    pub fn admits(&self, id: DiagnosticId, offset: TextSize, text_end: TextSize) -> bool {
        let in_scope = offset >= self.scope.start()
            && (offset < self.scope.end()
                || (offset == self.scope.end() && self.scope.end() == text_end));
        if !in_scope {
            return false;
        }
        match &self.filter {
            SuppressionFilter::All => true,
            SuppressionFilter::Only(codes) => codes.binary_search(&id).is_ok(),
        }
    }
}

/// The single implementation of the correspondence policy (design
/// section 8, fixed by the #58 triage). Foreign: a bare list, any
/// scope-wide or unmapped identifier, or a mapped code that fails
/// interning widens to `All` - over-suppression, never
/// under-suppression (the correspondence gate makes the failed-intern
/// arm unreachable; it is the honest fallback, not a code path).
/// Native: the union of the identifiers that intern; unknown ones are
/// excluded and never widen (they suppress nothing - CEL0041's reason
/// to exist).
pub(crate) fn filter_of(
    origin: DirectiveOrigin,
    identifiers: &[SuppressionIdentifier],
) -> SuppressionFilter {
    let mut codes: Vec<DiagnosticId> = Vec::new();
    match origin {
        DirectiveOrigin::Foreign => {
            if identifiers.is_empty() {
                return SuppressionFilter::All;
            }
            for identifier in identifiers {
                match identifier {
                    SuppressionIdentifier::Mapped { codes: mapped, .. } => {
                        // An empty mapped set is malformed input from
                        // a non-bridge provider (the bridge's unit
                        // tests pin non-empty entries): widen, never
                        // narrow.
                        if mapped.is_empty() {
                            return SuppressionFilter::All;
                        }
                        for code in mapped {
                            match celerrate_diagnostics::find_identifier(code) {
                                Some(id) => codes.push(id),
                                None => return SuppressionFilter::All,
                            }
                        }
                    }
                    _ => return SuppressionFilter::All,
                }
            }
        }
        DirectiveOrigin::Native => {
            // Invariant: a native provider must emit only
            // `SuppressionIdentifier::Native` identifiers. Any other
            // variant here contributes nothing to `codes` and does not
            // widen to `All` (unlike the foreign arm above), so a
            // native provider that ever emitted a `Mapped` or
            // `Unmapped` identifier would silently under-suppress -
            // the one failure direction this project treats as a
            // defect.
            for identifier in identifiers {
                if let SuppressionIdentifier::Native { written } = identifier
                    && let Some(id) = celerrate_diagnostics::find_identifier(written)
                {
                    codes.push(id);
                }
            }
        }
    }
    codes.sort();
    codes.dedup();
    SuppressionFilter::Only(codes)
}

/// The file's directives, resolved: every comment handed to every
/// registered provider, symbolic scopes resolved against the line
/// index, filters computed under the correspondence policy, sorted and
/// deduplicated. An own-tree read for strictly-local output.
/// `Eq`-comparable: a comment edit that leaves the directive set
/// unchanged backdates, and dependents never re-run.
#[salsa::tracked(returns(ref))]
pub fn suppression_directives(
    db: &dyn salsa::Database,
    file: SourceFile,
) -> Vec<ResolvedDirective> {
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
    let mut directives = Vec::new();
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
                    CommentDirective::Suppress {
                        scope,
                        origin,
                        identifiers,
                    } => {
                        let Some(resolved) = resolve_scope(scope, token, index, text_end) else {
                            continue;
                        };
                        directives.push(ResolvedDirective {
                            anchor: token.text_range(),
                            scope: resolved,
                            filter: filter_of(origin, &identifiers),
                            identifiers: identifiers
                                .iter()
                                .map(|identifier| identifier.written().to_owned())
                                .collect(),
                            origin,
                        });
                    }
                }
            }
        }
    }
    directives.sort_by(|left, right| {
        (
            left.anchor.start(),
            left.anchor.end(),
            left.scope.start(),
            left.scope.end(),
        )
            .cmp(&(
                right.anchor.start(),
                right.anchor.end(),
                right.scope.start(),
                right.scope.end(),
            ))
            .then_with(|| format!("{left:?}").cmp(&format!("{right:?}")))
    });
    directives.dedup();
    directives
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
                // file gets.
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
        let directives = suppression_directives(db, file);
        let offset = offset_of(source, needle);
        let text_end = TextSize::of(source);
        // The test marker directives carry no mapped identifier, so any
        // registered identifier probes the position logic.
        let id = celerrate_diagnostics::find_identifier("CEL0018").unwrap();
        directives
            .iter()
            .any(|directive| directive.admits(id, offset, text_end))
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
        assert!(suppression_directives(&db, file).is_empty());
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
        // The `NextLine` arm of `resolve_scope` returns `None` when the
        // next line does not exist, so no directive survives resolution
        // at all: this is the property the test pins, not merely that
        // `$x` happens not to be covered.
        assert!(suppression_directives(&db, file).is_empty());
        assert!(!suppressed_at(&db, file, source, "$x"));
    }

    #[test]
    fn an_end_of_file_anchor_is_suppressible_from_the_last_line() {
        // An exception: a diagnostic anchored exactly at the
        // text's end (an unexpected-end-of-file parse error) belongs
        // to the last line. `suppressed_at` is needle-based and cannot
        // name a position past the last character, so this probes
        // `admits` directly at `text_end`, the same primitive the
        // helper calls.
        let source = "<?php\n$x = 1; // @line";
        let (db, file) = fixture(source);
        let directives = suppression_directives(&db, file);
        let end = TextSize::of(source);
        let id = celerrate_diagnostics::find_identifier("CEL0018").unwrap();
        assert!(
            directives
                .iter()
                .any(|directive| directive.admits(id, end, end))
        );
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
        // on the comment's last line is adjacency too.
        let source = "<?php\n/* @trailing */ $x = 1;\n$y = 2;\n";
        let (db, file) = fixture(source);
        assert!(suppressed_at(&db, file, source, "$x"));
        assert!(!suppressed_at(&db, file, source, "$y"));
    }

    #[test]
    fn a_trailing_directive_alone_on_the_last_line_still_resolves() {
        // The next line does not exist: the scope degenerates to the empty
        // end-of-file range. Nothing on an ordinary line is
        // suppressed, but the directive survives resolution - the
        // reporting rules must see it - and through the end-of-file
        // exception it covers exactly the end-of-file position, the same
        // coverage the empty final line of a newline-terminated file gets.
        let source = "<?php\n$x = 1;\n// @trailing";
        let (db, file) = fixture(source);
        assert!(!suppressed_at(&db, file, source, "$x"));
        let end = TextSize::of(source);
        let directives = suppression_directives(&db, file);
        assert_eq!(directives.len(), 1);
        assert_eq!(
            directives.first().map(|directive| directive.scope),
            Some(TextRange::empty(end))
        );
    }

    #[test]
    fn identical_resolved_ranges_deduplicate() {
        let db = TestDatabase::default();
        let _ = CommentDirectiveRegistry::builder(vec![
            CommentDirectiveRegistration {
                identity: identity("fake-a"),
                provider: std::sync::Arc::new(FakeProvider),
            },
            CommentDirectiveRegistration {
                identity: identity("fake-b"),
                provider: std::sync::Arc::new(FakeProvider),
            },
        ])
        .durability(salsa::Durability::HIGH)
        .new(&db);
        let source = "<?php\n$x = 1; // @line\n";
        let file = celerrate_db::SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
        assert_eq!(suppression_directives(&db, file).len(), 1);
    }

    #[salsa::tracked]
    fn suppression_count(db: &dyn salsa::Database, file: celerrate_db::SourceFile) -> usize {
        suppression_directives(db, file).len()
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
                .any(|query| query.contains("suppression_directives")),
            "the own-tree read re-runs on any edit: {executed:?}",
        );
        assert!(
            !executed
                .iter()
                .any(|query| query.contains("suppression_count")),
            "an identical set backdates: the consumer never re-ran: {executed:?}",
        );
    }

    #[test]
    fn a_foreign_directive_with_only_mapped_identifiers_narrows_to_their_union() {
        let identifiers = vec![
            SuppressionIdentifier::mapped(
                "arguments.count".to_owned(),
                vec!["CEL0036".to_owned(), "CEL0037".to_owned()],
            ),
            SuppressionIdentifier::mapped("class.notFound".to_owned(), vec!["CEL0018".to_owned()]),
        ];
        let filter = filter_of(DirectiveOrigin::Foreign, &identifiers);
        assert_eq!(
            filter,
            SuppressionFilter::Only(vec![
                celerrate_diagnostics::find_identifier("CEL0018").unwrap(),
                celerrate_diagnostics::find_identifier("CEL0036").unwrap(),
                celerrate_diagnostics::find_identifier("CEL0037").unwrap(),
            ]),
        );
    }

    #[test]
    fn a_bare_foreign_directive_suppresses_the_whole_scope() {
        assert_eq!(
            filter_of(DirectiveOrigin::Foreign, &[]),
            SuppressionFilter::All
        );
    }

    #[test]
    fn any_unmapped_foreign_identifier_widens_to_the_whole_scope() {
        let identifiers = vec![
            SuppressionIdentifier::mapped("class.notFound".to_owned(), vec!["CEL0018".to_owned()]),
            SuppressionIdentifier::unmapped("something.else".to_owned()),
        ];
        assert_eq!(
            filter_of(DirectiveOrigin::Foreign, &identifiers),
            SuppressionFilter::All
        );
    }

    #[test]
    fn an_explicit_scope_wide_identifier_widens_to_the_whole_scope() {
        let identifiers = vec![SuppressionIdentifier::scope_wide("all".to_owned())];
        assert_eq!(
            filter_of(DirectiveOrigin::Foreign, &identifiers),
            SuppressionFilter::All
        );
    }

    #[test]
    fn a_mapped_identifier_with_no_codes_widens_to_the_whole_scope() {
        // Constructible through the public facade constructor even though
        // the bridge's table never produces it (a bridge unit test pins
        // non-empty code sets): malformed provider input widens, never
        // narrows to Only(empty) - the global fallback direction.
        let identifiers = vec![SuppressionIdentifier::mapped(
            "odd.entry".to_owned(),
            Vec::new(),
        )];
        assert_eq!(
            filter_of(DirectiveOrigin::Foreign, &identifiers),
            SuppressionFilter::All
        );
    }

    #[test]
    fn a_native_directive_unions_its_known_identifiers_and_drops_unknown_ones() {
        let identifiers = vec![
            SuppressionIdentifier::native("CEL0030".to_owned()),
            SuppressionIdentifier::native("CEL9999".to_owned()),
            SuppressionIdentifier::native("CEL0018".to_owned()),
        ];
        assert_eq!(
            filter_of(DirectiveOrigin::Native, &identifiers),
            SuppressionFilter::Only(vec![
                celerrate_diagnostics::find_identifier("CEL0018").unwrap(),
                celerrate_diagnostics::find_identifier("CEL0030").unwrap(),
            ]),
        );
    }

    #[test]
    fn a_bare_native_directive_suppresses_nothing() {
        assert_eq!(
            filter_of(DirectiveOrigin::Native, &[]),
            SuppressionFilter::Only(Vec::new()),
        );
    }

    #[test]
    fn an_only_filter_admits_exactly_its_codes_on_its_scope() {
        let directive = ResolvedDirective {
            anchor: TextRange::new(TextSize::from(10), TextSize::from(30)),
            scope: TextRange::new(TextSize::from(0), TextSize::from(31)),
            filter: SuppressionFilter::Only(vec![
                celerrate_diagnostics::find_identifier("CEL0018").unwrap(),
            ]),
            identifiers: vec!["class.notFound".to_owned()],
            origin: DirectiveOrigin::Foreign,
        };
        let text_end = TextSize::from(100);
        let inside = TextSize::from(5);
        let outside = TextSize::from(50);
        let cel0018 = celerrate_diagnostics::find_identifier("CEL0018").unwrap();
        let cel0019 = celerrate_diagnostics::find_identifier("CEL0019").unwrap();
        assert!(directive.admits(cel0018, inside, text_end));
        assert!(!directive.admits(cel0019, inside, text_end));
        assert!(!directive.admits(cel0018, outside, text_end));
    }

    #[test]
    fn the_end_of_file_exception_survives_in_admits() {
        let end = TextSize::from(20);
        let directive = ResolvedDirective {
            anchor: TextRange::new(TextSize::from(8), TextSize::from(20)),
            scope: TextRange::new(TextSize::from(6), end),
            filter: SuppressionFilter::All,
            identifiers: Vec::new(),
            origin: DirectiveOrigin::Foreign,
        };
        let cel0007 = celerrate_diagnostics::find_identifier("CEL0007").unwrap();
        assert!(directive.admits(cel0007, end, end));
        assert!(!directive.admits(cel0007, end, TextSize::from(40)));
    }

    #[test]
    fn an_empty_scope_at_the_end_of_file_admits_only_the_end_position() {
        // The degenerate last-line scope: the end-of-file
        // exception is its whole coverage.
        let end = TextSize::from(20);
        let directive = ResolvedDirective {
            anchor: TextRange::new(TextSize::from(8), TextSize::from(20)),
            scope: TextRange::empty(end),
            filter: SuppressionFilter::All,
            identifiers: Vec::new(),
            origin: DirectiveOrigin::Native,
        };
        let cel0007 = celerrate_diagnostics::find_identifier("CEL0007").unwrap();
        assert!(directive.admits(cel0007, end, end));
        assert!(!directive.admits(cel0007, TextSize::from(10), end));
    }

    #[test]
    fn the_query_resolves_anchor_scope_and_origin_per_directive() {
        // `@fake` carries one written identifier (`fake.identifier`), so
        // this doubles as the query-level pin for `ResolvedDirective`'s
        // `identifiers` field: the exact written strings, in written
        // order.
        let source = "<?php\n$x = 1; // @fake\n";
        let (db, file) = fixture(source);
        let directives = suppression_directives(&db, file);
        assert_eq!(directives.len(), 1);
        let directive = &directives[0];
        assert_eq!(directive.origin, DirectiveOrigin::Foreign);
        assert_eq!(directive.filter, SuppressionFilter::All);
        assert_eq!(directive.identifiers, vec!["fake.identifier".to_owned()]);
        let comment_start = offset_of(source, "// @fake");
        assert_eq!(directive.anchor.start(), comment_start);
    }
}
