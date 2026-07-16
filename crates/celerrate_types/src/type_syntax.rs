//! The type-syntax extension point: understand an annotation
//! notation. Owned by this crate per the design; the registry input
//! lives here too, or the DAG would break upward. Dispatch rule,
//! fixed now: implementations are consulted in registered order with
//! a can-parse protocol, first win — registration order is declared
//! at the composition root and therefore deterministic.

use std::sync::Arc;

use celerrate_semantics::PluginIdentity;

use crate::declared::NameSite;
use crate::representation::TypeId;

/// Assertion tag polarity: always asserts, or only when the condition
/// is true or false (design section 5, plan 5 consumer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum AssertionPolarity {
    Always,
    IfTrue,
    IfFalse,
}

/// Carried assertion from a docblock, lowered and ready for plan 5's
/// narrowing consumer. The subject travels verbatim; the negation
/// applies to the asserted type.
#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct ParsedAssertion<'db> {
    /// The asserted subject, verbatim (`$value`, `$this->prop`):
    /// interpretation is plan 5's.
    pub subject: String,
    pub asserted: TypeId<'db>,
    pub polarity: AssertionPolarity,
    pub negated: bool,
}

/// One `@template` declaration of the parsed docblock, in declaration
/// order (task 3's ancestor-argument zip relies on this order).
#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct ParsedTemplate<'db> {
    pub name: String,
    pub bound: Option<TypeId<'db>>,
}

/// One inheritance-position declaration: the ancestor's fully
/// qualified, pre-folded class name and its fixed generic arguments.
#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct ParsedAncestor<'db> {
    pub class_name: String,
    pub arguments: Vec<TypeId<'db>>,
}

/// The declaring-scope context one annotation parse needs beyond name
/// qualification: `@template` resolution needs to know the scope key
/// its own declarations bind under, and — for member docblocks — the
/// enclosing class-like's own scope key and docblock text, so
/// class-level `@template` declarations are visible while parsing a
/// member's annotations. The scope-key convention (`<class
/// key>::<member key>` or a function key) is `TypeId::template`'s.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnnotationContext<'a> {
    /// The declaring symbol's own scope key.
    pub declaring_scope: &'a str,
    /// The enclosing class-like's scope key, when the declaring site
    /// is a member.
    pub enclosing_class_scope: Option<&'a str>,
    /// The enclosing class-like's own docblock text, when the
    /// declaring site is a member.
    pub enclosing_class_docblock: Option<&'a str>,
}

/// A name-resolution and construction context for one annotation
/// parse, scoped to the declaring site. Handles are call-scoped:
/// implementations never retain the site, the database, or any
/// `TypeId` beyond the call (the WASM projection will enforce this
/// structurally; the native tier enforces it by review).
pub struct AnnotationSite<'db, 'site> {
    db: &'db dyn salsa::Database,
    site: &'site NameSite<'site>,
    context: AnnotationContext<'site>,
}

impl<'db, 'site> AnnotationSite<'db, 'site> {
    // Constructed only by this module's dispatch functions.
    pub(crate) fn new(
        db: &'db dyn salsa::Database,
        site: &'site NameSite<'site>,
        context: AnnotationContext<'site>,
    ) -> Self {
        Self { db, site, context }
    }

