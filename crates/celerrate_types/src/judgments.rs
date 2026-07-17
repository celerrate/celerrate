//! The three-valued judgments (spec section 3): `Holds`, `Fails`
//! (value-set inclusion refuted), `CannotProve` (undecidable with
//! available information). Every consumer states its posture toward
//! `CannotProve`; nothing here or above silently discards it.

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{
    ClassQuery, MemberKind, MemberQuery, SymbolSpace, folded_member_key, folded_symbol_key,
    linearized_class, lookup_member, stub_ancestors_of, stub_signature_table,
};
use celerrate_stubs::StubIndexInput;

use crate::representation::{StringConstraint, TypeData, TypeId};

/// The three-valued verdict of a typed judgment: `Holds` and `Fails` are
/// both decisions (`Fails` means value-set inclusion is refuted, not
/// merely unproven), while `CannotProve` means the judgment is
/// undecidable with the information available. Consumer contract:
/// `CannotProve` is never a silent discard, whether that means folding
/// it into `Fails`, folding it into `Holds`, or dropping it before it
/// reaches a diagnostic. Each diagnostic family states its own posture
/// toward `CannotProve` (report, suppress, or downgrade) explicitly at
/// its own boundary; plan 8 is where those postures are declared.
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

/// The calling file's coercion posture (design section 8): strict
/// under `declare(strict_types=1)`, weak otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoercionMode {
    Strict,
    Weak,
}

/// May a `source` value be assigned where `target` is declared?
/// **Coercion never proves, it only un-fails**: this answers exactly
/// `subtype_of`'s verdict, except a `Fails` is upgraded to
/// `CannotProve` when a mode-legal runtime coercion could make the
/// call work. `Holds` and `CannotProve` verdicts pass through
/// untouched — the judgment never claims a set-theoretic proof that
/// does not hold, so an upgraded verdict is silence, exactly what
/// "coercions PHP performs at runtime are not reported" (the argument
/// family, plan 8) demands.
#[salsa::tracked]
pub fn assignable_to<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    source: TypeId<'db>,
    target: TypeId<'db>,
    mode: CoercionMode,
) -> Proof {
    match subtype_of(db, files, stubs, configuration, source, target) {
        Proof::Fails
            if coercion_could_apply(db, files, stubs, configuration, source, target, mode) =>
        {
            Proof::CannotProve
        }
        verdict => verdict,
    }
}

/// Whether a runtime coercion the mode permits could make the value
/// pass: int to float always (PHP performs it under strict types too);
/// in weak mode, scalar interchange (never from `null`, never
/// `mixed`) and a `Stringable` object against a string target. Union
/// sources must be entirely coercible (every constituent already
/// non-`Fails` or itself coercible); union targets need one coercible
/// arm.
///
/// This never consults the `Proof` a `mixed` candidate carries: a
/// `mixed` source can reach this function with a genuine `Fails` in
/// hand (the shipped `judge` refutes `mixed` candidates on purpose),
/// and `is_coercible_scalar`/`is_scalar_target` answer `false` for it
/// (and for `null`) so it is never silently un-failed here. The
/// argument family's walk (plan 8, decision 10) guards `mixed` and
/// per-constituent union fits before ever calling the judgment.
fn coercion_could_apply<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    source: TypeId<'db>,
    target: TypeId<'db>,
    mode: CoercionMode,
) -> bool {
    let sources = source.constituents(db);
    if sources.len() > 1 {
        return sources.into_iter().all(|part| {
            subtype_of(db, files, stubs, configuration, part, target) != Proof::Fails
                || coercion_could_apply(db, files, stubs, configuration, part, target, mode)
        });
    }
    let targets = target.constituents(db);
    if targets.len() > 1 {
        return targets
            .into_iter()
            .any(|part| coercion_could_apply(db, files, stubs, configuration, source, part, mode));
    }
    if is_int_family(db, source) && is_float_family(db, target) {
        return true;
    }
    if mode == CoercionMode::Weak {
        if is_coercible_scalar(db, source) && is_scalar_target(db, target) {
            return true;
        }
        if is_string_family(db, target) && is_stringable(db, files, stubs, configuration, source) {
            return true;
        }
    }
    false
}

/// The `int` family: both a literal (a singleton range) and the
/// general, unbounded, or partially bounded range share `TypeData::Int`.
fn is_int_family<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    matches!(of.data(db), TypeData::Int { .. })
}

/// The `float` family: a literal or the general type, both
/// `TypeData::Float`.
fn is_float_family<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    matches!(of.data(db), TypeData::Float { .. })
}

