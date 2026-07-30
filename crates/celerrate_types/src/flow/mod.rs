//! The flow walk of one body: expression typing with an environment
//! of narrowing subjects threaded through statements. Branches join,
//! divergence (return, throw, break, goto) marks the path
//! unreachable, and loops run an inner join-ascent fixpoint with a
//! budget — the interprocedural discipline in miniature.
//! Absence is silence: a subject missing from the
//! environment reads as its wide type, `mixed` for locals.

use std::collections::BTreeMap;

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{
    AncestorRelation, ArrayEntry, BodyExpression, BodyIr, BodyStatement, ClassQuery,
    ClassReference, ExpressionId, MemberKind, MemberOrigin, MemberQuery, MemberReference,
    MemberResolution, StatementId, StringPart, UseTables, anonymous_class_key, folded_member_key,
    linearized_class, lookup_member, stub_ancestors_of, stub_signature_table,
};
use celerrate_stubs::StubIndexInput;
use celerrate_syntax::SyntaxKind;

use crate::declared::{
    DeclaredSignature, Trust, declared_function_signature, declared_member_signature,
};
use crate::inference::{InterproceduralEdgeCounts, StubCallRecord};
use crate::narrowing::{NarrowingSubject, subject_of};
use crate::operators;
use crate::records::TypedDependencies;
use crate::representation::{TypeData, TypeId};
use crate::widening::{join, widened_literals};

mod assignment;
mod boundary;
mod branching;
mod callables;
mod calls;
mod instantiation;
mod iteration;
mod walk;

pub(crate) use calls::resolved_function_key;

/// Join-ascent passes a loop may take before the still-moving
/// bindings widen to `mixed` — the deterministic bailout.
pub(crate) const LOOP_ITERATION_BUDGET: u32 = 4;

/// Everything one body's walk needs, resolved once by
/// `inferred_body_types`.
pub(crate) struct FlowContext<'db, 'body> {
    pub db: &'db dyn salsa::Database,
    pub files: AnalyzedFileSet,
    pub stubs: StubIndexInput,
    pub configuration: ProjectConfiguration,
    pub ir: &'body BodyIr,
    pub namespace: String,
    pub tables: UseTables,
    /// The defining class's folded key; `None` for free functions and
    /// anonymous-class methods.
    pub owner_class_key: Option<String>,
    pub method_is_static: bool,
    /// Parameter names with their seeded (declared) types.
    pub parameters: Vec<(String, TypeId<'db>)>,
    /// The declaring-scope key this body's own annotations bind under
    /// (`TypeId::template`'s scope convention, `declared.rs`'s own
    /// `<class key>::<member key>` for a method or the bare function
    /// key for a free function): the body's *own* declaration, never
    /// the using-class override applied to
    /// `owner_class_key` for a trait body — a class-level template's
    /// scope is a fact about which class or trait actually wrote the
    /// `@template`, not about which class later borrows the method
    /// through `use`.
    pub scope_key: String,
}

pub(crate) struct FlowResult<'db> {
    pub expression_types: Vec<TypeId<'db>>,
    pub return_type: TypeId<'db>,
    pub edge_counts: InterproceduralEdgeCounts,
    /// Every stub-function call this body made,
    /// with its mixed verdict — drained from `Walker::stub_calls`.
    pub stub_calls: Vec<StubCallRecord>,
    /// Every class, function, and inferred callee
    /// return this walk consulted — drained from `Walker::dependencies`,
    /// already sorted and deduped (the eq-cutoff contract).
    pub dependencies: TypedDependencies<'db>,
}

/// The abstract state at one program point. `reachable` is the
/// divergence flag: joins ignore an unreachable side, which is
/// exactly how an early return narrows the code after an `if`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Environment<'db> {
    bindings: BTreeMap<NarrowingSubject, TypeId<'db>>,
    reachable: bool,
}

impl<'db> Environment<'db> {
    pub(crate) fn new() -> Self {
        Self {
            bindings: BTreeMap::new(),
            reachable: true,
        }
    }

