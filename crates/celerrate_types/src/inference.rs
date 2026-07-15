//! Demand-driven inference: the per-body flow walk and (Task 10) the
//! interprocedural fixpoint. One query per body produces the full
//! expression type table plus the joined return type; nothing here
//! ever touches a syntax tree — the walker consumes the range-free
//! body IR, so LRU eviction of parse trees is structurally safe.
//!
//! Memory lever, named for plan 9b (design section 6): the full
//! expression type tables produced by [`inferred_body_types`] are the
//! LRU candidates (`salsa` supports `lru = N` on tracked functions),
//! while inferred returns stay resident — small, hot, and the
//! fixpoint's currency. No capacity is set here: plan 9b owns the
//! peak-memory measurement against its budget and pulls the lever
//! with a number.

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{
    BodyQuery, ExpressionId, FreeFunction, Member, MemberKind, MemberSignature, SymbolSpace,
    UseTables, body_ir, folded_member_key, folded_symbol_key, fully_qualified_name, item_tree,
    member_tree,
};
use celerrate_stubs::StubIndexInput;

use crate::declared::{declared_function_signature, declared_member_signature};
use crate::flow::{FlowContext, walk_body};
use crate::representation::TypeId;

/// The interprocedural edge classes one body's inference took, as
/// pure data: the design's residual instrument ("how many results
/// depend on *inferred* returns"), aggregated by the first
/// orchestration-layer consumer (plan 8).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, salsa::Update)]
pub struct InterproceduralEdgeCounts {
    /// Call results taken from a declared (native or annotated) return.
    pub declared_return_edges: u32,
    /// Call results taken from another body's inferred return.
    pub inferred_return_edges: u32,
    /// Call results taken from a dynamic type provider's claim.
    pub provider_edges: u32,
}

/// The inference result of one body: a type per arena expression, the
/// joined return type, and the edge-count instrument. `Eq`-comparable
/// on purpose: a body edit that leaves every inferred result identical
/// backdates, and dependents are spared.
#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct InferredBody<'db> {
    pub expression_types: Vec<TypeId<'db>>,
    pub return_type: TypeId<'db>,
    pub edge_counts: InterproceduralEdgeCounts,
}

impl<'db> InferredBody<'db> {
    pub fn expression_type(&self, id: ExpressionId) -> Option<TypeId<'db>> {
        self.expression_types.get(id.index() as usize).copied()
    }
}

/// The declaration a body belongs to: a free function, or a method of
/// a class-like (whose folded key is `None` for an anonymous class —
/// decision 12: no folded symbol key exists to resolve members
/// against). `Eq` so the tracked projection backdates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BodyOwner {
    Function(FreeFunction),
    Method {
        class_key: Option<String>,
        namespace: String,
        member: Member,
    },
}

/// Resolves the owning declaration of one body through the member
/// projection. `None` when the identity names no function or method
/// of `file`. A tracked query on purpose, and load-bearing for the
/// invalidation story: `member_tree` changes whenever *any* member of
/// the file changes, but this per-body projection backdates for every
/// body whose own declaration did not — so editing one signature
/// re-infers that member's body and no other (the design's harness-2
/// contract, pinned in Task 12).
#[salsa::tracked(returns(ref))]
pub(crate) fn body_owner<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<BodyOwner> {
    let ast_id = body.ast_id(db);
    let tree = member_tree(db, file);
    if let Some(function) = tree
        .functions
        .iter()
        .find(|function| function.ast_id == ast_id)
    {
        return Some(BodyOwner::Function(function.clone()));
    }
    for class in &tree.classes {
        let Some(member) = class.members.iter().find(|member| member.ast_id == ast_id) else {
            continue;
        };
        if member.kind != MemberKind::Method {
            return None;
        }
        let class_key = class.name.as_deref().map(|name| {
            folded_symbol_key(
                SymbolSpace::ClassLike,
                &fully_qualified_name(&class.namespace, name),
            )
        });
        return Some(BodyOwner::Method {
            class_key,
            namespace: class.namespace.clone(),
            member: member.clone(),
        });
    }
    None
}