/// The `string` family: every `StringConstraint` variant is a
/// `TypeData::String`, never a `ClassString`.
fn is_string_family<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    matches!(of.data(db), TypeData::String { .. })
}

/// A scalar in PHP's sense: `bool`, `int`, `float`, or `string`.
/// Deliberately excludes `null` and `mixed` — the mixed caveat above —
/// so neither reaches weak-mode scalar interchange or the
/// `Stringable` probe as a source, and neither is ever accepted as a
/// scalar target.
fn is_scalar<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    matches!(
        of.data(db),
        TypeData::Bool { .. }
            | TypeData::Int { .. }
            | TypeData::Float { .. }
            | TypeData::String { .. }
    )
}

/// The weak-mode scalar-interchange source test: never `null`, never
/// `mixed`.
fn is_coercible_scalar<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    is_scalar(db, of)
}

/// The weak-mode scalar-interchange target test: the same predicate as
/// the source side (PHP's scalar interchange is symmetric in kind).
fn is_scalar_target<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    is_scalar(db, of)
}

/// Whether `source` denotes a class whose instances PHP converts to a
/// string at the `string` parameter boundary: one that resolves
/// `__toString` through [`lookup_member`] (own, inherited, or through
/// a stub boundary), or whose ancestry (source or stub) names
/// `Stringable` directly.
fn is_stringable<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    source: TypeId<'db>,
) -> bool {
    let TypeData::Class { name, .. } = source.data(db) else {
        return false;
    };
    let query = MemberQuery::new(
        db,
        name.clone(),
        MemberKind::Method,
        folded_member_key(MemberKind::Method, "__toString"),
    );
    if lookup_member(db, files, stubs, configuration, query).is_some() {
        return true;
    }
    let class = ClassQuery::new(db, name.clone());
    match linearized_class(db, files, stubs, configuration, class) {
        Some(linearized) => {
            linearized
                .ancestry
                .iter()
                .any(|edge| edge.resolved.as_deref() == Some("stringable"))
                || linearized
                    .stub_ancestors
                    .iter()
                    .any(|key| key == "stringable")
        }
        None => {
            let table = stub_signature_table(db, stubs);
            stub_ancestors_of(table, name)
                .reached
                .iter()
                .any(|key| key == "stringable")
        }
    }
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
        // Not a source class-like: judge through the stub graph.
        return judge_stub_hierarchy(db, context, candidate_name, target_name);
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
    if linearized.cyclic || linearized.has_opaque_edge {
        Proof::CannotProve
    } else {
        Proof::Fails
    }
}

/// The stub-graph verdict for a candidate with no source declaration:
/// breadth-first over the compiled parent links. An unknown start key,
/// or a key whose surface is missing mid-walk, keeps the answer
/// undecidable; a fully walked graph without the target refutes. The
/// visited set only guards revisits, so the queue's recorded order fixes
/// the result.
fn judge_stub_hierarchy(
    db: &dyn salsa::Database,
    context: JudgmentContext,
    candidate_name: &str,
    target_name: &str,
) -> Proof {
    let table = stub_signature_table(db, context.stubs);
    if table.class(candidate_name).is_none() {
        // Unknown class: undecidable, as before.
        return Proof::CannotProve;
    }
    let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut opaque = false;
    queue.push_back(candidate_name.to_owned());
    while let Some(key) = queue.pop_front() {
        if !visited.insert(key.clone()) {
            continue;
        }
        if key == target_name {
            return Proof::Holds;
        }
        let Some(surface) = table.class(&key) else {
            opaque = true;
            continue;
        };
        for parent in &surface.parents {
            queue.push_back(folded_symbol_key(SymbolSpace::ClassLike, parent));
        }
    }
    if opaque {
        Proof::CannotProve
    } else {
        Proof::Fails
    }
}

/// Whether `of` denotes a value set with exactly one member: `Null`, a
/// `Bool` literal, a singleton `Int` range (`minimum == maximum`, both
/// bounded), a `Float` literal, a `String` literal constraint, or an
/// `EnumCase`. Used by the target-union decomposition to tell a
/// single-valued candidate (checked exhaustively against each target
/// constituent) from a splittable one that could straddle a partition.
fn is_single_valued<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    match of.data(db) {
        TypeData::Null
        | TypeData::Bool { literal: Some(_) }
        | TypeData::Float { literal: Some(_) }
        | TypeData::String {
            constraint: StringConstraint::Literal(_),
        }
        | TypeData::EnumCase { .. } => true,
        TypeData::Int {
            minimum: Some(low),
            maximum: Some(high),
        } => low == high,
        _ => false,
    }
}

