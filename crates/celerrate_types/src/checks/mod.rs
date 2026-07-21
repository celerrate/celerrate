//! The typed check families: unknown members, nullability, argument
//! types (design section 8). Verdicts are range-free — keyed by
//! `(AstId, ExpressionId)` — and reconcile to `TextRange` through the
//! body source map only in the rule framework's typed-body phase, so an
//! edit above a body backdates every verdict and re-runs only the
//! mapping.

use std::cell::RefCell;
use std::collections::BTreeSet;

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{
    AstId, BodyIr, BodyQuery, DeclarationKind, ExpressionId, MemberKind, UseTables, body_ir,
    item_tree, member_tree,
};
use celerrate_stubs::StubIndexInput;

use crate::InterproceduralEdgeCounts;
use crate::inference::{BodyOwner, InferredBody, body_owner, inferred_body_types};
use crate::records::FileDependencies;

pub(crate) mod arguments;
pub(crate) mod members;
pub(crate) mod nullability;
pub(crate) mod receivers;
#[cfg(test)]
pub(crate) mod test_support;

/// How one argument is addressed in a message: by 1-based position or
/// by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentLabel {
    Positional(usize),
    Named(String),
}

/// One range-free finding: the body it lives in, the arena expression
/// it anchors to, and what went wrong. Payloads are pre-rendered
/// display strings so the record is plain `Eq` data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedVerdict {
    pub body: AstId,
    pub expression: ExpressionId,
    pub kind: TypedVerdictKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedVerdictKind {
    UnknownMethod {
        member: String,
        receiver: String,
    },
    UnknownProperty {
        member: String,
        receiver: String,
    },
    UnknownClassConstant {
        member: String,
        receiver: String,
    },
    UnknownEnumCase {
        member: String,
        receiver: String,
    },
    NullDereference {
        member: String,
        receiver: String,
    },
    ArgumentType {
        label: ArgumentLabel,
        callee: String,
        expected: String,
        given: String,
    },
    TooFewArguments {
        callee: String,
        given: usize,
        required: usize,
    },
    TooManyArguments {
        callee: String,
        given: usize,
        accepted: usize,
    },
    UnknownNamedArgument {
        callee: String,
        name: String,
    },
}

/// One file's typed findings plus the inference instrument the
/// orchestration layer aggregates (plan 5's decision 13 lands here).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TypedFileResult {
    pub verdicts: Vec<TypedVerdict>,
    pub bodies: u32,
    pub edge_counts: InterproceduralEdgeCounts,
    /// Plan 9a, task 3: every body's [`crate::records::TypedDependencies`]
    /// (converted to [`FileDependencies`]'s `TypeId`-free shape) unioned
    /// with the checks' own consulted-class set
    /// (`CheckContext::dependencies`) — recorded even for a body that
    /// produced no verdict at all, because absence is a verdict too, and
    /// its revalidation needs the record just as much as a reported
    /// one's does.
    pub dependencies: FileDependencies,
}

/// Everything one body's walkers need, borrowed once. `namespace` and
/// `tables` mirror the `FlowContext` construction in
/// `inferred_body_types` (the owner's namespace, the file's use
/// tables) so scoped subjects (`Foo::bar()`) resolve written names
/// exactly as inference does.
///
/// Every field below is read only starting with the tasks that fill in
/// `members`/`nullability`/`arguments`/`receivers` (tasks 3-9); until
/// then it is constructed and unread, hence the crate-wide
/// `dead_code` allow just below.
#[allow(dead_code)]
pub(crate) struct CheckContext<'db, 'body> {
    pub db: &'db dyn salsa::Database,
    pub files: AnalyzedFileSet,
    pub stubs: StubIndexInput,
    pub configuration: ProjectConfiguration,
    pub file: SourceFile,
    pub body: AstId,
    pub ir: &'body BodyIr,
    pub inferred: &'body InferredBody<'db>,
    pub owner: Option<&'body BodyOwner>,
    pub namespace: String,
    pub tables: UseTables,
    /// Plan 9a, task 3: every class whose surface the checks family
    /// consulted (`member_existence`/`atom_existence`,
    /// `resolved_call_signature`, the coercion family's own
    /// `lookup_member`). A `RefCell`, not a plain field: `CheckContext`
    /// is passed as `&CheckContext` throughout `receivers.rs`,
    /// `members.rs`, `nullability.rs`, and `arguments.rs` (the shared
    /// read-only-context idiom every check function already uses), so
    /// interior mutability is the only way to thread a mutable
    /// recording set through without rewriting every one of those
    /// signatures to `&mut CheckContext` — a change task 3 does not
    /// otherwise call for. Each recording site only ever inserts into
    /// this set (never reads it back mid-walk), so a borrow is never
    /// held across a call into another function: no risk of the
    /// `RefCell`'s own panic-on-conflict path.
    pub dependencies: RefCell<BTreeSet<String>>,
}