    /// The database, for `TypeId` builders. Never retain it.
    pub fn database(&self) -> &'db dyn salsa::Database {
        self.db
    }

    /// The native keyword table (`int`, `string`, `self`, `static`,
    /// `iterable`, ...), shared with native signature lowering so the
    /// two paths can never disagree. `None` means ordinary class name.
    pub fn keyword_type(&self, name: &str) -> Option<TypeId<'db>> {
        crate::declared::lower_keyword(self.db, name)
    }

    /// Qualifies a written class name at the declaring site
    /// (namespace and `use` imports), returning the fully qualified
    /// name — feed it to `TypeId::class`.
    pub fn qualify_class_name(&self, written: &str) -> String {
        crate::declared::qualified_class_name(self.site, written)
    }

    /// The declaring symbol's own scope key: `TypeId::template`'s
    /// scope-key convention. Call-scoped, never retained.
    pub fn declaring_scope(&self) -> &'site str {
        self.context.declaring_scope
    }

    /// The enclosing class-like's scope key, when the declaring site
    /// is a member. Call-scoped, never retained.
    pub fn enclosing_class_scope(&self) -> Option<&'site str> {
        self.context.enclosing_class_scope
    }

    /// The enclosing class-like's own docblock text, when the
    /// declaring site is a member. Call-scoped, never retained.
    pub fn enclosing_class_docblock(&self) -> Option<&'site str> {
        self.context.enclosing_class_docblock
    }
}

/// One docblock, parsed: the annotation layer a member or function
/// declares. `return_type` and `value_type` are both carried; the
/// consumer picks by subject kind (decision 5 of the plan header).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedAnnotations<'db> {
    /// `@return`.
    pub return_type: Option<TypeId<'db>>,
    /// `@var`.
    pub value_type: Option<TypeId<'db>>,
    /// `@param`, by parameter name (without the `$`).
    pub parameters: Vec<(String, TypeId<'db>)>,
    /// `@throws`, accumulated across tags.
    pub throws: Vec<TypeId<'db>>,
    /// `@assert` family, accumulated across tags and carried for plan 5's narrowing.
    pub assertions: Vec<ParsedAssertion<'db>>,
    /// `@template` declarations, in declaration order.
    pub templates: Vec<ParsedTemplate<'db>>,
    /// `@extends`/`@implements`/`@use` (and their `@template-*` long
    /// forms): each ancestor's fully qualified name and fixed generic
    /// arguments.
    pub ancestors: Vec<ParsedAncestor<'db>>,
    /// Named inline `@var Type $name` entries, by variable name
    /// (without the `$`).
    pub variables: Vec<(String, TypeId<'db>)>,
}

/// An implementation understands one annotation notation. Must be a
/// deterministic pure function of its arguments; contributions are
/// consumed through deterministic dispatch. Object-safe by design
/// (lifetime-generic methods only), per the design's WASM projection
/// constraint.
pub trait TypeSyntax: Send + Sync {
    /// The can-parse protocol: consulted in registered order, the
    /// first implementation answering `true` wins the docblock.
    fn can_parse(&self, docblock: &str) -> bool;
    /// Parse one docblock into the annotation layer. A construct the
    /// notation cannot express degrades that element to absent, never
    /// the whole docblock (loss is per construct).
    fn parse_docblock<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        docblock: &str,
    ) -> ParsedAnnotations<'db>;
    /// Parse one bare type expression (virtual-member payloads).
    /// Dispatch: registered order, first `Some` wins.
    fn parse_type_expression<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        expression: &str,
    ) -> Option<TypeId<'db>>;
}

/// One registration: the implementation travels with its identity,
/// so reading it records the dependency an upgrade invalidates.
#[derive(Clone)]
pub struct TypeSyntaxRegistration {
    pub identity: PluginIdentity,
    pub implementation: Arc<dyn TypeSyntax>,
}

impl std::fmt::Debug for TypeSyntaxRegistration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TypeSyntaxRegistration")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// Set once per process at the composition root, HIGH durability,
/// never mutated. Unset (every plain test database): the no-plugin
/// path — annotations answer the default.
#[salsa::input(singleton)]
pub struct TypeSyntaxRegistry {
    #[returns(ref)]
    pub registrations: Vec<TypeSyntaxRegistration>,
}

