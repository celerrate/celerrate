//! The total lowering table (decision 3): every parsed construct maps
//! to a lattice value or a documented sound widening — a supertype,
//! never a subtype, so a widening can silence but never mis-report.
//!
//! | construct | lowering |
//! |---|---|
//! | names: native keywords | the shared keyword table (`AnnotationSite::keyword_type`) |
//! | `list`, `non-empty-list`, `non-empty-array`, `associative-array` | their builders over `mixed` |
//! | `non-empty-string`, `numeric-string`, `literal-string` | their builders |
//! | `class-string[<T>]` | `class_string` (the template argument is never severed) |
//! | `interface-string[<T>]`, `enum-string[<T>]`, `trait-string[<T>]` | `class_string` (kind refinement: recorded debt) |
//! | `callable-string` | `non-empty-string` (widening) |
//! | `lowercase-string`, `uppercase-string` | `string` (widening) |
//! | `non-falsy-string`, `truthy-string` | `non-empty-string` (widening) |
//! | `literal-int` | `int` (no literal-int marker: widening) |
//! | `positive-int`, `negative-int`, `non-negative-int`, `non-positive-int` | `int_range` |
//! | `int<a, b>` (`min`/`max` open ends) | `int_range`; a non-literal bound widens to `int` |
//! | `int-mask<...>`, `int-mask-of<...>` | `int` (widening) |
//! | `array-key` | `int\|string` |
//! | `scalar` | `bool\|int\|float\|string`; `numeric` | `int\|float\|numeric-string` |
//! | `double`/`integer`/`boolean` | the PHP aliases |
//! | `noreturn`, `no-return`, `never-return`, `never-returns` | `never` |
//! | `non-empty-mixed` | `mixed`; `open-resource`, `closed-resource` | `resource` |
//! | `pure-callable` | `mixed` (the bare-callable widening); `pure-Closure` | `Closure` |
//! | `callable-object` | `object` (widening) |
//! | literals | `int_literal`/`float_literal`/`string_literal` (an unparseable float text widens to `float`) |
//! | `array<K, V>`, `list<V>`, `iterable<K, V>` and the non-empty forms | their builders; wrong arity widens the slots to their defaults |
//! | `key-of<T>`, `value-of<T>` | their builders |
//! | a bare `*` generic argument (the bivariant wildcard) | already rewritten to `Name("mixed")` at the parser (`parser::parse_generic_arguments`); it lowers through the "names: native keywords" row above, never through a dedicated construct here |
//! | sealed shapes | `shape` (keyless tuple fields number sequentially; identifier keys are string keys) |
//! | unsealed shapes | the general array (`non_empty_array` when a field is required): key `int\|string`, value = the field-and-tail union (`mixed` for a bare `...`) — widening |
//! | `object{...}` | `object` (widening) |
//! | callables (`callable`, `Closure`, purity prefixes) | `callable` (purity and Closure classness drop: widening); callable-scoped template names lower to `mixed` (decision 12) |
//! | `Foo::BAR`, `Foo::*` | `mixed` (constant and enum-case facts arrive with plans 6-7: recorded debt) |
//! | `$this` | `static` (design section 3) |
//! | offset access `T[K]` | `mixed` (widening) |
//! | conditionals | `conditional` for an in-scope template subject (Task 9); otherwise the undecided branch union (design section 3) |
//! | a keyword or dialect atom with a spurious `<...>` list | the atom, arguments dropped |
//! | any other name | a class type, qualified at the declaring site |

use celerrate_plugin::{
    AnnotationSite, CallableParameter, ParsedAncestor, ShapeField, ShapeKey, TypeContext, TypeId,
};

use crate::expression::{ConditionalSubject, ShapeKeyExpression, TypeExpression, UnsealedTail};
use crate::tags::AncestorDeclaration;

/// The name-resolution scope one docblock lowers under: the docblock's
/// own declared template set (class-level, then own — task 9) and the
/// callable-scoped names active while lowering one callable signature.
#[derive(Debug, Default)]
pub(crate) struct LoweringScope<'db> {
    /// Declared template variables, resolved at declaration into
    /// their lattice value — later declarations shadow earlier ones.
    templates: Vec<(String, TypeId<'db>)>,
    callable_templates: Vec<String>,
}

