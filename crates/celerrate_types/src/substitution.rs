//! The structural substitution primitive (design section 6): template
//! maps solved at call sites, late-static-binding resolution against
//! an owner and a receiver, and conditional-type evaluation once a
//! subject becomes decidable. A plain function, not a query — every
//! caller is already inside one. The recursion mirrors
//! [`crate::widening::widened_literals`]: every composite rebuilds
//! through the capped constructors, so the structural depth cap
//! bounds every value this module can produce (decision 15).

// TEMPORARY: nothing in production code calls this module's items yet —
// Task 3 (the generic ancestry) is this plan's first consumer. Remove
// this allow once that task wires `substitute` in.
#![allow(dead_code)]

use std::collections::BTreeMap;

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::StubIndexInput;

use crate::judgments::{Proof, subtype_of};
use crate::representation::{CallableParameter, ShapeField, TypeData, TypeId};

/// A finite map from template variables — keyed by `(scope, name)` —
/// to their substituted types. `BTreeMap` for deterministic iteration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Substitution<'db> {
    bindings: BTreeMap<(String, String), TypeId<'db>>,
}

impl<'db> Substitution<'db> {
    pub(crate) fn bind(&mut self, scope: &str, name: &str, to: TypeId<'db>) {
        self.bindings
            .insert((scope.to_owned(), name.to_owned()), to);
    }

    pub(crate) fn binding(&self, scope: &str, name: &str) -> Option<TypeId<'db>> {
        self.bindings
            .get(&(scope.to_owned(), name.to_owned()))
            .copied()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

/// The late-static-binding targets of one call site (decision 1):
/// `self` resolves to the declaring owner, `parent` to the owner's
/// first `Extends` ancestor (the caller walks the ancestry — this
/// module stays lattice-pure), `static` to the receiver. A `None`
/// field widens its placeholder to `mixed`; passing no resolution at
/// all (`placeholders: None` on [`substitute`]) leaves placeholders
/// intact for pure template substitution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaceholderResolution<'db> {
    pub owner: Option<String>,
    pub parent: Option<String>,
    pub receiver: Option<TypeId<'db>>,
}

