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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use celerrate_db::testing::TestDatabase;

    #[derive(Debug)]
    struct FakeProvider;

    impl CommentDirectiveProvider for FakeProvider {
        fn directives(&self, kind: CommentKind, text: &str) -> Vec<CommentDirective> {
            if text.contains("@fake") && kind == CommentKind::Line {
                vec![CommentDirective::Suppress {
                    scope: DirectiveScope::CurrentLine,
                    identifiers: vec!["fake.identifier".to_owned()],
                }]
            } else {
                Vec::new()
            }
        }
    }

    fn identity(name: &str) -> crate::PluginIdentity {
        crate::PluginIdentity {
            name: name.to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        }
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
}
