//! Shared test-only fixture: a deliberately tiny docblock notation
//! understanding `@template` (with an optional `of Bound`),
//! `@extends`/`@implements`, `@return` (including the conditional form
//! `(NAME is NAME ? NAME : NAME)`), and `@param` tags (including
//! `class-string<NAME>`). Originally `inheritance.rs`'s own test fake;
//! lifted here so `declared.rs`'s task-4 tests (an inherited signature
//! must see the same template scoping `ancestor_arguments` threads) can
//! drive the exact same notation without duplicating it, and extended
//! by task 8 for the call-site solver's tests (a class-string binder
//! and a conditional return, task 8's remaining two grammar arms — the
//! template bound was already there), and further extended by task 8's
//! review fix for a bound naming another template, `NAME<ARG, ...>`
//! (e.g. `@template TKey of Collection<TValue>`), and by task 9 for the
//! named inline `@var NAME $variable` / `@var NAME<ARG, ...> $variable`
//! form (constructor inference and inline `@var`'s tests), reusing
//! `@param`'s own generic-argument-aware type lowering. Recorded debt:
//! the crate has no other shared test-support module, mirroring the
//! semantic-core crates' own duplicated test fakes.

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

    /// `templates` carries each declared template's OWN parsed bound
    /// (task 8): a written name matching one answers a `Template`
    /// carrying that real bound, not a placeholder `mixed` — otherwise
    /// `an_unconstrained_template_falls_to_its_bound_then_mixed`'s
    /// bounded case could never observe anything but `mixed`.
    fn lower_name<'db>(
        site: &AnnotationSite<'db, '_>,
        templates: &[ParsedTemplate<'db>],
        written: &str,
    ) -> TypeId<'db> {
        let db = site.database();
        if let Some(template) = templates.iter().find(|template| template.name == written) {
            let bound = template.bound.unwrap_or_else(|| TypeId::mixed(db));
            return TypeId::template(db, site.declaring_scope(), written, bound);
        }
        // The native keyword table (task 8): a conditional return's
        // subject and branches are ordinary written names too, and
        // `int`/`string`/`bool` must lower to the scalar lattice
        // members, never to a class named `int`.
        if let Some(keyword) = site.keyword_type(written) {
            return keyword;
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
        own_templates: &[ParsedTemplate<'db>],
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

    /// A generic-argument-aware type text — shared by the `@param` and
    /// `@var` arms below (task 9 extends this fake's original
    /// `@param`-only helper to the named inline `@var` form, the same
    /// notation either tag writes): `NAME<ARG, ...>` reuses the same
    /// `split_once('<')` / `strip_suffix('>')` shape
    /// `@extends`/`@implements` already parse (see
    /// `parse_docblock`'s `@extends`/`@implements` arm), so a receiver
    /// carrying `class_arguments` is actually expressible from either
    /// tag — otherwise `member_boundary_type`'s class-argument-binding
    /// branch has no fixture that can drive it. A bare name falls
    /// through to the ordinary class-or-own-template rule.
    fn lower_generic_type<'db>(
        site: &AnnotationSite<'db, '_>,
        own_templates: &[ParsedTemplate<'db>],
        written: &str,
    ) -> TypeId<'db> {
        let db = site.database();
        // `class-string<NAME>` (task 8): the primary template binder,
        // never a class literally named `class-string` — checked
        // before the generic `NAME<ARG, ...>` arm below, which would
        // otherwise swallow it.
        if let Some(rest) = written.strip_prefix("class-string<")
            && let Some(inner) = rest.strip_suffix('>')
        {
            let argument = Self::lower_member_name(site, own_templates, inner.trim());
            return TypeId::class_string(db, Some(argument));
        }
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

    /// The conditional return form `(NAME is NAME ? NAME : NAME)`
    /// (task 8): `None` when `written` is not shaped like one, so the
    /// caller falls through to the ordinary class-or-template rule.
    /// Every position lowers through [`Self::lower_member_name`], so a
    /// template declared on the same docblock is recognized in any of
    /// the four slots.
    fn parse_conditional_return<'db>(
        site: &AnnotationSite<'db, '_>,
        own_templates: &[ParsedTemplate<'db>],
        written: &str,
    ) -> Option<TypeId<'db>> {
        let inner = written.strip_prefix('(')?.strip_suffix(')')?;
        let (subject_text, rest) = inner.split_once(" is ")?;
        let (matches_text, rest) = rest.split_once(" ? ")?;
        let (then_text, otherwise_text) = rest.split_once(" : ")?;
        let subject = Self::lower_member_name(site, own_templates, subject_text.trim());
        let matches = Self::lower_member_name(site, own_templates, matches_text.trim());
        let then_branch = Self::lower_member_name(site, own_templates, then_text.trim());
        let otherwise_branch = Self::lower_member_name(site, own_templates, otherwise_text.trim());
        Some(TypeId::conditional(
            site.database(),
            subject,
            matches,
            then_branch,
            otherwise_branch,
            false,
        ))
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
        let mut parsed = ParsedAnnotations::default();
        for (name, bound_text) in &template_declarations {
            // A bound is an ordinary written name OR `NAME<ARG, ...>`
            // (task 8's follow-up fix: a bound naming another template
            // declared earlier in the same docblock, e.g. `@template
            // TKey of Collection<TValue>`, is legal Psalm/PHPStan
            // notation and the solver must resolve it rather than leak
            // it). Each argument resolves through `lower_name` against
            // `parsed.templates` as built so far — declaration order
            // means an earlier `@template` is already pushed by the
            // time a later one's bound looks it up.
            let bound = bound_text.as_deref().map(|written| {
                if let Some((head, rest)) = written.split_once('<')
                    && let Some(arguments_text) = rest.strip_suffix('>')
                {
                    let arguments: Vec<TypeId<'db>> = arguments_text
                        .split(',')
                        .map(|argument| Self::lower_name(site, &parsed.templates, argument.trim()))
                        .collect();
                    let qualified = site.qualify_class_name(head.trim()).to_lowercase();
                    TypeId::class(site.database(), &qualified, arguments)
                } else {
                    site.keyword_type(written).unwrap_or_else(|| {
                        let qualified = site.qualify_class_name(written).to_lowercase();
                        TypeId::class(site.database(), &qualified, vec![])
                    })
                }
            });
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
                    .map(|argument| Self::lower_name(site, &parsed.templates, argument.trim()))
                    .collect();
                let qualified = site.qualify_class_name(head.trim()).to_lowercase();
                parsed.ancestors.push(ParsedAncestor {
                    class_name: qualified,
                    arguments,
                });
            }
            if let Some(written) = line.strip_prefix("@return ") {
                let written = written.trim();
                // The conditional form is checked first: it is shaped
                // unlike any ordinary written name, so it can never be
                // mistaken for one.
                parsed.return_type = Some(
                    Self::parse_conditional_return(site, &parsed.templates, written)
                        .unwrap_or_else(|| {
                            // Class templates come into scope through the
                            // enclosing class docblock, like the bridge does.
                            Self::lower_member_name(site, &parsed.templates, written)
                        }),
                );
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
                        Self::lower_generic_type(site, &parsed.templates, type_text.trim());
                    let parameter_name = variable.trim_start_matches('$').to_owned();
                    parsed.parameters.push((parameter_name, parameter_type));
                }
            }
            if let Some(rest) = line.strip_prefix("@var ") {
                // Task 9: the named inline `@var NAME $variable` or
                // `@var NAME<ARG, ...> $variable` form, feeding
                // `parsed.variables` — same last-space split as
                // `@param` (a comma-separated argument list may itself
                // contain spaces). A bare `@var NAME` with no `$name`
                // (a property- or unnamed-level `@var`) never matches
                // `rsplit_once`'s one-space requirement, or the `$`
                // check just below, so it contributes nothing here —
                // this fake has no other consumer for it.
                if let Some((type_text, variable)) = rest.trim().rsplit_once(' ')
                    && let Some(name) = variable.trim().strip_prefix('$')
                {
                    let variable_type =
                        Self::lower_generic_type(site, &parsed.templates, type_text.trim());
                    parsed.variables.push((name.to_owned(), variable_type));
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