/// Substitutes templates and (optionally) placeholders through `of`,
/// evaluating conditionals whose substituted subject is decidable.
pub(crate) fn substitute<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    of: TypeId<'db>,
    map: &Substitution<'db>,
    placeholders: Option<&PlaceholderResolution<'db>>,
) -> TypeId<'db> {
    let recurse =
        |child: TypeId<'db>| substitute(db, files, stubs, configuration, child, map, placeholders);
    match of.data(db) {
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
        | TypeData::EnumCase { .. } => of,
        TypeData::Template { scope, name, .. } => map.binding(scope, name).unwrap_or(of),
        TypeData::SelfPlaceholder => match placeholders {
            None => of,
            Some(resolution) => match &resolution.owner {
                Some(owner) => TypeId::class(db, owner, vec![]),
                None => TypeId::mixed(db),
            },
        },
        TypeData::ParentPlaceholder => match placeholders {
            None => of,
            Some(resolution) => match &resolution.parent {
                Some(parent) => TypeId::class(db, parent, vec![]),
                None => TypeId::mixed(db),
            },
        },
        TypeData::StaticPlaceholder => match placeholders {
            None => of,
            Some(resolution) => resolution.receiver.unwrap_or_else(|| TypeId::mixed(db)),
        },
        TypeData::Union { constituents } => {
            let substituted: Vec<_> = constituents.iter().copied().map(recurse).collect();
            TypeId::union(db, substituted)
        }
        TypeData::Intersection { intersectands } => {
            let substituted: Vec<_> = intersectands.iter().copied().map(recurse).collect();
            TypeId::intersection(db, substituted)
        }
        TypeData::Array {
            key,
            value,
            is_list,
            non_empty,
        } => {
            let substituted_key = recurse(*key);
            let substituted_value = recurse(*value);
            match (is_list, non_empty) {
                (true, true) => TypeId::non_empty_list(db, substituted_value),
                (true, false) => TypeId::list(db, substituted_value),
                (false, true) => TypeId::non_empty_array(db, substituted_key, substituted_value),
                (false, false) => TypeId::array(db, substituted_key, substituted_value),
            }
        }
        TypeData::Shape { fields } => {
            let substituted: Vec<ShapeField<'db>> = fields
                .iter()
                .map(|field| ShapeField {
                    key: field.key.clone(),
                    optional: field.optional,
                    value: recurse(field.value),
                })
                .collect();
            TypeId::shape(db, substituted)
        }
        TypeData::ClassString { argument } => TypeId::class_string(db, argument.map(recurse)),
        TypeData::Class { name, arguments } => {
            let substituted: Vec<_> = arguments.iter().copied().map(recurse).collect();
            TypeId::class(db, name, substituted)
        }
        TypeData::Callable {
            parameters,
            return_type,
        } => {
            let substituted: Vec<CallableParameter<'db>> = parameters
                .iter()
                .map(|parameter| CallableParameter {
                    parameter_type: recurse(parameter.parameter_type),
                    optional: parameter.optional,
                    variadic: parameter.variadic,
                    by_reference: parameter.by_reference,
                })
                .collect();
            TypeId::callable(db, substituted, recurse(*return_type))
        }
        TypeData::KeyOf { subject } => TypeId::key_of(db, recurse(*subject)),
        TypeData::ValueOf { subject } => TypeId::value_of(db, recurse(*subject)),
        TypeData::Conditional {
            subject,
            matches,
            then_branch,
            otherwise_branch,
            negated,
        } => {
            let subject = recurse(*subject);
            let matches = recurse(*matches);
            let then_branch = recurse(*then_branch);
            let otherwise_branch = recurse(*otherwise_branch);
            // Substitution runs in stages: class-level templates first
            // (through `ancestor_arguments`), then method-level templates
            // at the call site. A conditional whose subject is still
            // symbolic after this pass has not yet been fully bound, so
            // it must survive unevaluated for a later, better-informed
            // pass to decide — rebuilding it here, rather than asking
            // `subtype_of` a question it cannot yet answer meaningfully.
            // At the call site, an unconstrained variable falls back to
            // its bound and then to `mixed`, which makes the subject
            // concrete and lets the `CannotProve` arm below deliver the
            // design's "falling back to the branch union when the
            // condition is undecided".
            if contains_symbolic(db, subject) {
                return TypeId::conditional(
                    db,
                    subject,
                    matches,
                    then_branch,
                    otherwise_branch,
                    *negated,
                );
            }
            let (on_holds, on_fails) = if *negated {
                (otherwise_branch, then_branch)
            } else {
                (then_branch, otherwise_branch)
            };
            match subtype_of(db, files, stubs, configuration, subject, matches) {
                Proof::Holds => on_holds,
                Proof::Fails => on_fails,
                Proof::CannotProve => TypeId::union(db, [then_branch, otherwise_branch]),
            }
        }
    }
}

