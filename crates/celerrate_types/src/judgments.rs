//! The three-valued judgments (spec section 3): `Holds`, `Fails`
//! (value-set inclusion refuted), `CannotProve` (undecidable with
//! available information). Every consumer states its posture toward
//! `CannotProve`; nothing here or above silently discards it.

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{ClassQuery, linearized_class};
use celerrate_stubs::StubIndexInput;

use crate::representation::{StringConstraint, TypeData, TypeId};

/// The three-valued verdict of a typed judgment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Proof {
    Holds,
    Fails,
    CannotProve,
}

impl Proof {
    /// Conjunction: all must hold; one refutation refutes the whole.
    pub fn all(proofs: impl IntoIterator<Item = Proof>) -> Proof {
        let mut result = Proof::Holds;
        for proof in proofs {
            match proof {
                Proof::Fails => return Proof::Fails,
                Proof::CannotProve => result = Proof::CannotProve,
                Proof::Holds => {}
            }
        }
        result
    }

    /// Disjunction: one hold suffices; only unanimous refutation refutes.
    pub fn any(proofs: impl IntoIterator<Item = Proof>) -> Proof {
        let mut result = Proof::Fails;
        for proof in proofs {
            match proof {
                Proof::Holds => return Proof::Holds,
                Proof::CannotProve => result = Proof::CannotProve,
                Proof::Fails => {}
            }
        }
        result
    }
}

/// The nullability verdict of one type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Nullability {
    NeverNull,
    PossiblyNull,
    AlwaysNull,
}

/// The salsa inputs the class rule needs, carried through the recursion.
#[derive(Clone, Copy)]
struct JudgmentContext {
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

/// Is `candidate` a subtype of `target`?
#[salsa::tracked]
pub fn subtype_of<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    candidate: TypeId<'db>,
    target: TypeId<'db>,
) -> Proof {
    judge(
        db,
        JudgmentContext {
            files,
            stubs,
            configuration,
        },
        candidate,
        target,
    )
}

/// May a `source` value be assigned where `target` is declared? Today
/// this is exactly the subtype judgment; the coercion posture (weak-mode
/// files, `Stringable`) is the argument family's and lands in plan 8
/// behind this signature.
#[salsa::tracked]
pub fn assignable_to<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    source: TypeId<'db>,
    target: TypeId<'db>,
) -> Proof {
    subtype_of(db, files, stubs, configuration, source, target)
}

/// The nullability of one type. `void` is `AlwaysNull`: reading a void
/// call's value yields `null` in PHP.
#[salsa::tracked]
pub fn nullability<'db>(db: &'db dyn salsa::Database, subject: TypeId<'db>) -> Nullability {
    match subject.data(db) {
        TypeData::Null | TypeData::Void => Nullability::AlwaysNull,
        TypeData::Mixed | TypeData::ValueOf { .. } => Nullability::PossiblyNull,
        TypeData::Union { constituents } => {
            let verdicts: Vec<Nullability> = constituents
                .iter()
                .map(|part| nullability(db, *part))
                .collect();
            if verdicts
                .iter()
                .all(|verdict| *verdict == Nullability::AlwaysNull)
            {
                Nullability::AlwaysNull
            } else if verdicts
                .iter()
                .any(|verdict| *verdict != Nullability::NeverNull)
            {
                Nullability::PossiblyNull
            } else {
                Nullability::NeverNull
            }
        }
        TypeData::Intersection { intersectands } => {
            if intersectands
                .iter()
                .any(|part| nullability(db, *part) == Nullability::NeverNull)
            {
                Nullability::NeverNull
            } else {
                Nullability::PossiblyNull
            }
        }
        TypeData::Template { bound, .. } => nullability(db, *bound),
        TypeData::Conditional {
            then_branch,
            otherwise_branch,
            ..
        } => {
            match (
                nullability(db, *then_branch),
                nullability(db, *otherwise_branch),
            ) {
                (Nullability::NeverNull, Nullability::NeverNull) => Nullability::NeverNull,
                (Nullability::AlwaysNull, Nullability::AlwaysNull) => Nullability::AlwaysNull,
                _ => Nullability::PossiblyNull,
            }
        }
        _ => Nullability::NeverNull,
    }
}