/// One body's typed findings plus the classes the checks family
/// consulted reaching them — [`body_typed_verdicts`]'s return shape.
/// Drained from `CheckContext::dependencies` once the three check
/// families finish (task 3): recorded even when `verdicts` stays empty,
/// because absence is a verdict too, and its revalidation needs the
/// record just as much as a reported one's does.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct BodyTypedResult {
    pub verdicts: Vec<TypedVerdict>,
    pub classes: BTreeSet<String>,
}

/// The typed findings of one body. Tracked per body on purpose:
/// editing one body never re-checks its siblings (harness 2).
#[salsa::tracked(returns(ref))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn body_typed_verdicts<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> BodyTypedResult {
    let Some(ir) = body_ir(db, file, body).as_ref() else {
        return BodyTypedResult::default();
    };
    let Some(inferred) = inferred_body_types(db, files, stubs, configuration, file, body).as_ref()
    else {
        return BodyTypedResult::default();
    };
    let owner = body_owner(db, file, body).as_ref();
    let namespace = match owner {
        Some(BodyOwner::Function(function)) => function.namespace.clone(),
        Some(BodyOwner::Method { namespace, .. }) => namespace.clone(),
        None => String::new(),
    };
    let context = CheckContext {
        db,
        files,
        stubs,
        configuration,
        file,
        body: body.ast_id(db),
        ir,
        inferred,
        owner,
        tables: UseTables::for_namespace(item_tree(db, file), &namespace),
        namespace,
        dependencies: RefCell::new(BTreeSet::new()),
    };
    let mut verdicts = Vec::new();
    members::check(&context, &mut verdicts);
    nullability::check(&context, &mut verdicts);
    arguments::check(&context, &mut verdicts);
    BodyTypedResult {
        verdicts,
        classes: context.dependencies.into_inner(),
    }
}