/// `value-of<SomeBackedEnum>` evaluates through member facts (plan
/// 2's recorded debt, settled here); every other `value-of` stays
/// symbolic.
///
/// Called at the entry of every `judge` invocation, this expansion
/// applies wherever `judge` recurses structurally, not just at the
/// outermost operands: union constituents, intersectands, array key
/// and value, shape fields, and callable parameters and returns each
/// re-enter `judge` and are expanded in turn. A `value-of<BackedEnum>`
/// buried inside, say, an array value (`array<int, value-of<Status>>`)
/// therefore also evaluates against its literal union, not just a
/// top-level occurrence.
///
/// This is sound: `enum_backing_union` is all-or-nothing ground truth
/// for a fully known backed enum, so expanding it at any nesting depth
/// can only replace a symbolic operand with the exact set of values it
/// denotes, never with an approximation. It is also strictly more
/// precise than restricting expansion to the top level, since it lets
/// nested `value-of` operands participate in the same union and array
/// reasoning as literals do. Termination is structural, not
/// depth-limited: an expanded union or array value contains only
/// literals (or the original operand when expansion did not apply),
/// never another `ValueOf`, so recursion cannot re-trigger expansion on
/// its own output.
///
/// This deliberately exceeds plan 3 task 13's "top-level only" wording.
/// That restriction was the plan's original intent, but the shipped
/// code expands at every `judge` entry point including the structural
/// recursion; review adjudicated the broader, more precise shipped
/// behavior as the keeper and directed that the documentation (and the
/// task report) be corrected to match it instead of narrowing the
/// code. See `a_nested_value_of_also_expands_through_structural_recursion`
/// below, which pins this behavior.
fn expand_value_of<'db>(
    db: &'db dyn salsa::Database,
    context: JudgmentContext,
    of: TypeId<'db>,
) -> TypeId<'db> {
    let TypeData::ValueOf { subject } = of.data(db) else {
        return of;
    };
    let TypeData::Class { name, .. } = subject.data(db) else {
        return of;
    };
    crate::declared::enum_backing_union(
        db,
        context.files,
        context.stubs,
        context.configuration,
        name,
    )
    .unwrap_or(of)
}