    pub(crate) fn bind(&mut self, subject: NarrowingSubject, of: TypeId<'db>) {
        self.bindings.insert(subject, of);
    }

    pub(crate) fn binding(&self, subject: &NarrowingSubject) -> Option<TypeId<'db>> {
        self.bindings.get(subject).copied()
    }

    pub(crate) fn remove(&mut self, subject: &NarrowingSubject) {
        self.bindings.remove(subject);
    }

    /// The call-result kill rule: local `name`'s value changed, so
    /// every fingerprint mentioning it (as base or argument) is
    /// stale. Deterministic: `retain` walks the `BTreeMap` in order.
    pub(crate) fn kill_call_results_involving(&mut self, name: &str) {
        self.bindings
            .retain(|subject, _| !subject.call_result_involves_local(name));
    }

    /// The subject-shaped face of the kill rule: a `Local` subject's
    /// value changed, so every fingerprint mentioning it (as base or
    /// argument) is stale. Non-`Local` subjects kill nothing — a
    /// property or call-result subject is not a fingerprint base or
    /// argument.
    pub(crate) fn kill_call_results_for_subject(&mut self, subject: &NarrowingSubject) {
        if let NarrowingSubject::Local { name } = subject {
            self.kill_call_results_involving(name);
        }
    }

    /// A keys snapshot for a sweep (the kill rule, `extract`, `eval`):
    /// deterministic because `bindings` is a `BTreeMap`.
    pub(crate) fn subjects(&self) -> Vec<NarrowingSubject> {
        self.bindings.keys().cloned().collect()
    }

    /// Forget everything (a `goto` label: any jump may land here).
    pub(crate) fn clear(&mut self) {
        self.bindings.clear();
        self.reachable = true;
    }

    pub(crate) fn mark_unreachable(&mut self) {
        self.reachable = false;
    }

    pub(crate) fn is_reachable(&self) -> bool {
        self.reachable
    }

    /// The control-flow join: an unreachable side contributes
    /// nothing; two reachable sides join pointwise, an absent side
    /// contributing `mixed` (absence is silence).
    pub(crate) fn join(db: &'db dyn salsa::Database, left: &Self, right: &Self) -> Self {
        if !left.reachable {
            return right.clone();
        }
        if !right.reachable {
            return left.clone();
        }
        Self::join_any(db, left, right)
    }

    /// The pointwise join regardless of reachability — the
    /// exception-edge combinator (a partially executed `try` body) and
    /// the loop accumulator. Reachable when either side is.
    ///
    /// Combines each subject's two candidate bindings with
    /// `TypeId::union` rather than `crate::widening::join`: a branch
    /// or a loop pass is a genuine execution-path alternative (`1|2`,
    /// not a widened `int<1, 2>`), and `TypeId::union` already falls
    /// back to the pairwise join once a subject's accumulated
    /// alternatives cross the arity cap, so termination is still
    /// guaranteed.
    pub(crate) fn join_any(db: &'db dyn salsa::Database, left: &Self, right: &Self) -> Self {
        let mut bindings = BTreeMap::new();
        let mixed = TypeId::mixed(db);
        for subject in left.bindings.keys().chain(right.bindings.keys()) {
            if bindings.contains_key(subject) {
                continue;
            }
            let a = left.binding(subject).unwrap_or(mixed);
            let b = right.binding(subject).unwrap_or(mixed);
            bindings.insert(subject.clone(), TypeId::union(db, [a, b]));
        }
        Self {
            bindings,
            reachable: left.reachable || right.reachable,
        }
    }

    /// The loop-budget bailout: every binding that still differs from
    /// `wider` widens to `mixed`, deterministically.
    pub(crate) fn widened_where_changed(&self, db: &'db dyn salsa::Database, wider: &Self) -> Self {
        let mut result = self.clone();
        for (subject, value) in &wider.bindings {
            if self.binding(subject) != Some(*value) {
                result.bindings.insert(subject.clone(), TypeId::mixed(db));
            }
        }
        result
    }
}