impl<'db> LoweringScope<'db> {
    /// Declares one template variable, resolving it immediately into
    /// its lattice value (`TypeId::template`'s scope-key convention).
    /// A later declaration of the same name shadows an earlier one
    /// (member templates over class templates).
    pub(crate) fn declare_template(
        &mut self,
        context: TypeContext<'db>,
        scope_key: &str,
        name: String,
        bound: TypeId<'db>,
    ) {
        let resolved = context.template(scope_key, &name, bound);
        self.templates.push((name, resolved));
    }

    /// The most recently declared template of this name, or `None`
    /// when no such template is in scope.
    fn resolve_template(&self, name: &str) -> Option<TypeId<'db>> {
        self.templates
            .iter()
            .rev()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, resolved)| *resolved)
    }
}

pub(crate) fn lower<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope<'db>,
    expression: &TypeExpression,
) -> TypeId<'db> {
    let context = site.types();
    match expression {
        TypeExpression::Name(name) => lower_name(site, scope, name),
        TypeExpression::Nullable(inner) => {
            context.union([lower(site, scope, inner), context.null()])
        }
        TypeExpression::Union(parts) => {
            let mut lowered = Vec::with_capacity(parts.len());
            for part in parts {
                lowered.push(lower(site, scope, part));
            }
            context.union(lowered)
        }
        TypeExpression::Intersection(parts) => {
            let mut lowered = Vec::with_capacity(parts.len());
            for part in parts {
                lowered.push(lower(site, scope, part));
            }
            context.intersection(lowered)
        }
        TypeExpression::ArrayOf(element) => {
            context.array(array_key(context), lower(site, scope, element))
        }
        TypeExpression::IntLiteral(value) => context.int_literal(*value),
        TypeExpression::FloatLiteral(text) => text
            .parse::<f64>()
            .map(|value| context.float_literal(value))
            .unwrap_or_else(|_| context.float()),
        TypeExpression::StringLiteral(value) => context.string_literal(value),
        TypeExpression::Generic { base, arguments } => lower_generic(site, scope, base, arguments),
        TypeExpression::Shape {
            base,
            fields,
            unsealed,
        } => lower_shape(site, scope, base, fields, unsealed.as_ref()),
        TypeExpression::Callable {
            templates,
            parameters,
            return_type,
            ..
        } => lower_callable(site, scope, templates, parameters, return_type),
        TypeExpression::ConstFetch { .. } => context.mixed(),
        TypeExpression::This => context.static_placeholder(),
        TypeExpression::Offset { .. } => context.mixed(),
        TypeExpression::Conditional {
            subject,
            negated,
            target,
            then_branch,
            otherwise_branch,
        } => lower_conditional(
            site,
            scope,
            subject,
            *negated,
            target,
            then_branch,
            otherwise_branch,
        ),
    }
}

fn array_key<'db>(context: TypeContext<'db>) -> TypeId<'db> {
    context.union([context.int(), context.string()])
}

fn lower_name<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope<'db>,
    name: &str,
) -> TypeId<'db> {
    let context = site.types();
    if scope
        .callable_templates
        .iter()
        .any(|template| template == name)
    {
        return context.mixed();
    }
    // The docblock template set resolves here, before keywords: a
    // template name shadows a same-named keyword.
    if let Some(resolved) = scope.resolve_template(name) {
        return resolved;
    }
    if let Some(keyword) = site.keyword_type(name) {
        return keyword;
    }
    if let Some(dialect) = lower_dialect_name(context, name) {
        return dialect;
    }
    context.class(&site.qualify_class_name(name), Vec::new())
}