/// PHP's numeric-string test for a known literal: optional surrounding
/// whitespace (PHP 8 semantics), optional sign, digits with an optional
/// fraction or exponent.
fn literal_is_numeric(value: &str) -> bool {
    let trimmed = value.trim_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    if trimmed.is_empty() {
        return false;
    }
    let unsigned = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if unsigned.is_empty() {
        return false;
    }
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (unsigned, None),
    };
    let (integer_part, fraction_part) = match mantissa.split_once('.') {
        Some((integer_part, fraction_part)) => (integer_part, Some(fraction_part)),
        None => (mantissa, None),
    };
    // PHP 8 DNUM: either side of the dot may be empty, never both.
    let integer_is_digits =
        !integer_part.is_empty() && integer_part.bytes().all(|byte| byte.is_ascii_digit());
    let fraction_is_digits = fraction_part
        .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    let mantissa_valid = match fraction_part {
        None => integer_is_digits,
        Some(fraction) => {
            if integer_part.is_empty() {
                fraction_is_digits
            } else {
                integer_is_digits && (fraction.is_empty() || fraction_is_digits)
            }
        }
    };
    let exponent_valid = exponent.is_none_or(|part| {
        let unsigned_exponent = part.strip_prefix(['+', '-']).unwrap_or(part);
        !unsigned_exponent.is_empty() && unsigned_exponent.bytes().all(|byte| byte.is_ascii_digit())
    });
    mantissa_valid && exponent_valid
}

/// The class-versus-class hierarchy verdict for differing folded names:
/// found in the walked ancestry proves; a stub boundary, an unresolved
/// edge, or a broken cycle leaves the answer undecidable; a fully
/// resolved hierarchy without the target refutes.
fn judge_class_hierarchy(
    db: &dyn salsa::Database,
    context: JudgmentContext,
    candidate_name: &str,
    target_name: &str,
) -> Proof {
    if candidate_name == target_name {
        return Proof::Holds;
    }
    let class = ClassQuery::new(db, candidate_name.to_owned());
    let Some(linearized) = linearized_class(
        db,
        context.files,
        context.stubs,
        context.configuration,
        class,
    ) else {
        // A stub or unknown class: the stub blob carries no hierarchy.
        return Proof::CannotProve;
    };
    let found = linearized
        .ancestry
        .iter()
        .any(|edge| edge.resolved.as_deref() == Some(target_name))
        || linearized
            .stub_ancestors
            .iter()
            .any(|key| key == target_name);
    if found {
        return Proof::Holds;
    }
    let opaque_boundary = linearized.cyclic
        || linearized
            .ancestry
            .iter()
            .any(|edge| edge.resolved.is_none());
    if opaque_boundary {
        Proof::CannotProve
    } else {
        Proof::Fails
    }
}

fn judge<'db>(
    db: &'db dyn salsa::Database,
    context: JudgmentContext,
    candidate: TypeId<'db>,
    target: TypeId<'db>,
) -> Proof {
    // Rules 1 to 5: the extremes.
    if candidate == target {
        return Proof::Holds;
    }
    if target.is_mixed(db) {
        return Proof::Holds;
    }
    if candidate.is_never(db) {
        return Proof::Holds;
    }
    if target.is_never(db) {
        return Proof::Fails;
    }
    if candidate.is_mixed(db) {
        return Proof::Fails;
    }
    // Rules 6 to 9: decomposition.
    if let TypeData::Union { constituents } = candidate.data(db) {
        return Proof::all(
            constituents
                .iter()
                .map(|part| judge(db, context, *part, target)),
        );
    }
    if let TypeData::Union { constituents } = target.data(db) {
        return Proof::any(
            constituents
                .iter()
                .map(|part| judge(db, context, candidate, *part)),
        );
    }
    if let TypeData::Intersection { intersectands } = candidate.data(db) {
        return match Proof::any(
            intersectands
                .iter()
                .map(|part| judge(db, context, *part, target)),
        ) {
            Proof::Holds => Proof::Holds,
            _ => Proof::CannotProve,
        };
    }
    if let TypeData::Intersection { intersectands } = target.data(db) {
        return Proof::all(
            intersectands
                .iter()
                .map(|part| judge(db, context, candidate, *part)),
        );
    }
    // Rules 10 and 11: templates.
    if let TypeData::Template { bound, .. } = candidate.data(db) {
        return match judge(db, context, *bound, target) {
            Proof::Holds => Proof::Holds,
            _ => Proof::CannotProve,
        };
    }
    if matches!(target.data(db), TypeData::Template { .. }) {
        return Proof::CannotProve;
    }
    // Rule 12: conditionals through their branch unions.
    if let TypeData::Conditional {
        then_branch,
        otherwise_branch,
        ..
    } = candidate.data(db)
    {
        let fallback = TypeId::union(db, [*then_branch, *otherwise_branch]);
        return match judge(db, context, fallback, target) {
            Proof::Holds => Proof::Holds,
            _ => Proof::CannotProve,
        };
    }
    if let TypeData::Conditional {
        then_branch,
        otherwise_branch,
        ..
    } = target.data(db)
    {
        let both = Proof::all([
            judge(db, context, candidate, *then_branch),
            judge(db, context, candidate, *otherwise_branch),
        ]);
        return match both {
            Proof::Holds => Proof::Holds,
            _ => Proof::CannotProve,
        };
    }
    // Rule 13: the symbolic key-of and value-of.
    if matches!(candidate.data(db), TypeData::KeyOf { .. }) {
        let keys = TypeId::union(db, [TypeId::int(db), TypeId::string(db)]);
        return match judge(db, context, keys, target) {
            Proof::Holds => Proof::Holds,
            _ => Proof::CannotProve,
        };
    }
    if matches!(candidate.data(db), TypeData::ValueOf { .. })
        || matches!(
            target.data(db),
            TypeData::KeyOf { .. } | TypeData::ValueOf { .. }
        )
    {
        return Proof::CannotProve;
    }
    // Rule 14: the placeholders.
    if matches!(
        candidate.data(db),
        TypeData::SelfPlaceholder | TypeData::ParentPlaceholder | TypeData::StaticPlaceholder
    ) || matches!(
        target.data(db),
        TypeData::SelfPlaceholder | TypeData::ParentPlaceholder | TypeData::StaticPlaceholder
    ) {
        return Proof::CannotProve;
    }
    // Rule 15: the ground matrix.
    judge_ground(db, context, candidate, target)
}