/// One conditional assertion collected while typing a call, applied
/// when that call is a condition (`IfTrue`/`IfFalse`). `origin` is the
/// call expression that produced it: `branch_environments` applies
/// only the facts whose origin is the condition's own top-level call,
/// so a fact from a call that was merely an argument never leaks.
struct PendingAssertion<'db> {
    origin: ExpressionId,
    subject: NarrowingSubject,
    asserted: TypeId<'db>,
    polarity: crate::type_syntax::AssertionPolarity,
    negated: bool,
}

pub(crate) struct Walker<'db, 'body, 'context> {
    context: &'context FlowContext<'db, 'body>,
    types: Vec<TypeId<'db>>,
    returns: Vec<TypeId<'db>>,
    saw_yield: bool,
    edge_counts: InterproceduralEdgeCounts,
    /// One record per free-function call this
    /// body made whose resolved key exists only in stubs, appended at
    /// the call boundary and drained into `InferredBody.stub_calls`.
    stub_calls: Vec<StubCallRecord>,
    /// Every class, function, and
    /// inferred callee return this walk consults, appended at the
    /// existing `edge_counts`/`lookup_member`/`linearized_class`
    /// consultation sites — constructive, never a second traversal.
    dependencies: TypedDependencies<'db>,
    /// Set while typing inside a `NullSafeChain` when a `?->` link's
    /// receiver was possibly null: the wrapper re-acquires `|null`
    /// once, at the end (the whole-chain rule).
    null_safe_reacquires: bool,
    /// The most recently typed call's conditional assertions, drained
    /// by `branch_environments`' default arm.
    pending_condition_facts: Vec<PendingAssertion<'db>>,
    /// Inline `@var` docblock texts, grouped by
    /// anchor once at entry: `None` for a comment trailing every
    /// statement of the body (bound once, at body entry), `Some(id)`
    /// for one anchored to statement `id` (bound immediately before
    /// the walker processes it and re-bound immediately after — the
    /// declaration survives the statement's own assignment). Each
    /// text parses on demand, at bind time.
    inline_variable_texts: BTreeMap<Option<StatementId>, Vec<&'body str>>,
}

/// Walks one body from its seeded parameter environment.
pub(crate) fn walk_body<'db>(context: &FlowContext<'db, '_>) -> FlowResult<'db> {
    let db = context.db;
    let mut inline_variable_texts: BTreeMap<Option<StatementId>, Vec<&str>> = BTreeMap::new();
    for annotation in &context.ir.annotations {
        inline_variable_texts
            .entry(annotation.anchor)
            .or_default()
            .push(annotation.text.as_str());
    }
    let mut walker = Walker {
        context,
        types: vec![TypeId::mixed(db); context.ir.expressions.len()],
        returns: Vec::new(),
        saw_yield: false,
        edge_counts: InterproceduralEdgeCounts::default(),
        stub_calls: Vec::new(),
        dependencies: TypedDependencies::default(),
        null_safe_reacquires: false,
        pending_condition_facts: Vec::new(),
        inline_variable_texts,
    };
    let mut environment = Environment::new();
    for (name, seeded) in &context.parameters {
        environment.bind(NarrowingSubject::Local { name: name.clone() }, *seeded);
    }
    // A comment trailing every statement of the body (no anchor) binds
    // once, before anything runs.
    walker.bind_inline_variables(None, &mut environment);
    walker.statements(&context.ir.root.clone(), &mut environment);
    let return_type = if walker.saw_yield {
        TypeId::class(db, "Generator", vec![])
    } else {
        // The explicit returns are alternative outcomes, not a
        // widened common supertype: `TypeId::union` preserves them
        // (`1|2`), matching the branch and loop join above.
        let explicit = walker
            .returns
            .iter()
            .copied()
            .reduce(|left, right| TypeId::union(db, [left, right]));
        match (explicit, environment.is_reachable()) {
            // A reachable end of body returns null implicitly.
            (Some(joined), true) => TypeId::union(db, [joined, TypeId::null(db)]),
            (None, true) => TypeId::null(db),
            (Some(joined), false) => joined,
            // No return statement ever reached: the body never
            // returns normally.
            (None, false) => TypeId::never(db),
        }
    };
    // The eq-cutoff contract (`TypedDependencies`'s own
    // rustdoc): deterministic order regardless of the walk's own
    // traversal, so two inference-identical bodies backdate.
    walker.dependencies.sort_and_dedup();
    FlowResult {
        expression_types: walker.types,
        return_type,
        edge_counts: walker.edge_counts,
        stub_calls: walker.stub_calls,
        dependencies: walker.dependencies,
    }
}

