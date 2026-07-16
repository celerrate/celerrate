//! Shared test-only fixture: a deliberately tiny docblock notation
//! understanding `@template`, `@extends`/`@implements`, `@return`, and
//! `@param` tags. Originally `inheritance.rs`'s own test fake; lifted
//! here so `declared.rs`'s task-4 tests (an inherited signature must
//! see the same template scoping `ancestor_arguments` threads) can
//! drive the exact same notation without duplicating it. Recorded
//! debt: the crate has no other shared test-support module, mirroring
//! the semantic-core crates' own duplicated test fakes.

use std::sync::Arc;

use celerrate_db::testing::TestDatabase;
use celerrate_semantics::PluginIdentity;

use crate::representation::TypeId;
use crate::type_syntax::{
    AnnotationSite, ParsedAncestor, ParsedAnnotations, ParsedTemplate, TypeSyntax,
    TypeSyntaxRegistration, TypeSyntaxRegistry,
};

/// A deliberately tiny notation for these tests, one tag per docblock
/// line: `@template NAME`, `@extends NAME<ARG, ...>`, `@implements
/// NAME<ARG, ...>`, `@return NAME`, `@param NAME $variable` or `@param
/// NAME<ARG, ...> $variable`. An `ARG`, a `@return` target, or a
/// `@param` type that names a template declared in the same docblock
/// lowers to that template; anything else lowers to a class qualified
/// at the site and folded — a `@param` head with `<ARG, ...>` lowers
/// to that class carrying its arguments (`TypeId::class`'s
/// `class_arguments`), the one shape in this notation that can hand a
/// receiver its own class-level arguments.
pub(crate) struct FakeSyntax;

impl FakeSyntax {
    /// Normalizes a docblock, single-line or multi-line, into one
    /// plain-text line per tag: strips the outer `/**` / `*/`
    /// delimiters and each line's leading `*`. `docblock.lines()` alone
    /// only copes with the multi-line convention (each line already
    /// starting with `* `); a one-line docblock like `/** @extends
    /// Base<User> */` needs the outer markers peeled first or no tag is
    /// ever recognized.
    fn docblock_lines(docblock: &str) -> Vec<String> {
        let text = docblock.trim();
        let text = text.strip_prefix("/**").unwrap_or(text);
        let text = text.strip_suffix("*/").unwrap_or(text);
        text.lines()
            .map(|line| line.trim().trim_start_matches('*').trim().to_owned())
            .filter(|line| !line.is_empty())
            .collect()
    }

    fn lower_name<'db>(
        site: &AnnotationSite<'db, '_>,
        templates: &[String],
        written: &str,
    ) -> TypeId<'db> {
        let db = site.database();
        if templates.iter().any(|name| name == written) {
            return TypeId::template(db, site.declaring_scope(), written, TypeId::mixed(db));
        }
        let qualified = site.qualify_class_name(written).to_lowercase();
        TypeId::class(db, &qualified, vec![])
    }

    /// Splits one `@template` tag's content into its declared name and,
    /// when present, its bound: `T of Bound` (the form the bridge's own
    /// `TemplateDeclaration` recognizes, see
    /// `celerrate_phpdoc_bridge::tags::TemplateDeclaration`) becomes
    /// `("T", Some("Bound"))`; a boundless `T` becomes `("T", None)`.
    fn split_template_declaration(rest: &str) -> (String, Option<String>) {
        let rest = rest.trim();
        match rest.split_once(" of ") {
            Some((name, bound)) => (name.trim().to_owned(), Some(bound.trim().to_owned())),
            None => (rest.to_owned(), None),
        }
    }

    /// The declared names only (bound stripped) of every `@template`
    /// tag in `docblock`, in declaration order — the scope tests need
    /// to decide whether a written name refers to a template rather
    /// than a class.
    fn template_names_in(docblock: &str) -> Vec<String> {
        Self::docblock_lines(docblock)
            .iter()
            .filter_map(|line| line.strip_prefix("@template "))
            .map(|rest| Self::split_template_declaration(rest).0)
            .collect()
    }

    /// A written name at a member's declaring site: a template declared
    /// on the enclosing class (not the member's own `@template` list,
    /// which this fake does not carry) lowers class-scoped, exactly the
    /// scope `ancestor_substitution` fixes against; anything else falls
    /// through the ordinary class-or-own-template rule.
    fn lower_member_name<'db>(
        site: &AnnotationSite<'db, '_>,
        own_templates: &[String],
        written: &str,
    ) -> TypeId<'db> {
        let db = site.database();
        let enclosing: Vec<String> = site
            .enclosing_class_docblock()
            .map(Self::template_names_in)
            .unwrap_or_default();
        let scope = site.enclosing_class_scope().unwrap_or("");
        if enclosing.iter().any(|name| name == written) {
            TypeId::template(db, scope, written, TypeId::mixed(db))
        } else {
            Self::lower_name(site, own_templates, written)
        }
    }

    /// A `@param` type text, generic-argument aware: `NAME<ARG, ...>`
    /// reuses the same `split_once('<')` / `strip_suffix('>')` shape
    /// `@extends`/`@implements` already parse (see
    /// `parse_docblock`'s `@extends`/`@implements` arm), so a receiver
    /// carrying `class_arguments` is actually expressible from a
    /// `@param` tag — otherwise `member_boundary_type`'s
    /// class-argument-binding branch has no fixture that can drive
    /// it. A bare name falls through to the ordinary
    /// class-or-own-template rule.
    fn lower_param_type<'db>(
        site: &AnnotationSite<'db, '_>,
        own_templates: &[String],
        written: &str,
    ) -> TypeId<'db> {
        let db = site.database();
        if let Some((head, rest)) = written.split_once('<')
            && let Some(arguments_text) = rest.strip_suffix('>')
        {
            let arguments: Vec<TypeId<'db>> = arguments_text
                .split(',')
                .map(|argument| Self::lower_member_name(site, own_templates, argument.trim()))
                .collect();
            let qualified = site.qualify_class_name(head.trim()).to_lowercase();
            return TypeId::class(db, &qualified, arguments);
        }
        Self::lower_member_name(site, own_templates, written)
    }
}