fn judge_ground<'db>(
    db: &'db dyn salsa::Database,
    context: JudgmentContext,
    candidate: TypeId<'db>,
    target: TypeId<'db>,
) -> Proof {
    match (candidate.data(db), target.data(db)) {
        // Booleans: a literal sits under the general type.
        (TypeData::Bool { literal: Some(_) }, TypeData::Bool { literal: None }) => Proof::Holds,
        (TypeData::Bool { .. }, TypeData::Bool { .. }) => Proof::Fails,
        // Integers: range inclusion.
        (
            TypeData::Int {
                minimum: a_min,
                maximum: a_max,
            },
            TypeData::Int {
                minimum: b_min,
                maximum: b_max,
            },
        ) => {
            let low_included = match (a_min, b_min) {
                (_, None) => true,
                (None, Some(_)) => false,
                (Some(a), Some(b)) => a >= b,
            };
            let high_included = match (a_max, b_max) {
                (_, None) => true,
                (None, Some(_)) => false,
                (Some(a), Some(b)) => a <= b,
            };
            if low_included && high_included {
                Proof::Holds
            } else {
                Proof::Fails
            }
        }
        // Floats: a literal sits under the general type.
        (TypeData::Float { literal: Some(_) }, TypeData::Float { literal: None }) => Proof::Holds,
        (TypeData::Float { .. }, TypeData::Float { .. }) => Proof::Fails,
        // The string-constraint table.
        (TypeData::String { constraint: a }, TypeData::String { constraint: b }) => {
            let holds = match (a, b) {
                (_, StringConstraint::General) => true,
                (StringConstraint::Literal(value), StringConstraint::NonEmpty) => !value.is_empty(),
                (StringConstraint::Literal(value), StringConstraint::Numeric) => {
                    literal_is_numeric(value)
                }
                (StringConstraint::Literal(_), StringConstraint::LiteralMarker) => true,
                (StringConstraint::Numeric, StringConstraint::NonEmpty) => true,
                _ => false,
            };
            if holds { Proof::Holds } else { Proof::Fails }
        }
        // class-string.
        (TypeData::ClassString { .. }, TypeData::ClassString { argument: None }) => Proof::Holds,
        (
            TypeData::ClassString { argument: Some(a) },
            TypeData::ClassString { argument: Some(b) },
        ) => judge(db, context, *a, *b),
        (TypeData::ClassString { .. }, TypeData::ClassString { .. }) => Proof::Fails,
        (
            TypeData::ClassString { .. },
            TypeData::String {
                constraint: StringConstraint::General | StringConstraint::NonEmpty,
            },
        ) => Proof::Holds,
        (TypeData::ClassString { .. }, TypeData::String { .. }) => Proof::Fails,
        (
            TypeData::String {
                constraint: StringConstraint::Literal(_),
            },
            TypeData::ClassString { .. },
        ) => Proof::CannotProve,
        // Arrays: flags gate, then key and value covariance.
        (
            TypeData::Array {
                key: a_key,
                value: a_value,
                is_list: a_list,
                non_empty: a_non_empty,
            },
            TypeData::Array {
                key: b_key,
                value: b_value,
                is_list: b_list,
                non_empty: b_non_empty,
            },
        ) => {
            if (*b_list && !*a_list) || (*b_non_empty && !*a_non_empty) {
                return Proof::Fails;
            }
            Proof::all([
                judge(db, context, *a_key, *b_key),
                judge(db, context, *a_value, *b_value),
            ])
        }
        // Shapes: sealed, width-strict, optionality-aware.
        (TypeData::Shape { fields: a }, TypeData::Shape { fields: b }) => {
            if a.iter()
                .any(|field| !b.iter().any(|other| other.key == field.key))
            {
                return Proof::Fails;
            }
            Proof::all(b.iter().map(|target_field| {
                match a.iter().find(|field| field.key == target_field.key) {
                    Some(candidate_field) => {
                        let value = judge(db, context, candidate_field.value, target_field.value);
                        if !target_field.optional && candidate_field.optional {
                            Proof::all([value, Proof::CannotProve])
                        } else {
                            value
                        }
                    }
                    None if target_field.optional => Proof::Holds,
                    None => Proof::Fails,
                }
            }))
        }
        (TypeData::Shape { fields }, TypeData::Array { .. }) => {
            let (key, value, is_list, non_empty) = TypeId::shape_as_array(db, fields);
            let widened = match (is_list, non_empty) {
                (true, true) => TypeId::non_empty_list(db, value),
                (true, false) => TypeId::list(db, value),
                (false, true) => TypeId::non_empty_array(db, key, value),
                (false, false) => TypeId::array(db, key, value),
            };
            judge(db, context, widened, target)
        }
        (TypeData::Array { .. }, TypeData::Shape { .. }) => Proof::Fails,
        // Class-likes.
        (TypeData::Class { .. } | TypeData::EnumCase { .. }, TypeData::Object) => Proof::Holds,
        (TypeData::Object, TypeData::Class { .. } | TypeData::EnumCase { .. }) => Proof::Fails,
        (
            TypeData::Class {
                name: a_name,
                arguments: a_arguments,
            },
            TypeData::Class {
                name: b_name,
                arguments: b_arguments,
            },
        ) => {
            if a_name == b_name {
                if b_arguments.is_empty() || a_arguments == b_arguments {
                    Proof::Holds
                } else {
                    // Invariant arguments; variance is out of scope.
                    Proof::CannotProve
                }
            } else {
                match judge_class_hierarchy(db, context, a_name, b_name) {
                    Proof::Holds if b_arguments.is_empty() => Proof::Holds,
                    Proof::Holds => Proof::CannotProve,
                    verdict => verdict,
                }
            }
        }
        (TypeData::EnumCase { enum_name, .. }, TypeData::Class { name, arguments }) => {
            if arguments.is_empty() {
                judge_class_hierarchy(db, context, enum_name, name)
            } else {
                Proof::CannotProve
            }
        }
        (TypeData::EnumCase { .. }, TypeData::EnumCase { .. }) => Proof::Fails,
        (TypeData::Class { name, .. }, TypeData::EnumCase { enum_name, .. }) => {
            if name == enum_name {
                Proof::CannotProve
            } else {
                Proof::Fails
            }
        }
        // Callables: contravariant parameters, covariant return.
        (
            TypeData::Callable {
                parameters: a_parameters,
                return_type: a_return,
            },
            TypeData::Callable {
                parameters: b_parameters,
                return_type: b_return,
            },
        ) => judge_callable(
            db,
            context,
            a_parameters,
            *a_return,
            b_parameters,
            *b_return,
        ),
        // The CannotProve islands, both directions: which values inhabit a
        // callable signature is program-dependent (a matching function-name
        // string, class-string, array callable, shape, or class may or may
        // not exist at runtime), so invokable objects and callable strings,
        // class-strings, arrays, shapes, and classes stay undecidable
        // whichever side is the candidate.
        (TypeData::Callable { .. }, TypeData::Object)
        | (TypeData::Object | TypeData::Class { .. }, TypeData::Callable { .. })
        | (TypeData::String { .. } | TypeData::ClassString { .. }, TypeData::Callable { .. })
        | (TypeData::Array { .. } | TypeData::Shape { .. }, TypeData::Callable { .. })
        | (
            TypeData::Callable { .. },
            TypeData::String { .. }
            | TypeData::ClassString { .. }
            | TypeData::Array { .. }
            | TypeData::Shape { .. }
            | TypeData::Class { .. },
        ) => Proof::CannotProve,
        // Everything else is a refuted cross-kind pair.
        _ => Proof::Fails,
    }
}