/// The inference of one body: `None` when the identity carries no
/// body in `file` (mirroring `body_ir`). Task 3 replaces the
/// all-`mixed` table with the flow walk.
#[salsa::tracked(returns(ref))]
pub fn inferred_body_types<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<InferredBody<'db>> {
    let ir = body_ir(db, file, body).as_ref()?;
    let owner = body_owner(db, file, body);
    let (namespace, owner_class_key, method_is_static, parameters) = match owner {
        Some(BodyOwner::Function(function)) => {
            let key = folded_symbol_key(
                SymbolSpace::Function,
                &fully_qualified_name(&function.namespace, &function.name),
            );
            let declared = declared_function_signature(
                db,
                files,
                stubs,
                configuration,
                crate::declared::FunctionQuery::new(db, key),
            );
            (
                function.namespace.clone(),
                None,
                false,
                seeded_parameters(db, declared.as_ref(), &function.signature),
            )
        }
        Some(BodyOwner::Method {
            class_key,
            namespace,
            member,
        }) => {
            let declared = class_key.as_ref().and_then(|key| {
                declared_member_signature(
                    db,
                    files,
                    stubs,
                    configuration,
                    celerrate_semantics::MemberQuery::new(
                        db,
                        key.clone(),
                        MemberKind::Method,
                        folded_member_key(MemberKind::Method, &member.name),
                    ),
                )
            });
            (
                namespace.clone(),
                class_key.clone(),
                member.flags.is_static,
                seeded_parameters(db, declared.as_ref(), &member.signature),
            )
        }
        None => (String::new(), None, false, Vec::new()),
    };
    let tables = UseTables::for_namespace(item_tree(db, file), &namespace);
    let context = FlowContext {
        db,
        files,
        stubs,
        configuration,
        ir,
        namespace,
        tables,
        owner_class_key,
        method_is_static,
        parameters,
    };
    let result = walk_body(&context);
    Some(InferredBody {
        expression_types: result.expression_types,
        return_type: result.return_type,
        edge_counts: result.edge_counts,
    })
}