/// Whether any `Template` or late-static-binding placeholder occurs
/// anywhere inside `of` — the "still symbolic" test conditional
/// evaluation and the solver's return-substitution use.
pub(crate) fn contains_symbolic<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    match of.data(db) {
        TypeData::Template { .. }
        | TypeData::SelfPlaceholder
        | TypeData::ParentPlaceholder
        | TypeData::StaticPlaceholder => true,
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
        | TypeData::EnumCase { .. } => false,
        TypeData::Union { constituents } => constituents
            .iter()
            .any(|child| contains_symbolic(db, *child)),
        TypeData::Intersection { intersectands } => intersectands
            .iter()
            .any(|child| contains_symbolic(db, *child)),
        TypeData::Array { key, value, .. } => {
            contains_symbolic(db, *key) || contains_symbolic(db, *value)
        }
        TypeData::Shape { fields } => fields
            .iter()
            .any(|field| contains_symbolic(db, field.value)),
        TypeData::ClassString { argument } => argument
            .map(|child| contains_symbolic(db, child))
            .unwrap_or(false),
        TypeData::Class { arguments, .. } => {
            arguments.iter().any(|child| contains_symbolic(db, *child))
        }
        TypeData::Callable {
            parameters,
            return_type,
        } => {
            parameters
                .iter()
                .any(|parameter| contains_symbolic(db, parameter.parameter_type))
                || contains_symbolic(db, *return_type)
        }
        TypeData::KeyOf { subject } | TypeData::ValueOf { subject } => {
            contains_symbolic(db, *subject)
        }
        TypeData::Conditional {
            subject,
            matches,
            then_branch,
            otherwise_branch,
            ..
        } => {
            contains_symbolic(db, *subject)
                || contains_symbolic(db, *matches)
                || contains_symbolic(db, *then_branch)
                || contains_symbolic(db, *otherwise_branch)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{StubIndex, StubIndexInput};

    use super::{PlaceholderResolution, Substitution, contains_symbolic, substitute};
    use crate::representation::{CallableParameter, ShapeField, ShapeKey, TypeId};

    struct Fixture {
        db: TestDatabase,
        files: AnalyzedFileSet,
        stubs: StubIndexInput,
        configuration: ProjectConfiguration,
    }

    fn fixture() -> Fixture {
        let db = TestDatabase::default();
        // `app\admin` is declared, parent-less, so the decided-conditional
        // test's hierarchy walk can decisively refute it against
        // `app\user` (`Proof::Fails`) rather than treating an entirely
        // unregistered class as undecidable (`Proof::CannotProve`).
        let handles = vec![SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php namespace app; class admin {}".to_vec(),
        )];
        let files = AnalyzedFileSet::new(&db, handles);
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        Fixture {
            db,
            files,
            stubs,
            configuration,
        }
    }

    #[test]
    fn a_bound_template_substitutes_and_an_unbound_template_stays() {
        let f = fixture();
        let db = &f.db;
        let user = TypeId::class(db, "app\\user", vec![]);
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "app\\collection", "T", bound);
        let u = TypeId::template(db, "app\\collection", "U", bound);
        let mut map = Substitution::default();
        map.bind("app\\collection", "T", user);
        let substituted_t = substitute(db, f.files, f.stubs, f.configuration, t, &map, None);
        let substituted_u = substitute(db, f.files, f.stubs, f.configuration, u, &map, None);
        assert_eq!(substituted_t, user, "a bound template takes its binding");
        assert_eq!(substituted_u, u, "an unbound template stays itself");
    }

    #[test]
    fn placeholders_resolve_against_owner_parent_and_receiver() {
        let f = fixture();
        let db = &f.db;
        let receiver = TypeId::class(db, "app\\child", vec![]);
        let resolution = PlaceholderResolution {
            owner: Some("app\\base".to_owned()),
            parent: Some("app\\grandbase".to_owned()),
            receiver: Some(receiver),
        };
        let map = Substitution::default();
        let cases = [
            (
                TypeId::self_placeholder(db),
                TypeId::class(db, "app\\base", vec![]),
            ),
            (
                TypeId::parent_placeholder(db),
                TypeId::class(db, "app\\grandbase", vec![]),
            ),
            (TypeId::static_placeholder(db), receiver),
        ];
        for (input, expected) in cases {
            let answer = substitute(
                db,
                f.files,
                f.stubs,
                f.configuration,
                input,
                &map,
                Some(&resolution),
            );
            assert_eq!(answer, expected);
        }
    }

    #[test]
    fn an_unresolvable_placeholder_widens_to_mixed_and_none_leaves_it_intact() {
        let f = fixture();
        let db = &f.db;
        let map = Substitution::default();
        let no_parent = PlaceholderResolution {
            owner: Some("app\\base".to_owned()),
            parent: None,
            receiver: None,
        };
        let parent = TypeId::parent_placeholder(db);
        let widened = substitute(
            db,
            f.files,
            f.stubs,
            f.configuration,
            parent,
            &map,
            Some(&no_parent),
        );
        assert_eq!(widened, TypeId::mixed(db), "no parent resolves to mixed");
        let untouched = substitute(db, f.files, f.stubs, f.configuration, parent, &map, None);
        assert_eq!(
            untouched, parent,
            "no resolution requested leaves it intact"
        );
    }

    #[test]
    fn a_static_placeholder_receiver_forwards() {
        // `self::create()` inside a method: the receiver for
        // substitution is the current `static` type — the placeholder
        // itself — so a `static` return survives substitution and
        // forwards to the outer caller (decision 2).
        let f = fixture();
        let db = &f.db;
        let resolution = PlaceholderResolution {
            owner: Some("app\\base".to_owned()),
            parent: None,
            receiver: Some(TypeId::static_placeholder(db)),
        };
        let map = Substitution::default();
        let answer = substitute(
            db,
            f.files,
            f.stubs,
            f.configuration,
            TypeId::static_placeholder(db),
            &map,
            Some(&resolution),
        );
        assert_eq!(answer, TypeId::static_placeholder(db));
    }

    #[test]
    fn substitution_recurses_through_composites() {
        let f = fixture();
        let db = &f.db;
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "s", "T", bound);
        let user = TypeId::class(db, "app\\user", vec![]);
        let mut map = Substitution::default();
        map.bind("s", "T", user);
        let composite = TypeId::union(
            db,
            [
                TypeId::class(db, "app\\collection", vec![t]),
                TypeId::null(db),
            ],
        );
        let expected = TypeId::union(
            db,
            [
                TypeId::class(db, "app\\collection", vec![user]),
                TypeId::null(db),
            ],
        );
        let answer = substitute(db, f.files, f.stubs, f.configuration, composite, &map, None);
        assert_eq!(answer, expected);
    }

    #[test]
    fn a_decided_conditional_picks_its_branch_and_negation_flips_it() {
        let f = fixture();
        let db = &f.db;
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "s", "T", bound);
        let user = TypeId::class(db, "app\\user", vec![]);
        let admin = TypeId::class(db, "app\\admin", vec![]);
        let then_branch = TypeId::class(db, "app\\then", vec![]);
        let otherwise_branch = TypeId::class(db, "app\\otherwise", vec![]);
        // (T is app\user ? then : otherwise) with T := app\user.
        let conditional = TypeId::conditional(db, t, user, then_branch, otherwise_branch, false);
        let negated = TypeId::conditional(db, t, user, then_branch, otherwise_branch, true);
        let mut holds = Substitution::default();
        holds.bind("s", "T", user);
        let mut fails = Substitution::default();
        fails.bind("s", "T", admin);
        let picked = substitute(
            db,
            f.files,
            f.stubs,
            f.configuration,
            conditional,
            &holds,
            None,
        );
        assert_eq!(picked, then_branch, "Holds picks the then branch");
        let flipped = substitute(db, f.files, f.stubs, f.configuration, negated, &holds, None);
        assert_eq!(flipped, otherwise_branch, "negation flips the pick");
        let missed = substitute(
            db,
            f.files,
            f.stubs,
            f.configuration,
            conditional,
            &fails,
            None,
        );
        assert_eq!(missed, otherwise_branch, "Fails picks the otherwise branch");
    }

    #[test]
    fn a_still_symbolic_conditional_survives_for_a_later_pass() {
        let f = fixture();
        let db = &f.db;
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "s", "T", bound);
        let user = TypeId::class(db, "app\\user", vec![]);
        let then_branch = TypeId::class(db, "app\\then", vec![]);
        let otherwise_branch = TypeId::class(db, "app\\otherwise", vec![]);
        let conditional = TypeId::conditional(db, t, user, then_branch, otherwise_branch, false);
        // T bound to another template: the subject is still symbolic
        // after substitution, so the conditional must be rebuilt
        // unevaluated for a later, better-informed pass to decide.
        let other = TypeId::template(db, "other", "U", bound);
        let mut map = Substitution::default();
        map.bind("s", "T", other);
        let answer = substitute(
            db,
            f.files,
            f.stubs,
            f.configuration,
            conditional,
            &map,
            None,
        );
        assert_eq!(
            answer,
            TypeId::conditional(db, other, user, then_branch, otherwise_branch, false)
        );
    }

    #[test]
    fn a_concrete_but_undecidable_conditional_answers_the_branch_union() {
        let f = fixture();
        let db = &f.db;
        // Both classes are concrete (no template involved, so the
        // "still symbolic" guard does not fire) but neither is declared
        // as a source class-like nor present in the (empty) stub index,
        // so `subtype_of` cannot walk any hierarchy and genuinely
        // answers `Proof::CannotProve` (see `judge_stub_hierarchy`'s
        // "unknown class" arm in judgments.rs).
        let subject = TypeId::class(db, "app\\unregistered_candidate", vec![]);
        let matches = TypeId::class(db, "app\\unregistered_target", vec![]);
        let then_branch = TypeId::class(db, "app\\then", vec![]);
        let otherwise_branch = TypeId::class(db, "app\\otherwise", vec![]);
        let conditional =
            TypeId::conditional(db, subject, matches, then_branch, otherwise_branch, false);
        let map = Substitution::default();
        let answer = substitute(
            db,
            f.files,
            f.stubs,
            f.configuration,
            conditional,
            &map,
            None,
        );
        assert_eq!(answer, TypeId::union(db, [then_branch, otherwise_branch]));
    }

    #[test]
    fn contains_symbolic_sees_through_composites() {
        let f = fixture();
        let db = &f.db;
        let t = TypeId::template(db, "s", "T", TypeId::mixed(db));
        let nested = TypeId::class(db, "app\\collection", vec![t]);
        assert!(contains_symbolic(db, nested));
        assert!(contains_symbolic(db, TypeId::static_placeholder(db)));
        assert!(!contains_symbolic(
            db,
            TypeId::class(db, "app\\user", vec![])
        ));
    }

    #[test]
    fn substitution_dispatches_the_four_array_forms_correctly() {
        let f = fixture();
        let db = &f.db;
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "s", "T", bound);
        let key_template = TypeId::template(db, "s", "K", bound);
        let value_template = TypeId::template(db, "s", "V", bound);
        let user = TypeId::class(db, "app\\user", vec![]);
        let admin = TypeId::class(db, "app\\admin", vec![]);
        let mut map = Substitution::default();
        map.bind("s", "T", user);
        // Key and value bind to two DIFFERENT classes so a transposed
        // key/value would be caught by asserting each position.
        map.bind("s", "K", admin);
        map.bind("s", "V", user);

        let list = TypeId::list(db, t);
        let non_empty_list = TypeId::non_empty_list(db, t);
        let array = TypeId::array(db, key_template, value_template);
        let non_empty_array = TypeId::non_empty_array(db, key_template, value_template);

        let substituted_list = substitute(db, f.files, f.stubs, f.configuration, list, &map, None);
        let substituted_non_empty_list = substitute(
            db,
            f.files,
            f.stubs,
            f.configuration,
            non_empty_list,
            &map,
            None,
        );
        let substituted_array =
            substitute(db, f.files, f.stubs, f.configuration, array, &map, None);
        let substituted_non_empty_array = substitute(
            db,
            f.files,
            f.stubs,
            f.configuration,
            non_empty_array,
            &map,
            None,
        );

        assert_eq!(substituted_list, TypeId::list(db, user), "(true, false)");
        assert_eq!(
            substituted_non_empty_list,
            TypeId::non_empty_list(db, user),
            "(true, true)"
        );
        assert_eq!(
            substituted_array,
            TypeId::array(db, admin, user),
            "(false, false), key and value in the right position"
        );
        assert_eq!(
            substituted_non_empty_array,
            TypeId::non_empty_array(db, admin, user),
            "(false, true), key and value in the right position"
        );
    }

    #[test]
    fn substitution_recurses_through_an_intersection() {
        let f = fixture();
        let db = &f.db;
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "s", "T", bound);
        let user = TypeId::class(db, "app\\user", vec![]);
        let mut map = Substitution::default();
        map.bind("s", "T", user);
        let composite = TypeId::intersection(
            db,
            [
                TypeId::class(db, "app\\collection", vec![t]),
                TypeId::class(db, "app\\countable", vec![]),
            ],
        );
        let expected = TypeId::intersection(
            db,
            [
                TypeId::class(db, "app\\collection", vec![user]),
                TypeId::class(db, "app\\countable", vec![]),
            ],
        );
        let answer = substitute(db, f.files, f.stubs, f.configuration, composite, &map, None);
        assert_eq!(answer, expected);
    }

    #[test]
    fn substitution_copies_shape_field_flags_while_recursing_into_values() {
        let f = fixture();
        let db = &f.db;
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "s", "T", bound);
        let user = TypeId::class(db, "app\\user", vec![]);
        let mut map = Substitution::default();
        map.bind("s", "T", user);
        let shape = TypeId::shape(
            db,
            vec![
                ShapeField {
                    key: ShapeKey::String("optional_field".to_owned()),
                    optional: true,
                    value: t,
                },
                ShapeField {
                    key: ShapeKey::String("required_field".to_owned()),
                    optional: false,
                    value: TypeId::string(db),
                },
            ],
        );
        let expected = TypeId::shape(
            db,
            vec![
                ShapeField {
                    key: ShapeKey::String("optional_field".to_owned()),
                    optional: true,
                    value: user,
                },
                ShapeField {
                    key: ShapeKey::String("required_field".to_owned()),
                    optional: false,
                    value: TypeId::string(db),
                },
            ],
        );
        let answer = substitute(db, f.files, f.stubs, f.configuration, shape, &map, None);
        assert_eq!(
            answer, expected,
            "each field's optional flag survives substitution"
        );
    }

    #[test]
    fn substitution_copies_callable_parameter_flags_while_recursing() {
        let f = fixture();
        let db = &f.db;
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "s", "T", bound);
        let u = TypeId::template(db, "s", "U", bound);
        let user = TypeId::class(db, "app\\user", vec![]);
        let admin = TypeId::class(db, "app\\admin", vec![]);
        let mut map = Substitution::default();
        map.bind("s", "T", user);
        map.bind("s", "U", admin);
        let callable = TypeId::callable(
            db,
            vec![CallableParameter {
                parameter_type: t,
                optional: true,
                variadic: false,
                by_reference: true,
            }],
            u,
        );
        let expected = TypeId::callable(
            db,
            vec![CallableParameter {
                parameter_type: user,
                optional: true,
                variadic: false,
                by_reference: true,
            }],
            admin,
        );
        let answer = substitute(db, f.files, f.stubs, f.configuration, callable, &map, None);
        assert_eq!(
            answer, expected,
            "the parameter's optional, variadic and by_reference flags survive"
        );
    }

    #[test]
    fn substitution_recurses_through_class_string_key_of_and_value_of() {
        let f = fixture();
        let db = &f.db;
        let bound = TypeId::mixed(db);
        let t = TypeId::template(db, "s", "T", bound);
        let user = TypeId::class(db, "app\\user", vec![]);
        let mut map = Substitution::default();
        map.bind("s", "T", user);

        let class_string = TypeId::class_string(db, Some(t));
        let key_of = TypeId::key_of(db, t);
        let value_of = TypeId::value_of(db, t);

        let substituted_class_string = substitute(
            db,
            f.files,
            f.stubs,
            f.configuration,
            class_string,
            &map,
            None,
        );
        let substituted_key_of =
            substitute(db, f.files, f.stubs, f.configuration, key_of, &map, None);
        let substituted_value_of =
            substitute(db, f.files, f.stubs, f.configuration, value_of, &map, None);

        assert_eq!(
            substituted_class_string,
            TypeId::class_string(db, Some(user))
        );
        assert_eq!(substituted_key_of, TypeId::key_of(db, user));
        assert_eq!(substituted_value_of, TypeId::value_of(db, user));
    }
}