fn judge_callable<'db>(
    db: &'db dyn salsa::Database,
    context: JudgmentContext,
    candidate_parameters: &[crate::CallableParameter<'db>],
    candidate_return: TypeId<'db>,
    target_parameters: &[crate::CallableParameter<'db>],
    target_return: TypeId<'db>,
) -> Proof {
    // A void target accepts any return; otherwise the return is covariant.
    let return_proof = if target_return.is_void(db) {
        Proof::Holds
    } else {
        judge(db, context, candidate_return, target_return)
    };
    let mut proofs = vec![return_proof];
    let candidate_variadic = candidate_parameters
        .last()
        .filter(|parameter| parameter.variadic);
    for (index, target_parameter) in target_parameters.iter().enumerate() {
        let candidate_parameter = candidate_parameters
            .get(index)
            .filter(|parameter| !parameter.variadic)
            .or(candidate_variadic);
        match candidate_parameter {
            Some(parameter) => {
                if parameter.by_reference != target_parameter.by_reference {
                    proofs.push(Proof::CannotProve);
                } else {
                    // Contravariant: the target's argument flows into the
                    // candidate's parameter.
                    proofs.push(judge(
                        db,
                        context,
                        target_parameter.parameter_type,
                        parameter.parameter_type,
                    ));
                }
            }
            // The target may pass an argument the candidate cannot take.
            None => proofs.push(Proof::Fails),
        }
    }
    // Candidate parameters beyond the target's arity must be optional.
    let required_beyond = candidate_parameters
        .iter()
        .skip(target_parameters.len())
        .any(|parameter| !parameter.optional && !parameter.variadic);
    if required_beyond {
        proofs.push(Proof::Fails);
    }
    Proof::all(proofs)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{StubIndex, StubIndexInput};

    use super::{Nullability, Proof, assignable_to, nullability, subtype_of};
    use crate::TypeId;

    struct Fixture {
        db: TestDatabase,
        files: AnalyzedFileSet,
        stubs: StubIndexInput,
        configuration: ProjectConfiguration,
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

    fn judge<'db>(fixture: &'db Fixture, candidate: TypeId<'db>, target: TypeId<'db>) -> Proof {
        subtype_of(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            candidate,
            target,
        )
    }

    #[test]
    fn the_extremes_anchor_the_lattice() {
        let f = fixture(&[]);
        let db = &f.db;
        assert_eq!(judge(&f, TypeId::int(db), TypeId::mixed(db)), Proof::Holds);
        assert_eq!(judge(&f, TypeId::never(db), TypeId::int(db)), Proof::Holds);
        assert_eq!(judge(&f, TypeId::mixed(db), TypeId::int(db)), Proof::Fails);
        assert_eq!(judge(&f, TypeId::int(db), TypeId::never(db)), Proof::Fails);
        assert_eq!(judge(&f, TypeId::int(db), TypeId::int(db)), Proof::Holds);
    }

    #[test]
    fn scalar_inclusion_follows_the_matrix() {
        let f = fixture(&[]);
        let db = &f.db;
        assert_eq!(
            judge(&f, TypeId::bool_literal(db, true), TypeId::bool(db)),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, TypeId::bool(db), TypeId::bool_literal(db, true)),
            Proof::Fails
        );
        assert_eq!(
            judge(&f, TypeId::int_range(db, Some(1), Some(3)), TypeId::int(db)),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, TypeId::int(db), TypeId::int_range(db, Some(1), Some(3))),
            Proof::Fails
        );
        assert_eq!(
            judge(
                &f,
                TypeId::int_literal(db, 2),
                TypeId::int_range(db, Some(1), Some(3))
            ),
            Proof::Holds
        );
        assert_eq!(judge(&f, TypeId::int(db), TypeId::float(db)), Proof::Fails);
        assert_eq!(
            judge(&f, TypeId::float_literal(db, 1.5), TypeId::float(db)),
            Proof::Holds
        );
    }

    #[test]
    fn the_string_family_matrix_holds() {
        let f = fixture(&[]);
        let db = &f.db;
        let literal = TypeId::string_literal(db, "active");
        assert_eq!(judge(&f, literal, TypeId::string(db)), Proof::Holds);
        assert_eq!(
            judge(&f, literal, TypeId::non_empty_string(db)),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, literal, TypeId::literal_string_type(db)),
            Proof::Holds
        );
        assert_eq!(judge(&f, literal, TypeId::numeric_string(db)), Proof::Fails);
        assert_eq!(
            judge(
                &f,
                TypeId::string_literal(db, "42"),
                TypeId::numeric_string(db)
            ),
            Proof::Holds
        );
        assert_eq!(
            judge(
                &f,
                TypeId::string_literal(db, ""),
                TypeId::non_empty_string(db)
            ),
            Proof::Fails
        );
        assert_eq!(
            judge(&f, TypeId::numeric_string(db), TypeId::non_empty_string(db)),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, TypeId::non_empty_string(db), TypeId::numeric_string(db)),
            Proof::Fails
        );
        assert_eq!(
            judge(&f, TypeId::string(db), TypeId::non_empty_string(db)),
            Proof::Fails
        );
        assert_eq!(
            judge(&f, TypeId::class_string(db, None), TypeId::string(db)),
            Proof::Holds
        );
        assert_eq!(
            judge(
                &f,
                TypeId::class_string(db, None),
                TypeId::non_empty_string(db)
            ),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, literal, TypeId::class_string(db, None)),
            Proof::CannotProve
        );
    }

    #[test]
    fn unions_and_intersections_decompose() {
        let f = fixture(&[]);
        let db = &f.db;
        let nullable_int = TypeId::union(db, [TypeId::int(db), TypeId::null(db)]);
        assert_eq!(judge(&f, TypeId::int(db), nullable_int), Proof::Holds);
        assert_eq!(judge(&f, nullable_int, TypeId::int(db)), Proof::Fails);
        assert_eq!(judge(&f, nullable_int, nullable_int), Proof::Holds);
        let counted = TypeId::intersection(
            db,
            [
                TypeId::class(db, "Foo", vec![]),
                TypeId::class(db, "Countable", vec![]),
            ],
        );
        assert_eq!(
            judge(&f, counted, TypeId::class(db, "Foo", vec![])),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, TypeId::class(db, "Foo", vec![]), counted),
            Proof::CannotProve
        );
    }

    #[test]
    fn templates_judge_through_their_bounds() {
        let f = fixture(&[]);
        let db = &f.db;
        let bound = TypeId::class(db, "FormTypeInterface", vec![]);
        let template = TypeId::template(db, "scope", "T", bound);
        // T of Foo <: Foo holds definitionally through the bound.
        assert_eq!(judge(&f, template, bound), Proof::Holds);
        assert_eq!(judge(&f, template, template), Proof::Holds);
        assert_eq!(judge(&f, template, TypeId::int(db)), Proof::CannotProve);
        assert_eq!(judge(&f, TypeId::int(db), template), Proof::CannotProve);
    }

    #[test]
    fn arrays_shapes_and_callables_follow_their_rules() {
        let f = fixture(&[]);
        let db = &f.db;
        let list = TypeId::list(db, TypeId::int(db));
        let array = TypeId::array(db, TypeId::int(db), TypeId::int(db));
        assert_eq!(judge(&f, list, array), Proof::Holds);
        assert_eq!(judge(&f, array, list), Proof::Fails);
        assert_eq!(
            judge(
                &f,
                TypeId::non_empty_array(db, TypeId::int(db), TypeId::int(db)),
                array
            ),
            Proof::Holds
        );
        assert_eq!(
            judge(
                &f,
                array,
                TypeId::non_empty_array(db, TypeId::int(db), TypeId::int(db))
            ),
            Proof::Fails
        );
        let narrow = TypeId::shape(
            db,
            vec![crate::ShapeField {
                key: crate::ShapeKey::String("id".to_owned()),
                optional: false,
                value: TypeId::int_literal(db, 1),
            }],
        );
        let wide = TypeId::shape(
            db,
            vec![crate::ShapeField {
                key: crate::ShapeKey::String("id".to_owned()),
                optional: false,
                value: TypeId::int(db),
            }],
        );
        assert_eq!(judge(&f, narrow, wide), Proof::Holds);
        assert_eq!(judge(&f, wide, narrow), Proof::Fails);
        // A sealed shape with an extra key fails against a shape without it.
        let extra = TypeId::shape(
            db,
            vec![
                crate::ShapeField {
                    key: crate::ShapeKey::String("id".to_owned()),
                    optional: false,
                    value: TypeId::int(db),
                },
                crate::ShapeField {
                    key: crate::ShapeKey::String("extra".to_owned()),
                    optional: false,
                    value: TypeId::string(db),
                },
            ],
        );
        assert_eq!(judge(&f, extra, wide), Proof::Fails);
        // A shape is a subtype of its general array form.
        assert_eq!(
            judge(
                &f,
                wide,
                TypeId::array(db, TypeId::string(db), TypeId::int(db))
            ),
            Proof::Holds
        );
        assert_eq!(
            judge(
                &f,
                TypeId::array(db, TypeId::string(db), TypeId::int(db)),
                wide
            ),
            Proof::Fails
        );
        // Callables: parameters contravariant, return covariant, void target accepts all.
        let takes_int_returns_literal = TypeId::callable(
            db,
            vec![crate::CallableParameter {
                parameter_type: TypeId::int(db),
                optional: false,
                variadic: false,
                by_reference: false,
            }],
            TypeId::int_literal(db, 1),
        );
        let takes_literal_returns_int = TypeId::callable(
            db,
            vec![crate::CallableParameter {
                parameter_type: TypeId::int_literal(db, 1),
                optional: false,
                variadic: false,
                by_reference: false,
            }],
            TypeId::int(db),
        );
        assert_eq!(
            judge(&f, takes_int_returns_literal, takes_literal_returns_int),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, takes_literal_returns_int, takes_int_returns_literal),
            Proof::Fails
        );
        let void_target = TypeId::callable(db, vec![], TypeId::void(db));
        let no_parameter_int = TypeId::callable(db, vec![], TypeId::int(db));
        assert_eq!(judge(&f, no_parameter_int, void_target), Proof::Holds);
    }

    #[test]
    fn class_likes_use_the_hierarchy_hook() {
        let f = fixture(&[]);
        let db = &f.db;
        let user = TypeId::class(db, "User", vec![]);
        assert_eq!(judge(&f, user, TypeId::object(db)), Proof::Holds);
        assert_eq!(judge(&f, TypeId::object(db), user), Proof::Fails);
        // Different names answer CannotProve in this task; Task 9 tightens.
        assert_eq!(
            judge(&f, user, TypeId::class(db, "Entity", vec![])),
            Proof::CannotProve
        );
        // Same name, differing generic arguments: invariant, CannotProve.
        let of_int = TypeId::class(db, "Collection", vec![TypeId::int(db)]);
        let of_string = TypeId::class(db, "Collection", vec![TypeId::string(db)]);
        assert_eq!(judge(&f, of_int, of_string), Proof::CannotProve);
        // An unparameterized target erases.
        assert_eq!(
            judge(&f, of_int, TypeId::class(db, "Collection", vec![])),
            Proof::Holds
        );
        // Enum cases sit under their enum type.
        let case = TypeId::enum_case(db, "Status", "Active");
        assert_eq!(
            judge(&f, case, TypeId::class(db, "Status", vec![])),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, case, TypeId::enum_case(db, "Status", "Inactive")),
            Proof::Fails
        );
        assert_eq!(
            judge(&f, TypeId::class(db, "Status", vec![]), case),
            Proof::CannotProve
        );
    }

    #[test]
    fn assignability_delegates_and_nullability_answers() {
        let f = fixture(&[]);
        let db = &f.db;
        assert_eq!(
            assignable_to(
                &f.db,
                f.files,
                f.stubs,
                f.configuration,
                TypeId::int(db),
                TypeId::mixed(db)
            ),
            Proof::Holds
        );
        assert_eq!(
            nullability(&f.db, TypeId::null(db)),
            Nullability::AlwaysNull
        );
        assert_eq!(
            nullability(&f.db, TypeId::void(db)),
            Nullability::AlwaysNull
        );
        assert_eq!(
            nullability(&f.db, TypeId::mixed(db)),
            Nullability::PossiblyNull
        );
        assert_eq!(nullability(&f.db, TypeId::int(db)), Nullability::NeverNull);
        assert_eq!(
            nullability(
                &f.db,
                TypeId::union(db, [TypeId::int(db), TypeId::null(db)])
            ),
            Nullability::PossiblyNull
        );
        let nullable_bound = TypeId::template(
            db,
            "scope",
            "T",
            TypeId::union(db, [TypeId::int(db), TypeId::null(db)]),
        );
        assert_eq!(
            nullability(&f.db, nullable_bound),
            Nullability::PossiblyNull
        );
    }

    #[test]
    fn callable_cross_kind_pairs_are_undecidable_in_both_directions() {
        let f = fixture(&[]);
        let db = &f.db;
        let callable = TypeId::callable(db, vec![], TypeId::void(db));
        for other in [
            TypeId::string(db),
            TypeId::class_string(db, None),
            TypeId::array(db, TypeId::int(db), TypeId::int(db)),
            TypeId::class(db, "Closure", vec![]),
            TypeId::object(db),
        ] {
            assert_eq!(judge(&f, callable, other), Proof::CannotProve);
            assert_eq!(judge(&f, other, callable), Proof::CannotProve);
        }
    }

    #[test]
    fn a_resolved_hierarchy_proves_and_refutes() {
        let f = fixture(&[
            "<?php class Entity {} interface Timestamped {}",
            "<?php class User extends Entity implements Timestamped {}",
            "<?php class Order {}",
        ]);
        let db = &f.db;
        let user = TypeId::class(db, "User", vec![]);
        assert_eq!(
            judge(&f, user, TypeId::class(db, "Entity", vec![])),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, user, TypeId::class(db, "Timestamped", vec![])),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, TypeId::class(db, "Entity", vec![]), user),
            Proof::Fails
        );
        assert_eq!(
            judge(&f, user, TypeId::class(db, "Order", vec![])),
            Proof::Fails
        );
    }

    #[test]
    fn grandparents_count_and_generic_targets_stay_invariant() {
        let f = fixture(&["<?php class A {} class B extends A {} class C extends B {}"]);
        let db = &f.db;
        let c = TypeId::class(db, "C", vec![]);
        assert_eq!(judge(&f, c, TypeId::class(db, "A", vec![])), Proof::Holds);
        // A parameterized target cannot be proven through erasure.
        assert_eq!(
            judge(&f, c, TypeId::class(db, "A", vec![TypeId::int(db)])),
            Proof::CannotProve
        );
    }

    #[test]
    fn boundaries_answer_cannot_prove() {
        let f = fixture(&[
            // Extends a class that exists nowhere in the file set.
            "<?php class Repository extends ServiceEntityRepository {}",
            // A genuine cycle, broken by linearization.
            "<?php class Ouro extends Boros {} class Boros extends Ouro {}",
        ]);
        let db = &f.db;
        let repository = TypeId::class(db, "Repository", vec![]);
        assert_eq!(
            judge(
                &f,
                repository,
                TypeId::class(db, "ObjectRepository", vec![])
            ),
            Proof::CannotProve
        );
        let ouro = TypeId::class(db, "Ouro", vec![]);
        assert_eq!(
            judge(&f, ouro, TypeId::class(db, "Unrelated", vec![])),
            Proof::CannotProve
        );
        // An unknown candidate class is undecidable too.
        assert_eq!(
            judge(
                &f,
                TypeId::class(db, "Ghost", vec![]),
                TypeId::class(db, "Entity", vec![])
            ),
            Proof::CannotProve
        );
    }

    #[test]
    fn enum_cases_inherit_through_their_enum_hierarchy() {
        let f = fixture(&[
            "<?php interface HasLabel {} enum Status implements HasLabel { case Active; }",
        ]);
        let db = &f.db;
        let case = TypeId::enum_case(db, "Status", "Active");
        assert_eq!(
            judge(&f, case, TypeId::class(db, "HasLabel", vec![])),
            Proof::Holds
        );
    }

    #[test]
    fn numeric_string_literals_follow_php_8_semantics() {
        let f = fixture(&[]);
        let db = &f.db;
        let numeric = TypeId::numeric_string(db);
        for value in ["5.", ".5", "5.e3", "+1.5", " 42 ", "1e10", "007"] {
            assert_eq!(
                judge(&f, TypeId::string_literal(db, value), numeric),
                Proof::Holds,
                "expected '{value}' to be numeric"
            );
        }
        for value in ["", ".", "e5", "5..", "1.2.3", "abc", "0x1A"] {
            assert_eq!(
                judge(&f, TypeId::string_literal(db, value), numeric),
                Proof::Fails,
                "expected '{value}' to be non-numeric"
            );
        }
    }
}
