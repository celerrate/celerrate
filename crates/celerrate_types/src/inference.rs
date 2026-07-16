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
    BodyQuery, ExpressionId, FreeFunction, Member, MemberKind, MemberOrigin, MemberResolution,
    MemberSignature, SymbolQuery, SymbolSpace, UseTables, analyzed_file_index, body_ir,
    folded_member_key, folded_symbol_key, fully_qualified_name, item_tree,
    lookup_function_declaration, lookup_member, member_tree,
};
use celerrate_stubs::StubIndexInput;

use crate::declared::{FunctionQuery, declared_function_signature, declared_member_signature};
use crate::flow::{FlowContext, walk_body};
use crate::representation::TypeId;

/// The interprocedural edge classes one body's inference took, as
/// pure data: the design's residual instrument ("how many results
/// depend on *inferred* returns"). Counters never live inside queries
/// (the workspace rule); this struct is the query-side data plan 8/9a
/// aggregate into the `CELERRATE_CACHE_STATS` rendering once the
/// orchestration layer first demands inference (decision 13) — until
/// then the field exists and is tested (task 12), but nothing renders
/// it.
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

/// The class context one body is analyzed *for* (decision 5): the
/// using class of a trait body, and `None` everywhere else — so the
/// memo space of every non-trait body stays exactly one entry wide.
/// An explicit query parameter rather than ambient state, because
/// salsa memoizes on the parameter list: the same trait body analyzed
/// for two using classes must be two memos, not one overwriting the
/// other. `inferred_method_return`'s trait arm is the only source of a
/// `Some` value today (task 6 threads the parameter; task 7 owns trait
/// behavior proper).
#[salsa::interned(debug)]
pub struct InferenceContext<'db> {
    /// The **pre-folded** ClassLike key of the using class, when this
    /// body is a trait body analyzed for one.
    #[returns(ref)]
    pub using_class_key: Option<String>,
}

/// The inference of one body: `None` when the identity carries no
/// body in `file` (mirroring `body_ir`). Task 3 replaces the
/// all-`mixed` table with the flow walk.
#[salsa::tracked(returns(ref))]
#[allow(clippy::too_many_arguments)]
pub fn inferred_body_types<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: BodyQuery<'db>,
    context: InferenceContext<'db>,
) -> Option<InferredBody<'db>> {
    let ir = body_ir(db, file, body).as_ref()?;
    let owner = body_owner(db, file, body);
    let (namespace, owner_class_key, method_is_static, parameters, scope_key) = match owner {
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
                crate::declared::FunctionQuery::new(db, key.clone()),
            );
            (
                function.namespace.clone(),
                None,
                false,
                seeded_parameters(db, declared.as_ref(), &function.signature),
                key,
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
            // The body's *own* scope key — `class_key` here, before the
            // using-class override just below — matching
            // `declared.rs`'s `<class key>::<member key>` convention.
            // A trait method analyzed for a using class (decision 5)
            // still binds its own class-level templates under the
            // trait's key: which class or trait wrote the `@template`
            // is unaffected by who later `use`s the method, unlike the
            // receiver facts (`self`, `static`, `$this`) the override
            // just below governs.
            let member_key = folded_member_key(MemberKind::Method, &member.name);
            let scope_key = class_key
                .as_ref()
                .map(|key| format!("{key}::{member_key}"))
                .unwrap_or(member_key);
            (
                namespace.clone(),
                class_key.clone(),
                member.flags.is_static,
                seeded_parameters(db, declared.as_ref(), &member.signature),
                scope_key,
            )
        }
        None => (String::new(), None, false, Vec::new(), String::new()),
    };
    let tables = UseTables::for_namespace(item_tree(db, file), &namespace);
    // Decision 5: with a using-class context the walker's owner class
    // key *is* the using class — `self`/`static` inside a trait body
    // resolve against the class that uses it, not the trait. Without
    // one, the body's own declaration answers, exactly as before.
    let owner_class_key = context.using_class_key(db).clone().or(owner_class_key);
    // `method_is_static` and `parameters` still come from `body_owner`
    // above, the trait's own syntactic member, even for a trait body —
    // only `owner_class_key` is overridden to the using class here.
    // That split is deliberate, not an oversight: the *signature* (is
    // this a static method, what are its parameters) is a fact about
    // the trait method's own declaration, unaffected by which class
    // uses it; only the *receiver* facts (`self`, `static`, `$this`,
    // `parent`) are the using class's business (decision 5).
    let flow = FlowContext {
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
        scope_key,
    };
    let result = walk_body(&flow);
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

/// The iteration budget of the interprocedural fixpoint: exhaustion
/// widens deterministically to `mixed`. Far below salsa's own
/// `MAX_ITERATIONS = 200` panic cap (`salsa-0.27.2/src/cycle.rs`) —
/// reaching that panic would be a zero-panic breach, so the budget is
/// the bailout that makes it unreachable.
pub const FIXPOINT_ITERATION_BUDGET: u32 = 32;

/// One join-ascent step: the computed iterate joins the previous
/// approximation, and a still-moving value past the budget widens to
/// `mixed`.
///
/// The join operator is [`TypeId::union`], deliberately, not
/// [`crate::widening::join`] (the decision-2 text names `join`, but its
/// disjoint-scalar `mixed` fallback would erase precision — `join(int,
/// string)` collapses to `mixed`, whereas the discipline wants `int|
/// string`). `union` accumulates precisely; termination is delegated,
/// exactly as the per-loop join-ascent (`flow.rs`) does, to the
/// lattice caps ([`crate::widening::UNION_ARITY_CAP`],
/// [`crate::widening::STRUCTURAL_DEPTH_CAP`]) and to
/// [`FIXPOINT_ITERATION_BUDGET`]. The ascent is monotone because
/// `union` only grows: a subsumed iterate absorbs into the previous
/// approximation, so `ascended == last_provisional` fires at
/// convergence, oscillation between two values is impossible, and every
/// entry point converges to the same fixpoint (`union` is deterministic
/// and entry-point independent).
pub(crate) fn ascend<'db>(
    db: &'db dyn salsa::Database,
    iteration: u32,
    last_provisional: TypeId<'db>,
    computed: TypeId<'db>,
) -> TypeId<'db> {
    let ascended = TypeId::union(db, [last_provisional, computed]);
    if ascended == last_provisional {
        return ascended;
    }
    if iteration >= FIXPOINT_ITERATION_BUDGET {
        return TypeId::mixed(db);
    }
    ascended
}

fn return_cycle_initial<'db>(
    db: &'db dyn salsa::Database,
    _id: salsa::Id,
    _files: AnalyzedFileSet,
    _stubs: StubIndexInput,
    _configuration: ProjectConfiguration,
    _query: FunctionQuery<'db>,
) -> TypeId<'db> {
    // The lattice bottom: ascent starts from nothing.
    TypeId::never(db)
}

#[allow(clippy::too_many_arguments)]
fn return_cycle_recover<'db>(
    db: &'db dyn salsa::Database,
    cycle: &salsa::Cycle,
    last_provisional: &TypeId<'db>,
    computed: TypeId<'db>,
    _files: AnalyzedFileSet,
    _stubs: StubIndexInput,
    _configuration: ProjectConfiguration,
    _query: FunctionQuery<'db>,
) -> TypeId<'db> {
    // The macro converges when the returned value equals
    // `last_provisional` (`salsa-0.27.2/src/function/execute.rs:266`);
    // `ascend` returns exactly `last_provisional` at convergence, so
    // returning its answer directly drives the fixpoint to a fixed
    // point without a separate `CycleRecoveryAction` (absent in 0.27.2).
    ascend(db, cycle.iteration(), *last_provisional, computed)
}

/// The inferred return of one free function: the projection of its
/// body's inference — small, resident (never LRU-evicted), the
/// fixpoint's currency. Early cutoff is the point: a body edit that
/// leaves the inferred return identical backdates here, and callers
/// are spared. Unresolvable functions answer `mixed` (silence).
#[salsa::tracked(cycle_fn = return_cycle_recover, cycle_initial = return_cycle_initial)]
pub fn inferred_function_return<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: FunctionQuery<'db>,
) -> TypeId<'db> {
    let symbol_query = SymbolQuery::new(db, SymbolSpace::Function, query.key(db).clone());
    let Some(ast_id) = lookup_function_declaration(db, files, symbol_query) else {
        return TypeId::mixed(db);
    };
    let index = analyzed_file_index(db, files);
    let Ok(position) = index.binary_search_by_key(&ast_id.file, |(id, _)| *id) else {
        return TypeId::mixed(db);
    };
    let Some(&(_, file)) = index.get(position) else {
        return TypeId::mixed(db);
    };
    inferred_body_types(
        db,
        files,
        stubs,
        configuration,
        file,
        BodyQuery::new(db, ast_id),
        InferenceContext::new(db, None),
    )
    .as_ref()
    .map(|inferred| inferred.return_type)
    .unwrap_or_else(|| TypeId::mixed(db))
}

/// One method to infer the return of: the receiver-resolution class
/// and the method, both **pre-folded**. `class_key` is a class
/// *definition* identity — never a type carrying generic arguments —
/// so the memo space stays pinned to the finite set of class-likes
/// (decision 4); the receiver's arguments bind at the call boundary
/// (`member_boundary_type`), not here.
#[salsa::interned(debug)]
pub struct MethodQuery<'db> {
    /// Pre-folded ClassLike key: the receiver-resolution class.
    #[returns(ref)]
    pub class_key: String,
    /// Pre-folded method key (`folded_member_key(Method, name)`).
    #[returns(ref)]
    pub member_key: String,
}

fn method_return_cycle_initial<'db>(
    db: &'db dyn salsa::Database,
    _id: salsa::Id,
    _files: AnalyzedFileSet,
    _stubs: StubIndexInput,
    _configuration: ProjectConfiguration,
    _query: MethodQuery<'db>,
) -> TypeId<'db> {
    // The lattice bottom: ascent starts from nothing.
    TypeId::never(db)
}

#[allow(clippy::too_many_arguments)]
fn method_return_cycle_recover<'db>(
    db: &'db dyn salsa::Database,
    cycle: &salsa::Cycle,
    last_provisional: &TypeId<'db>,
    computed: TypeId<'db>,
    _files: AnalyzedFileSet,
    _stubs: StubIndexInput,
    _configuration: ProjectConfiguration,
    _query: MethodQuery<'db>,
) -> TypeId<'db> {
    // The same contract `return_cycle_recover` above documents and the
    // same shared `ascend`: convergence is "the returned value equals
    // `last_provisional`", which `ascend` reports exactly, and the
    // shared budget bails to `mixed` far below salsa's panic cap.
    ascend(db, cycle.iteration(), *last_provisional, computed)
}