impl<'db> Walker<'db, '_, '_> {
    fn db(&self) -> &'db dyn salsa::Database {
        self.context.db
    }

    fn record(&mut self, id: ExpressionId, of: TypeId<'db>) -> TypeId<'db> {
        if let Some(slot) = self.types.get_mut(id.index() as usize) {
            *slot = of;
        }
        of
    }

    fn recorded(&self, id: ExpressionId) -> TypeId<'db> {
        self.types
            .get(id.index() as usize)
            .copied()
            .unwrap_or_else(|| TypeId::mixed(self.db()))
    }

    /// A written class name resolved at the body's declaring site.
    fn class_type_of_written(&self, written: &str) -> TypeId<'db> {
        let site = crate::declared::NameSite::Source {
            namespace: &self.context.namespace,
            tables: &self.context.tables,
        };
        TypeId::class(
            self.db(),
            &crate::declared::qualified_class_name(&site, written),
            vec![],
        )
    }

    /// The current type of a subject: its binding, or the wide type —
    /// `mixed` for a local, the declared type for a property subject
    /// (a dropped or never-narrowed property
    /// still reads as its declaration).
    fn subject_type(
        &mut self,
        environment: &Environment<'db>,
        subject: &NarrowingSubject,
    ) -> TypeId<'db> {
        if let Some(bound) = environment.binding(subject) {
            return bound;
        }
        let db = self.db();
        match subject {
            NarrowingSubject::Local { .. } => TypeId::mixed(db),
            NarrowingSubject::ThisProperty { name } | NarrowingSubject::StaticProperty { name } => {
                // `current_static_type`, not `this_type`: this fallback
                // also covers `self::$prop`/`static::$prop`
                // (`StaticProperty`), available in a static method where
                // `this_type`'s `$this`-value gate would wrongly answer
                // `mixed`.
                let owner_class_key = self.context.owner_class_key.clone();
                let current_static_type = self.current_static_type();
                owner_class_key
                    .and_then(|key| {
                        self.member_value_type(
                            std::slice::from_ref(&key),
                            MemberKind::Property,
                            name,
                            current_static_type,
                        )
                    })
                    .unwrap_or_else(|| TypeId::mixed(db))
            }
            NarrowingSubject::CallResult { .. } => TypeId::mixed(db),
        }
    }

    /// `$this`: the symbolic late-static-binding placeholder in a
    /// non-static method; `mixed` (silence) everywhere
    /// else — a free function, an anonymous-class method, or a static
    /// method (no `$this` value there). Substitution
    /// happens only at a member boundary the placeholder later
    /// crosses (`member_boundary_type`), never here.
    fn this_type(&self) -> TypeId<'db> {
        match (&self.context.owner_class_key, self.context.method_is_static) {
            (Some(_), false) => TypeId::static_placeholder(self.db()),
            _ => TypeId::mixed(self.db()),
        }
    }

    /// The current `static` type in the defining context:
    /// the forwarding placeholder when an owner class exists, `mixed`
    /// otherwise. Unlike [`Self::this_type`] this carries no
    /// static-method gate: the `self`/`static` *class keyword* is
    /// available in a static method, only the `$this` *value* is not.
    fn current_static_type(&self) -> TypeId<'db> {
        if self.context.owner_class_key.is_some() {
            TypeId::static_placeholder(self.db())
        } else {
            TypeId::mixed(self.db())
        }
    }

    /// The class-like keys a receiver type addresses; `None` when any
    /// part is opaque (mixed, object, a scalar, an unresolvable
    /// shape) — a recorded silent stance. Union constituents
    /// must all resolve (null skipped: the nullability family's
    /// business); intersection contributes every resolving part.
    fn receiver_parts(&mut self, of: TypeId<'db>) -> Option<Vec<String>> {
        let db = self.db();
        if of == TypeId::self_placeholder(db) || of == TypeId::static_placeholder(db) {
            return self.context.owner_class_key.clone().map(|key| vec![key]);
        }
        if of == TypeId::parent_placeholder(db) {
            return self
                .context
                .owner_class_key
                .as_ref()
                .and_then(|key| self.parent_class_key_of(key))
                .map(|key| vec![key]);
        }
        if let Some(name) = of.class_name(db) {
            return Some(vec![name]);
        }
        if let Some((enum_name, _)) = of.enum_case_parts(db) {
            return Some(vec![enum_name]);
        }
        let constituents = of.constituents(db);
        if constituents.len() > 1 {
            let mut keys = Vec::new();
            for part in constituents {
                if part.is_null(db) {
                    continue;
                }
                keys.extend(self.receiver_parts(part)?);
            }
            return (!keys.is_empty()).then_some(keys);
        }
        let intersectands = of.intersectands(db);
        if intersectands.len() > 1 {
            let mut keys = Vec::new();
            for part in intersectands {
                if let Some(mut resolved) = self.receiver_parts(part) {
                    keys.append(&mut resolved);
                }
            }
            return (!keys.is_empty()).then_some(keys);
        }
        if let Some(bound) = of.template_bound(db) {
            return self.receiver_parts(bound);
        }
        None
    }

    /// A `Foo::`/`self::`/`static::`/`parent::` subject: its type for
    /// substitution and the keys it addresses. An expression subject
    /// (an object, a `class-string`) resolves through its type.
    fn scoped_subject(
        &mut self,
        subject: ExpressionId,
        environment: &mut Environment<'db>,
    ) -> (TypeId<'db>, Option<Vec<String>>) {
        let db = self.db();
        match self.context.ir.expression(subject).cloned() {
            Some(BodyExpression::NamedReference { text }) => {
                let folded = text.to_ascii_lowercase();
                let keys = match folded.as_str() {
                    "self" | "static" => self.context.owner_class_key.clone().map(|key| vec![key]),
                    "parent" => self.parent_class_key().map(|key| vec![key]),
                    _ => {
                        let class = self.class_type_of_written(&text);
                        self.record(subject, TypeId::mixed(db));
                        let keys = class.class_name(db).map(|name| vec![name]);
                        let of = class;
                        return (of, keys);
                    }
                };
                self.record(subject, TypeId::mixed(db));
                // Forwarding: the receiver for
                // substitution at a `self::`/`static::`/`parent::` call
                // subject is the *current* `static` type — the
                // placeholder itself — so a `static` return stays
                // symbolic here and resolves only at the outer caller.
                // `keys` (the concrete owner or parent key, used for
                // member lookup) is unaffected — member resolution
                // inside the body stays exactly as it was.
                (self.current_static_type(), keys)
            }
            _ => {
                let of = self.expression(subject, environment);
                let through_class_string = of.class_string_argument(db).flatten();
                let effective = through_class_string.unwrap_or(of);
                (effective, self.receiver_parts(effective))
            }
        }
    }

    /// The protocol interfaces the iteration chain recognizes by
    /// their folded, unqualified name: `Generator`, `Iterator`,
    /// `IteratorAggregate`, `Traversable`. Every name in this crate's
    /// class-key convention is folded lowercase with no leading
    /// separator (`construction.rs`'s `class_names_fold_at_construction`),
    /// so a root-namespace `\Iterator` folds to exactly `"iterator"`.
    const ITERATION_PROTOCOL: [&'static str; 4] =
        ["generator", "iterator", "iteratoraggregate", "traversable"];
}