/// The dialect atom table, folded ASCII-case-insensitively like the
/// native keyword table. `None` means "an ordinary class name".
fn lower_dialect_name<'db>(context: TypeContext<'db>, name: &str) -> Option<TypeId<'db>> {
    let folded = name.to_ascii_lowercase();
    Some(match folded.as_str() {
        "list" => context.list(context.mixed()),
        "non-empty-list" => context.non_empty_list(context.mixed()),
        "non-empty-array" => context.non_empty_array(array_key(context), context.mixed()),
        "associative-array" => context.array(array_key(context), context.mixed()),
        "non-empty-string" => context.non_empty_string(),
        "numeric-string" => context.numeric_string(),
        "literal-string" => context.literal_string_type(),
        "class-string" | "interface-string" | "enum-string" | "trait-string" => {
            context.class_string(None)
        }
        "callable-string" => context.non_empty_string(),
        "lowercase-string" | "uppercase-string" => context.string(),
        "non-falsy-string" | "truthy-string" => context.non_empty_string(),
        "literal-int" => context.int(),
        "positive-int" => context.int_range(Some(1), None),
        "negative-int" => context.int_range(None, Some(-1)),
        "non-negative-int" => context.int_range(Some(0), None),
        "non-positive-int" => context.int_range(None, Some(0)),
        "array-key" => array_key(context),
        "scalar" => context.union([
            context.bool(),
            context.int(),
            context.float(),
            context.string(),
        ]),
        "numeric" => context.union([context.int(), context.float(), context.numeric_string()]),
        "double" => context.float(),
        "integer" => context.int(),
        "boolean" => context.bool(),
        "noreturn" | "no-return" | "never-return" | "never-returns" => context.never(),
        "non-empty-mixed" => context.mixed(),
        "open-resource" | "closed-resource" => context.resource(),
        "pure-callable" => context.mixed(),
        "pure-closure" => context.class("Closure", Vec::new()),
        "callable-object" => context.object(),
        _ => return None,
    })
}

fn lower_generic<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope<'db>,
    base: &str,
    arguments: &[TypeExpression],
) -> TypeId<'db> {
    let context = site.types();
    let folded = base.to_ascii_lowercase();
    // `int<a, b>` reads its bounds at the expression level: a lowered
    // bound would already have lost `min`/`max`.
    if folded == "int" {
        if let (Some(minimum), Some(maximum)) = (
            range_bound(arguments.first()),
            range_bound(arguments.get(1)),
        ) && arguments.len() == 2
        {
            return context.int_range(minimum, maximum);
        }
        return context.int();
    }
    if folded == "int-mask" || folded == "int-mask-of" {
        return context.int();
    }
    let mut lowered = Vec::with_capacity(arguments.len());
    for argument in arguments {
        lowered.push(lower(site, scope, argument));
    }
    match (folded.as_str(), lowered.as_slice()) {
        ("array", [value]) => context.array(array_key(context), *value),
        ("array", [key, value]) => context.array(*key, *value),
        ("array", _) => context.array(array_key(context), context.mixed()),
        ("non-empty-array", [value]) => context.non_empty_array(array_key(context), *value),
        ("non-empty-array", [key, value]) => context.non_empty_array(*key, *value),
        ("non-empty-array", _) => context.non_empty_array(array_key(context), context.mixed()),
        ("list", [value]) => context.list(*value),
        ("list", _) => context.list(context.mixed()),
        ("non-empty-list", [value]) => context.non_empty_list(*value),
        ("non-empty-list", _) => context.non_empty_list(context.mixed()),
        ("iterable", [value]) => context.iterable(context.mixed(), *value),
        ("iterable", [key, value]) => context.iterable(*key, *value),
        ("iterable", _) => context.iterable(context.mixed(), context.mixed()),
        ("class-string" | "interface-string" | "enum-string" | "trait-string", [argument]) => {
            context.class_string(Some(*argument))
        }
        ("class-string" | "interface-string" | "enum-string" | "trait-string", _) => {
            context.class_string(None)
        }
        ("key-of", [subject]) => context.key_of(*subject),
        ("value-of", [subject]) => context.value_of(*subject),
        ("key-of" | "value-of", _) => context.mixed(),
        _ => {
            // A template base drops its (spurious) argument list too:
            // a template variable is never itself generic.
            if let Some(resolved) = scope.resolve_template(base) {
                return resolved;
            }
            if site.keyword_type(base).is_some() || lower_dialect_name(context, base).is_some() {
                // A keyword or dialect atom with a spurious argument
                // list: the atom stands, the arguments drop.
                lower_name(site, scope, base)
            } else {
                context.class(&site.qualify_class_name(base), lowered)
            }
        }
    }
}

/// `int<a, b>` bounds: an integer literal, or `min`/`max` for an open
/// end. Anything else invalidates the range and the construct widens
/// to plain `int`.
fn range_bound(argument: Option<&TypeExpression>) -> Option<Option<i64>> {
    match argument? {
        TypeExpression::IntLiteral(value) => Some(Some(*value)),
        TypeExpression::Name(name)
            if name.eq_ignore_ascii_case("min") || name.eq_ignore_ascii_case("max") =>
        {
            Some(None)
        }
        _ => None,
    }
}