impl TypeSyntax for FakeSyntax {
    fn can_parse(&self, docblock: &str) -> bool {
        docblock.contains('@')
    }

    fn parse_docblock<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        docblock: &str,
    ) -> ParsedAnnotations<'db> {
        let lines = Self::docblock_lines(docblock);
        let template_declarations: Vec<(String, Option<String>)> = lines
            .iter()
            .filter_map(|line| line.strip_prefix("@template "))
            .map(Self::split_template_declaration)
            .collect();
        let template_names: Vec<String> = template_declarations
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let mut parsed = ParsedAnnotations::default();
        for (name, bound_text) in &template_declarations {
            // A bound is itself an ordinary written name: it lowers
            // through the same class-or-template rule as any
            // `@extends`/`@implements` argument or `@return` value.
            let bound = bound_text
                .as_deref()
                .map(|written| Self::lower_name(site, &template_names, written));
            parsed.templates.push(ParsedTemplate {
                name: name.clone(),
                bound,
            });
        }
        for line in &lines {
            let line = line.as_str();
            let tag_content = line
                .strip_prefix("@extends ")
                .or_else(|| line.strip_prefix("@implements "));
            if let Some(content) = tag_content
                && let Some((head, rest)) = content.trim().split_once('<')
                && let Some(arguments_text) = rest.strip_suffix('>')
            {
                let arguments: Vec<TypeId<'db>> = arguments_text
                    .split(',')
                    .map(|argument| Self::lower_name(site, &template_names, argument.trim()))
                    .collect();
                let qualified = site.qualify_class_name(head.trim()).to_lowercase();
                parsed.ancestors.push(ParsedAncestor {
                    class_name: qualified,
                    arguments,
                });
            }
            if let Some(written) = line.strip_prefix("@return ") {
                // Class templates come into scope through the enclosing
                // class docblock, like the bridge does.
                parsed.return_type = Some(Self::lower_member_name(
                    site,
                    &template_names,
                    written.trim(),
                ));
            }
            if let Some(rest) = line.strip_prefix("@param ") {
                // `@param NAME $variable` or `@param NAME<ARG, ...>
                // $variable`: the variable is always the last
                // whitespace-separated token, so splitting on the
                // *last* space keeps a comma-separated argument
                // list's internal spaces intact — plain
                // `split_whitespace` would tear `Box<int, string>`
                // apart.
                if let Some((type_text, variable)) = rest.trim().rsplit_once(' ') {
                    let parameter_type =
                        Self::lower_param_type(site, &template_names, type_text.trim());
                    let parameter_name = variable.trim_start_matches('$').to_owned();
                    parsed.parameters.push((parameter_name, parameter_type));
                }
            }
        }
        parsed
    }

    fn parse_type_expression<'db>(
        &self,
        _site: &AnnotationSite<'db, '_>,
        _expression: &str,
    ) -> Option<TypeId<'db>> {
        None
    }
}

fn fake_identity(name: &str) -> PluginIdentity {
    PluginIdentity {
        name: name.to_owned(),
        version: "0.0.0".to_owned(),
        configuration: String::new(),
    }
}

/// Registers `FakeSyntax` exactly the way `inference.rs`'s own
/// `register_fake_assertions` registers its fake: same registration
/// struct, same HIGH durability, one implementation.
pub(crate) fn register_fake_syntax(db: &TestDatabase) {
    let _ = TypeSyntaxRegistry::builder(vec![TypeSyntaxRegistration {
        identity: fake_identity("fake-inheritance"),
        implementation: Arc::new(FakeSyntax),
    }])
    .durability(salsa::Durability::HIGH)
    .new(db);
}
