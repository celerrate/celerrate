//! The call-site template solver (generics
//! inference-only): structural constraint collection over
//! (declared parameter, argument) pairs, least-upper-bound
//! resolution, and the bound-then-mixed fallback — never the
//! first-seen constituent, which would leak wrong member sets into
//! the unknown-members family. This module's whole contract:
//! it is structural and never guesses, and it never emits a
//! generic-mismatch diagnostic — a failed constraint simply
//! contributes nothing, silently.

use std::collections::BTreeMap;

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::ClassQuery;
use celerrate_stubs::StubIndexInput;

use crate::representation::{StringConstraint, TypeData, TypeId};
use crate::substitution::Substitution;

/// Solves the template variables constrained by `pairs`. Multiple
/// constraints on one variable take `TypeId::union` (the lattice
/// least upper bound); a variable no pair constrains is simply
/// absent from the map — `finalize_return` owns its fallback.
pub(crate) fn solve<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    pairs: &[(TypeId<'db>, TypeId<'db>)],
) -> Substitution<'db> {
    let mut constraints: BTreeMap<(String, String), Vec<TypeId<'db>>> = BTreeMap::new();
    for (declared, argument) in pairs {
        collect(
            db,
            files,
            stubs,
            configuration,
            *declared,
            *argument,
            &mut constraints,
        );
    }
    let mut map = Substitution::default();
    for ((scope, name), collected) in constraints {
        map.bind(&scope, &name, TypeId::union(db, collected));
    }
    map
}

/// Matches `declared` against `argument` structurally, pushing every
/// `Template` binding site it finds into `constraints`. Recurses only
/// into `declared`'s own structural children (already bounded by the
/// depth cap every constructor enforces), so this terminates in
/// lockstep with `declared`'s finite tree — it never grows a value or
/// re-derives `argument`'s shape. Any shape this does not recognize
/// (or an argument that does not match the declared shape) simply
/// contributes nothing: silence, never a guess.
///
/// Every arm is pinned in `inference.rs`'s test module: the `Array`
/// arm (against both an `Array` and a `Shape` argument), `Callable`,
/// `Union`, and `Intersection` arms were production code with no test
/// since they were first written, because the shared fake type syntax
/// (`test_support::FakeSyntax`) parsed none of `array<K, V>`, `|`,
/// `&`, or `callable(...)`. That fake's grammar was later extended
/// (additively — every previously supported form still lowers exactly
/// as before) and mutation-verified each arm. The declared-`Shape` arm
/// arrived later (issue #40): the earlier wording, "the `Shape` arm",
/// named only the `Array` arm's shape-argument side, and the
/// "shapes recurse element-wise" clause was half-implemented until the
/// declared side landed with its own fake form (`array{key: TYPE}`)
/// and pin.
fn collect<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    declared: TypeId<'db>,
    argument: TypeId<'db>,
    constraints: &mut BTreeMap<(String, String), Vec<TypeId<'db>>>,
) {
    let recurse =
        |declared: TypeId<'db>,
         argument: TypeId<'db>,
         constraints: &mut BTreeMap<(String, String), Vec<TypeId<'db>>>| {
            collect(
                db,
                files,
                stubs,
                configuration,
                declared,
                argument,
                constraints,
            );
        };
    match declared.data(db) {
        TypeData::Template { scope, name, .. } => {
            constraints
                .entry((scope.clone(), name.clone()))
                .or_default()
                .push(argument);
        }
        TypeData::ClassString {
            argument: Some(inner),
        } => {
            // Binds through a string literal, `Foo::class` (typed
            // `class-string<Foo>`), or any other `class-string` value.
            let extracted = match argument.data(db) {
                TypeData::String {
                    constraint: StringConstraint::Literal(text),
                } => Some(TypeId::class(db, text, vec![])),
                TypeData::ClassString {
                    argument: Some(carried),
                } => Some(*carried),
                _ => None,
            };
            if let Some(extracted) = extracted {
                recurse(*inner, extracted, constraints);
            }
        }
        TypeData::Class {
            name: declared_name,
            arguments: declared_arguments,
        } => match argument.data(db) {
            TypeData::Class { name, arguments } if name == declared_name => {
                for (left, right) in declared_arguments.iter().zip(arguments.iter()) {
                    recurse(*left, *right, constraints);
                }
            }
            TypeData::Class { name, .. } => {
                // The argument is a subclass (or otherwise unrelated
                // name): its threaded arguments for the declared
                // ancestor constrain, when the ancestry actually
                // reaches it — an unrelated class contributes nothing.
                let class = ClassQuery::new(db, name.clone());
                let threaded =
                    crate::inheritance::ancestor_arguments(db, files, stubs, configuration, class)
                        .iter()
                        .find(|(ancestor, _)| ancestor == declared_name)
                        .map(|(_, fixed)| fixed.clone());
                if let Some(fixed) = threaded {
                    for (left, right) in declared_arguments.iter().zip(fixed.iter()) {
                        recurse(*left, *right, constraints);
                    }
                }
            }
            _ => {}
        },
        TypeData::Array { key, value, .. } => match argument.data(db) {
            TypeData::Array {
                key: argument_key,
                value: argument_value,
                ..
            } => {
                recurse(*key, *argument_key, constraints);
                recurse(*value, *argument_value, constraints);
            }
            TypeData::Shape { .. } => {
                // `key-of`/`value-of` evaluate shapes eagerly.
                recurse(*key, TypeId::key_of(db, argument), constraints);
                recurse(*value, TypeId::value_of(db, argument), constraints);
            }
            _ => {}
        },
        TypeData::Shape { fields } => {
            // The shape clause (issue #40): field-wise, each
            // declared field matching the argument field with the same
            // key. A key the argument lacks, or a non-shape argument,
            // contributes nothing.
            if let TypeData::Shape {
                fields: argument_fields,
            } = argument.data(db)
            {
                for field in fields {
                    let matching = argument_fields
                        .iter()
                        .find(|candidate| candidate.key == field.key);
                    if let Some(matching) = matching {
                        recurse(field.value, matching.value, constraints);
                    }
                }
            }
        }
        TypeData::Callable { return_type, .. } => {
            if let TypeData::Callable {
                return_type: argument_return,
                ..
            } = argument.data(db)
            {
                recurse(*return_type, *argument_return, constraints);
            }
        }
        TypeData::Union { constituents } => {
            for constituent in constituents {
                recurse(*constituent, argument, constraints);
            }
        }
        TypeData::Intersection { intersectands } => {
            for intersectand in intersectands {
                recurse(*intersectand, argument, constraints);
            }
        }
        _ => {}
    }
}