fn lower_shape<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope<'db>,
    base: &str,
    fields: &[crate::expression::ShapeFieldExpression],
    unsealed: Option<&UnsealedTail>,
) -> TypeId<'db> {
    let context = site.types();
    if base.eq_ignore_ascii_case("object") {
        return context.object();
    }
    if let Some(tail) = unsealed {
        let mut values = Vec::with_capacity(fields.len() + 1);
        for field in fields {
            values.push(lower(site, scope, &field.value));
        }
        let value = match tail.value.as_deref() {
            Some(tail_value) => {
                values.push(lower(site, scope, tail_value));
                context.union(values)
            }
            None => context.mixed(),
        };
        let key = array_key(context);
        return if fields.iter().any(|field| !field.optional) {
            context.non_empty_array(key, value)
        } else {
            context.array(key, value)
        };
    }
    let mut next_index: i64 = 0;
    let mut lowered_fields = Vec::with_capacity(fields.len());
    for field in fields {
        let key = match &field.key {
            Some(ShapeKeyExpression::Integer(value)) => {
                if *value >= next_index {
                    next_index = value.saturating_add(1);
                }
                ShapeKey::Integer(*value)
            }
            Some(ShapeKeyExpression::String(value)) => ShapeKey::String(value.clone()),
            Some(ShapeKeyExpression::Identifier(name)) => ShapeKey::String(name.clone()),
            None => {
                let key = ShapeKey::Integer(next_index);
                next_index = next_index.saturating_add(1);
                key
            }
        };
        lowered_fields.push(ShapeField {
            key,
            optional: field.optional,
            value: lower(site, scope, &field.value),
        });
    }
    context.shape(lowered_fields)
}

fn lower_callable<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope<'db>,
    templates: &[String],
    parameters: &[crate::expression::CallableParameterExpression],
    return_type: &TypeExpression,
) -> TypeId<'db> {
    let context = site.types();
    let before = scope.callable_templates.len();
    scope.callable_templates.extend(templates.iter().cloned());
    let mut lowered_parameters = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        lowered_parameters.push(CallableParameter {
            parameter_type: lower(site, scope, &parameter.parameter_type),
            optional: parameter.optional,
            variadic: parameter.variadic,
            by_reference: parameter.by_reference,
        });
    }
    let lowered_return = lower(site, scope, return_type);
    scope.callable_templates.truncate(before);
    context.callable(lowered_parameters, lowered_return)
}

#[allow(clippy::too_many_arguments)]
fn lower_conditional<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope<'db>,
    subject: &ConditionalSubject,
    negated: bool,
    target: &TypeExpression,
    then_branch: &TypeExpression,
    otherwise_branch: &TypeExpression,
) -> TypeId<'db> {
    let context = site.types();
    let then_lowered = lower(site, scope, then_branch);
    let otherwise_lowered = lower(site, scope, otherwise_branch);
    // An in-scope template subject resolves to `TypeId::conditional`.
    // For a parameter subject the undecided fallback is the branch
    // union permanently, not merely until a later plan: no
    // expression-to-template resolution exists at lowering time for a
    // parameter subject, and none is planned (decision 9, recorded
    // debt — the fallback is permanent-until-demanded). The same
    // fallback covers a template name not currently in scope (design
    // section 3).
    if let ConditionalSubject::Template(name) = subject
        && let Some(template) = scope.resolve_template(name)
    {
        let target_lowered = lower(site, scope, target);
        return context.conditional(
            template,
            target_lowered,
            then_lowered,
            otherwise_lowered,
            negated,
        );
    }
    context.union([then_lowered, otherwise_lowered])
}

/// Lowers one inheritance-position declaration through the scope, then
/// reads the head and fixed arguments back off the lowered `TypeId`
/// via its `class_name`/`class_arguments` accessors — `class_name`
/// arrives pre-folded because `lower` already qualified it at the
/// site. An expression that lowers to something that is not a class
/// type (a malformed ancestor tag) drops: per-construct loss.
pub(crate) fn lower_ancestor<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope<'db>,
    declaration: &AncestorDeclaration,
) -> Option<ParsedAncestor<'db>> {
    let context = site.types();
    let lowered = lower(site, scope, &declaration.expression);
    let class_name = context.class_name(lowered)?;
    let arguments = context.class_arguments(lowered);
    Some(ParsedAncestor::new(class_name, arguments))
}