/// Parameter names paired with their seeded types: the declared
/// parameter type (the plan-3 layer, annotation-refined) or `mixed`,
/// a variadic parameter collecting into a list of it.
fn seeded_parameters<'db>(
    db: &'db dyn salsa::Database,
    declared: Option<&crate::declared::DeclaredSignature<'db>>,
    signature: &MemberSignature,
) -> Vec<(String, TypeId<'db>)> {
    signature
        .parameters
        .iter()
        .map(|parameter| {
            let declared_type = declared
                .and_then(|signature| {
                    signature
                        .parameters
                        .iter()
                        .find(|candidate| candidate.name == parameter.name)
                })
                .and_then(|candidate| candidate.parameter_type)
                .unwrap_or_else(|| TypeId::mixed(db));
            let seeded = if parameter.variadic {
                TypeId::list(db, declared_type)
            } else {
                declared_type
            };
            (parameter.name.clone(), seeded)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_semantics::{AstId, BodyQuery, body_ir};
    use celerrate_source::FileId;
    use celerrate_stubs::{StubIndex, StubIndexInput};

    use super::{BodyOwner, body_owner, inferred_body_types};

    struct Fixture {
        db: TestDatabase,
        handles: Vec<SourceFile>,
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
        let files = AnalyzedFileSet::new(&db, handles.clone());
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
            handles,
            files,
            stubs,
            configuration,
        }
    }

    /// The body of the declaration numbered `index` in file 0.
    fn body_query(fixture: &Fixture, index: u32) -> BodyQuery<'_> {
        BodyQuery::new(
            &fixture.db,
            AstId {
                file: FileId::new(0),
                index,
            },
        )
    }

    #[test]
    fn the_query_answers_a_table_sized_to_the_body_arena() {
        let fixture = fixture(&["<?php function f() { return 1 + 2; }"]);
        let file = fixture.handles[0];
        let body = body_query(&fixture, 0);
        let inferred = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        )
        .as_ref()
        .unwrap();
        let ir = body_ir(&fixture.db, file, body).as_ref().unwrap();
        assert_eq!(inferred.expression_types.len(), ir.expressions.len());
        assert!(!ir.expressions.is_empty(), "the fixture has expressions");
        // `1 + 2` types as int now that the walk runs.
        let super::InferredBody {
            expression_types, ..
        } = inferred;
        assert!(
            expression_types
                .iter()
                .any(|of| *of == crate::TypeId::int(&fixture.db)),
            "the sum typed as int",
        );
    }

    /// The display of the inferred return of declaration `index` in
    /// file 0 — the assertion shape most flow tests use (decision 16).
    fn return_display(fixture: &Fixture, index: u32) -> String {
        let file = fixture.handles[0];
        let body = body_query(fixture, index);
        inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        )
        .as_ref()
        .unwrap()
        .return_type
        .display(&fixture.db)
    }

    #[test]
    fn a_literal_return_types_the_body() {
        let fixture = fixture(&["<?php function f() { return 1; }"]);
        assert_eq!(return_display(&fixture, 0), "1");
    }

    #[test]
    fn assignment_propagates_to_a_later_read() {
        let fixture = fixture(&["<?php function f() { $x = 'a'; return $x; }"]);
        assert_eq!(return_display(&fixture, 0), "'a'");
    }

    #[test]
    fn parameters_seed_from_their_declared_types() {
        let fixture = fixture(&["<?php function f(int $x) { return $x; }"]);
        assert_eq!(return_display(&fixture, 0), "int");
    }

    #[test]
    fn a_variadic_parameter_seeds_as_a_list() {
        let fixture = fixture(&["<?php function f(int ...$x) { return $x; }"]);
        assert_eq!(return_display(&fixture, 0), "list<int>");
    }

    #[test]
    fn branches_join_and_one_sided_assignment_is_silence() {
        let fixture = fixture(&["<?php
            function two(bool $c) { if ($c) { $x = 1; } else { $x = 2; } return $x; }
            function one(bool $c) { if ($c) { $y = 1; } return $y; }"]);
        assert_eq!(return_display(&fixture, 0), "1|2");
        // Assigned on one path only: the absent side reads mixed.
        assert_eq!(return_display(&fixture, 1), "mixed");
    }

    #[test]
    fn a_reachable_fall_through_joins_null_and_a_throwing_body_is_never() {
        let fixture = fixture(&["<?php
            function maybe(bool $c) { if ($c) { return 1; } }
            function raises() { throw new \\RuntimeException('boom'); }"]);
        assert_eq!(return_display(&fixture, 0), "1|null");
        assert_eq!(return_display(&fixture, 1), "never");
    }

    #[test]
    fn a_yielding_body_returns_a_generator() {
        let fixture = fixture(&["<?php function f() { yield 1; }"]);
        assert_eq!(return_display(&fixture, 0), "generator");
    }

    #[test]
    fn a_loop_joins_its_passes_and_terminates_deterministically() {
        let fixture = fixture(&["<?php
            function joins(bool $c) { $x = 1; while ($c) { $x = 'a'; } return $x; }
            function grows(bool $c) { $x = 1; while ($c) { $x = [$x]; } return $x; }"]);
        assert_eq!(return_display(&fixture, 0), "1|'a'");
        // The growing case must terminate (budget + caps) and be
        // reproducible; the exact widened form is not the contract.
        let first = return_display(&fixture, 1);
        let again = self::fixture(&["<?php
            function joins(bool $c) { $x = 1; while ($c) { $x = 'a'; } return $x; }
            function grows(bool $c) { $x = 1; while ($c) { $x = [$x]; } return $x; }"]);
        assert_eq!(first, return_display(&again, 1));
    }

    #[test]
    fn unset_forgets_and_a_catch_variable_types() {
        let fixture = fixture(&[
            "<?php
            function forgets() { $x = 1; unset($x); return $x; }
            function catches() { try { return 1; } catch (\\RuntimeException $e) { return $e; } }",
        ]);
        assert_eq!(return_display(&fixture, 0), "mixed");
        assert_eq!(return_display(&fixture, 1), "1|runtimeexception");
    }

    #[test]
    fn destructuring_binds_element_types() {
        let fixture = fixture(&["<?php function f() { [$a, $b] = [1, 'x']; return $a; }"]);
        assert_eq!(return_display(&fixture, 0), "1");
    }

    #[test]
    fn methods_seed_their_declared_parameters_too() {
        let fixture = fixture(&["<?php class A { public function m(string $s) { return $s; } }"]);
        // Numbering: class = 0, method = 1.
        assert_eq!(return_display(&fixture, 1), "string");
    }

    #[test]
    fn a_non_body_identity_answers_none() {
        let fixture = fixture(&["<?php class A { public function m() {} }"]);
        let file = fixture.handles[0];
        // Numbering: class = 0 (no body), method = 1 (the 1a contract).
        let class = body_query(&fixture, 0);
        assert!(
            inferred_body_types(
                &fixture.db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                file,
                class,
            )
            .is_none()
        );
    }

    #[test]
    fn body_owner_resolves_free_functions_and_methods() {
        let fixture =
            fixture(&["<?php namespace App; function f() {} class A { public function m() {} }"]);
        let file = fixture.handles[0];
        // Numbering: the namespace declaration is itself a numbered item
        // (namespace = 0, f = 1, class = 2, m = 3).
        let function = body_owner(&fixture.db, file, body_query(&fixture, 1))
            .clone()
            .unwrap();
        let BodyOwner::Function(free_function) = function else {
            panic!("expected a free function owner");
        };
        assert_eq!(free_function.name, "f");
        assert_eq!(free_function.namespace, "App");

        let method = body_owner(&fixture.db, file, body_query(&fixture, 3))
            .clone()
            .unwrap();
        let BodyOwner::Method {
            class_key, member, ..
        } = method
        else {
            panic!("expected a method owner");
        };
        assert_eq!(class_key.as_deref(), Some("app\\a"));
        assert_eq!(member.name, "m");
    }

    #[test]
    fn an_anonymous_class_method_owner_has_no_key() {
        let fixture =
            fixture(&["<?php function wrapper() { return new class { public function m() {} }; }"]);
        let file = fixture.handles[0];
        // Numbering: wrapper = 0, anonymous class = 1, method = 2.
        let owner = body_owner(&fixture.db, file, body_query(&fixture, 2))
            .clone()
            .unwrap();
        let BodyOwner::Method { class_key, .. } = owner else {
            panic!("expected a method owner");
        };
        assert!(class_key.is_none());
    }

    #[test]
    fn instanceof_narrows_both_branches() {
        let fixture = fixture(&["<?php class Foo {}
            function f(mixed $x) { if ($x instanceof Foo) { return $x; } return 1; }
            function negated(mixed $x) { if (!($x instanceof Foo)) { return 1; } return $x; }"]);
        assert_eq!(return_display(&fixture, 1), "1|foo");
        assert_eq!(return_display(&fixture, 2), "1|foo");
    }

    #[test]
    fn strict_null_comparisons_narrow() {
        let fixture = fixture(&["<?php class Foo {}
            function f(?Foo $x) { if ($x === null) { return 1; } return $x; }
            function g(?Foo $x) { if ($x !== null) { return $x; } return 1; }"]);
        assert_eq!(return_display(&fixture, 1), "1|foo");
        assert_eq!(return_display(&fixture, 2), "1|foo");
    }

    #[test]
    fn false_comparisons_narrow_the_strpos_idiom() {
        let fixture = fixture(&["<?php function f(int|false $position) {
                if ($position === false) { return 'missing'; }
                return $position;
            }"]);
        assert_eq!(return_display(&fixture, 0), "int|'missing'");
    }

    #[test]
    fn the_is_family_narrows() {
        let fixture = fixture(&[
            "<?php function f(mixed $x) { if (is_string($x)) { return $x; } return 1; }",
        ]);
        assert_eq!(return_display(&fixture, 0), "1|string");
    }

    #[test]
    fn boolean_composition_distributes() {
        let fixture = fixture(&["<?php class Foo {}
            function both(mixed $x) {
                if ($x instanceof Foo && is_string($x)) { return 1; }
                return 2;
            }
            function either(?Foo $x) {
                if ($x === null || $x instanceof Foo) { return 1; }
                return $x;
            }"]);
        // `either`'s fall-through sees the union minus both
        // alternatives — never — so the function's return joins to
        // exactly the then-branch's literal. Without `||`
        // distribution the answer would be "null|1|foo".
        assert_eq!(return_display(&fixture, 2), "1");
        // `both` must compose without crashing or mis-joining.
        let _ = return_display(&fixture, 1);
    }

    #[test]
    fn early_returns_narrow_the_rest_of_the_body() {
        let fixture = fixture(&["<?php class Foo {}
            function f(?Foo $x) {
                if ($x === null) { return 1; }
                return $x;
            }"]);
        assert_eq!(return_display(&fixture, 1), "1|foo");
    }

    #[test]
    fn isset_and_empty_narrow_their_targets() {
        let fixture = fixture(&["<?php class Foo {}
            function set(?Foo $x) { if (isset($x)) { return $x; } return 1; }
            function filled(string|null $x) { if (!empty($x)) { return $x; } return 1; }"]);
        assert_eq!(return_display(&fixture, 1), "1|foo");
        assert_eq!(return_display(&fixture, 2), "1|string");
    }

    #[test]
    fn truthiness_narrows_and_a_while_condition_narrows_its_body() {
        let fixture = fixture(&["<?php class Foo {}
            function truthy(?Foo $x) { if ($x) { return $x; } return 1; }
            function looped(?Foo $x) { while ($x !== null) { return $x; } return 1; }"]);
        assert_eq!(return_display(&fixture, 1), "1|foo");
        assert_eq!(return_display(&fixture, 2), "1|foo");
    }

    #[test]
    fn an_assign_and_test_condition_narrows_the_assigned_subject() {
        let fixture = fixture(&["<?php class Foo {}
            function f(?Foo $source) {
                if (($x = $source) !== null) { return $x; }
                return 1;
            }"]);
        assert_eq!(return_display(&fixture, 1), "1|foo");
    }

    #[test]
    fn match_arms_narrow_their_subject_and_the_default_subtracts() {
        let fixture = fixture(&["<?php function f(int|string $x) {
                return match ($x) { 1, 2 => $x, default => 'other' };
            }"]);
        // Arm: 1|2. Default: the literals are not subtractable from
        // the general int, so int|string stays — joined with the arm.
        assert_eq!(return_display(&fixture, 0), "1|2|'other'");
    }

    #[test]
    fn the_match_true_idiom_narrows_by_arm_condition() {
        let fixture = fixture(&["<?php function f(mixed $x) {
                return match (true) { is_string($x) => $x, default => 1 };
            }"]);
        assert_eq!(return_display(&fixture, 0), "1|string");
    }

    #[test]
    fn switch_narrows_strict_safe_cases() {
        let fixture = fixture(&["<?php function f(int $x) {
                switch ($x) { case 1: return $x; }
                return 2;
            }"]);
        assert_eq!(return_display(&fixture, 0), "1|2");
    }

    #[test]
    fn coalescing_drops_null_from_its_left_operand() {
        let fixture = fixture(&["<?php class Foo {}
            function coalesce(?string $x) { return $x ?? 'd'; }
            function keeps(?Foo $x) { return $x ?? null; }
            function assigns(?int $x) { $x ??= 0; return $x; }"]);
        // join(string, 'd') absorbs the literal.
        assert_eq!(return_display(&fixture, 1), "string");
        // The brief's original expectation transposed the display's
        // established null-last convention (`display.rs`'s
        // `composites_render_with_null_last_in_unions`, and the
        // sibling test above at line 380 rendering "1|null"); the
        // correct rendering of `Foo|null` is "foo|null".
        assert_eq!(return_display(&fixture, 2), "foo|null");
        assert_eq!(return_display(&fixture, 3), "int");
    }

    #[test]
    fn coalescing_preserves_a_multi_literal_left_operand() {
        let fixture = fixture(&["<?php function f(int $flag) {
                $x = $flag === 1 ? 1 : ($flag === 2 ? 2 : null);
                return $x ?? 3;
            }"]);
        // The left operand is the union `1|2|null`; dropping null
        // leaves `1|2`, which must survive (only a single-value
        // literal widens). The single-value right literal `3` widens
        // to `int`; `union` performs no subsumption, so `1` and `2`
        // are not absorbed under it. The display's structural order
        // renders the general `int` before the literals (decision 16).
        assert_eq!(return_display(&fixture, 0), "int|1|2");
    }

    #[test]
    fn assert_narrows_the_rest_of_the_body() {
        let fixture = fixture(&["<?php class Foo {}
            function f(mixed $x) { assert($x instanceof Foo); return $x; }"]);
        assert_eq!(return_display(&fixture, 1), "foo");
    }

    #[test]
    fn this_and_its_property_reads_type_from_the_declaration() {
        let fixture = fixture(&["<?php class A {
                public ?string $s = null;
                public function own() { return $this; }
                public function read() { return $this->s; }
            }"]);
        // Numbering: class 0, property 1, own 2, read 3.
        assert_eq!(return_display(&fixture, 2), "a");
        // The brief's original expectation transposed the display's
        // established null-last convention (`display.rs`'s
        // `composites_render_with_null_last_in_unions`, already
        // corrected once for the same reason in
        // `coalescing_drops_null_from_its_left_operand` above): the
        // correct rendering of `?string` is "string|null".
        assert_eq!(return_display(&fixture, 3), "string|null");
    }

    #[test]
    fn method_calls_take_declared_returns_and_count_the_edge() {
        let fixture = fixture(&[
            "<?php class A { public function name(): string { return 'a'; } }
            function f(A $a) { return $a->name(); }",
        ]);
        assert_eq!(return_display(&fixture, 2), "string");
        let file = fixture.handles[0];
        let body = body_query(&fixture, 2);
        let inferred = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        )
        .as_ref()
        .unwrap();
        assert_eq!(inferred.edge_counts.declared_return_edges, 1);
    }

    #[test]
    fn fluent_static_returns_substitute_the_receiver() {
        let fixture = fixture(&[
            "<?php class Builder { public function with(): static { return $this; } }
            function f(Builder $b) { return $b->with(); }",
        ]);
        assert_eq!(return_display(&fixture, 2), "builder");
    }

    #[test]
    fn static_calls_and_scoped_reads_resolve() {
        let fixture = fixture(&["<?php class K {
                const int N = 1;
                public static function make(): float { return 1.0; }
            }
            function call() { return K::make(); }
            function constant() { return K::N; }
            function name() { return K::class; }"]);
        assert_eq!(return_display(&fixture, 3), "float");
        assert_eq!(return_display(&fixture, 4), "int");
        assert_eq!(return_display(&fixture, 5), "class-string<k>");
    }

    #[test]
    fn an_enum_case_read_types_as_the_case() {
        let fixture = fixture(&["<?php enum E { case A; } function f() { return E::A; }"]);
        let file = fixture.handles[0];
        let body = body_query(&fixture, 2);
        let inferred = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        )
        .as_ref()
        .unwrap();
        assert_eq!(
            inferred.return_type.enum_case_parts(&fixture.db),
            Some(("e".to_owned(), "A".to_owned())),
        );
    }

    #[test]
    fn union_receivers_join_and_opaque_receivers_stay_silent() {
        let fixture = fixture(&["<?php class A { public function n(): int { return 1; } }
            class B { public function n(): string { return 'b'; } }
            function joined(A|B $x) { return $x->n(); }
            function nullable(?A $x) { return $x->n(); }
            function opaque(mixed $x) { return $x->n(); }"]);
        assert_eq!(return_display(&fixture, 4), "int|string");
        // The null constituent is the nullability family's business
        // (plan 8); the read types from the non-null part.
        assert_eq!(return_display(&fixture, 5), "int");
        assert_eq!(return_display(&fixture, 6), "mixed");
    }

    #[test]
    fn parent_and_self_resolve_against_the_defining_class() {
        let fixture = fixture(&[
            "<?php class Base { public function root(): int { return 1; } }
            class Child extends Base {
                public function up() { return parent::root(); }
                public function own() { return self::class; }
            }",
        ]);
        // Numbering: Base 0, root 1, Child 2, up 3, own 4.
        assert_eq!(return_display(&fixture, 3), "int");
        assert_eq!(return_display(&fixture, 4), "class-string<child>");
    }

    #[test]
    fn new_types_as_the_class_and_anonymous_stays_mixed() {
        let fixture = fixture(&["<?php class A {}
            function named() { return new A(); }
            function anonymous() { return new class {}; }"]);
        assert_eq!(return_display(&fixture, 1), "a");
        assert_eq!(return_display(&fixture, 2), "mixed");
    }

    #[test]
    fn new_self_static_parent_type_as_the_defining_or_parent_class() {
        let fixture = fixture(&["<?php class Base {}
            class Child extends Base {
                public function makeSelf() { return new self(); }
                public function makeParent() { return new parent(); }
                public function makeStatic() { return new static(); }
                public static function makeStaticInStatic() { return new static(); }
            }"]);
        // Numbering: Base 0, Child 1, makeSelf 2, makeParent 3,
        // makeStatic 4, makeStaticInStatic 5.
        // Decision 5: `self`/`static` are the defining class, `parent`
        // the first Extends ancestor. The class type renders as its
        // folded (lowercase) key (decision 16).
        assert_eq!(return_display(&fixture, 2), "child");
        assert_eq!(return_display(&fixture, 3), "base");
        assert_eq!(return_display(&fixture, 4), "child");
        // The case that was silently `mixed`: `new static()` in a
        // static method still types as the defining class ($this's
        // static-method unavailability does not gate the class keyword).
        assert_eq!(return_display(&fixture, 5), "child");
    }

    #[test]
    fn a_null_safe_chain_reacquires_null_once_at_the_end() {
        let fixture = fixture(&["<?php class B { public function c(): int { return 1; } }
            class A { public function b(): B { return new B(); } }
            function f(?A $a) { return $a?->b()->c(); }"]);
        // One null receiver short-circuits the whole chain: the inner
        // ->c() sees B (never B|null), the chain result is int|null.
        assert_eq!(return_display(&fixture, 4), "int|null");
    }

    #[test]
    fn a_narrowed_receiver_reacquires_nothing() {
        let fixture = fixture(&["<?php class B {}
            class A { public function b(): B { return new B(); } }
            function f(?A $a) {
                if ($a === null) { return 1; }
                return $a?->b();
            }"]);
        assert_eq!(return_display(&fixture, 3), "1|b");
    }

    #[test]
    fn every_null_safe_link_strips_before_resolving() {
        let fixture = fixture(&["<?php class B { public function c(): int { return 1; } }
            class A { public function b(): ?B { return null; } }
            function f(?A $a) { return $a?->b()?->c(); }"]);
        assert_eq!(return_display(&fixture, 4), "int|null");
    }
}