/// The typed findings of one file: every body the member tree names
/// (free functions and methods of non-trait class-likes), in tree
/// order, plus the summed inference instrument. Trait-owned bodies
/// are skipped (decision 3: plan 6 analyzes them per using class;
/// checking one against the trait's own surface is a false-positive
/// class). Debt ledger: typed checks never run inside trait-owned
/// bodies — owner: a per-using-class walk over plan 6's
/// `InferenceContext` seam, future. Top-level statement code has no
/// member-tree body and stays unchecked by the typed families — owner:
/// the rule framework of sub-project 4, or earlier if the corpus
/// demands it.
#[salsa::tracked(returns(ref))]
pub fn typed_file_verdicts(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
) -> TypedFileResult {
    let tree = member_tree(db, file);
    let mut result = TypedFileResult::default();
    let function_bodies = tree.functions.iter().map(|function| function.ast_id);
    let method_bodies = tree
        .classes
        .iter()
        .filter(|class| class.kind != DeclarationKind::Trait)
        .flat_map(|class| {
            class
                .members
                .iter()
                .filter(|member| member.kind == MemberKind::Method)
                .map(|member| member.ast_id)
        });
    for ast_id in function_bodies.chain(method_bodies) {
        let body = BodyQuery::new(db, ast_id);
        let body_result = body_typed_verdicts(db, files, stubs, configuration, file, body);
        result.verdicts.extend(body_result.verdicts.iter().cloned());
        result
            .dependencies
            .classes
            .extend(body_result.classes.iter().cloned());
        if let Some(inferred) =
            inferred_body_types(db, files, stubs, configuration, file, body).as_ref()
        {
            result.bodies += 1;
            result.edge_counts.accumulate(&inferred.edge_counts);
            result
                .dependencies
                .extend_from_body(db, &inferred.dependencies);
        }
    }
    // The eq-cutoff contract (`TypedDependencies`'s own rustdoc)
    // extends to this file-level aggregate too: deterministic order
    // regardless of the member tree's own body ordering.
    result.dependencies.finish();
    result
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use super::test_support::{fixture, handle_of};
    use super::typed_file_verdicts;

    /// Plan 9a, task 3: a file whose body dereferences `$user->name`
    /// (`User` defined in a SEPARATE file, resolved through the
    /// project's global symbol index) — `typed_file_verdicts` must
    /// record `App\User`'s folded key in `dependencies.classes` even
    /// though the property genuinely exists and no diagnostic fires.
    /// Absence is a verdict too, and its revalidation (tasks 7 and 9)
    /// needs the record exactly as much as a reported unknown-member
    /// finding's does — otherwise a later edit to `User` (e.g. removing
    /// `$name`) would leave this file's stale "no defect" verdict
    /// uninvalidated.
    #[test]
    fn the_checks_record_the_receivers_they_consult() {
        let fixture = fixture(&[
            r#"<?php
function scene(\App\User $u): void { $u->name; }
"#,
            r#"<?php
namespace App;
class User { public string $name = ''; }
"#,
        ]);
        let result = typed_file_verdicts(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            handle_of(&fixture, 0),
        );
        assert!(
            result.verdicts.is_empty(),
            "the property genuinely exists: {:?}",
            result.verdicts,
        );
        assert!(
            result.dependencies.classes.contains("app\\user"),
            "the cross-file receiver's class is recorded: {:?}",
            result.dependencies,
        );
    }

    /// Issue #51's exact reproduction: a self-recursive free function
    /// declared with NO caller ahead of it. `typed_file_verdicts` walks it
    /// through `body_typed_verdicts`, whose `inferred_body_types` demand
    /// used to re-enter its own still-active claim when the body's
    /// recursive call resolved back through `inferred_function_return` —
    /// salsa's `Panic` strategy, a crash on valid recursive PHP. The
    /// public `inferred_body_types` now warms the cycle-safe return query
    /// first, so this must answer (no verdicts: the body is clean) rather
    /// than panic.
    #[test]
    fn a_recursive_function_type_checks_without_a_caller() {
        let fixture = fixture(&[r#"<?php
function down(int $n) {
    if ($n <= 0) { return 0; }
    return down($n - 1);
}
"#]);
        let result = typed_file_verdicts(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            handle_of(&fixture, 0),
        );
        assert!(
            result.verdicts.is_empty(),
            "a clean self-recursive function raises no typed verdict: {:?}",
            result.verdicts,
        );
    }

    /// The method-recursion twin, entering the cycle through
    /// `inferred_method_return` instead: `$this->down(...)` inside the
    /// method's own body, again with no caller anywhere.
    #[test]
    fn a_recursive_method_type_checks_without_a_caller() {
        let fixture = fixture(&[r#"<?php
class Walker {
    public function down(int $n) {
        if ($n <= 0) { return 0; }
        return $this->down($n - 1);
    }
}
"#]);
        let result = typed_file_verdicts(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            handle_of(&fixture, 0),
        );
        assert!(
            result.verdicts.is_empty(),
            "a clean self-recursive method raises no typed verdict: {:?}",
            result.verdicts,
        );
    }
}