/// A call result is concrete: any template the solver left unbound
/// substitutes to its bound, then `mixed` — this module's fallback,
/// never a first-seen constituent. Placeholders pass through
/// untouched (they belong to `member_boundary_type`).
pub(crate) fn finalize_return<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    of: TypeId<'db>,
) -> TypeId<'db> {
    if !crate::substitution::contains_symbolic(db, of) {
        return of;
    }
    let mut map = Substitution::default();
    collect_remaining_templates(db, files, stubs, configuration, of, &mut map);
    crate::substitution::substitute(db, files, stubs, configuration, of, &map, None)
}

/// Every `Template` still reachable inside `of` binds to its own
/// bound — a boundless template's bound is already `mixed`
/// (`TypeId::template`'s constructor), so bound-then-mixed is one
/// move, not two. Exhaustive over every `TypeData` variant, no
/// wildcard: a lattice form added later must be triaged here
/// explicitly rather than silently falling through unresolved.
///
/// A bound can itself name another template (legal Psalm/PHPStan
/// notation, e.g. `@template TKey of Collection<TValue>`), so the
/// `Template` arm recurses into `bound` first — registering that
/// nested template's own fallback in the same map — and only then
/// resolves `bound` against the accumulated map before storing it.
/// Storing the *resolved* value, not the raw `bound`, matters because
/// `substitute`'s `Template` arm returns a stored replacement verbatim
/// rather than re-substituting it (`substitution.rs`): if `bound`
/// still carried a live nested template when stored, the final
/// `substitute` call in `finalize_return` would hand it back
/// unresolved, breaking this module's own "a call result is concrete"
/// contract. This recursion always terminates: a bound must already be
/// a fully interned `TypeId` before the template that names it can be
/// constructed (constructors take their bound by value), so bounds
/// form a DAG, never a cycle.
fn collect_remaining_templates<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    of: TypeId<'db>,
    map: &mut Substitution<'db>,
) {
    let recurse = |child: TypeId<'db>, map: &mut Substitution<'db>| {
        collect_remaining_templates(db, files, stubs, configuration, child, map);
    };
    match of.data(db) {
        TypeData::Template { scope, name, bound } => {
            recurse(*bound, map);
            let resolved =
                crate::substitution::substitute(db, files, stubs, configuration, *bound, map, None);
            map.bind(scope, name, resolved);
        }
        TypeData::Mixed
        | TypeData::Never
        | TypeData::Void
        | TypeData::Null
        | TypeData::Object
        | TypeData::Resource
        | TypeData::Bool { .. }
        | TypeData::Int { .. }
        | TypeData::Float { .. }
        | TypeData::String { .. }
        | TypeData::EnumCase { .. }
        | TypeData::SelfPlaceholder
        | TypeData::ParentPlaceholder
        | TypeData::StaticPlaceholder => {}
        TypeData::Union { constituents } => {
            for child in constituents {
                recurse(*child, map);
            }
        }
        TypeData::Intersection { intersectands } => {
            for child in intersectands {
                recurse(*child, map);
            }
        }
        TypeData::Array { key, value, .. } => {
            recurse(*key, map);
            recurse(*value, map);
        }
        TypeData::Shape { fields } => {
            for field in fields {
                recurse(field.value, map);
            }
        }
        TypeData::ClassString { argument } => {
            if let Some(child) = argument {
                recurse(*child, map);
            }
        }
        TypeData::Class { arguments, .. } => {
            for child in arguments {
                recurse(*child, map);
            }
        }
        TypeData::Callable {
            parameters,
            return_type,
        } => {
            for parameter in parameters {
                recurse(parameter.parameter_type, map);
            }
            recurse(*return_type, map);
        }
        TypeData::KeyOf { subject } | TypeData::ValueOf { subject } => {
            recurse(*subject, map);
        }
        TypeData::Conditional {
            subject,
            matches,
            then_branch,
            otherwise_branch,
            ..
        } => {
            recurse(*subject, map);
            recurse(*matches, map);
            recurse(*then_branch, map);
            recurse(*otherwise_branch, map);
        }
    }
}