fn judge<'db>(
    db: &'db dyn salsa::Database,
    context: JudgmentContext,
    candidate: TypeId<'db>,
    target: TypeId<'db>,
) -> Proof {
    let candidate = expand_value_of(db, context, candidate);
    let target = expand_value_of(db, context, target);
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
        let folded = Proof::any(
            constituents
                .iter()
                .map(|part| judge(db, context, candidate, *part)),
        );
        // A `Fails` here means every constituent refuted the candidate
        // individually, which is sound only when the candidate cannot
        // itself straddle the union's partition. A single-valued
        // candidate (one concrete runtime value) is checked exhaustively
        // by that per-constituent walk, so its `Fails` stands. So does a
        // `Fails` against a union where at most one constituent shares
        // the candidate's top-level kind: there is nothing of that kind
        // to straddle across. Otherwise the candidate is a splittable
        // range or set that could be covered by the union's kind-sharing
        // constituents together without any one of them covering it
        // alone (`int <: int<min, -1>|int<0, max>` holds even though
        // `int` fails against each half), so the refutation is demoted
        // to `CannotProve`.
        if folded == Proof::Fails && !is_single_valued(db, candidate) {
            let candidate_rank = crate::ordering::rank(candidate.data(db));
            let matching_kinds = constituents
                .iter()
                .filter(|part| crate::ordering::rank(part.data(db)) == candidate_rank)
                .count();
            if matching_kinds >= 2 {
                return Proof::CannotProve;
            }
        }
        return folded;
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
        // string, class-string, array callable, shape, class, or enum case
        // may or may not exist at runtime: PHP enums may declare
        // `__invoke`), so invokable objects and callable strings,
        // class-strings, arrays, shapes, classes, and enum cases stay
        // undecidable whichever side is the candidate.
        (TypeData::Callable { .. }, TypeData::Object)
        | (
            TypeData::Object | TypeData::Class { .. } | TypeData::EnumCase { .. },
            TypeData::Callable { .. },
        )
        | (TypeData::String { .. } | TypeData::ClassString { .. }, TypeData::Callable { .. })
        | (TypeData::Array { .. } | TypeData::Shape { .. }, TypeData::Callable { .. })
        | (
            TypeData::Callable { .. },
            TypeData::String { .. }
            | TypeData::ClassString { .. }
            | TypeData::Array { .. }
            | TypeData::Shape { .. }
            | TypeData::Class { .. }
            | TypeData::EnumCase { .. },
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
    use celerrate_stubs::{
        StubAvailability, StubClassSurface, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    use super::{CoercionMode, Nullability, Proof, assignable_to, nullability, subtype_of};
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
        let stubs = StubIndexInput::builder(crate::inheritance::test_support::minimal_stub_index())
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

    /// A fixture whose stub payload carries real class surfaces. Every
    /// surface key and every parent it names becomes a `StubSymbol`, plus
    /// a default `Exception` symbol without a surface, so a source class
    /// extending `\Exception` records an opaque stub boundary.
    fn fixture_with_stub_classes(
        sources: &[&str],
        classes: Vec<(String, StubClassSurface)>,
    ) -> Fixture {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let files = AnalyzedFileSet::new(&db, handles);
        let mut names: Vec<String> = vec!["Exception".to_owned()];
        for (name, surface) in &classes {
            names.push(name.clone());
            for parent in &surface.parents {
                names.push(parent.clone());
            }
        }
        names.sort();
        names.dedup();
        let symbols: Vec<StubSymbol> = names
            .into_iter()
            .map(|name| StubSymbol {
                name,
                kind: StubSymbolKind::Class,
                availability: StubAvailability::ALWAYS,
            })
            .collect();
        let stubs = StubIndexInput::builder(StubIndex::new(symbols, vec![], classes))
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

    /// The `RuntimeException -> Exception -> Throwable` stub surface
    /// chain shared by the transitive-hierarchy tests.
    fn exception_chain() -> Vec<(String, StubClassSurface)> {
        vec![
            (
                "RuntimeException".to_owned(),
                StubClassSurface {
                    parents: vec!["Exception".to_owned()],
                    members: vec![],
                },
            ),
            (
                "Exception".to_owned(),
                StubClassSurface {
                    parents: vec!["Throwable".to_owned()],
                    members: vec![],
                },
            ),
            ("Throwable".to_owned(), StubClassSurface::default()),
        ]
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
        // `Countable` now resolves from the default stub surface, but the
        // answers are unaffected: `counted <: Foo` holds by intersection
        // decomposition (no hierarchy walk), and `Foo <: counted` stays
        // `CannotProve` because the subject `Foo` is neither declared nor
        // stubbed, so `Foo <: Countable` is undecidable regardless of
        // whether `Countable` itself is known.
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
                TypeId::mixed(db),
                CoercionMode::Strict,
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
            TypeId::enum_case(db, "Status", "Active"),
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
    fn a_transitive_stub_hierarchy_proves_and_a_fully_walked_one_refutes() {
        let fixture = fixture_with_stub_classes(
            &["<?php class MyError extends RuntimeException {}"],
            exception_chain(),
        );
        let db = &fixture.db;
        let my_error = TypeId::class(db, "MyError", vec![]);
        let exception = TypeId::class(db, "Exception", vec![]);
        let countable = TypeId::class(db, "Countable", vec![]);
        assert_eq!(judge(&fixture, my_error, exception), Proof::Holds);
        // Fully walked and absent: refuted, no longer CannotProve.
        assert_eq!(judge(&fixture, my_error, countable), Proof::Fails);
    }

    #[test]
    fn a_stub_only_candidate_judges_through_the_blob_graph() {
        let fixture = fixture_with_stub_classes(&["<?php"], exception_chain());
        let db = &fixture.db;
        let runtime = TypeId::class(db, "RuntimeException", vec![]);
        let throwable = TypeId::class(db, "Throwable", vec![]);
        let countable = TypeId::class(db, "Countable", vec![]);
        assert_eq!(judge(&fixture, runtime, throwable), Proof::Holds);
        // Fully walked without the target: refuted.
        assert_eq!(judge(&fixture, runtime, countable), Proof::Fails);
    }

    #[test]
    fn a_missing_stub_surface_stays_undecidable() {
        // The `Exception` symbol carries no compiled surface: the stub
        // boundary is opaque, so the answer stays CannotProve.
        let fixture =
            fixture_with_stub_classes(&["<?php class AppException extends \\Exception {}"], vec![]);
        let db = &fixture.db;
        let app_exception = TypeId::class(db, "AppException", vec![]);
        let throwable = TypeId::class(db, "Throwable", vec![]);
        assert_eq!(
            judge(&fixture, app_exception, throwable),
            Proof::CannotProve,
        );
        // A stub-only candidate whose surface is missing is undecidable too.
        let unknown = TypeId::class(db, "Exception", vec![]);
        assert_eq!(judge(&fixture, unknown, throwable), Proof::CannotProve);
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
    fn a_splittable_candidate_crossing_a_partitioned_union_is_undecidable() {
        let f = fixture(&[]);
        let db = &f.db;
        let split = TypeId::union(
            db,
            [
                TypeId::int_range(db, None, Some(-1)),
                TypeId::int_range(db, Some(0), None),
            ],
        );
        assert_eq!(judge(&f, TypeId::int(db), split), Proof::CannotProve);
        // A literal still refutes decisively: 5 is in neither part.
        let gapped = TypeId::union(
            db,
            [
                TypeId::int_range(db, Some(0), Some(3)),
                TypeId::int_range(db, Some(10), Some(20)),
            ],
        );
        assert_eq!(judge(&f, TypeId::int_literal(db, 5), gapped), Proof::Fails);
        // A candidate of a kind absent from the union still refutes.
        assert_eq!(judge(&f, TypeId::float(db), split), Proof::Fails);
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

    #[test]
    fn value_of_a_backed_enum_expands_to_its_literal_union() {
        let fixture = fixture(&["<?php enum Status: string {\n\
             case Active = 'active';\n\
             case Retired = 'retired';\n\
         }"]);
        let db = &fixture.db;
        let value_of_status = TypeId::value_of(db, TypeId::class(db, "Status", vec![]));
        let literals = TypeId::union(
            db,
            [
                TypeId::string_literal(db, "active"),
                TypeId::string_literal(db, "retired"),
            ],
        );
        assert_eq!(judge(&fixture, value_of_status, literals), Proof::Holds);
        assert_eq!(
            judge(
                &fixture,
                TypeId::string_literal(db, "active"),
                value_of_status
            ),
            Proof::Holds,
        );
        assert_eq!(
            judge(
                &fixture,
                TypeId::string_literal(db, "ghost"),
                value_of_status
            ),
            Proof::Fails,
        );
    }

    #[test]
    fn value_of_a_pure_or_unknown_enum_stays_symbolic() {
        let fixture = fixture(&["<?php enum Suit { case Hearts; }"]);
        let db = &fixture.db;
        let value_of_suit = TypeId::value_of(db, TypeId::class(db, "Suit", vec![]));
        // No backing values: undecidable, exactly as before this task.
        assert_eq!(
            judge(&fixture, value_of_suit, TypeId::string(db)),
            Proof::CannotProve,
        );
        let value_of_ghost = TypeId::value_of(db, TypeId::class(db, "Ghost", vec![]));
        assert_eq!(
            judge(&fixture, value_of_ghost, TypeId::string(db)),
            Proof::CannotProve,
        );
    }

    #[test]
    fn a_nested_value_of_also_expands_through_structural_recursion() {
        let fixture = fixture(&["<?php enum Status: string {\n\
             case Active = 'active';\n\
             case Retired = 'retired';\n\
         }"]);
        let db = &fixture.db;
        let value_of_status = TypeId::value_of(db, TypeId::class(db, "Status", vec![]));
        let candidate = TypeId::array(db, TypeId::int(db), value_of_status);
        let target = TypeId::array(db, TypeId::int(db), TypeId::string(db));
        // The nested value-of expands through the array-value recursion:
        // a deliberate, recorded widening of the plan's top-level wording.
        assert_eq!(judge(&fixture, candidate, target), Proof::Holds);
    }

    #[test]
    fn coercion_never_proves_it_only_un_fails() {
        let fixture = fixture(&[
            "<?php class WithString { public function __toString(): string { return ''; } } class Plain {}",
        ]);
        let db = &fixture.db;
        let judge = |source, target, mode| {
            assignable_to(
                db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                source,
                target,
                mode,
            )
        };
        let int = TypeId::int(db);
        let float = TypeId::float(db);
        let string = TypeId::string(db);
        let bool_type = TypeId::bool(db);
        let null = TypeId::null(db);
        let stringable = TypeId::class(db, "withstring", vec![]);
        let plain = TypeId::class(db, "plain", vec![]);
        use CoercionMode::{Strict, Weak};
        use Proof::{CannotProve, Fails, Holds};
        // Subtyping is untouched by the mode.
        assert_eq!(judge(int, int, Strict), Holds);
        assert_eq!(judge(int, string, Strict), Fails);
        // The one strict-mode coercion PHP performs: int to float.
        assert_eq!(judge(int, float, Strict), CannotProve);
        // Weak mode un-fails scalar interchange…
        assert_eq!(judge(string, int, Weak), CannotProve);
        assert_eq!(judge(bool_type, string, Weak), CannotProve);
        assert_eq!(judge(int, string, Weak), CannotProve);
        // …but never null, and never non-scalar targets.
        assert_eq!(judge(null, string, Weak), Fails);
        assert_eq!(judge(string, plain, Weak), Fails);
        // Stringable passes a string parameter in weak mode only.
        assert_eq!(judge(stringable, string, Weak), CannotProve);
        assert_eq!(judge(stringable, string, Strict), Fails);
        assert_eq!(judge(plain, string, Weak), Fails);
    }

    /// `coercion_could_apply`'s union arms in isolation: a source union
    /// needs every constituent coercible (`all`), a target union needs
    /// only one (`any`). Each assertion is built so the verdict
    /// genuinely depends on the reduction, not on a single-constituent
    /// shortcut: `int|Plain` and `Plain|Address` mix one coercible and
    /// one refuted-and-uncoercible constituent (or two refuted ones),
    /// so a broken `all`/`any` (say, `any` swapped in for the source
    /// side, or the target loop stopping at the first constituent)
    /// would flip these answers.
    #[test]
    fn union_source_and_target_reduction_is_load_bearing() {
        let fixture = fixture(&[
            "<?php class WithString { public function __toString(): string { return ''; } } class Plain {} class Address {}",
        ]);
        let db = &fixture.db;
        let judge = |source, target, mode| {
            assignable_to(
                db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                source,
                target,
                mode,
            )
        };
        use CoercionMode::Weak;
        use Proof::{CannotProve, Fails};
        let int = TypeId::int(db);
        let bool_type = TypeId::bool(db);
        let string = TypeId::string(db);
        let plain = TypeId::class(db, "plain", vec![]);
        let address = TypeId::class(db, "address", vec![]);

        // Source union: one non-coercible constituent (`Plain`, no
        // `__toString`, no `Stringable` ancestry) sinks the whole
        // `all()`, even though `int` alone would un-fail.
        let int_or_plain = TypeId::union(db, [int, plain]);
        assert_eq!(judge(int_or_plain, string, Weak), Fails);

        // Source union: every constituent individually coercible to a
        // scalar target un-fails the whole union.
        let int_or_bool = TypeId::union(db, [int, bool_type]);
        assert_eq!(judge(int_or_bool, string, Weak), CannotProve);

        // Target union: `int` cannot subtype `string` or `Plain`, but
        // one arm (`string`) is reachable through weak-mode scalar
        // interchange, so `any()` un-fails the whole union.
        let string_or_plain = TypeId::union(db, [string, plain]);
        assert_eq!(judge(int, string_or_plain, Weak), CannotProve);

        // Target union with no coercible arm at all (two plain classes,
        // neither scalar nor `Stringable`) stays refuted.
        let plain_or_address = TypeId::union(db, [plain, address]);
        assert_eq!(judge(int, plain_or_address, Weak), Fails);
    }

    /// `is_stringable`'s stub-only branch: a class with no source
    /// declaration at all, known only through a synthetic stub surface
    /// whose parent names `Stringable` — reached only through
    /// `stub_ancestors_of`, never through `lookup_member` (this stub
    /// surface declares no members, so no `__toString` is ever found)
    /// nor through `linearized_class` (which answers `None` for a
    /// class with no source declaration).
    #[test]
    fn a_stub_only_stringable_ancestry_un_fails_in_weak_mode_only() {
        let fixture = fixture_with_stub_classes(
            &["<?php"],
            vec![(
                "StubStringable".to_owned(),
                StubClassSurface {
                    parents: vec!["Stringable".to_owned()],
                    members: vec![],
                },
            )],
        );
        let db = &fixture.db;
        let stub_stringable = TypeId::class(db, "StubStringable", vec![]);
        let string = TypeId::string(db);
        assert_eq!(
            assignable_to(
                &fixture.db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                stub_stringable,
                string,
                CoercionMode::Weak,
            ),
            Proof::CannotProve
        );
        assert_eq!(
            assignable_to(
                &fixture.db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                stub_stringable,
                string,
                CoercionMode::Strict,
            ),
            Proof::Fails
        );
    }
}