/// Registered order, can-parse first win. Wired into the annotation
/// seam (`declared::member_annotations`); the virtual-member payload
/// path is a later task.
pub(crate) fn annotations_for_docblock<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    context: &AnnotationContext<'_>,
    docblock: &str,
) -> ParsedAnnotations<'db> {
    let Some(registry) = TypeSyntaxRegistry::try_get(db) else {
        return ParsedAnnotations::default();
    };
    let annotation_site = AnnotationSite::new(db, site, *context);
    for registration in registry.registrations(db) {
        if registration.implementation.can_parse(docblock) {
            return registration
                .implementation
                .parse_docblock(&annotation_site, docblock);
        }
    }
    ParsedAnnotations::default()
}

/// Registered order, first `Some` wins. Wired into the virtual-member
/// payload path (`declared::declared_member_signature`'s `Virtual`
/// arm).
pub(crate) fn type_of_expression<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    context: &AnnotationContext<'_>,
    expression: &str,
) -> Option<TypeId<'db>> {
    let registry = TypeSyntaxRegistry::try_get(db)?;
    let annotation_site = AnnotationSite::new(db, site, *context);
    for registration in registry.registrations(db) {
        if let Some(answer) = registration
            .implementation
            .parse_type_expression(&annotation_site, expression)
        {
            return Some(answer);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_semantics::PluginIdentity;
    use celerrate_source::FileId;

    use super::{
        AnnotationContext, AnnotationSite, ParsedAnnotations, TypeSyntax, TypeSyntaxRegistration,
        TypeSyntaxRegistry,
    };
    use crate::declared::NameSite;
    use crate::representation::TypeId;
    use crate::type_syntax::{annotations_for_docblock, type_of_expression};

    struct Fixture {
        db: TestDatabase,
    }

    fn fixture(sources: &[&str]) -> Fixture {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let _files = AnalyzedFileSet::new(&db, handles);
        Fixture { db }
    }

    fn identity(name: &str) -> PluginIdentity {
        PluginIdentity {
            name: name.to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        }
    }

    #[derive(Debug)]
    struct FakeSyntax {
        accepts: &'static str,
        answer_int_return: bool,
    }

    impl TypeSyntax for FakeSyntax {
        fn can_parse(&self, docblock: &str) -> bool {
            docblock.contains(self.accepts)
        }
        fn parse_docblock<'db>(
            &self,
            site: &AnnotationSite<'db, '_>,
            _docblock: &str,
        ) -> ParsedAnnotations<'db> {
            let db = site.database();
            ParsedAnnotations {
                return_type: self.answer_int_return.then(|| TypeId::int(db)),
                ..ParsedAnnotations::default()
            }
        }
        fn parse_type_expression<'db>(
            &self,
            site: &AnnotationSite<'db, '_>,
            expression: &str,
        ) -> Option<TypeId<'db>> {
            (expression == "int").then(|| TypeId::int(site.database()))
        }
    }

    #[test]
    fn dispatch_is_registered_order_with_can_parse_first_win() {
        let fixture = fixture(&["<?php class C {}"]);
        let db = &fixture.db;
        let _ = TypeSyntaxRegistry::builder(vec![
            TypeSyntaxRegistration {
                identity: identity("first"),
                implementation: std::sync::Arc::new(FakeSyntax {
                    accepts: "@return",
                    answer_int_return: true,
                }),
            },
            TypeSyntaxRegistration {
                identity: identity("second"),
                implementation: std::sync::Arc::new(FakeSyntax {
                    accepts: "@",
                    answer_int_return: false,
                }),
            },
        ])
        .durability(salsa::Durability::HIGH)
        .new(db);

        // Both can parse this: the first registered wins.
        let parsed = annotations_for_docblock(
            db,
            &NameSite::Global,
            &AnnotationContext::default(),
            "/** @return int */",
        );
        assert_eq!(parsed.return_type, Some(TypeId::int(db)));
        // Only the second can parse this: first win falls through.
        let parsed = annotations_for_docblock(
            db,
            &NameSite::Global,
            &AnnotationContext::default(),
            "/** @var string */",
        );
        assert_eq!(parsed, ParsedAnnotations::default());
        // No implementation can parse: the default.
        let parsed = annotations_for_docblock(
            db,
            &NameSite::Global,
            &AnnotationContext::default(),
            "/** prose */",
        );
        assert_eq!(parsed, ParsedAnnotations::default());
    }

    #[test]
    fn the_first_matching_implementations_answer_stands_with_no_fall_through() {
        // Both implementations can parse the docblock; the first
        // registered wins outright, even though its answer is the
        // default and the second implementation would have answered
        // something else. Dispatch never falls through past a winner.
        let fixture = fixture(&["<?php class C {}"]);
        let db = &fixture.db;
        let _ = TypeSyntaxRegistry::builder(vec![
            TypeSyntaxRegistration {
                identity: identity("first"),
                implementation: std::sync::Arc::new(FakeSyntax {
                    accepts: "@",
                    answer_int_return: false,
                }),
            },
            TypeSyntaxRegistration {
                identity: identity("second"),
                implementation: std::sync::Arc::new(FakeSyntax {
                    accepts: "@",
                    answer_int_return: true,
                }),
            },
        ])
        .durability(salsa::Durability::HIGH)
        .new(db);

        let parsed = annotations_for_docblock(
            db,
            &NameSite::Global,
            &AnnotationContext::default(),
            "/** @return int */",
        );
        assert_eq!(parsed, ParsedAnnotations::default());
    }

    #[test]
    fn an_unset_registry_answers_the_default() {
        let fixture = fixture(&["<?php class C {}"]);
        let parsed = annotations_for_docblock(
            &fixture.db,
            &NameSite::Global,
            &AnnotationContext::default(),
            "/** @return int */",
        );
        assert_eq!(parsed, ParsedAnnotations::default());
        assert_eq!(
            type_of_expression(
                &fixture.db,
                &NameSite::Global,
                &AnnotationContext::default(),
                "int",
            ),
            None,
        );
    }

    #[test]
    fn expression_dispatch_takes_the_first_some() {
        let fixture = fixture(&["<?php class C {}"]);
        let db = &fixture.db;
        let _ = TypeSyntaxRegistry::builder(vec![TypeSyntaxRegistration {
            identity: identity("fake"),
            implementation: std::sync::Arc::new(FakeSyntax {
                accepts: "@",
                answer_int_return: false,
            }),
        }])
        .durability(salsa::Durability::HIGH)
        .new(db);
        assert_eq!(
            type_of_expression(db, &NameSite::Global, &AnnotationContext::default(), "int"),
            Some(TypeId::int(db))
        );
        assert_eq!(
            type_of_expression(
                db,
                &NameSite::Global,
                &AnnotationContext::default(),
                "garbage!!",
            ),
            None,
        );
    }

    #[test]
    fn the_annotation_site_shares_the_native_keyword_table_and_the_site_qualifier() {
        let fixture = fixture(&["<?php class C {}"]);
        let db = &fixture.db;
        let site = AnnotationSite::new(db, &NameSite::Global, AnnotationContext::default());
        assert_eq!(site.keyword_type("int"), Some(TypeId::int(db)));
        assert_eq!(
            site.keyword_type("static"),
            Some(TypeId::static_placeholder(db))
        );
        assert_eq!(site.keyword_type("NotAKeyword"), None);
        assert_eq!(site.qualify_class_name("\\App\\User"), "App\\User");
    }

    #[test]
    fn the_annotation_site_exposes_the_declaring_context() {
        let fixture = fixture(&["<?php class C {}"]);
        let context = AnnotationContext {
            declaring_scope: "c::find",
            enclosing_class_scope: Some("c"),
            enclosing_class_docblock: Some("/** @template T */"),
        };
        let site = AnnotationSite::new(&fixture.db, &NameSite::Global, context);
        assert_eq!(site.declaring_scope(), "c::find");
        assert_eq!(site.enclosing_class_scope(), Some("c"));
        assert_eq!(site.enclosing_class_docblock(), Some("/** @template T */"));
    }
}