/// The inferred return of one method, keyed per defining class
/// (decision 4): an inherited member re-keys to its owner so every
/// subclass shares one memo; a trait member analyzes per using class
/// (the origin's anchor, PHPStan's model); stub and virtual
/// members answer `mixed` — their types are declared, consulted at the
/// earlier tier. The second cycle-recovered query in the workspace;
/// the discipline (join ascent, shared budget, deterministic bailout)
/// is plan 5's, unchanged, so termination is inherited rather than
/// re-argued: the participant set is the finite set of (class-like,
/// member) pairs, the ascent is monotone, and the budget bounds it.
#[salsa::tracked(cycle_fn = method_return_cycle_recover, cycle_initial = method_return_cycle_initial)]
pub fn inferred_method_return<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MethodQuery<'db>,
) -> TypeId<'db> {
    let member_query = celerrate_semantics::MemberQuery::new(
        db,
        query.class_key(db).clone(),
        MemberKind::Method,
        query.member_key(db).clone(),
    );
    // Stub and virtual resolutions (and an unresolvable member) fall
    // through to silence here on purpose: neither carries a body to
    // infer, and both already answered at the declared tier.
    let Some(MemberResolution::Source {
        member,
        owner,
        origin,
    }) = lookup_member(db, files, stubs, configuration, member_query)
    else {
        return TypeId::mixed(db);
    };
    let context = match origin {
        MemberOrigin::Inherited => {
            // Re-key to the defining class: one memo, shared by every
            // subclass, rather than one per subclass of an identical
            // body. `owner` is a strictly higher ancestor, so the
            // re-key descends the finite ancestry and terminates.
            //
            // This arm never sees a trait-provided member: a member
            // arrives here only across an `extends`/`implements` edge,
            // so `owner` genuinely declares it and its body needs no
            // using-class context (which is also what keeps decision 5's
            // memo space for non-trait bodies exactly one entry wide).
            // Task 7b is what makes that true — before the anchor, a
            // trait member reached one `extends` step out was classified
            // `Inherited`, so this arm re-keyed to the *trait* and
            // analyzed its body with no context at all, resolving `$this`
            // against the trait: `mixed` when the trait declared no such
            // member, and a wrong concrete answer when it did.
            let owner_query = MethodQuery::new(db, owner, query.member_key(db).clone());
            return inferred_method_return(db, files, stubs, configuration, owner_query);
        }
        MemberOrigin::Own => InferenceContext::new(db, None),
        // Decision 4's "per using class" key is the origin's anchor, not
        // the queried class: they coincide only for a direct use.
        // Queried through a subclass of the user, or through traits
        // using traits, the anchor still names the class the trait was
        // pasted into, which is the one PHP binds `self`, `parent` and
        // `$this`'s member table to. No re-key to the anchor's own
        // `MethodQuery` is needed for either economy or agreement: the
        // memo below is keyed by `(body, context)`, so every subclass of
        // one using class already shares that using class's single body
        // memo, and `member_owner` (`flow.rs`) reads the same anchor
        // from the same resolution.
        MemberOrigin::Trait { anchor } => InferenceContext::new(db, Some(anchor)),
    };
    // `member.ast_id.file` is always the *declaring* file — the
    // trait's, for a `Trait`-origin member — never the using class's;
    // that is correct by construction, since a body can only be walked
    // from the file that actually contains its syntax tree. `context`
    // (the using-class key, threaded above) is what makes the same
    // trait body's *analysis* per using class, not this lookup.
    let index = analyzed_file_index(db, files);
    let Ok(position) = index.binary_search_by_key(&member.ast_id.file, |(id, _)| *id) else {
        return TypeId::mixed(db);
    };
    let Some(&(_, file)) = index.get(position) else {
        return TypeId::mixed(db);
    };
    let body = BodyQuery::new(db, member.ast_id);
    inferred_body_types(db, files, stubs, configuration, file, body, context)
        .as_ref()
        .map(|inferred| inferred.return_type)
        .unwrap_or_else(|| TypeId::mixed(db))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        // The budget-below-the-cap assertion compares two constants on
        // purpose: it pins `FIXPOINT_ITERATION_BUDGET < 200` so the
        // salsa panic cap stays structurally unreachable.
        clippy::assertions_on_constants
    )]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_semantics::{
        AstId, BodyQuery, SymbolSpace, body_ir, folded_symbol_key, fully_qualified_name,
        member_tree,
    };
    use celerrate_source::FileId;
    use celerrate_stubs::{StubIndex, StubIndexInput};

    use super::{
        BodyOwner, InferenceContext, MethodQuery, body_owner, inferred_body_types,
        inferred_function_return, inferred_method_return,
    };
    use crate::declared::FunctionQuery;

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

    /// `fixture` plus the shared generics-capable docblock fake
    /// (`inheritance::test_support::FakeSyntax`) registered: task 5's
    /// class-argument-binding test needs `@template`/`@param
    /// NAME<ARG>` parsing that `fixture` alone does not provide (no
    /// `TypeSyntax` is registered there at all). A variant rather than
    /// a change to `fixture` itself — registering the fake globally
    /// would give every existing docblock-bearing test in this module
    /// a different annotation reading.
    fn fixture_with_generics(sources: &[&str]) -> Fixture {
        let built = fixture(sources);
        crate::inheritance::test_support::register_fake_syntax(&built.db);
        built
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
            InferenceContext::new(&fixture.db, None),
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
            InferenceContext::new(&fixture.db, None),
        )
        .as_ref()
        .unwrap()
        .return_type
        .display(&fixture.db)
    }

    /// The display of a free function's inferred return, resolved
    /// straight through `inferred_function_return` by its folded key
    /// (task 5's receiver-model tests, which call across function
    /// boundaries rather than reading one numbered declaration).
    fn caller_return_display(fixture: &Fixture, key: &str) -> String {
        inferred_function_return(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            FunctionQuery::new(&fixture.db, key.to_owned()),
        )
        .display(&fixture.db)
    }

    /// The display of one method's own inferred return, found by class
    /// key and method name through `member_tree` rather than by
    /// declaration index — task 5's tests read a method body's return
    /// directly (no external caller) to pin the placeholder that
    /// survives inside it.
    fn method_return_display(fixture: &Fixture, class_key: &str, method_name: &str) -> String {
        for &file in &fixture.handles {
            let tree = member_tree(&fixture.db, file);
            for class in &tree.classes {
                let Some(name) = &class.name else {
                    continue;
                };
                let key = folded_symbol_key(
                    SymbolSpace::ClassLike,
                    &fully_qualified_name(&class.namespace, name),
                );
                if key != class_key {
                    continue;
                }
                let Some(member) = class
                    .members
                    .iter()
                    .find(|member| member.name.eq_ignore_ascii_case(method_name))
                else {
                    continue;
                };
                let body = BodyQuery::new(&fixture.db, member.ast_id);
                return inferred_body_types(
                    &fixture.db,
                    fixture.files,
                    fixture.stubs,
                    fixture.configuration,
                    file,
                    body,
                    InferenceContext::new(&fixture.db, None),
                )
                .as_ref()
                .unwrap()
                .return_type
                .display(&fixture.db);
            }
        }
        panic!("method {method_name} not found on class {class_key}");
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
                InferenceContext::new(&fixture.db, None),
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
        // Task 5 (the receiver model, decision 1) supersedes this
        // expectation: `$this` types as the symbolic `static`
        // placeholder inside the body — substitution is the call
        // site's job, not the body's (see
        // `this_types_as_the_static_placeholder_inside_the_body`).
        assert_eq!(return_display(&fixture, 2), "static");
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
            InferenceContext::new(&fixture.db, None),
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
            InferenceContext::new(&fixture.db, None),
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
    fn a_native_static_return_substitutes_to_the_receiver() {
        let f = fixture(&[r#"<?php
namespace App;
class Base {
    public static function create(): static { return new static(); }
}
class Child extends Base {}
function caller(Child $c) { return $c::create(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\child");
    }

    #[test]
    fn a_native_self_return_stays_the_declaring_class() {
        let f = fixture(&[r#"<?php
namespace App;
class Base {
    public function make(): self { return $this; }
}
class Child extends Base {}
function caller(Child $c) { return $c->make(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\base");
    }

    #[test]
    fn static_forwards_through_self_calls_and_rebinds_on_a_named_class() {
        let f = fixture(&[r#"<?php
namespace App;
class Base {
    public static function create(): static { return new static(); }
    public static function viaSelf(): static { return self::create(); }
}
class Child extends Base {}
function forwarded(Child $c) { return $c::viaSelf(); }
function rebound() { return Base::create(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\forwarded"), "app\\child");
        assert_eq!(caller_return_display(&f, "app\\rebound"), "app\\base");
        // The forwarding receiver itself: `viaSelf`'s body resolves
        // `self::create()` through the *current* `static` type, which
        // stays the symbolic placeholder here — never eagerly resolved
        // to `Base` — so the outer `$c::viaSelf()` call above can still
        // rebind it to `Child`. An eager `scoped_subject` (answering
        // the owner's own class instead of forwarding the placeholder)
        // would make this read `"app\\base"` while the two assertions
        // above stayed green — the defect Finding 1 identified.
        assert_eq!(method_return_display(&f, "app\\base", "viaSelf"), "static");
    }

    #[test]
    fn this_types_as_the_static_placeholder_inside_the_body() {
        let f = fixture(&[r#"<?php
namespace App;
class Chainable {
    public function itself(): static { return $this; }
}
"#]);
        // The method body's return carries the placeholder symbolically:
        // substitution is the call site's job, not the body's.
        assert_eq!(
            method_return_display(&f, "app\\chainable", "itself"),
            "static"
        );
    }

    #[test]
    fn parent_calls_resolve_members_and_keep_forwarding() {
        let f = fixture(&[r#"<?php
namespace App;
class Base {
    public function name(): string { return 'base'; }
}
class Child extends Base {
    public function viaParent() { return parent::name(); }
}
"#]);
        assert_eq!(
            method_return_display(&f, "app\\child", "viaParent"),
            "string"
        );
    }

    #[test]
    fn a_union_receiver_resolves_each_key_s_self_against_its_own_owner() {
        // Both `A` and `B` declare a `self`-returning `m`: `member_owner`
        // must resolve per key, not once for the whole call, or both
        // signatures substitute `self` against whichever key resolved
        // first — a wrong concrete answer (`app\a|app\a`) rather than
        // conservative silence (Finding 3).
        let f = fixture(&[r#"<?php
namespace App;
class A {
    public function m(): self { return $this; }
}
class B {
    public function m(): self { return $this; }
}
function joined(A|B $x) { return $x->m(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\joined"), "app\\a|app\\b");
    }

    #[test]
    fn a_receiver_s_class_arguments_bind_its_class_level_templates() {
        // `member_boundary_type`'s class-argument-binding branch (the
        // zip of a receiver's `class_arguments` against its class's
        // `class_annotations(...).templates`) has no test in this
        // diff that can drive it: the plain-name `@param` grammar
        // cannot write `Box<Marker>`, so no receiver anywhere ever
        // carries `class_arguments`. `fixture_with_generics` plus the
        // extended `@param NAME<ARG>` grammar (`test_support.rs`)
        // finally expresses one.
        let f = fixture_with_generics(&[r#"<?php
namespace App;
/** @template T */
class Box {
    /** @return T */
    public function get() {}
}
class Marker {}
/**
 * @param Box<Marker> $b
 */
function unwrap($b) { return $b->get(); }
"#]);
        // The argument type must come back, not the unresolved
        // template (`"T"`, the answer if the binding branch never
        // ran) and not `mixed` (conservative silence) or `Box` (the
        // bound, ignored once an argument is supplied) — the argument
        // and the template are different types, so this discriminates.
        assert_eq!(caller_return_display(&f, "app\\unwrap"), "app\\marker");
    }

    // Task 8: the call-site template solver (decision 10). Every test
    // below registers the shared generics-capable fake
    // (`fixture_with_generics`), the same fixture
    // `a_receiver_s_class_arguments_bind_its_class_level_templates`
    // uses above — the brief's own pseudocode calls plain `fixture`,
    // but this module's docblock-bearing tests only ever get
    // `@template`/`@param`/`@return` parsed through the registered
    // fake, and only `fixture_with_generics` registers it.

    #[test]
    fn a_template_parameter_solves_from_its_argument() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class User {}
/**
 * @template T
 * @param T $value
 * @return T
 */
function identity($value) { return $value; }
function caller() { return identity(new User()); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user");
    }

    #[test]
    fn multiple_constraints_take_the_least_upper_bound() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
/**
 * @template T
 * @param T $left
 * @param T $right
 * @return T
 */
function pick($left, $right) { return $left; }
function caller() { return pick(1, 'one'); }
"#]);
        // `1` and `'one'` conflict: a first-seen-constituent bug would
        // answer `"1"` alone, not their union.
        assert_eq!(caller_return_display(&f, "app\\caller"), "1|'one'");
    }

    #[test]
    fn class_string_binds_the_template_through_class_constants() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class User {}
/**
 * @template T
 * @param class-string<T> $name
 * @return T
 */
function make(string $name) {}
function caller() { return make(User::class); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user");
    }

    #[test]
    fn a_union_receiver_s_class_constant_binds_the_template_as_a_union() {
        // Finding 1 (final review, type-engine-6): `$obj::class` on a
        // union receiver must answer every constituent, never a
        // first-seen one. A first-seen-constituent bug would answer
        // `"app\\a"` alone, not the union.
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class A {}
class B {}
/**
 * @template T
 * @param class-string<T> $c
 * @return T
 */
function make(string $c) {}
function caller(A|B $obj) { return make($obj::class); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\a|app\\b");
    }

    #[test]
    fn a_class_constant_types_as_a_class_string_of_its_class() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class User {}
function caller() { return User::class; }
"#]);
        assert_eq!(
            caller_return_display(&f, "app\\caller"),
            "class-string<app\\user>"
        );
    }

    #[test]
    fn an_unconstrained_template_falls_to_its_bound_then_mixed() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class Fallback {}
/**
 * @template T of Fallback
 * @return T
 */
function bounded() {}
/**
 * @template U
 * @return U
 */
function boundless() {}
function bound_caller() { return bounded(); }
function mixed_caller() { return boundless(); }
"#]);
        assert_eq!(
            caller_return_display(&f, "app\\bound_caller"),
            "app\\fallback"
        );
        assert_eq!(caller_return_display(&f, "app\\mixed_caller"), "mixed");
    }

    #[test]
    fn a_generic_class_parameter_recurses_through_the_ancestry() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class User {}
/** @template T */
class Collection {}
/** @extends Collection<User> */
class UserCollection extends Collection {}
/**
 * @template T
 * @param Collection<T> $collection
 * @return T
 */
function first($collection) {}
function caller(UserCollection $users) { return first($users); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user");
    }

    #[test]
    fn a_conditional_return_evaluates_at_the_call_site() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
/**
 * @template T
 * @param T $value
 * @return (T is int ? string : bool)
 */
function flip($value) {}
function on_int() { return flip(1); }
function on_string() { return flip('text'); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\on_int"), "string");
        assert_eq!(caller_return_display(&f, "app\\on_string"), "bool");
    }

    // Review follow-up (task 8): three findings, each pinning a gap the
    // original seven tests left untested.

    #[test]
    fn a_template_bound_naming_another_template_resolves_rather_than_leaking() {
        // `finalize_return`'s fallback previously bound an unresolved
        // template straight to its raw `bound` without recursing into
        // it: `TKey`'s bound `Collection<TValue>` carried a live,
        // still-unresolved `TValue` through untouched. Neither `TKey`
        // nor `TValue` is constrained by any argument here (`make`
        // takes none), so both must fall through bound-then-mixed:
        // `TKey` to its bound `Collection<TValue>`, and `TValue`
        // (boundless) to `mixed` within it. The pre-fix behavior
        // answers `app\collection<TValue>` (the nested template
        // rendered literally, unresolved); the fix answers
        // `app\collection<mixed>` — different types, so this
        // discriminates.
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class Collection {}
/**
 * @template TValue
 * @template TKey of Collection<TValue>
 * @return TKey
 */
function make() {}
function caller() { return make(); }
"#]);
        assert_eq!(
            caller_return_display(&f, "app\\caller"),
            "app\\collection<mixed>"
        );
    }

    #[test]
    fn a_generic_class_parameter_binds_directly_when_names_match() {
        // Decision 10's FIRST `Class` rule: when the declared and
        // argument class names match, recurse argument-wise directly
        // rather than through the ancestry. No existing test drives
        // this: `a_generic_class_parameter_recurses_through_the_ancestry`
        // pairs `Collection<T>` against `UserCollection` (a DIFFERENT
        // name), landing on the fallback arm. Here the caller's own
        // `$collection` parameter is annotated `Collection<User>`
        // directly (same name as the callee's declared `Collection<T>`),
        // so the name-match arm is the only one that can bind `T`; the
        // fallback arm would look for `app\collection` in its OWN
        // ancestry (which has none, since it declares no `@extends`)
        // and contribute nothing, falling `T` to `mixed`.
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class User {}
/** @template T */
class Collection {}
/**
 * @template T
 * @param Collection<T> $items
 * @return T
 */
function first_item($items) {}
/**
 * @param Collection<User> $collection
 */
function caller(Collection $collection) { return first_item($collection); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user");
    }

    #[test]
    fn a_raw_string_literal_binds_the_template_through_class_string() {
        // Decision 10's FIRST `class-string` binder: a plain string
        // literal argument against a declared `class-string<T>`
        // parameter. `class_string_binds_the_template_through_class_constants`
        // only reaches the SECOND binder (`Foo::class`, already typed
        // `class-string<Foo>`); no test reached this arm, the one line
        // task 8 altered from the brief's prescribed code (dropping the
        // caller-side `.to_lowercase()`, since `TypeId::class` already
        // strips a leading `\` and folds through `folded_symbol_key`).
        // The literal here is both mixed-case and backslash-prefixed so
        // the assertion pins that folding, not just that the arm runs.
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class User {}
/**
 * @template T
 * @param class-string<T> $name
 * @return T
 */
function make(string $name) {}
function caller() { return make('\App\User'); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user");
    }

    // Task 9: constructor inference and inline `@var` deliver class
    // generics (decision 11). Docblock-bearing, so these use
    // `fixture_with_generics` for the same reason every task 8 test
    // above does — the brief's own pseudocode calls plain `fixture`,
    // but plain `fixture` registers no `TypeSyntax` at all, so
    // `@template`/`@param`/`@var` would all silently parse to nothing.

    #[test]
    fn constructor_arguments_solve_the_class_templates() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class User {}
/** @template T */
class Box {
    /** @param T $item */
    public function __construct($item) {}
    /** @return T */
    public function get() {}
}
function caller() {
    $box = new Box(new User());
    return $box->get();
}
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user");
    }

    #[test]
    fn a_constructorless_generic_new_stays_a_plain_class() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
/** @template T */
class Box {
    /** @return T */
    public function get() {}
}
function build() { return new Box(); }
function read() { return (new Box())->get(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\build"), "app\\box");
        assert_eq!(
            caller_return_display(&f, "app\\read"),
            "mixed",
            "an unconstrained template falls to its bound then mixed",
        );
    }

    #[test]
    fn a_constructor_argument_that_binds_nothing_stays_the_plain_class() {
        // The brief's own four tests never drive the `any_bound`
        // guard's `false` branch with an argument actually present:
        // `a_constructorless_generic_new_stays_a_plain_class`'s
        // `new Box()` has none at all (an earlier, cheaper guard
        // returns first), so a mutation deleting `any_bound` entirely
        // — always minting `TypeId::class(db, &key, arguments)` — left
        // every other test in this module green. Here `__construct`
        // takes an argument but declares no type for it (no `@param`,
        // no native hint), so `solver_pairs` contributes no pair and
        // nothing binds `T`: the guard must still answer the plain
        // `app\box`, not a `Box<mixed>` spelling of the same thing.
        let f = fixture_with_generics(&[r#"<?php
namespace App;
/** @template T */
class Box {
    public function __construct($item) {}
    /** @return T */
    public function get() {}
}
function build() { return new Box(1); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\build"), "app\\box");
    }

    #[test]
    fn an_inline_var_declares_the_local_through_its_assignment() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class User {}
/** @template T */
class Collection {
    /** @return T */
    public function first() {}
}
function caller($opaque) {
    /** @var Collection<User> $items */
    $items = $opaque;
    return $items->first();
}
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user");
    }

    #[test]
    fn an_inline_var_binds_before_its_anchored_statement_too() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
function caller($input) {
    /** @var int $input */
    return $input;
}
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "int");
    }

    // Task 11: decision 10's five structural collect arms
    // (`Array`, `Shape`, `Callable`, `Union`, `Intersection`) were
    // production code with no test since task 8 first wrote them —
    // the shared fake docblock syntax (`test_support::FakeSyntax`)
    // parsed none of `array<K, V>`, `|`, `&`, or `callable(...)`. Task
    // 11 extends that fake (additively — every existing form still
    // lowers exactly as before) and pins each arm here.

    #[test]
    fn the_array_arm_recurses_into_key_and_value_against_an_array_argument() {
        // Decision 10's `Array` rule, argument side `Array`: both `K`
        // and `V` bind from the argument array's own key and value,
        // not from each other or from the whole array — the two
        // assertions below (`int` and `string`) are different types
        // from each other and from the `mixed` an unconstrained
        // template would fall to, so either binding failing alone
        // discriminates.
        let f = fixture_with_generics(&[r#"<?php
namespace App;
/**
 * @template K
 * @template V
 * @param array<K, V> $items
 * @return K
 */
function first_key($items) {}
/**
 * @template K
 * @template V
 * @param array<K, V> $items
 * @return V
 */
function first_value($items) {}
/** @param array<int, string> $items */
function caller_key($items) { return first_key($items); }
/** @param array<int, string> $items */
function caller_value($items) { return first_value($items); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller_key"), "int");
        assert_eq!(caller_return_display(&f, "app\\caller_value"), "string");
    }

    #[test]
    fn the_array_arm_evaluates_key_of_and_value_of_against_a_shape_argument() {
        // Decision 10's `Array` rule, argument side `Shape`: an actual
        // PHP array literal infers a `Shape` through ordinary body
        // inference (no fixture grammar needed for the argument), and
        // the declared `array<K, V>` recurses against `key-of`/`value-of`
        // of that shape, evaluated eagerly since the shape is concrete.
        // The single field `'id' => 42` makes `K` the string literal
        // `'id'` and `V` the int literal `42` — distinct from each
        // other and from the `mixed` fallback.
        let f = fixture_with_generics(&[r#"<?php
namespace App;
/**
 * @template K
 * @template V
 * @param array<K, V> $items
 * @return K
 */
function first_key($items) {}
/**
 * @template K
 * @template V
 * @param array<K, V> $items
 * @return V
 */
function first_value($items) {}
function caller_key() { return first_key(['id' => 42]); }
function caller_value() { return first_value(['id' => 42]); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller_key"), "'id'");
        assert_eq!(caller_return_display(&f, "app\\caller_value"), "42");
    }

    #[test]
    fn the_callable_arm_recurses_into_the_return_type() {
        // Decision 10's `Callable` rule: `T` binds from the argument
        // closure's own inferred return type (a native `: int` return
        // hint, no fixture grammar needed on the argument side), never
        // from the closure's parameters (the arm does not look at
        // them). `int` differs from the `mixed` fallback.
        let f = fixture_with_generics(&[r#"<?php
namespace App;
/**
 * @template T
 * @param callable(): T $callback
 * @return T
 */
function invoke($callback) {}
function caller() { return invoke(function (): int { return 1; }); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "int");
    }

    #[test]
    fn the_union_arm_collects_from_every_constituent() {
        // Decision 10's `Union` rule: a declared `T|Fallback` recurses
        // the SAME argument into every constituent — `T` binds to the
        // argument's own type (the int literal `1`, displayed `"1"`),
        // while `Fallback` (an unrelated class) matches nothing against
        // an int argument and contributes silently. `"1"` differs from
        // the `mixed` fallback an unconstrained `T` would produce.
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class Fallback {}
/**
 * @template T
 * @param T|Fallback $value
 * @return T
 */
function first($value) {}
function caller() { return first(1); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "1");
    }

    #[test]
    fn the_intersection_arm_collects_from_every_constituent() {
        // Decision 10's `Intersection` rule: the dual of the `Union`
        // test above — a declared `T&Constraint` recurses the same
        // argument into every intersectand, `T` binds to the int
        // literal `1` (displayed `"1"`), and `Constraint` contributes
        // nothing against an int argument.
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class Constraint {}
/**
 * @template T
 * @param T&Constraint $value
 * @return T
 */
function first($value) {}
function caller() { return first(1); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "1");
    }

    #[test]
    fn the_shape_arm_recurses_field_wise_against_a_shape_argument() {
        // Issue #40: decision 10's prose says "arrays, shapes, and
        // callables recurse element-wise", but `collect` had no
        // declared-`Shape` arm — a shape-typed parameter contributed
        // no constraint and the return fell to `mixed`. The argument
        // `['id' => 42, 'name' => 'Ada']` infers a genuine `Shape`
        // through ordinary body inference; a declared `array{id: T}`
        // must bind `T` field-wise to the int literal `42`, and a
        // declared `array{name: T}` to `'Ada'`. The two assertions
        // together kill every near mutant: `mixed` (no arm), a
        // key/value mix-up (`'id'`/`'name'`), a first- or last-field
        // binding that ignores the declared key (the shape's sorted
        // field order puts `id` first and `name` last, so each
        // assertion catches one direction), and an every-field union
        // (`42|'Ada'`).
        let f = fixture_with_generics(&[r#"<?php
namespace App;
/**
 * @template T
 * @param array{id: T} $data
 * @return T
 */
function unwrap_id($data) {}
/**
 * @template T
 * @param array{name: T} $data
 * @return T
 */
function unwrap_name($data) {}
function caller_id() { return unwrap_id(['id' => 42, 'name' => 'Ada']); }
function caller_name() { return unwrap_name(['id' => 42, 'name' => 'Ada']); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller_id"), "42");
        assert_eq!(caller_return_display(&f, "app\\caller_name"), "'Ada'");
    }

    // Debt 3: `solved_call_result` (`flow.rs`) documents the provider
    // tier as needing "no special exemption" from call-site solving
    // because a provider's answer is "already concrete, so
    // `contains_symbolic` is already false for it" — an assertion
    // about every `DynamicTypeProvider` implementation, never
    // enforced by the call-site solver itself. This pins what actually
    // happens if that assertion is ever wrong: a provider that answers
    // a raw, symbolic template is neither a guess nor a panic — the
    // ordinary `solve`/`substitute`/`finalize_return` pipeline any
    // symbolic declared return already goes through runs over it
    // unchanged, and the unconstrained template falls to its bound
    // then `mixed`, the same conservative fallback everywhere else.

    #[test]
    fn a_symbolic_provider_answer_still_finalizes_to_its_bound_then_mixed() {
        use crate::dynamic_type_provider::{
            DynamicTypeProvider, DynamicTypeProviderRegistration, DynamicTypeProviderRegistry,
            Invocation, SymbolClaim,
        };
        use celerrate_semantics::PluginIdentity;

        #[derive(Debug)]
        struct SymbolicProvider;

        impl DynamicTypeProvider for SymbolicProvider {
            fn claims(&self) -> Vec<SymbolClaim> {
                vec![SymbolClaim::Function {
                    key: "app\\make".to_owned(),
                }]
            }
            fn return_type<'db>(
                &self,
                db: &'db dyn salsa::Database,
                _invocation: &Invocation<'db>,
            ) -> Option<crate::TypeId<'db>> {
                // A boundless template under a scope no declared
                // signature could ever share — exactly the leak the
                // doc says never happens today.
                Some(crate::TypeId::template(
                    db,
                    "provider-scope",
                    "T",
                    crate::TypeId::mixed(db),
                ))
            }
        }

        let f = fixture(&["<?php
namespace App;
function make() { return 1; }
function caller() { return make(); }
"]);
        let _ = DynamicTypeProviderRegistry::builder(vec![DynamicTypeProviderRegistration {
            identity: PluginIdentity {
                name: "symbolic".to_owned(),
                version: "0.0.0".to_owned(),
                configuration: String::new(),
            },
            provider: std::sync::Arc::new(SymbolicProvider),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&f.db);
        assert_eq!(caller_return_display(&f, "app\\caller"), "mixed");
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
        // Task 5 (the receiver model, decision 1) supersedes the
        // original expectations here: `self` and `parent` resolve
        // immediately in the defining context (both are structurally
        // known right here, no forwarding needed), so `new self()` and
        // `new parent()` still answer the defining/parent class. But
        // `new static()`'s identity depends on whoever eventually calls
        // in through late static binding — unknowable from inside the
        // body — so it stays the symbolic `static` placeholder here,
        // exactly like `$this`
        // (`this_types_as_the_static_placeholder_inside_the_body`), and
        // resolves only at the outer call boundary
        // (`a_native_static_return_substitutes_to_the_receiver`). The
        // class type renders as its folded (lowercase) key (decision 16).
        assert_eq!(return_display(&fixture, 2), "child");
        assert_eq!(return_display(&fixture, 3), "base");
        assert_eq!(return_display(&fixture, 4), "static");
        // No static-method gate on the class keyword ($this's
        // unavailability in a static method does not gate `static::`):
        // `new static()` still forwards the same way in
        // `makeStaticInStatic`.
        assert_eq!(return_display(&fixture, 5), "static");
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

    #[test]
    fn the_lazy_getter_narrows_its_property() {
        let fixture = fixture(&["<?php class Service {}
            class Locator {
                private ?Service $service = null;
                public function get(): object {
                    if ($this->service === null) {
                        $this->service = new Service();
                    }
                    return $this->service;
                }
            }"]);
        // Numbering: Service 0, Locator 1, property 2, get 3.
        assert_eq!(return_display(&fixture, 3), "service");
    }

    #[test]
    fn a_method_call_kills_property_narrowings() {
        let fixture = fixture(&["<?php class Service {}
            class Holder {
                private ?Service $service = null;
                private function log(): void {}
                public function get() {
                    if ($this->service === null) { return 1; }
                    $this->log();
                    return $this->service;
                }
            }"]);
        // The call re-widens the property to its declared type. The
        // brief's original expectation ("null|1|service") transposed
        // the display's established null-last convention (`display.rs`'s
        // `composites_render_with_null_last_in_unions`, already
        // corrected for the same reason above): non-null constituents
        // render in structural rank order (the int literal before the
        // class), null last.
        assert_eq!(return_display(&fixture, 4), "1|service|null");
    }

    #[test]
    fn by_reference_arguments_take_the_write_back_type() {
        let fixture = fixture(&[
            "<?php class W { public function fill(array &$out): void {} }
            function f(W $w) {
                $x = null;
                $w->fill($x);
                return $x;
            }",
        ]);
        assert_eq!(return_display(&fixture, 2), "array<int|string, mixed>");
    }

    #[test]
    fn a_by_reference_closure_use_degrades_the_local() {
        let fixture = fixture(&["<?php function f() {
                $x = 'a';
                $g = function () use (&$x) {};
                return $x;
            }"]);
        assert_eq!(return_display(&fixture, 0), "mixed");
    }

    #[test]
    fn extract_forgets_every_local() {
        let fixture =
            fixture(&["<?php function f() { $x = 1; extract(['x' => 'a']); return $x; }"]);
        assert_eq!(return_display(&fixture, 0), "mixed");
    }

    #[test]
    fn a_by_reference_property_argument_takes_the_write_back_type() {
        let fixture = fixture(&["<?php class Holder {
                private ?int $count = null;
                private function fill(int &$out): void {}
                public function run() {
                    $this->fill($this->count);
                    return $this->count;
                }
            }"]);
        // Numbering: Holder 0, property 1, fill 2, run 3.
        // The kill runs first (dropping any property narrowing), then
        // the by-reference write-back binds `$this->count` to `&$out`'s
        // declared `int` — so the final read is the write-back type,
        // NOT the wider declared property type `int|null`. This guards
        // the kill-then-write-back order against Task 9's rewrite of
        // this same Call arm (which reuses `apply_by_reference`).
        assert_eq!(return_display(&fixture, 3), "int");
    }

    #[test]
    fn function_calls_take_declared_returns_and_resolve_through_the_namespace() {
        let fixture = fixture(&["<?php namespace App;
            function g(): string { return 'x'; }
            function f() { return g(); }"]);
        // Numbering: the namespace declaration is itself a numbered
        // item (namespace = 0, g = 1, f = 2 — the same convention as
        // `body_owner_resolves_free_functions_and_methods` above).
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
            InferenceContext::new(&fixture.db, None),
        )
        .as_ref()
        .unwrap();
        assert_eq!(inferred.edge_counts.declared_return_edges, 1);
    }

    fn fake_identity(name: &str) -> celerrate_semantics::PluginIdentity {
        celerrate_semantics::PluginIdentity {
            name: name.to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        }
    }

    fn register_fake_provider(fixture: &Fixture) {
        use crate::{
            DynamicTypeProviderRegistration, DynamicTypeProviderRegistry, Invocation, SymbolClaim,
        };

        #[derive(Debug)]
        struct FakeMaker;

        impl crate::DynamicTypeProvider for FakeMaker {
            fn claims(&self) -> Vec<SymbolClaim> {
                vec![SymbolClaim::Function {
                    key: "maker".to_owned(),
                }]
            }

            fn return_type<'db>(
                &self,
                db: &'db dyn salsa::Database,
                _invocation: &Invocation<'db>,
            ) -> Option<crate::TypeId<'db>> {
                Some(crate::TypeId::int(db))
            }
        }

        let _ = DynamicTypeProviderRegistry::builder(vec![DynamicTypeProviderRegistration {
            identity: fake_identity("fake-maker"),
            provider: std::sync::Arc::new(FakeMaker),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
    }

    #[test]
    fn a_dynamic_provider_claim_answers_first_and_counts() {
        let fixture = fixture(&["<?php function maker(): string { return 'x'; }
            function f() { return maker(); }"]);
        register_fake_provider(&fixture);
        assert_eq!(return_display(&fixture, 1), "int");
        let file = fixture.handles[0];
        let body = body_query(&fixture, 1);
        let inferred = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
            InferenceContext::new(&fixture.db, None),
        )
        .as_ref()
        .unwrap();
        assert_eq!(inferred.edge_counts.provider_edges, 1);
        assert_eq!(inferred.edge_counts.declared_return_edges, 0);
    }

    /// The instrument's end-to-end pin (task 12): one body's calls span
    /// all three tiers at once. `declared_edge` has a native return, so
    /// it counts as a declared edge; `inferred_edge` has none, so it
    /// counts as an inferred edge; `maker` has BOTH a declared `: string`
    /// AND a registered provider claim, and the provider is consulted
    /// before the declared tier (`provider_return` runs first in
    /// `function_call_result`'s caller), so it counts once as a
    /// provider edge and not again as declared. Each tier is counted
    /// exactly once.
    #[test]
    fn the_edge_count_instrument_counts_each_tier_once() {
        let fixture = fixture(&["<?php
            function declared_edge(): int { return 1; }
            function inferred_edge() { return 'x'; }
            function maker(): string { return 'x'; }
            function f() { return [declared_edge(), inferred_edge(), maker()]; }"]);
        register_fake_provider(&fixture);
        let file = fixture.handles[0];
        let body = body_query(&fixture, 3);
        let inferred = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
            InferenceContext::new(&fixture.db, None),
        )
        .as_ref()
        .unwrap();
        assert_eq!(inferred.edge_counts.declared_return_edges, 1);
        assert_eq!(inferred.edge_counts.inferred_return_edges, 1);
        assert_eq!(inferred.edge_counts.provider_edges, 1);
    }

    #[test]
    fn closures_and_arrows_type_as_callables_and_invoke() {
        let fixture = fixture(&["<?php
            function declared() { $g = function (): int { return 1; }; return $g(); }
            function inferred() { $g = function () { return 'a'; }; return $g(); }
            function captured() { $x = 'a'; $g = fn () => $x; return $g(); }"]);
        assert_eq!(return_display(&fixture, 0), "int");
        assert_eq!(return_display(&fixture, 1), "'a'");
        assert_eq!(return_display(&fixture, 2), "'a'");
    }

    #[test]
    fn first_class_callables_project_the_declared_signature() {
        let fixture = fixture(&["<?php function g(int $n): string { return 'x'; }
            function f() { $r = g(...); return $r(); }"]);
        assert_eq!(return_display(&fixture, 1), "string");
    }

    #[test]
    fn function_by_reference_arguments_write_back() {
        let fixture = fixture(&["<?php function fill(array &$out): void {}
            function f() { $x = null; fill($x); return $x; }"]);
        assert_eq!(return_display(&fixture, 1), "array<int|string, mixed>");
    }

    #[test]
    fn ascend_joins_monotonically_and_bails_to_mixed_past_the_budget() {
        let db = TestDatabase::default();
        let int = crate::TypeId::int(&db);
        let string = crate::TypeId::string(&db);
        let never = crate::TypeId::never(&db);
        // Ascent from the bottom.
        assert_eq!(super::ascend(&db, 0, never, int), int);
        // A widening iterate joins, never replaces.
        assert_eq!(
            super::ascend(&db, 1, int, string),
            crate::TypeId::union(&db, [int, string]),
        );
        // Convergence: identical join answers the provisional value.
        assert_eq!(super::ascend(&db, 5, int, int), int);
        // Budget exhaustion on a still-moving value: mixed, the
        // deterministic bailout — never salsa's panic.
        assert_eq!(
            super::ascend(&db, super::FIXPOINT_ITERATION_BUDGET, int, string),
            crate::TypeId::mixed(&db),
        );
        // The budget sits far below salsa's cap (MAX_ITERATIONS=200).
        assert!(super::FIXPOINT_ITERATION_BUDGET < 200);
    }

    fn register_fake_assertions(fixture: &Fixture) {
        use crate::{
            AssertionPolarity, ParsedAnnotations, ParsedAssertion, TypeSyntax,
            TypeSyntaxRegistration, TypeSyntaxRegistry,
        };

        #[derive(Debug)]
        struct FakeAssertions;

        impl TypeSyntax for FakeAssertions {
            fn can_parse(&self, docblock: &str) -> bool {
                docblock.contains("@fake-")
            }

            fn parse_docblock<'db>(
                &self,
                site: &crate::AnnotationSite<'db, '_>,
                docblock: &str,
            ) -> ParsedAnnotations<'db> {
                let db = site.database();
                let mut parsed = ParsedAnnotations::default();
                if docblock.contains("@fake-assert-string") {
                    parsed.assertions.push(ParsedAssertion {
                        subject: "$value".to_owned(),
                        asserted: crate::TypeId::string(db),
                        polarity: AssertionPolarity::Always,
                        negated: false,
                    });
                }
                if docblock.contains("@fake-if-true-string") {
                    parsed.assertions.push(ParsedAssertion {
                        subject: "$value".to_owned(),
                        asserted: crate::TypeId::string(db),
                        polarity: AssertionPolarity::IfTrue,
                        negated: false,
                    });
                }
                if docblock.contains("@fake-assert-this-prop") {
                    parsed.assertions.push(ParsedAssertion {
                        subject: "$this->prop".to_owned(),
                        asserted: crate::TypeId::string(db),
                        polarity: AssertionPolarity::Always,
                        negated: false,
                    });
                }
                parsed
            }

            fn parse_type_expression<'db>(
                &self,
                _site: &crate::AnnotationSite<'db, '_>,
                _expression: &str,
            ) -> Option<crate::TypeId<'db>> {
                None
            }
        }

        let _ = TypeSyntaxRegistry::builder(vec![TypeSyntaxRegistration {
            identity: fake_identity("fake-assertions"),
            implementation: std::sync::Arc::new(FakeAssertions),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
    }

    #[test]
    fn an_always_assertion_narrows_after_the_call() {
        let fixture = fixture(&["<?php class Assert {
                /** @fake-assert-string */
                public static function string(mixed $value): void {}
            }
            function f(mixed $x) { Assert::string($x); return $x; }"]);
        register_fake_assertions(&fixture);
        assert_eq!(return_display(&fixture, 2), "string");
    }

    #[test]
    fn an_if_true_assertion_narrows_the_condition_branches() {
        let fixture = fixture(&["<?php
            /** @fake-if-true-string */
            function ok(mixed $value): bool { return true; }
            function f(mixed $x) { if (ok($x)) { return $x; } return 1; }"]);
        register_fake_assertions(&fixture);
        assert_eq!(return_display(&fixture, 1), "1|string");
    }

    #[test]
    fn a_nested_argument_calls_condition_fact_does_not_leak() {
        // `helper($y)` is only an argument to the tested call `ok(...)`;
        // its truthiness is never tested, so its `IfTrue` fact must not
        // narrow `$y` in the true branch (decision 17). `$y` stays
        // mixed, so the body returns mixed (mixed absorbs the `1`).
        let fixture = fixture(&["<?php
            /** @fake-if-true-string */
            function ok(mixed $value): bool { return true; }
            /** @fake-if-true-string */
            function helper(mixed $value): bool { return true; }
            function f(mixed $x, mixed $y) { if (ok(helper($y))) { return $y; } return 1; }"]);
        register_fake_assertions(&fixture);
        // Numbering: ok 0, helper 1, f 2.
        assert_eq!(return_display(&fixture, 2), "mixed");
    }

    #[test]
    fn a_this_subject_assertion_narrows_the_callers_property() {
        let fixture = fixture(&["<?php class A {
                public mixed $prop = null;
                /** @fake-assert-this-prop */
                public function check(): void {}
                public function read() { $this->check(); return $this->prop; }
            }"]);
        register_fake_assertions(&fixture);
        // Numbering: class 0, property 1, check 2, read 3.
        assert_eq!(return_display(&fixture, 3), "string");
    }

    /// Counts how many times a query appears in an executed-query log
    /// (the `celerrate_semantics` invalidation-scope tests'
    /// `executions_of` pattern, duplicated here exactly as
    /// `tests/invalidation_scope.rs` duplicates it: no shared
    /// test-support module exists per the design).
    fn executions_of(log: &[String], query: &str) -> usize {
        let prefix = format!("{query}(");
        log.iter()
            .filter(|entry| entry.contains(prefix.as_str()))
            .count()
    }

    #[test]
    fn a_method_call_takes_the_inferred_return_when_no_declaration_exists() {
        let f = fixture(&[r#"<?php
namespace App;
class Greeter {
    public function greeting() { return 'hello'; }
}
function caller(Greeter $g) { return $g->greeting(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "'hello'");
    }

    #[test]
    fn a_declared_return_still_wins_over_the_body() {
        let f = fixture(&[r#"<?php
namespace App;
class Greeter {
    public function greeting(): string { return 'hello'; }
}
function caller(Greeter $g) { return $g->greeting(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "string");
    }

    #[test]
    fn mutual_method_recursion_converges_to_the_joined_union() {
        let f = fixture(&[r#"<?php
namespace App;
class Pair {
    public function left(bool $flip) {
        if ($flip) { return 1; }
        return $this->right($flip);
    }
    public function right(bool $flip) {
        if ($flip) { return 'one'; }
        return $this->left($flip);
    }
}
function caller(Pair $p) { return $p->left(true); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "1|'one'");
    }

    /// Determinism, the salsa requirement the fixpoint must not break:
    /// a mutual cluster converges to one answer whatever member is
    /// asked first. Same source as the test above, entered from
    /// `right` instead of through `caller` — `left` must still answer
    /// `1|'one'`.
    #[test]
    fn a_mutual_method_cluster_converges_the_same_from_either_entry_point() {
        let source = r#"<?php
namespace App;
class Pair {
    public function left(bool $flip) {
        if ($flip) { return 1; }
        return $this->right($flip);
    }
    public function right(bool $flip) {
        if ($flip) { return 'one'; }
        return $this->left($flip);
    }
}
"#;
        let method_return = |f: &Fixture, name: &str| {
            inferred_method_return(
                &f.db,
                f.files,
                f.stubs,
                f.configuration,
                MethodQuery::new(&f.db, "app\\pair".to_owned(), name.to_owned()),
            )
            .display(&f.db)
        };

        let from_left = fixture(&[source]);
        let left_first = method_return(&from_left, "left");
        assert_eq!(left_first, "1|'one'");

        let from_right = fixture(&[source]);
        // Enter the cluster from the other member first. Both members
        // of this cluster share the one joined answer (`left` is
        // `1 | right`, `right` is `'one' | left`), so the entry point
        // changes nothing — neither which member is asked, nor which
        // is asked first.
        assert_eq!(method_return(&from_right, "right"), left_first);
        assert_eq!(method_return(&from_right, "left"), left_first);
    }

    #[test]
    fn an_inherited_method_infers_once_per_defining_class() {
        let f = fixture(&[r#"<?php
namespace App;
class Base {
    public function answer() { return 42; }
}
class LeftChild extends Base {}
class RightChild extends Base {}
"#]);
        let left = MethodQuery::new(&f.db, "app\\leftchild".to_owned(), "answer".to_owned());
        let right = MethodQuery::new(&f.db, "app\\rightchild".to_owned(), "answer".to_owned());
        f.db.take_executed();
        let left_return = inferred_method_return(&f.db, f.files, f.stubs, f.configuration, left);
        let right_return = inferred_method_return(&f.db, f.files, f.stubs, f.configuration, right);
        assert_eq!(left_return, right_return);
        assert_eq!(left_return.display(&f.db), "42");
        let log = f.db.take_executed();
        assert_eq!(
            executions_of(&log, "inferred_body_types"),
            1,
            "one body, inferred once: {log:?}",
        );

        // The re-key itself (decision 4), pinned so that removing it
        // fails *this* assertion: both subclasses answered *through*
        // the defining class's query, so the defining class's own memo
        // is already populated and demanding it now executes nothing.
        // The body-execution count above cannot pin this — the body
        // memo is keyed by the member's AST identity, which is the
        // defining class's either way — so without this assertion the
        // test would pass whether or not the re-key fires.
        let base = MethodQuery::new(&f.db, "app\\base".to_owned(), "answer".to_owned());
        let base_return = inferred_method_return(&f.db, f.files, f.stubs, f.configuration, base);
        assert_eq!(base_return, left_return);
        let log = f.db.take_executed();
        assert_eq!(
            executions_of(&log, "inferred_method_return"),
            0,
            "every subclass re-keys into the defining class's one memo: {log:?}",
        );
    }

    #[test]
    fn a_growing_method_recursion_bails_to_mixed_within_the_budget() {
        let f = fixture(&[r#"<?php
namespace App;
class Nest {
    public function deeper() { return [$this->deeper()]; }
}
function caller(Nest $n) { return $n->deeper(); }
"#]);
        // The array constructor grows the type every iterate; the
        // ascent widens deterministically to mixed — never salsa's
        // panic. Which guard fires first is deliberately not the
        // contract: in this fixture the lattice caps
        // (`UNION_ARITY_CAP`, `STRUCTURAL_DEPTH_CAP`) converge the
        // ascent before the budget ever reaches its bail (raising
        // `FIXPOINT_ITERATION_BUDGET` to 250 leaves this answer
        // `mixed`), and the budget's own bail is pinned directly by
        // `ascend_joins_monotonically_and_bails_to_mixed_past_the_budget`.
        // What this test pins is the property both guards exist for: a
        // growing *method* cycle terminates, at mixed, without
        // reaching salsa's `MAX_ITERATIONS` panic.
        assert_eq!(caller_return_display(&f, "app\\caller"), "mixed");
    }

    #[test]
    fn an_inferred_this_return_substitutes_to_the_receiver() {
        let f = fixture(&[r#"<?php
namespace App;
class Base {
    public function itself() { return $this; }
}
class Child extends Base {}
function caller(Child $c) { return $c->itself(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\child");
    }

    #[test]
    fn a_stub_or_unknown_receiver_method_stays_mixed() {
        let f = fixture(&[r#"<?php
namespace App;
function caller($anything) { return $anything->whatever(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "mixed");
    }

    /// The Stub arm of decision 4 — the one origin the test above does
    /// *not* reach (an opaque receiver resolves to no key at all, so
    /// the query is never asked). A stub method with no declared
    /// return fails the declared gate and does reach the tier; it has
    /// no body, so the answer is silence. Reachability is the point,
    /// and it is mutation-checked: making this arm answer a concrete
    /// type fails this test and no other.
    #[test]
    fn a_stub_method_without_a_declared_return_answers_mixed() {
        use celerrate_stubs::{
            StubAvailability, StubClassSurface, StubMember, StubMemberKind, StubSignature,
            StubSymbol, StubSymbolKind, StubVisibility, VersionedTypeText,
        };

        let db = TestDatabase::default();
        let files = AnalyzedFileSet::new(
            &db,
            vec![SourceFile::new(&db, FileId::new(0), b"<?php".to_vec())],
        );
        let untyped_method = StubMember {
            kind: StubMemberKind::Method,
            name: "compute".to_owned(),
            visibility: StubVisibility::Public,
            is_static: false,
            availability: StubAvailability::ALWAYS,
            // No declared return: the gate fails, the tier is reached.
            signature: Some(StubSignature {
                parameters: vec![],
                return_type: VersionedTypeText::from_text(None),
                by_reference: false,
            }),
            type_text: VersionedTypeText::default(),
            value_text: None,
        };
        let stubs = StubIndexInput::builder(StubIndex::new(
            vec![StubSymbol {
                name: "Legacy".to_owned(),
                kind: StubSymbolKind::Class,
                availability: StubAvailability::ALWAYS,
            }],
            vec![],
            vec![(
                "Legacy".to_owned(),
                StubClassSurface {
                    parents: vec![],
                    members: vec![untyped_method],
                },
            )],
        ))
        .durability(salsa::Durability::HIGH)
        .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);

        let query = MethodQuery::new(&db, "legacy".to_owned(), "compute".to_owned());
        let answer = inferred_method_return(&db, files, stubs, configuration, query);
        assert_eq!(answer.display(&db), "mixed");
    }

    /// The Trait arm of decision 4 and the context of decision 5,
    /// together: a trait member analyzes *per using class* — the
    /// query's own `class_key` — so `self` inside the trait body is
    /// the using class, which is what PHP means by `self` in a trait
    /// (PHPStan's model). Mutation-checked: threading `None` here
    /// instead of the using class answers `app\speaks`, the trait
    /// itself, and fails this test alone. Task 7 owns trait behavior
    /// proper; this pins what task 6's mechanical threading already
    /// decides.
    #[test]
    fn a_trait_method_infers_for_the_using_class() {
        let f = fixture(&[r#"<?php
namespace App;
trait Speaks {
    public function speak() { return 'hi'; }
    public function make() { $x = new self(); return $x; }
}
class Talker { use Speaks; }
function caller(Talker $t) { return $t->speak(); }
function using_class(Talker $t) { return $t->make(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "'hi'");
        assert_eq!(caller_return_display(&f, "app\\using_class"), "app\\talker");
    }

    #[test]
    fn a_trait_body_types_against_each_using_class() {
        let f = fixture(&[r#"<?php
namespace App;
trait Reader {
    public function read() { return $this->value; }
}
class IntBox {
    use Reader;
    public int $value = 0;
}
class StringBox {
    use Reader;
    public string $value = '';
}
function ints(IntBox $box) { return $box->read(); }
function strings(StringBox $box) { return $box->read(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\ints"), "int");
        assert_eq!(caller_return_display(&f, "app\\strings"), "string");
    }

    #[test]
    fn two_using_classes_mean_two_memos_for_one_trait_body() {
        let f = fixture(&[r#"<?php
namespace App;
trait Reader {
    public function read() { return $this->value; }
}
class IntBox { use Reader; public int $value = 0; }
class StringBox { use Reader; public string $value = ''; }
"#]);
        f.db.take_executed();
        let _ = inferred_method_return(
            &f.db,
            f.files,
            f.stubs,
            f.configuration,
            MethodQuery::new(&f.db, "app\\intbox".to_owned(), "read".to_owned()),
        );
        let _ = inferred_method_return(
            &f.db,
            f.files,
            f.stubs,
            f.configuration,
            MethodQuery::new(&f.db, "app\\stringbox".to_owned(), "read".to_owned()),
        );
        let log = f.db.take_executed();
        assert_eq!(
            executions_of(&log, "inferred_body_types"),
            2,
            "the per-receiver key exists exactly where substitution is impossible: {log:?}",
        );
    }

    #[test]
    fn an_aliased_trait_method_still_finds_its_body() {
        let f = fixture(&[r#"<?php
namespace App;
trait Maker {
    public function make() { return 42; }
}
class Factory {
    use Maker { make as build; }
}
function caller(Factory $factory) { return $factory->build(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "42");
    }

    #[test]
    fn insteadof_routes_to_the_chosen_trait_body() {
        let f = fixture(&[r#"<?php
namespace App;
trait Ints {
    public function pick() { return 1; }
}
trait Strings {
    public function pick() { return 'one'; }
}
class Chooser {
    use Ints, Strings {
        Ints::pick insteadof Strings;
    }
}
function caller(Chooser $chooser) { return $chooser->pick(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "1");
    }

    #[test]
    fn a_trait_body_calling_the_users_helper_resolves_against_the_user() {
        let f = fixture(&[r#"<?php
namespace App;
trait Delegating {
    public function invoke() { return $this->helper(); }
}
class WithHelper {
    use Delegating;
    public function helper() { return 'helped'; }
}
function caller(WithHelper $subject) { return $subject->invoke(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "'helped'");
    }

    /// The trait boundary owner mismatch (task 7's flagged latent
    /// defect): `member_owner` (`flow.rs`) answers `lookup_member`'s
    /// `owner`, which for a `Trait`-origin resolution names the trait
    /// itself, not the using class — even though decision 5 already
    /// analyzes the trait body *for* the using class. None of the
    /// brief's five pinning tests exercises this: they all reach the
    /// trait body through `$this` (a `StaticPlaceholder`, substituted
    /// against the *receiver*, never the owner) or through members the
    /// using class declares itself (`Own` origin, no trait involved).
    /// A `self`-typed *declared* return annotation on a trait method is
    /// the one shape that crosses the boundary through `member_owner`:
    /// its `SelfPlaceholder` substitutes unconditionally against
    /// whatever `owner` names (`substitution.rs`'s `SelfPlaceholder`
    /// arm has no scope key to fall back through, unlike `Template`),
    /// so a wrong owner is a wrong concrete answer, not silence. Two
    /// using classes of the same trait, asked through two different
    /// callers, must answer *their own* class, not the trait and not
    /// each other's — mutation-checked: reverting `member_owner`'s
    /// `Trait` arm makes both calls answer `app\maker` instead.
    #[test]
    fn a_trait_method_s_self_return_resolves_against_the_using_class() {
        let f = fixture(&[r#"<?php
namespace App;
trait Maker {
    public function make(): self { return $this; }
}
class Factory {
    use Maker;
}
class OtherFactory {
    use Maker;
}
function caller(Factory $f) { return $f->make(); }
function other(OtherFactory $o) { return $o->make(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\factory");
        assert_eq!(caller_return_display(&f, "app\\other"), "app\\otherfactory");
    }

    /// Task 7b, shape 1: the trait boundary survives one `extends` step
    /// out of the using class. Task 7's fix answered `key` — the class
    /// the lookup was queried against — which is the using class only
    /// for a *direct* use. Queried through a subclass, `self` in the
    /// trait method still means the class that wrote `use`, because PHP
    /// pastes a trait into its user at compile time and `self` does not
    /// follow late static binding: the answer is `app\factory`, never
    /// `app\maker` (the trait, the pre-fix answer) and never `app\sub`
    /// (the receiver, what a bare `key.to_owned()` would say). Two
    /// independent trait users, each with their own subclass, answer
    /// their own using class — one fixture cannot tell "per using
    /// class" apart from "one hard-coded class".
    #[test]
    fn a_trait_method_s_self_return_anchors_to_the_using_class_not_the_subclass() {
        let f = fixture(&[r#"<?php
namespace App;
trait Maker {
    public function make(): self { return $this; }
}
class Factory { use Maker; }
class OtherFactory { use Maker; }
class Sub extends Factory {}
class OtherSub extends OtherFactory {}
function caller(Sub $s) { return $s->make(); }
function other(OtherSub $o) { return $o->make(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\factory");
        assert_eq!(caller_return_display(&f, "app\\other"), "app\\otherfactory");
    }

    /// Task 7b, shape 2: a trait using a trait. The anchor is carried
    /// forward across every trait-use step, so the innermost trait's
    /// `self` still names the class that used the outermost trait —
    /// `app\c`, never `app\inner` (the innermost trait, the pre-fix
    /// answer) and never `app\outer`. Two using classes of one nested
    /// trait pair answer differently.
    #[test]
    fn a_nested_trait_method_s_self_return_anchors_to_the_using_class() {
        let f = fixture(&[r#"<?php
namespace App;
trait Inner {
    public function make(): self { return $this; }
}
trait Outer { use Inner; }
class C { use Outer; }
class D { use Outer; }
function c_caller(C $c) { return $c->make(); }
function d_caller(D $d) { return $d->make(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\c_caller"), "app\\c");
        assert_eq!(caller_return_display(&f, "app\\d_caller"), "app\\d");
    }

    /// Task 7b's residual-risk probe, and the anchored `InferenceContext`
    /// it justifies. The investigation left `$this->helper()` inside a
    /// trait body degrading to `mixed` one `extends` step out, because
    /// the `Inherited` re-key threaded no context and the trait body
    /// resolved `$this` against the trait itself. That is conservative
    /// only while the trait declares no same-named member — the flagged
    /// residual. With the anchor, the trait body is analyzed for its
    /// using class in both shapes, so the *user's* `helper()` answers,
    /// exactly as it already does for a direct use. Two using classes
    /// with different helper types, each reached through a subclass.
    #[test]
    fn a_trait_body_calling_the_users_helper_resolves_against_the_user_through_a_subclass() {
        let f = fixture(&[r#"<?php
namespace App;
trait Delegating {
    public function invoke() { return $this->helper(); }
}
class WithHelper {
    use Delegating;
    public function helper() { return 'helped'; }
}
class OtherWithHelper {
    use Delegating;
    public function helper() { return 7; }
}
class Sub extends WithHelper {}
class OtherSub extends OtherWithHelper {}
function caller(Sub $subject) { return $subject->invoke(); }
function other(OtherSub $subject) { return $subject->invoke(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "'helped'");
        assert_eq!(caller_return_display(&f, "app\\other"), "7");
    }

    /// The residual risk the investigation flagged, isolated: the trait
    /// declares a member of the same name as the one its body calls, so
    /// resolving `$this` against the trait would find a *real* member
    /// and answer a wrong concrete type (`'trait'`) instead of failing
    /// to `mixed`. The using class's own `helper()` shadows the trait's
    /// (own beats trait, PHP's rule), and the trait body analyzed for
    /// that using class must see the shadowing member — through a
    /// subclass just as through a direct use.
    #[test]
    fn a_trait_declaring_the_same_helper_does_not_shadow_the_users_own() {
        let f = fixture(&[r#"<?php
namespace App;
trait Delegating {
    public function invoke() { return $this->helper(); }
    public function helper() { return 'trait'; }
}
class WithHelper {
    use Delegating;
    public function helper() { return 'user'; }
}
class Sub extends WithHelper {}
function direct(WithHelper $subject) { return $subject->invoke(); }
function through_subclass(Sub $subject) { return $subject->invoke(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\direct"), "'user'");
        assert_eq!(caller_return_display(&f, "app\\through_subclass"), "'user'");
    }

    /// A trait-use adaptation is written at the using site, so it must
    /// keep applying when the member is reached one `extends` step out:
    /// the alias `make as build` belongs to `Factory`'s clause, and
    /// `Sub` inherits the adapted table, not the raw trait.
    #[test]
    fn an_aliased_trait_method_still_finds_its_body_through_a_subclass() {
        let f = fixture(&[r#"<?php
namespace App;
trait Maker {
    public function make() { return 42; }
}
class Factory {
    use Maker { make as build; }
}
class Sub extends Factory {}
function caller(Sub $subject) { return $subject->build(); }
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "42");
    }

    // Task 10: iteration typing through the protocol chain (decision
    // 12). `foreach` resolves key and value through
    // `Walker::iteration_types`: array forms answer directly; the
    // protocol interfaces (`Generator`, `Iterator`, `IteratorAggregate`,
    // `Traversable`) carrying two or more arguments answer them
    // directly; an implementor's `getIterator` unwraps recursively
    // under a depth guard of 8; a class with threaded ancestor
    // arguments answers them, else its `current`/`key` returns; a
    // union joins its constituents, skipping `null` and `false`; a
    // template recurses through its bound; everything else is `mixed`.
    // Review fix: the `getIterator`/`current`-`key` arms additionally
    // gate on the class genuinely implementing an iteration protocol
    // interface (`Walker::implements_iteration_protocol`), never on
    // method presence alone.

    /// Minimal source declarations of the iteration protocol
    /// interfaces (`Traversable`, `Iterator`, `IteratorAggregate`),
    /// unqualified in the global namespace exactly like the real SPL
    /// builtins. `fixture`'s stub index is empty
    /// (`StubIndex::from_symbols(vec![])`), so nothing resolves these
    /// names as a genuine ancestor unless a fixture's own sources
    /// declare them: `Walker::implements_iteration_protocol` (the
    /// review's Finding 1 fix) walks `linearized_class`'s ancestry,
    /// which only records a resolved edge (`AncestorEdge::resolved`/
    /// `stub`) when the name actually resolves to a known class-like.
    /// Every fixture below that exercises a genuine implementor
    /// prepends this source alongside its own.
    const PROTOCOL_INTERFACES: &str = r#"<?php
interface Traversable {}
interface Iterator extends Traversable {}
interface IteratorAggregate extends Traversable {}
"#;

    /// Deviation from the brief's illustrative expectations, verified
    /// against this crate's actual, deterministic behavior rather than
    /// taken on faith: a bare literal `[1, 2]` (`flow.rs`'s
    /// `array_literal`) types as a `Shape` with literal integer keys
    /// `0`/`1`, not a widened `Array`/list — decision 12's "a list
    /// answers `int` keys" rule is for the already-widened `Array`
    /// form (`representation.rs`: "a list always stores the general
    /// int key"), which this literal never becomes before the
    /// `foreach` on the very same expression reads it. So `keys()`
    /// answers the shape's own literal key union (`"0|1"`, decision
    /// 12's other clause: "a shape answers its key union"), not
    /// `"int|0"` as the brief's pseudocode assumed. `values()`
    /// similarly answers `"0|1|2"` — the sorted literal union, not the
    /// brief's insertion-ordered `"1|2|0"` (`ordering.rs`'s
    /// `structural_order` sorts `Int` literals by value, the same
    /// deterministic rule every other union in this crate already
    /// follows).
    #[test]
    fn foreach_over_an_array_literal_types_key_and_value() {
        let f = fixture(&[r#"<?php
namespace App;
function values() {
    foreach ([1, 2] as $key => $value) { return $value; }
    return 0;
}
function keys() {
    foreach ([1, 2] as $key => $value) { return $key; }
    return 0;
}
"#]);
        assert_eq!(caller_return_display(&f, "app\\values"), "0|1|2");
        assert_eq!(caller_return_display(&f, "app\\keys"), "0|1");
    }

    #[test]
    fn a_declared_generator_return_drives_foreach() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class User {}
/** @return \Generator<int, User> */
function stream() {}
function caller() {
    foreach (stream() as $user) { return $user; }
    return null;
}
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user|null");
    }

    #[test]
    fn the_protocol_interfaces_carry_their_own_arguments() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class User {}
/** @return \Iterator<string, User> */
function iterate() {}
function caller() {
    foreach (iterate() as $key => $user) { return $key; }
    return '';
}
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "string|''");
    }

    #[test]
    fn an_iterator_aggregate_unwraps_get_iterator() {
        let f = fixture_with_generics(&[
            PROTOCOL_INTERFACES,
            r#"<?php
namespace App;
class User {}
class Users implements \IteratorAggregate {
    /** @return \Generator<int, User> */
    public function getIterator(): \Generator {}
}
function caller(Users $users) {
    foreach ($users as $user) { return $user; }
    return null;
}
"#,
        ]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\user|null");
    }

    #[test]
    fn a_protocol_class_without_threading_falls_to_current_and_key() {
        let f = fixture(&[
            PROTOCOL_INTERFACES,
            r#"<?php
namespace App;
class Numbers implements \Iterator {
    public function current(): int {}
    public function key(): string {}
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool {}
}
function values(Numbers $numbers) {
    foreach ($numbers as $value) { return $value; }
    return 0;
}
function keys(Numbers $numbers) {
    foreach ($numbers as $key => $value) { return $key; }
    return '';
}
"#,
        ]);
        assert_eq!(caller_return_display(&f, "app\\values"), "int|0");
        assert_eq!(caller_return_display(&f, "app\\keys"), "string|''");
    }

    /// Same ordering deviation as
    /// `foreach_over_an_array_literal_types_key_and_value` above: the
    /// brief's illustrative `"1|2|0"` is insertion-ordered, but
    /// `TypeId::union`'s canonical sort places `Int` literals by value
    /// (`ordering.rs`), so the verified, deterministic answer is
    /// `"0|1|2"`.
    #[test]
    fn a_union_subject_joins_and_skips_its_null_constituent() {
        let f = fixture(&[r#"<?php
namespace App;
function caller(bool $flag) {
    $subject = $flag ? [1, 2] : null;
    foreach ($subject as $value) { return $value; }
    return 0;
}
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "0|1|2");
    }

    /// The other half of decision 12's skip rule: `false` contributes
    /// no alternative either (a falsy subject iterates nothing). Not
    /// exercised by `a_union_subject_joins_and_skips_its_null_constituent`
    /// above (which only ever puts `null` through the union arm) — this
    /// discriminates the two literals separately. Without the skip, the
    /// `false` constituent falls to `_ => mixed`, and `TypeId::union`
    /// collapses the whole join to `mixed` the moment it sees it
    /// (`construction.rs`'s cap-point rule), so the wrong answer is
    /// `"mixed"`, not `"0|1|2"`.
    #[test]
    fn a_union_subject_also_skips_its_false_constituent() {
        let f = fixture(&[r#"<?php
namespace App;
function caller(bool $flag) {
    $subject = $flag ? [1, 2] : false;
    foreach ($subject as $value) { return $value; }
    return 0;
}
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "0|1|2");
    }

    /// Decision 12's template arm, untested by the brief: a generic
    /// parameter's own `foreach` (no call site involved — the solver
    /// never runs, so `$items` stays a raw, unresolved `Template`
    /// inside `each`'s own body) recurses through its bound, a class
    /// answering through `current`/`key`. Without the `Template` arm,
    /// the match's `_ => mixed` fallback would answer `"mixed|0"`
    /// instead of `"int|0"`.
    #[test]
    fn a_template_recurses_through_its_bound_for_iteration() {
        let f = fixture_with_generics(&[
            PROTOCOL_INTERFACES,
            r#"<?php
namespace App;
class Numbers implements \Iterator {
    public function current(): int {}
    public function key(): string {}
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool {}
}
/**
 * @template T of Numbers
 * @param T $items
 */
function each($items) {
    foreach ($items as $value) { return $value; }
    return 0;
}
"#,
        ]);
        assert_eq!(caller_return_display(&f, "app\\each"), "int|0");
    }

    /// Precedence pinning, not just presence: a class threading
    /// `@implements Iterator<TKey, TValue>` arguments must answer
    /// *those*, not fall through to `current`/`key` — the two-branch
    /// rule's "when present" side, which
    /// `a_protocol_class_without_threading_falls_to_current_and_key`
    /// (only ever exercising the absent side) cannot pin. `current`
    /// and `key` are declared with a deliberately different type
    /// (`int`) from the threaded `TKey` (`string`): if the fallback
    /// wrongly ran first (or the threading were ignored), the answer
    /// would be `"0|int"`, not `"0|string"`. `Iterator` is declared
    /// here, unqualified, in the global namespace (no `namespace`
    /// statement in this fixture) so its folded key is exactly
    /// `"iterator"` — the bare name `class_iteration_types` checks
    /// `ancestor_arguments` against — mirroring the real built-in's own
    /// unqualified name; the fake's `class_annotations` needs an actual
    /// declaration with its own `@template` tags to compose a non-empty
    /// threaded argument list at all (`ancestor_arguments`'s
    /// `zip_templates` zips against the *target's own* declared
    /// templates).
    #[test]
    fn threaded_ancestor_arguments_precede_the_current_key_fallback() {
        let f = fixture_with_generics(&[r#"<?php
class User {}
/**
 * @template TKey
 * @template TValue
 */
interface Iterator {}
/** @implements Iterator<string, User> */
class Pairs implements Iterator {
    public function current(): int {}
    public function key(): int {}
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool {}
}
function keys(Pairs $pairs) {
    foreach ($pairs as $key => $value) { return $key; }
    return 0;
}
"#]);
        assert_eq!(caller_return_display(&f, "keys"), "0|string");
    }

    /// Precedence pinning between the `getIterator` unwrap and the
    /// threaded/`current`-`key` arms: a class implementing both
    /// `IteratorAggregate` and `Iterator` at once must answer through
    /// `getIterator`, never `current`/`key` — no single-interface
    /// fixture (like the two tests above) can discriminate the two
    /// arms' relative order, only a class carrying both at once can.
    /// `getIterator`'s declared return (`TypeA`) and `current`'s
    /// (`TypeB`) are different classes on purpose: the wrong order
    /// answers `"app\\typeb|null"`.
    #[test]
    fn an_iterator_aggregate_precedes_iterator_when_both_are_implemented() {
        let f = fixture_with_generics(&[
            PROTOCOL_INTERFACES,
            r#"<?php
namespace App;
class TypeA {}
class TypeB {}
class Both implements \IteratorAggregate, \Iterator {
    /** @return \Generator<int, TypeA> */
    public function getIterator(): \Generator {}
    public function current(): TypeB {}
    public function key(): int {}
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool {}
}
function caller(Both $both) {
    foreach ($both as $item) { return $item; }
    return null;
}
"#,
        ]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "app\\typea|null");
    }

    /// The depth guard (decision 12: capped at 8) is load-bearing, not
    /// decorative: a `getIterator` returning `$this` recurses on the
    /// exact same subject forever without it. The guard makes the walk
    /// terminate deterministically, answering `mixed` — the reachable
    /// end of body still returns `null`, but `TypeId::union` collapses
    /// any join touching `mixed` to `mixed` outright
    /// (`construction.rs`'s cap-point rule), so `"mixed"` is itself
    /// evidence the guard fired rather than the loop hanging.
    #[test]
    fn foreach_over_a_self_returning_get_iterator_terminates_via_the_depth_guard() {
        let f = fixture(&[
            PROTOCOL_INTERFACES,
            r#"<?php
namespace App;
class Loopy implements \IteratorAggregate {
    public function getIterator(): self {}
}
function caller(Loopy $loopy) {
    foreach ($loopy as $item) { return $item; }
    return null;
}
"#,
        ]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "mixed");
    }

    /// The guard's other shape (decision 12's own wording): two classes
    /// whose `getIterator`s return each other. Not a self-loop, so it
    /// would not terminate even if the guard only ever checked for the
    /// exact same `TypeId` recurring; the guard's plain depth count
    /// bounds this shape too.
    #[test]
    fn foreach_over_mutually_referential_get_iterators_terminates_via_the_depth_guard() {
        let f = fixture(&[
            PROTOCOL_INTERFACES,
            r#"<?php
namespace App;
class A implements \IteratorAggregate {
    public function getIterator(): B {}
}
class B implements \IteratorAggregate {
    public function getIterator(): A {}
}
function caller(A $a) {
    foreach ($a as $item) { return $item; }
    return null;
}
"#,
        ]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "mixed");
    }

    /// Review Finding 1: the `getIterator` unwrap must gate on the
    /// class genuinely implementing `IteratorAggregate` (or another
    /// protocol interface), never on the method's mere presence.
    /// `Duck` declares a `getIterator` but implements nothing at all —
    /// decision 12's own default (a plain object's property iteration)
    /// applies, so the correct answer is `mixed`. `Duck::getIterator`
    /// is deliberately typed `\Generator<int, Wrong>` (via docblock) —
    /// a docblock override the "protocol interface's own arguments"
    /// arm always honors regardless of who implements what — so the
    /// wrong (un-gated) answer is the concrete, guessed
    /// `"app\\wrong|null"`, not another `mixed`; the two answers
    /// differ, which is what lets this test discriminate a fixed gate
    /// from a missing one.
    #[test]
    fn a_class_implementing_nothing_does_not_unwrap_its_get_iterator() {
        let f = fixture_with_generics(&[r#"<?php
namespace App;
class Wrong {}
class Duck {
    /** @return \Generator<int, Wrong> */
    public function getIterator(): \Generator {}
}
function caller(Duck $duck) {
    foreach ($duck as $item) { return $item; }
    return null;
}
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "mixed");
    }

    /// Review Finding 1's other arm: `current`/`key` must gate the
    /// same way. `Cursor` declares both but implements nothing —
    /// `mixed` is correct; the un-gated answer would be the declared
    /// `current(): int` joined with the trailing `return 0`
    /// (`"int|0"`, the same string
    /// `a_protocol_class_without_threading_falls_to_current_and_key`'s
    /// `values()` answers for a *genuine* `\Iterator` implementor),
    /// so this test's pass/fail is a real discriminator, not a
    /// coincidental match.
    #[test]
    fn a_class_implementing_nothing_does_not_answer_through_current_and_key() {
        let f = fixture(&[r#"<?php
namespace App;
class Cursor {
    public function current(): int {}
    public function key(): string {}
}
function values(Cursor $cursor) {
    foreach ($cursor as $value) { return $value; }
    return 0;
}
"#]);
        assert_eq!(caller_return_display(&f, "app\\values"), "mixed");
    }

    /// Review Finding 2: decision 12's `mixed`/`mixed` default (the
    /// match's `_ => mixed` arm in `iteration_types`) is itself a step
    /// of the chain, not an incidental fallback, and the plan's own
    /// binding constraint demands a direct-hit test — verified none of
    /// the other fixtures exercise this arm (every other subject is an
    /// array, a shape, a union, a template, or a class). `foreach` over
    /// a plain `int` reaches no other arm at all: not an array/shape,
    /// not a union, not a template, not a `Class`.
    #[test]
    fn foreach_over_a_non_iterable_scalar_answers_mixed_through_the_default_arm() {
        let f = fixture(&[r#"<?php
namespace App;
function caller(int $number) {
    foreach ($number as $value) { return $value; }
    return 0;
}
"#]);
        assert_eq!(caller_return_display(&f, "app\\caller"), "mixed");
    }

    /// Decision 12's depth guard is stated as an exact value ("capped
    /// at 8"), not merely "some cap exists" — the review verified that
    /// mutating `depth > 8` to `depth > 100` leaves both existing
    /// guard tests green, since a self-loop and a mutual pair never
    /// terminate on their own regardless of the bound. Only a
    /// boundary pair like this one can tell 8 apart from any other
    /// value: a chain of exactly 8 `getIterator` hops (`Hop0` through
    /// `Hop7`) still reaches its terminal `\Iterator` implementor and
    /// answers concretely (`within`); one hop more (`Deep0` through
    /// `Deep8`, 9 hops) crosses the guard and answers `mixed`
    /// (`beyond`). Changing the bound in either direction breaks one
    /// side of this pair.
    #[test]
    fn the_depth_guard_pins_its_bound_at_exactly_eight() {
        let mut source = String::from(
            r#"<?php
interface Traversable {}
interface Iterator extends Traversable {}
interface IteratorAggregate extends Traversable {}
class TerminalValue {}
class TerminalKey {}
class Terminal implements Iterator {
    public function current(): TerminalValue {}
    public function key(): TerminalKey {}
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool {}
}
"#,
        );
        // Eight `getIterator` hops: `Hop0` -> `Hop1` -> ... -> `Hop7`
        // -> `Terminal`, reached at depth 8 — still within the guard
        // (`depth > 8` is false at 8).
        for index in 0..8 {
            let next = if index == 7 {
                "Terminal".to_owned()
            } else {
                format!("Hop{}", index + 1)
            };
            source.push_str(&format!(
                "class Hop{index} implements IteratorAggregate {{ public function getIterator(): {next} {{}} }}\n"
            ));
        }
        source.push_str(
            r#"function within(Hop0 $subject) {
    foreach ($subject as $key => $value) { return $value; }
    return null;
}
"#,
        );
        // Nine hops: `Deep0` -> ... -> `Deep8` -> `Terminal`, reached
        // at depth 9 — one past the guard.
        for index in 0..9 {
            let next = if index == 8 {
                "Terminal".to_owned()
            } else {
                format!("Deep{}", index + 1)
            };
            source.push_str(&format!(
                "class Deep{index} implements IteratorAggregate {{ public function getIterator(): {next} {{}} }}\n"
            ));
        }
        source.push_str(
            r#"function beyond(Deep0 $subject) {
    foreach ($subject as $key => $value) { return $value; }
    return null;
}
"#,
        );
        let f = fixture(&[&source]);
        assert_eq!(caller_return_display(&f, "within"), "terminalvalue|null");
        assert_eq!(caller_return_display(&f, "beyond"), "mixed");
    }

    /// Re-review finding: `implements_iteration_protocol` must also
    /// consult `stub_ancestors`, not only `ancestry`. Every fixture
    /// above that exercises a genuine protocol implementor names the
    /// protocol interface directly in the source's own `implements`/
    /// `extends` clause, so `linearized_class`'s `ancestry` already
    /// carries the resolved edge and none of them can catch a gate that
    /// forgot `stub_ancestors` entirely. A real `\ArrayObject`
    /// implementor (`\ArrayIterator`, `\SplStack`,
    /// `\SplObjectStorage`, ... are the same shape) is different:
    /// `Users extends \ArrayObject {}` records only
    /// `ancestry = [edge { stub: "arrayobject" }]` — `linearize.rs`'s
    /// `AncestorAnswer::Stub` arm pushes only the direct edge there.
    /// `ArrayObject`'s own compiled surface names `IteratorAggregate`
    /// as a parent, which the transitive stub-frontier walk
    /// (`linearize.rs:347-376`) folds into `stub_ancestors` alone,
    /// mirroring the walk `stub_ancestry_walks_transitively_through_the_blob`
    /// pins in `celerrate_semantics`. `fixture`'s own stub index is
    /// empty, so this crate needs its own isolated stub index (this
    /// test builds its own `Fixture` by hand rather than widening
    /// `fixture`'s shared, empty one, following
    /// `a_stub_method_without_a_declared_return_answers_mixed`'s idiom
    /// above) — no other test in this module is affected.
    ///
    /// The gap answers `mixed` (the gate reads `false`, so the
    /// `getIterator` unwrap never runs); the fix answers `element|null`
    /// (`getIterator`'s declared return, `Cursor`, is a genuine
    /// `\Iterator` implementor whose `current(): Element` supplies the
    /// element type). The two answers differ, which is what lets this
    /// test discriminate a fixed gate from a missing one; verified by
    /// reverting `implements_iteration_protocol` to check `ancestry`
    /// alone, which turns this assertion red.
    #[test]
    fn a_stub_ancestor_carries_a_genuine_protocol_implementor_transitively() {
        use celerrate_stubs::{
            StubAvailability, StubClassSurface, StubIndex, StubMember, StubMemberKind,
            StubSignature, StubSymbol, StubSymbolKind, StubVisibility, VersionedTypeText,
        };

        let db = TestDatabase::default();
        let source = r#"<?php
interface Traversable {}
interface Iterator extends Traversable {}
interface IteratorAggregate extends Traversable {}
class Element {}
class ElementKey {}
class Cursor implements Iterator {
    public function current(): Element {}
    public function key(): ElementKey {}
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool {}
}
class Users extends \ArrayObject {}
function caller(Users $users) {
    foreach ($users as $item) { return $item; }
    return null;
}
"#;
        let handles = vec![SourceFile::new(
            &db,
            FileId::new(0),
            source.as_bytes().to_vec(),
        )];
        let files = AnalyzedFileSet::new(&db, handles.clone());
        let get_iterator = StubMember {
            kind: StubMemberKind::Method,
            name: "getIterator".to_owned(),
            visibility: StubVisibility::Public,
            is_static: false,
            availability: StubAvailability::ALWAYS,
            signature: Some(StubSignature {
                parameters: vec![],
                return_type: VersionedTypeText::from_text(Some("Cursor".to_owned())),
                by_reference: false,
            }),
            type_text: VersionedTypeText::default(),
            value_text: None,
        };
        let stubs = StubIndexInput::builder(StubIndex::new(
            vec![StubSymbol {
                name: "ArrayObject".to_owned(),
                kind: StubSymbolKind::Class,
                availability: StubAvailability::ALWAYS,
            }],
            vec![],
            vec![(
                "ArrayObject".to_owned(),
                StubClassSurface {
                    // The transitive stub frontier, not a direct
                    // `ancestry` edge: only `ArrayObject`'s own
                    // compiled surface names `IteratorAggregate`.
                    parents: vec!["IteratorAggregate".to_owned()],
                    members: vec![get_iterator],
                },
            )],
        ))
        .durability(salsa::Durability::HIGH)
        .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        let f = Fixture {
            db,
            handles,
            files,
            stubs,
            configuration,
        };
        assert_eq!(caller_return_display(&f, "caller"), "element|null");
    }
}
