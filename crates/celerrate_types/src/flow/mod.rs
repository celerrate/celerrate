//! The flow walk of one body: expression typing with an environment
//! of narrowing subjects threaded through statements. Branches join,
//! divergence (return, throw, break, goto) marks the path
//! unreachable, and loops run an inner join-ascent fixpoint with a
//! budget — the interprocedural discipline in miniature (design
//! section 6). Absence is silence: a subject missing from the
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

mod boundary;
mod calls;
mod instantiation;
mod iteration;

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
    /// anonymous-class methods (decision 12).
    pub owner_class_key: Option<String>,
    pub method_is_static: bool,
    /// Parameter names with their seeded (declared) types.
    pub parameters: Vec<(String, TypeId<'db>)>,
    /// The declaring-scope key this body's own annotations bind under
    /// (`TypeId::template`'s scope convention, `declared.rs`'s own
    /// `<class key>::<member key>` for a method or the bare function
    /// key for a free function): the body's *own* declaration, never
    /// the using-class override decision 5 applies to
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
    /// Task 10, decision 14: every stub-function call this body made,
    /// with its mixed verdict — drained from `Walker::stub_calls`.
    pub stub_calls: Vec<StubCallRecord>,
    /// Task 3 (plan 9a): every class, function, and inferred callee
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
    /// Task 10, decision 14: one record per free-function call this
    /// body made whose resolved key exists only in stubs, appended at
    /// the call boundary and drained into `InferredBody.stub_calls`.
    stub_calls: Vec<StubCallRecord>,
    /// Task 3 (plan 9a, decision 4): every class, function, and
    /// inferred callee return this walk consults, appended at the
    /// existing `edge_counts`/`lookup_member`/`linearized_class`
    /// consultation sites — constructive, never a second traversal.
    dependencies: TypedDependencies<'db>,
    /// Set while typing inside a `NullSafeChain` when a `?->` link's
    /// receiver was possibly null: the wrapper re-acquires `|null`
    /// once, at the end (the design's whole-chain rule).
    null_safe_reacquires: bool,
    /// The most recently typed call's conditional assertions, drained
    /// by `branch_environments`' default arm.
    pending_condition_facts: Vec<PendingAssertion<'db>>,
    /// Inline `@var` docblock texts (task 9, decision 11), grouped by
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
    // The eq-cutoff contract (decision 4, `TypedDependencies`'s own
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
    /// (decision 9's fallback; a dropped or never-narrowed property
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
    /// non-static method (decision 1); `mixed` (silence) everywhere
    /// else — a free function, an anonymous-class method (decision
    /// 12), or a static method (no `$this` value there). Substitution
    /// happens only at a member boundary the placeholder later
    /// crosses (`member_boundary_type`), never here.
    fn this_type(&self) -> TypeId<'db> {
        match (&self.context.owner_class_key, self.context.method_is_static) {
            (Some(_), false) => TypeId::static_placeholder(self.db()),
            _ => TypeId::mixed(self.db()),
        }
    }

    /// The current `static` type in the defining context (decision 2):
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
    /// shape) — the silent stance of decision 11. Union constituents
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
                // Decision 2 (forwarding): the receiver for
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

    fn statements(&mut self, list: &[StatementId], environment: &mut Environment<'db>) {
        for &statement in list {
            if !environment.is_reachable() {
                // Dead code still gets its expressions typed (the
                // table covers the whole arena), against a throwaway
                // empty environment — reachable locally so nested
                // joins behave, discarded so it cannot resurrect the
                // real path.
                let mut scratch = Environment::new();
                self.bind_inline_variables(Some(statement), &mut scratch);
                self.statement(statement, &mut scratch);
                self.bind_inline_variables(Some(statement), &mut scratch);
                continue;
            }
            // Decision 11: an inline `@var` anchored to this statement
            // binds immediately before it runs and re-binds immediately
            // after — the declaration survives the statement's own
            // assignment (the same bracketing idiom `looped` uses
            // around a pass).
            self.bind_inline_variables(Some(statement), environment);
            self.statement(statement, environment);
            self.bind_inline_variables(Some(statement), environment);
        }
    }

    fn statement(&mut self, id: StatementId, environment: &mut Environment<'db>) {
        let db = self.db();
        let Some(statement) = self.context.ir.statement(id).cloned() else {
            return;
        };
        match statement {
            BodyStatement::Missing | BodyStatement::Declaration { .. } => {}
            BodyStatement::Block { statements } => self.statements(&statements, environment),
            BodyStatement::Expression { expression } => {
                self.expression(expression, environment);
            }
            BodyStatement::Return { value } => {
                let returned = match value {
                    Some(value) => self.expression(value, environment),
                    None => TypeId::null(db),
                };
                self.returns.push(returned);
                environment.mark_unreachable();
            }
            BodyStatement::Echo { values } => {
                for value in values {
                    self.expression(value, environment);
                }
            }
            BodyStatement::Break { level } | BodyStatement::Continue { level } => {
                if let Some(level) = level {
                    self.expression(level, environment);
                }
                // Conservative: the path's bindings are dropped, and
                // dropped reads as mixed — silence (decision 9).
                environment.mark_unreachable();
            }
            BodyStatement::Global { targets } => {
                for target in targets {
                    self.expression(target, environment);
                    if let Some(subject) = subject_of(self.context.ir, target) {
                        if let NarrowingSubject::Local { name } = &subject {
                            environment.kill_call_results_involving(name);
                        }
                        environment.bind(subject, TypeId::mixed(db));
                    }
                }
            }
            BodyStatement::StaticVariables { variables } => {
                for variable in variables {
                    if let Some(initializer) = variable.initializer {
                        self.expression(initializer, environment);
                    }
                    // A static local persists across calls: mixed.
                    environment.kill_call_results_involving(&variable.name);
                    environment.bind(
                        NarrowingSubject::Local {
                            name: variable.name.clone(),
                        },
                        TypeId::mixed(db),
                    );
                }
            }
            BodyStatement::Unset { targets } => {
                for target in targets {
                    self.expression(target, environment);
                    if let Some(subject) = subject_of(self.context.ir, target) {
                        if let NarrowingSubject::Local { name } = &subject {
                            environment.kill_call_results_involving(name);
                        }
                        environment.remove(&subject);
                    }
                }
            }
            BodyStatement::Goto { .. } => environment.mark_unreachable(),
            BodyStatement::Label { .. } => environment.clear(),
            BodyStatement::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let (mut when_true, mut when_false) =
                    self.branch_environments(condition, environment);
                self.statements(&then_branch, &mut when_true);
                self.statements(&else_branch, &mut when_false);
                *environment = Environment::join(db, &when_true, &when_false);
            }
            BodyStatement::While { condition, body } => {
                self.looped(environment, |walker, env| {
                    let (mut when_true, when_false) = walker.branch_environments(condition, env);
                    walker.statements(&body, &mut when_true);
                    *env = when_true;
                    when_false
                });
            }
            BodyStatement::DoWhile { body, condition } => {
                self.looped(environment, |walker, env| {
                    walker.statements(&body, env);
                    walker.expression(condition, env);
                    env.clone()
                });
            }
            BodyStatement::For {
                initializers,
                conditions,
                updates,
                body,
            } => {
                for initializer in &initializers {
                    self.expression(*initializer, environment);
                }
                self.looped(environment, |walker, env| {
                    for condition in &conditions {
                        walker.expression(*condition, env);
                    }
                    let exit = env.clone();
                    walker.statements(&body, env);
                    for update in &updates {
                        walker.expression(*update, env);
                    }
                    exit
                });
            }
            BodyStatement::Foreach {
                subject,
                key,
                value,
                by_reference: _,
                body,
            } => {
                let subject_type = self.expression(subject, environment);
                let (key_type, value_type) = self.iteration_types(subject_type, 0);
                self.looped(environment, |walker, env| {
                    // Iteration typing follows the protocol chain
                    // (decision 12): `iteration_types` resolved the
                    // key and value once, above, from the subject's
                    // type before the loop. A by-reference value
                    // binds exactly like a plain value here — no
                    // write-back (decision 12's recorded stance).
                    if let Some(key) = key
                        && let Some(subject) = subject_of(walker.context.ir, key)
                    {
                        walker.expression(key, env);
                        env.bind(subject, key_type);
                    }
                    walker.expression(value, env);
                    if let Some(subject) = subject_of(walker.context.ir, value) {
                        env.bind(subject, value_type);
                    }
                    walker.statements(&body, env);
                    env.clone()
                });
            }
            BodyStatement::Switch { subject, cases } => {
                self.expression(subject, environment);
                let pre = environment.clone();
                let mut fall_in: Option<Environment<'db>> = None;
                let mut exits: Vec<Environment<'db>> = Vec::new();
                let mut has_default = false;
                for case in &cases {
                    let mut case_fact: Option<(NarrowingSubject, TypeId<'db>)> = None;
                    if let Some(condition) = case.condition {
                        // Conditions evaluate against a scratch clone:
                        // their side effects on later cases are
                        // conservatively dropped.
                        let mut scratch = pre.clone();
                        let condition_type = self.expression(condition, &mut scratch);
                        // A case narrows when its condition is an int
                        // or string literal and the subject already
                        // lies entirely within that scalar family —
                        // loose equality then coincides with strict
                        // (the design's "strict-safe cases").
                        let literal_family = if condition_type.int_literal_value(db).is_some() {
                            Some(TypeId::int(db))
                        } else if condition_type.string_literal_value(db).is_some() {
                            Some(TypeId::string(db))
                        } else {
                            None
                        };
                        if let (Some(family), Some(switch_subject)) =
                            (literal_family, subject_of(self.context.ir, subject))
                        {
                            let current = self.subject_type(&pre, &switch_subject);
                            let strict_safe = crate::judgments::subtype_of(
                                db,
                                self.context.files,
                                self.context.stubs,
                                self.context.configuration,
                                current,
                                family,
                            ) == crate::judgments::Proof::Holds;
                            if strict_safe {
                                case_fact = Some((
                                    switch_subject,
                                    self.narrowed_to(current, condition_type),
                                ));
                            }
                        }
                    } else {
                        has_default = true;
                    }
                    let mut narrowed_pre = pre.clone();
                    if let Some((switch_subject, narrowed)) = case_fact {
                        narrowed_pre.bind(switch_subject, narrowed);
                    }
                    let mut entry = match fall_in.take() {
                        Some(previous) => Environment::join_any(db, &previous, &narrowed_pre),
                        None => narrowed_pre,
                    };
                    self.statements(&case.statements, &mut entry);
                    if entry.is_reachable() {
                        fall_in = Some(entry.clone());
                    }
                    exits.push(entry);
                }
                let mut post = exits
                    .into_iter()
                    .reduce(|left, right| Environment::join(db, &left, &right))
                    .unwrap_or_else(|| pre.clone());
                if !has_default {
                    post = Environment::join(db, &post, &pre);
                }
                *environment = post;
            }
            BodyStatement::Try {
                body,
                catches,
                finally,
            } => {
                let entry = environment.clone();
                let mut after_try = environment.clone();
                self.statements(&body, &mut after_try);
                // A catch entry sees any prefix of the try body:
                // pointwise join regardless of reachability.
                let catch_entry = Environment::join_any(db, &entry, &after_try);
                let mut exits = vec![after_try];
                for catch in &catches {
                    let mut arm = catch_entry.clone();
                    let caught = TypeId::union(
                        db,
                        catch
                            .types
                            .iter()
                            .map(|written| self.class_type_of_written(written)),
                    );
                    if let Some(variable) = &catch.variable {
                        arm.bind(
                            NarrowingSubject::Local {
                                name: variable.clone(),
                            },
                            caught,
                        );
                    }
                    self.statements(&catch.statements, &mut arm);
                    exits.push(arm);
                }
                let mut post = exits
                    .into_iter()
                    .reduce(|left, right| Environment::join(db, &left, &right))
                    .unwrap_or(entry);
                if let Some(finally) = finally {
                    self.statements(&finally, &mut post);
                }
                *environment = post;
            }
            BodyStatement::Declare { statements } => self.statements(&statements, environment),
        }
    }

    /// The loop discipline (decision 9): join-ascent passes to a
    /// fixpoint under `LOOP_ITERATION_BUDGET`, deterministic widening
    /// to `mixed` on exhaustion, then one final recording pass from
    /// the fixed environment. The pass closure answers the loop's
    /// *exit* environment (a `while`'s condition-false state); the
    /// final pass's exit becomes the post-loop environment.
    fn looped<F>(&mut self, environment: &mut Environment<'db>, mut pass: F)
    where
        F: FnMut(&mut Self, &mut Environment<'db>) -> Environment<'db>,
    {
        let db = self.db();
        let mut current = environment.clone();
        let mut budget = LOOP_ITERATION_BUDGET;
        loop {
            let mut attempt = current.clone();
            let _ = pass(self, &mut attempt);
            let joined = Environment::join_any(db, &current, &attempt);
            if joined == current {
                break;
            }
            if budget == 0 {
                current = current.widened_where_changed(db, &joined);
                break;
            }
            budget -= 1;
            current = joined;
        }
        let mut fixed = current;
        let exit = pass(self, &mut fixed);
        *environment = exit;
    }

    fn expression(&mut self, id: ExpressionId, environment: &mut Environment<'db>) -> TypeId<'db> {
        let db = self.db();
        let condition_shaped = matches!(
            self.context.ir.expression(id),
            Some(BodyExpression::Unary {
                operator: SyntaxKind::Bang,
                ..
            }) | Some(BodyExpression::Binary {
                operator: SyntaxKind::AmpersandAmpersand
                    | SyntaxKind::PipePipe
                    | SyntaxKind::And
                    | SyntaxKind::Or
                    | SyntaxKind::InstanceOf
                    | SyntaxKind::EqualsEqualsEquals
                    | SyntaxKind::BangEqualsEquals,
                ..
            })
        );
        if condition_shaped {
            let (when_true, when_false) = self.branch_environments(id, environment);
            *environment = Environment::join_any(db, &when_true, &when_false);
            return self.recorded(id);
        }
        self.expression_value(id, environment)
    }

    /// The value-position typing of every expression shape that is
    /// not condition-shaped (routed through [`Self::expression`]
    /// above, which forwards condition forms to
    /// [`Self::branch_environments`] instead so their operands narrow
    /// and their two environments join).
    fn expression_value(
        &mut self,
        id: ExpressionId,
        environment: &mut Environment<'db>,
    ) -> TypeId<'db> {
        let db = self.db();
        let Some(expression) = self.context.ir.expression(id).cloned() else {
            return self.record(id, TypeId::mixed(db));
        };
        let of = match expression {
            BodyExpression::Missing => TypeId::mixed(db),
            BodyExpression::Literal { text } => operators::literal_type(db, &text),
            BodyExpression::Variable { name } => {
                if name == "this" {
                    self.this_type()
                } else {
                    self.subject_type(environment, &NarrowingSubject::Local { name })
                }
            }
            BodyExpression::NamedReference { text } => {
                operators::named_reference_type(db, &text).unwrap_or_else(|| TypeId::mixed(db))
            }
            BodyExpression::DynamicVariable { target } => {
                self.expression(target, environment);
                TypeId::mixed(db)
            }
            BodyExpression::Unary { operator, operand } => {
                let operand_type = self.expression(operand, environment);
                operators::unary_type(db, operator, operand_type)
            }
            BodyExpression::Postfix { operand, .. } => {
                let operand_type = self.expression(operand, environment);
                operators::postfix_type(db, operand_type)
            }
            BodyExpression::Binary {
                operator: SyntaxKind::QuestionQuestion,
                lhs,
                rhs,
            } => {
                let lhs_type = self.expression(lhs, environment);
                let subject = subject_of(self.context.ir, lhs);
                // The right operand evaluates only when the left was
                // null; the result path re-joins both sides.
                let mut when_null = environment.clone();
                let mut when_present = environment.clone();
                if let Some(subject) = &subject {
                    when_null.bind(subject.clone(), TypeId::null(db));
                    let current = self.subject_type(environment, subject);
                    when_present.bind(subject.clone(), current.without_null(db));
                }
                let rhs_type = self.expression(rhs, &mut when_null);
                *environment = Environment::join_any(db, &when_present, &when_null);
                // `TypeId::union`, not `widening::join`: the two sides
                // are alternative outcomes of a control-flow split
                // (`Foo|null` must survive, not collapse to `mixed` the
                // way `join(Foo, null)` does — the Task-3 convention).
                // A side that is *itself* a single-value narrowing
                // literal widens to its general type first, so a
                // same-family literal absorbs before the union runs
                // (`union` does no subsumption elimination, so without
                // this `?string ?? 'd'` would answer `string|'d'`); a
                // pre-existing union stays intact, so `(1|2|null) ?? 3`
                // keeps `1|2`.
                TypeId::union(
                    db,
                    [
                        widen_if_literal(db, lhs_type.without_null(db)),
                        widen_if_literal(db, rhs_type),
                    ],
                )
            }
            BodyExpression::Binary { operator, lhs, rhs } => {
                let lhs_type = self.expression(lhs, environment);
                let rhs_type = self.expression(rhs, environment);
                operators::binary_type(db, operator, lhs_type, rhs_type)
            }
            BodyExpression::Assignment {
                operator,
                by_reference,
                target,
                value,
            } => {
                self.expression(target, environment);
                let value_type = self.expression(value, environment);
                self.assignment(
                    operator,
                    by_reference,
                    target,
                    value,
                    value_type,
                    environment,
                )
            }
            BodyExpression::Cast { operator, operand } => {
                let operand_type = self.expression(operand, environment);
                operators::cast_type(db, operator, operand_type)
            }
            BodyExpression::Ternary {
                condition,
                middle,
                alternative,
            } => {
                let (mut when_true, mut when_false) =
                    self.branch_environments(condition, environment);
                let middle_type = match middle {
                    Some(middle) => self.expression(middle, &mut when_true),
                    // The short ternary answers the condition's value,
                    // tightened to what a truthy condition can be.
                    None => crate::narrowing::remove_falsy(db, self.recorded(condition)),
                };
                let alternative_type = self.expression(alternative, &mut when_false);
                *environment = Environment::join(db, &when_true, &when_false);
                // The two branches are alternative outcomes: preserve
                // them (see `join_any`'s doc comment).
                TypeId::union(db, [middle_type, alternative_type])
            }
            BodyExpression::Array { entries } => self.array_literal(&entries, environment),
            BodyExpression::InterpolatedString { parts } => {
                self.string_parts(&parts, environment);
                TypeId::string(db)
            }
            BodyExpression::ShellExec { parts } => {
                self.string_parts(&parts, environment);
                // Decision 10: a shell-exec runs arbitrary code.
                self.kill_property_bindings(environment);
                TypeId::union(
                    db,
                    [
                        TypeId::string(db),
                        TypeId::bool_literal(db, false),
                        TypeId::null(db),
                    ],
                )
            }
            BodyExpression::Isset { targets } => {
                for target in targets {
                    self.expression(target, environment);
                }
                TypeId::bool(db)
            }
            BodyExpression::Empty { target } => {
                self.expression(target, environment);
                TypeId::bool(db)
            }
            BodyExpression::Eval { argument } => {
                self.expression(argument, environment);
                // eval can rewrite every local and every property
                // binding: forget them all (decision 10).
                *environment = {
                    let mut cleared = Environment::new();
                    if !environment.is_reachable() {
                        cleared.mark_unreachable();
                    }
                    cleared
                };
                TypeId::mixed(db)
            }
            BodyExpression::Exit { argument } => {
                if let Some(argument) = argument {
                    self.expression(argument, environment);
                }
                environment.mark_unreachable();
                TypeId::never(db)
            }
            BodyExpression::Print { operand } => {
                self.expression(operand, environment);
                TypeId::int_literal(db, 1)
            }
            BodyExpression::Clone { operand } => self.expression(operand, environment),
            BodyExpression::Throw { operand } => {
                self.expression(operand, environment);
                environment.mark_unreachable();
                TypeId::never(db)
            }
            BodyExpression::Yield { key, value, .. } => {
                self.saw_yield = true;
                if let Some(key) = key {
                    self.expression(key, environment);
                }
                if let Some(value) = value {
                    self.expression(value, environment);
                }
                // A `yield` hands control back to the caller, which may
                // resume with arbitrary state changes (decision 10).
                self.kill_property_bindings(environment);
                TypeId::mixed(db)
            }
            BodyExpression::Include { operand, .. } => {
                self.expression(operand, environment);
                // An include runs arbitrary code (decision 10).
                self.kill_property_bindings(environment);
                TypeId::mixed(db)
            }
            BodyExpression::Match { subject, arms } => {
                // Walk the subject for its recording side effect (the
                // arms narrow off its subject binding, not its value).
                self.expression(subject, environment);
                // `match (true)` — the subject is the literal `true`.
                let is_match_true = matches!(
                    self.context.ir.expression(subject),
                    Some(BodyExpression::NamedReference { text })
                        if text.eq_ignore_ascii_case("true")
                );
                let match_subject = subject_of(self.context.ir, subject);
                let mut result: Option<TypeId<'db>> = None;
                let mut exits: Vec<Environment<'db>> = Vec::new();
                let mut seen_condition_types: Vec<TypeId<'db>> = Vec::new();
                for arm in &arms {
                    let mut arm_env = environment.clone();
                    if is_match_true {
                        // Each condition is itself a condition; the
                        // arm runs when any of them held.
                        let mut condition_envs: Vec<Environment<'db>> = Vec::new();
                        for condition in &arm.conditions {
                            let mut base = environment.clone();
                            let (when_true, _) = self.branch_environments(*condition, &mut base);
                            condition_envs.push(when_true);
                        }
                        if let Some(joined) = condition_envs
                            .into_iter()
                            .reduce(|left, right| Environment::join_any(db, &left, &right))
                        {
                            arm_env = joined;
                        }
                    } else {
                        let mut literals: Vec<TypeId<'db>> = Vec::new();
                        let mut all_literal = !arm.conditions.is_empty();
                        for condition in &arm.conditions {
                            let condition_type = self.expression(*condition, &mut arm_env);
                            if crate::narrowing::is_narrowing_literal(db, condition_type) {
                                literals.push(condition_type);
                            } else {
                                all_literal = false;
                            }
                        }
                        seen_condition_types.extend(literals.iter().copied());
                        if arm.is_default {
                            // The default arm subtracts every literal
                            // condition seen across the arms.
                            if let Some(match_subject) = &match_subject {
                                let mut current = self.subject_type(&arm_env, match_subject);
                                for literal in &seen_condition_types {
                                    current = self.removed_type(current, *literal);
                                }
                                arm_env.bind(match_subject.clone(), current);
                            }
                        } else if all_literal && let Some(match_subject) = &match_subject {
                            let current = self.subject_type(&arm_env, match_subject);
                            let target = TypeId::union(db, literals.iter().copied());
                            arm_env.bind(match_subject.clone(), self.narrowed_to(current, target));
                        }
                    }
                    let body_type = self.expression(arm.body, &mut arm_env);
                    result = Some(match result {
                        // Alternative arm outcomes: preserve them
                        // (`TypeId::union`, not `widening::join`, so
                        // `1|2|'other'` does not collapse to `mixed`).
                        Some(previous) => TypeId::union(db, [previous, body_type]),
                        None => body_type,
                    });
                    exits.push(arm_env);
                }
                // An unmatched subject throws: only arm exits join.
                if let Some(post) = exits
                    .into_iter()
                    .reduce(|left, right| Environment::join(db, &left, &right))
                {
                    *environment = post;
                }
                result.unwrap_or_else(|| TypeId::never(db))
            }
            BodyExpression::MemberAccess {
                receiver,
                member,
                null_safe,
            } => {
                let receiver_type = self.expression(receiver, environment);
                let resolving = if null_safe {
                    if crate::judgments::nullability(db, receiver_type)
                        != crate::judgments::Nullability::NeverNull
                    {
                        self.null_safe_reacquires = true;
                    }
                    receiver_type.without_null(db)
                } else {
                    receiver_type
                };
                match member {
                    MemberReference::Named { name } => {
                        // A narrowed (or assigned) stable-base property:
                        // the environment wins over the declaration.
                        // The receiver is still typed above (the table
                        // covers it).
                        if let Some(subject) = subject_of(self.context.ir, id)
                            && let Some(bound) = environment.binding(&subject)
                        {
                            bound
                        } else {
                            self.receiver_parts(resolving)
                                .and_then(|keys| {
                                    self.member_value_type(
                                        &keys,
                                        MemberKind::Property,
                                        &name,
                                        resolving,
                                    )
                                })
                                .unwrap_or_else(|| TypeId::mixed(db))
                        }
                    }
                    MemberReference::Computed { expression } => {
                        self.expression(expression, environment);
                        TypeId::mixed(db)
                    }
                    MemberReference::Variable { .. } | MemberReference::Missing => {
                        TypeId::mixed(db)
                    }
                }
            }
            BodyExpression::ScopedAccess { subject, member } => {
                let (subject_type, keys) = self.scoped_subject(subject, environment);
                match member {
                    MemberReference::Named { name } => {
                        if name.eq_ignore_ascii_case("class") {
                            // `Foo::class`, `self::class`, `static::class`.
                            // A union receiver's keys are exclusive
                            // control-flow alternatives, so every key
                            // contributes through `TypeId::union` —
                            // never a first-seen constituent (Finding
                            // 1, mirroring `member_value_type`'s
                            // per-key reduce).
                            let argument = keys.as_ref().and_then(|keys| {
                                keys.iter()
                                    .map(|key| TypeId::class(db, key, vec![]))
                                    .reduce(|left, right| TypeId::union(db, [left, right]))
                            });
                            TypeId::class_string(db, argument)
                        } else {
                            keys.as_ref()
                                .and_then(|keys| {
                                    self.member_value_type(
                                        keys,
                                        MemberKind::ClassConstant,
                                        &name,
                                        subject_type,
                                    )
                                    .or_else(|| {
                                        self.member_value_type(
                                            keys,
                                            MemberKind::EnumCase,
                                            &name,
                                            subject_type,
                                        )
                                    })
                                })
                                .unwrap_or_else(|| TypeId::mixed(db))
                        }
                    }
                    // `Foo::$prop`: a static property is a Property.
                    // Same environment-first rule as `MemberAccess`.
                    MemberReference::Variable { name } => {
                        if let Some(subject) = subject_of(self.context.ir, id)
                            && let Some(bound) = environment.binding(&subject)
                        {
                            bound
                        } else {
                            keys.as_ref()
                                .and_then(|keys| {
                                    self.member_value_type(
                                        keys,
                                        MemberKind::Property,
                                        &name,
                                        subject_type,
                                    )
                                })
                                .unwrap_or_else(|| TypeId::mixed(db))
                        }
                    }
                    MemberReference::Computed { expression } => {
                        self.expression(expression, environment);
                        TypeId::mixed(db)
                    }
                    MemberReference::Missing => TypeId::mixed(db),
                }
            }
            BodyExpression::NullSafeChain { chain } => {
                let saved = std::mem::replace(&mut self.null_safe_reacquires, false);
                let chain_type = self.expression(chain, environment);
                let reacquires = std::mem::replace(&mut self.null_safe_reacquires, saved);
                if reacquires {
                    // `widening::join` would collapse `int` and `null`
                    // to `mixed` (disjoint kinds hit its `_ => mixed`
                    // fallback); the re-acquired null is a precise
                    // alternative outcome, not a widened common
                    // supertype, so `TypeId::union` preserves it as
                    // `int|null` instead (display renders null last).
                    TypeId::union(db, [chain_type, TypeId::null(db)])
                } else {
                    chain_type
                }
            }
            BodyExpression::Call { callee, arguments } => {
                // The `assert()` special form: its argument is a
                // condition, and a truthy assertion narrows the rest
                // of the body exactly like an early-return `if` would
                // (Tasks 6 and 9 keep this check first when they
                // rewrite this arm for call resolution).
                if let Some(BodyExpression::NamedReference { text }) =
                    self.context.ir.expression(callee).cloned()
                {
                    let name = text.strip_prefix('\\').unwrap_or(text.as_str());
                    if name.eq_ignore_ascii_case("assert")
                        && let Some(first) = arguments.first().filter(|argument| !argument.spread)
                    {
                        let value = first.value;
                        self.record(callee, TypeId::mixed(db));
                        let (when_true, _) = self.branch_environments(value, environment);
                        *environment = when_true;
                        for argument in arguments.iter().skip(1) {
                            self.expression(argument.value, environment);
                        }
                        return self.record(id, TypeId::bool(db));
                    }
                }
                match self.context.ir.expression(callee).cloned() {
                    Some(BodyExpression::MemberAccess {
                        receiver,
                        member: MemberReference::Named { name },
                        null_safe,
                    }) => {
                        let receiver_is_this = matches!(
                            self.context.ir.expression(receiver),
                            Some(BodyExpression::Variable { name }) if name == "this"
                        );
                        let receiver_type = self.expression(receiver, environment);
                        let resolving = if null_safe {
                            if crate::judgments::nullability(db, receiver_type)
                                != crate::judgments::Nullability::NeverNull
                            {
                                self.null_safe_reacquires = true;
                            }
                            receiver_type.without_null(db)
                        } else {
                            receiver_type
                        };
                        self.record(callee, TypeId::mixed(db));
                        let argument_types = self.typed_arguments(&arguments, environment);
                        let (of, signature) = self.method_call_result_with_provider(
                            resolving,
                            &name,
                            &argument_types,
                        );
                        // Order is load-bearing: the receiver and
                        // arguments were typed under the pre-call
                        // environment above; the kill runs after, and
                        // the write-back — a known postcondition of
                        // this same call — is applied after the kill
                        // so it survives (decision 10, design section 6).
                        self.kill_property_bindings(environment);
                        if let Some(signature) = &signature {
                            self.apply_by_reference(&signature.parameters, &arguments, environment);
                        }
                        // The assertion tags apply after the kill too
                        // (they are knowledge about the post-call
                        // state): only when exactly one receiver key
                        // resolved (Task 8's single-signature channel)
                        // does `signature` carry the unambiguous
                        // declared parameters this needs.
                        if let Some(signature) = &signature
                            && let Some(keys) = self.receiver_parts(resolving)
                            && let [key] = keys.as_slice()
                        {
                            let annotations = crate::declared::member_annotations(
                                db,
                                self.context.files,
                                self.context.stubs,
                                self.context.configuration,
                                MemberQuery::new(
                                    db,
                                    key.clone(),
                                    MemberKind::Method,
                                    folded_member_key(MemberKind::Method, &name),
                                ),
                            );
                            self.apply_call_assertions(
                                id,
                                &annotations.assertions,
                                &signature.parameters,
                                receiver_is_this,
                                &arguments,
                                environment,
                            );
                        }
                        // Task 8: the call-site solver, applied to
                        // whichever tier answered `of` (the provider
                        // tier is exempt without special-casing — its
                        // answer is already concrete, so
                        // `contains_symbolic` is already false for it).
                        let computed = match &signature {
                            Some(signature) => self.solved_call_result(
                                of,
                                &signature.parameters,
                                &arguments,
                                &argument_types,
                            ),
                            None => of,
                        };
                        // A narrowed call-result fingerprint: the
                        // environment wins over the fresh return type
                        // (issue #54; the property-fetch arm's idiom).
                        // The binding survived this call's own
                        // `kill_property_bindings` above by the
                        // survival rule.
                        if let Some(subject) = subject_of(self.context.ir, id)
                            && let Some(bound) = environment.binding(&subject)
                        {
                            bound
                        } else {
                            computed
                        }
                    }
                    Some(BodyExpression::ScopedAccess {
                        subject,
                        member: MemberReference::Named { name },
                    }) => {
                        let receiver_is_this = matches!(
                            self.context.ir.expression(subject),
                            Some(BodyExpression::NamedReference { text })
                                if matches!(text.to_ascii_lowercase().as_str(), "self" | "static")
                        );
                        let (subject_type, keys) = self.scoped_subject(subject, environment);
                        self.record(callee, TypeId::mixed(db));
                        let argument_types = self.typed_arguments(&arguments, environment);
                        let (of, signature) = match &keys {
                            Some(keys) => {
                                let receiver = subject_type;
                                self.method_call_result_for_keys_with_provider(
                                    keys,
                                    receiver,
                                    &name,
                                    &argument_types,
                                )
                            }
                            None => (TypeId::mixed(db), None),
                        };
                        self.kill_property_bindings(environment);
                        if let Some(signature) = &signature {
                            self.apply_by_reference(&signature.parameters, &arguments, environment);
                        }
                        // Assertions apply after the kill (post-call
                        // knowledge); the single-signature channel
                        // gates them on an unambiguous receiver key.
                        if let Some(signature) = &signature
                            && let Some(keys) = &keys
                            && let [key] = keys.as_slice()
                        {
                            let annotations = crate::declared::member_annotations(
                                db,
                                self.context.files,
                                self.context.stubs,
                                self.context.configuration,
                                MemberQuery::new(
                                    db,
                                    key.clone(),
                                    MemberKind::Method,
                                    folded_member_key(MemberKind::Method, &name),
                                ),
                            );
                            self.apply_call_assertions(
                                id,
                                &annotations.assertions,
                                &signature.parameters,
                                receiver_is_this,
                                &arguments,
                                environment,
                            );
                        }
                        // Task 8: the call-site solver (see the sibling
                        // `MemberAccess` arm above for the provider-tier
                        // exemption's reasoning).
                        match &signature {
                            Some(signature) => self.solved_call_result(
                                of,
                                &signature.parameters,
                                &arguments,
                                &argument_types,
                            ),
                            None => of,
                        }
                    }
                    Some(BodyExpression::NamedReference { text }) => {
                        self.record(callee, TypeId::mixed(db));
                        let argument_types = self.typed_arguments(&arguments, environment);
                        let (key, source_exists) = self.resolved_function_key(&text);
                        // `extract()` rewrites every local from its
                        // array argument's keys: an aggressive sweep on
                        // top of the general kill below.
                        let name = text.strip_prefix('\\').unwrap_or(text.as_str());
                        if name.eq_ignore_ascii_case("extract") {
                            for subject in environment.subjects() {
                                if matches!(subject, NarrowingSubject::Local { .. }) {
                                    environment.remove(&subject);
                                }
                            }
                        }
                        let claim = crate::dynamic_type_provider::SymbolClaim::Function {
                            key: key.clone(),
                        };
                        let of = self
                            .provider_return(claim.clone(), None, &argument_types)
                            .unwrap_or_else(|| self.function_call_result(&key, source_exists));
                        // Task 10, decision 14: the instrument records
                        // at the source. The walker already knows both
                        // facts the recording condition needs —
                        // `source_exists` from `resolved_function_key`
                        // just above, and the call's own answer `of` —
                        // so nothing outside the walker re-implements
                        // callee resolution to reconstruct them.
                        //
                        // Task-12 debt (owner: the mixed-rate
                        // instrument, decision 14's stated scope): this
                        // recording arm exists only on the free-function
                        // call path. A stub METHOD call (the task-5
                        // class-refinement channel) still moves the
                        // global expressions-mixed counter through the
                        // ordinary `record` below, but never reaches
                        // this arm, so it never enters `stub_calls` and
                        // decision 15's per-callee exit table cannot see
                        // it.
                        if !source_exists
                            && celerrate_semantics::stub_symbol_table(
                                db,
                                self.context.stubs,
                                self.context.configuration,
                            )
                            .lookup(celerrate_semantics::SymbolSpace::Function, &key)
                            .is_some()
                        {
                            self.stub_calls.push(StubCallRecord {
                                callee: key.clone(),
                                mixed: of.is_mixed(db),
                            });
                        }
                        let declared = declared_function_signature(
                            db,
                            self.context.files,
                            self.context.stubs,
                            self.context.configuration,
                            crate::declared::FunctionQuery::new(db, key.clone()),
                        );
                        self.kill_property_bindings(environment);
                        if let Some(signature) = &declared {
                            self.apply_by_reference(&signature.parameters, &arguments, environment);
                        }
                        let contributions =
                            self.provider_by_reference(claim, None, &argument_types);
                        self.apply_provider_by_reference(&contributions, &arguments, environment);
                        // A named function has no receiver: the
                        // declared parameter list comes straight from
                        // the signature (empty when unresolved).
                        let parameters = declared
                            .as_ref()
                            .map(|signature| signature.parameters.as_slice())
                            .unwrap_or(&[]);
                        let annotations = crate::declared::function_annotations(
                            db,
                            self.context.files,
                            self.context.stubs,
                            self.context.configuration,
                            crate::declared::FunctionQuery::new(db, key),
                        );
                        self.apply_call_assertions(
                            id,
                            &annotations.assertions,
                            parameters,
                            false,
                            &arguments,
                            environment,
                        );
                        // Task 8: the call-site solver (see the
                        // `MemberAccess` arm above for the
                        // provider-tier exemption's reasoning); a named
                        // function has no receiver, so `parameters`
                        // (already the declared list, empty when
                        // unresolved) is exactly `solver_pairs`' input.
                        self.solved_call_result(of, parameters, &arguments, &argument_types)
                    }
                    _ => {
                        // A callable value: a variable, an array
                        // `[obj, 'method']` shape, an invocation result
                        // — anything not statically a named or member
                        // callee. `callable_return` invokes through the
                        // callable-typed value (Decision 3's final,
                        // dynamic-shape tier); an opaque or non-callable
                        // value stays silent.
                        let callee_type = self.expression(callee, environment);
                        self.typed_arguments(&arguments, environment);
                        self.kill_property_bindings(environment);
                        callee_type
                            .callable_return(db)
                            .unwrap_or_else(|| TypeId::mixed(db))
                    }
                }
            }
            BodyExpression::CallableReference { callee } => {
                match self.context.ir.expression(callee).cloned() {
                    Some(BodyExpression::NamedReference { text }) => {
                        self.record(callee, TypeId::mixed(db));
                        let (key, source_exists) = self.resolved_function_key(&text);
                        self.projected_callable_of_function(&key, source_exists)
                    }
                    Some(BodyExpression::MemberAccess {
                        receiver,
                        member: MemberReference::Named { name },
                        ..
                    }) => {
                        let receiver_type = self.expression(receiver, environment);
                        self.record(callee, TypeId::mixed(db));
                        self.projected_callable_of_method(receiver_type, &name)
                    }
                    Some(BodyExpression::ScopedAccess {
                        subject,
                        member: MemberReference::Named { name },
                    }) => {
                        let (subject_type, keys) = self.scoped_subject(subject, environment);
                        self.record(callee, TypeId::mixed(db));
                        match keys {
                            Some(keys) => {
                                self.projected_callable_of_keys(&keys, subject_type, &name)
                            }
                            None => TypeId::mixed(db),
                        }
                    }
                    _ => {
                        self.expression(callee, environment);
                        TypeId::mixed(db)
                    }
                }
            }
            BodyExpression::New { class, arguments } => {
                let of = match &class {
                    // `new self()`/`new static()`/`new parent()` name the
                    // same scope keywords `self::`/`static::`/`parent::`
                    // resolve (`scope_keyword_class`), not
                    // `class_type_of_written` (which would qualify them
                    // into bogus class names `self`/`parent`). Their
                    // placeholders resolve immediately below, right here
                    // in the defining context (decision 1): `self`/`parent`
                    // answer the owner/its parent concretely, `static`
                    // stays the forwarding placeholder — `new static()`'s
                    // identity is only known at the outer call boundary.
                    ClassReference::Named { name } => self
                        .scope_keyword_class(name)
                        .unwrap_or_else(|| self.class_type_of_written(name)),
                    ClassReference::StaticKeyword => self.current_static_type(),
                    ClassReference::Dynamic { expression } => {
                        let dynamic = self.expression(*expression, environment);
                        dynamic
                            .class_string_argument(db)
                            .flatten()
                            .or_else(|| {
                                dynamic.class_name(db).map(|name| {
                                    TypeId::class(db, &name, dynamic.class_arguments(db))
                                })
                            })
                            .unwrap_or_else(|| TypeId::mixed(db))
                    }
                    // An anonymous-class receiver (`new class { }`)
                    // types as its synthetic folded key, so it
                    // linearizes, resolves `$this`, and types like any
                    // named class. `Missing` (a malformed `new` with no
                    // class reference at all) keeps the silent `mixed`
                    // fallback: there is no declaration to key by.
                    ClassReference::Anonymous { declaration } => {
                        TypeId::class(db, &anonymous_class_key(*declaration), vec![])
                    }
                    ClassReference::Missing => TypeId::mixed(db),
                };
                let of = self.member_boundary_type(
                    of,
                    self.context.owner_class_key.as_deref(),
                    self.current_static_type(),
                );
                let argument_types = self.typed_arguments(&arguments, environment);
                // Decision 11 (task 9): `Foo`'s own class-level
                // templates, when it declares any, solve from the
                // `__construct` arguments — the same call-site solver
                // task 8 built for an ordinary call.
                let of = self.constructor_solved_class(of, &arguments, &argument_types);
                // Instantiation may run arbitrary constructor code
                // (decision 10).
                self.kill_property_bindings(environment);
                of
            }
            BodyExpression::Index { subject, index } => {
                let subject_type = self.expression(subject, environment);
                let index_type = index.map(|index| self.expression(index, environment));
                operators::index_type(db, subject_type, index_type)
            }
            BodyExpression::Closure {
                parameters,
                uses,
                return_type_text,
                is_static: _,
                by_reference: _,
                body,
            } => {
                let mut inner = Environment::new();
                for capture in &uses {
                    let subject = NarrowingSubject::Local {
                        name: capture.name.clone(),
                    };
                    if capture.by_reference {
                        // `use (&$x)`: the local is aliased into the
                        // closure's scope for as long as the closure
                        // lives, unknowable without alias analysis —
                        // degrade both sides now (decision 10).
                        inner.bind(subject.clone(), TypeId::mixed(db));
                        environment.bind(subject, TypeId::mixed(db));
                    } else {
                        let captured = self.subject_type(environment, &subject);
                        inner.bind(subject, captured);
                    }
                }
                self.seed_written_parameters(&parameters, &mut inner);
                let (returns, saw_yield, end_reachable) =
                    self.nested_returns(|walker, env| walker.statements(&body, env), &mut inner);
                // Closure creation may run arbitrary code (decision 10).
                self.kill_property_bindings(environment);
                self.closure_type(
                    &parameters,
                    return_type_text.as_deref(),
                    returns,
                    saw_yield,
                    end_reachable,
                )
            }
            BodyExpression::ArrowFunction {
                parameters,
                return_type_text,
                is_static: _,
                by_reference: _,
                body,
            } => {
                // Arrow functions capture the whole scope by value.
                let mut inner = environment.clone();
                self.seed_written_parameters(&parameters, &mut inner);
                let mut returned: Vec<TypeId<'db>> = Vec::new();
                let (_, saw_yield, _) = self.nested_returns(
                    |walker, env| {
                        let of = walker.expression(body, env);
                        returned.push(of);
                    },
                    &mut inner,
                );
                // Decision 10: closure creation kills property bindings.
                self.kill_property_bindings(environment);
                self.closure_type(
                    &parameters,
                    return_type_text.as_deref(),
                    returned,
                    saw_yield,
                    false,
                )
            }
        };
        self.record(id, of)
    }

    /// Types `condition` (side effects included) and answers the two
    /// environments its truth and falsity establish. Composition is
    /// structural: `!` swaps, `&&` chains the true side and joins the
    /// false sides, `||` is the dual (design section 6: negation and
    /// boolean composition are in the floor — early returns are
    /// vacuous without them).
    fn branch_environments(
        &mut self,
        condition: ExpressionId,
        environment: &mut Environment<'db>,
    ) -> (Environment<'db>, Environment<'db>) {
        let db = self.db();
        let ir = self.context.ir;
        match ir.expression(condition).cloned() {
            Some(BodyExpression::Unary {
                operator: SyntaxKind::Bang,
                operand,
            }) => {
                let (when_true, when_false) = self.branch_environments(operand, environment);
                self.record(condition, TypeId::bool(db));
                (when_false, when_true)
            }
            Some(BodyExpression::Binary {
                operator: SyntaxKind::AmpersandAmpersand | SyntaxKind::And,
                lhs,
                rhs,
            }) => {
                let (mut lhs_true, lhs_false) = self.branch_environments(lhs, environment);
                let (rhs_true, rhs_false) = self.branch_environments(rhs, &mut lhs_true);
                self.record(condition, TypeId::bool(db));
                (rhs_true, Environment::join_any(db, &lhs_false, &rhs_false))
            }
            Some(BodyExpression::Binary {
                operator: SyntaxKind::PipePipe | SyntaxKind::Or,
                lhs,
                rhs,
            }) => {
                let (lhs_true, mut lhs_false) = self.branch_environments(lhs, environment);
                let (rhs_true, rhs_false) = self.branch_environments(rhs, &mut lhs_false);
                self.record(condition, TypeId::bool(db));
                (Environment::join_any(db, &lhs_true, &rhs_true), rhs_false)
            }
            Some(BodyExpression::Binary {
                operator: SyntaxKind::InstanceOf,
                lhs,
                rhs,
            }) => {
                let subject_type = self.expression(lhs, environment);
                let target = self.instanceof_target(rhs, environment);
                self.record(condition, TypeId::bool(db));
                let subject = subject_of(ir, lhs);
                self.split_on_subject(
                    environment,
                    subject,
                    subject_type,
                    |walker, current| walker.narrowed_to(current, target),
                    |walker, current| walker.removed_type(current, target),
                )
            }
            Some(BodyExpression::Binary {
                operator: operator @ (SyntaxKind::EqualsEqualsEquals | SyntaxKind::BangEqualsEquals),
                lhs,
                rhs,
            }) => {
                let lhs_type = self.expression(lhs, environment);
                let rhs_type = self.expression(rhs, environment);
                self.record(condition, TypeId::bool(db));
                let sides = if crate::narrowing::is_narrowing_literal(db, rhs_type) {
                    Some((lhs, lhs_type, rhs_type))
                } else if crate::narrowing::is_narrowing_literal(db, lhs_type) {
                    Some((rhs, rhs_type, lhs_type))
                } else {
                    None
                };
                let Some((subject_expression, subject_type, literal)) = sides else {
                    return (environment.clone(), environment.clone());
                };
                let subject = subject_of(ir, subject_expression);
                let (equal, unequal) = (
                    self.narrowed_to(subject_type, literal),
                    self.removed_type(subject_type, literal),
                );
                let mut when_true = environment.clone();
                let mut when_false = environment.clone();
                if let Some(subject) = subject {
                    if operator == SyntaxKind::EqualsEqualsEquals {
                        when_true.bind(subject.clone(), equal);
                        when_false.bind(subject, unequal);
                    } else {
                        when_true.bind(subject.clone(), unequal);
                        when_false.bind(subject, equal);
                    }
                }
                (when_true, when_false)
            }
            Some(BodyExpression::Isset { targets }) => {
                let mut when_true = environment.clone();
                for &target in &targets {
                    self.expression(target, environment);
                    if let Some(subject) = subject_of(ir, target) {
                        let current = self.subject_type(environment, &subject);
                        when_true.bind(subject.clone(), current.without_null(db));
                    }
                }
                self.record(condition, TypeId::bool(db));
                // isset false can mean unset, not only null: no facts.
                (when_true, environment.clone())
            }
            Some(BodyExpression::Empty { target }) => {
                self.expression(target, environment);
                self.record(condition, TypeId::bool(db));
                let subject = subject_of(ir, target);
                let current = subject
                    .as_ref()
                    .map(|subject| match subject {
                        // The truthiness arm's twin (issue #72): the
                        // just-typed target's record beats the
                        // `mixed` fallback for a call result.
                        NarrowingSubject::CallResult { .. } => self.recorded(target),
                        _ => self.subject_type(environment, subject),
                    })
                    .unwrap_or_else(|| TypeId::mixed(db));
                self.split_on_subject(
                    environment,
                    subject,
                    current,
                    |walker, current| crate::narrowing::keep_falsy(walker.db(), current),
                    |walker, current| crate::narrowing::remove_falsy(walker.db(), current),
                )
            }
            // The default: type it, then facts — an is_* call's table
            // facts, or truthiness on the condition's own subject (a
            // bare variable, an assign-and-test).
            _ => {
                // Cleared before typing: a call typed while evaluating
                // an earlier, unrelated condition must never leak its
                // facts into this one.
                self.pending_condition_facts.clear();
                self.expression_value(condition, environment);
                if let Some((subject, target)) = self.type_check_facts(condition) {
                    let current = self.subject_type(environment, &subject);
                    let mut when_true = environment.clone();
                    let mut when_false = environment.clone();
                    when_true.bind(subject.clone(), self.narrowed_to(current, target));
                    when_false.bind(subject, self.removed_type(current, target));
                    return (when_true, when_false);
                }
                // Only the condition's OWN top-level call contributes
                // conditional facts. A call that was merely an argument
                // to it (`if (ok(helper($y)))`) also queued its facts,
                // but its truthiness is never tested: filter those out
                // by origin so they cannot narrow the branches.
                let pending: Vec<PendingAssertion<'db>> =
                    std::mem::take(&mut self.pending_condition_facts)
                        .into_iter()
                        .filter(|fact| fact.origin == condition)
                        .collect();
                if !pending.is_empty() {
                    let mut when_true = environment.clone();
                    let mut when_false = environment.clone();
                    for fact in pending {
                        use crate::type_syntax::AssertionPolarity;
                        let current = self.subject_type(environment, &fact.subject);
                        let (narrowed, removed) = if fact.negated {
                            (
                                self.removed_type(current, fact.asserted),
                                self.narrowed_to(current, fact.asserted),
                            )
                        } else {
                            (
                                self.narrowed_to(current, fact.asserted),
                                self.removed_type(current, fact.asserted),
                            )
                        };
                        match fact.polarity {
                            AssertionPolarity::IfTrue => {
                                when_true.bind(fact.subject.clone(), narrowed);
                            }
                            AssertionPolarity::IfFalse => {
                                when_false.bind(fact.subject.clone(), removed);
                            }
                            AssertionPolarity::Always => {}
                        }
                    }
                    return (when_true, when_false);
                }
                let subject = subject_of(ir, condition);
                let current = subject
                    .as_ref()
                    .map(|subject| match subject {
                        // An unbound call-result fingerprint has no wide
                        // type of its own (`subject_type` answers
                        // `mixed`), but the condition was just typed:
                        // its recorded type is the call's computed
                        // return — and for a bound fingerprint the
                        // record IS the binding ("environment wins"
                        // consult records it), so the substitution is
                        // uniform (issue #72).
                        NarrowingSubject::CallResult { .. } => self.recorded(condition),
                        _ => self.subject_type(environment, subject),
                    })
                    .unwrap_or_else(|| TypeId::mixed(db));
                self.split_on_subject(
                    environment,
                    subject,
                    current,
                    |walker, current| crate::narrowing::remove_falsy(walker.db(), current),
                    |walker, current| crate::narrowing::keep_falsy(walker.db(), current),
                )
            }
        }
    }

    /// Clone-and-bind: the two environments a decided subject fact
    /// produces. No subject, no facts — two clones.
    fn split_on_subject(
        &mut self,
        environment: &Environment<'db>,
        subject: Option<NarrowingSubject>,
        current: TypeId<'db>,
        when_true: impl FnOnce(&mut Self, TypeId<'db>) -> TypeId<'db>,
        when_false: impl FnOnce(&mut Self, TypeId<'db>) -> TypeId<'db>,
    ) -> (Environment<'db>, Environment<'db>) {
        let mut true_env = environment.clone();
        let mut false_env = environment.clone();
        if let Some(subject) = subject {
            let positive = when_true(self, current);
            let negative = when_false(self, current);
            true_env.bind(subject.clone(), positive);
            false_env.bind(subject, negative);
        }
        (true_env, false_env)
    }

    fn narrowed_to(&self, current: TypeId<'db>, target: TypeId<'db>) -> TypeId<'db> {
        crate::narrowing::narrow_to(
            self.db(),
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            current,
            target,
        )
    }

    fn removed_type(&self, current: TypeId<'db>, target: TypeId<'db>) -> TypeId<'db> {
        crate::narrowing::remove_type(
            self.db(),
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            current,
            target,
        )
    }

    /// The instanceof right-hand side: a written class name resolves
    /// at the declaring site; an expression whose type is
    /// `class-string<T>` or a class contributes that class; anything
    /// else narrows nothing (`mixed`).
    fn instanceof_target(
        &mut self,
        rhs: ExpressionId,
        environment: &mut Environment<'db>,
    ) -> TypeId<'db> {
        let db = self.db();
        match self.context.ir.expression(rhs).cloned() {
            Some(BodyExpression::NamedReference { text }) => {
                if operators::named_reference_type(db, &text).is_some() {
                    // `true`/`false`/`null` are not class names.
                    TypeId::mixed(db)
                } else {
                    self.record(rhs, TypeId::mixed(db));
                    self.class_type_of_written(&text)
                }
            }
            _ => {
                let of = self.expression(rhs, environment);
                if let Some(argument) = of.class_string_argument(db).flatten() {
                    argument
                } else if of.class_name(db).is_some() {
                    of
                } else {
                    TypeId::mixed(db)
                }
            }
        }
    }

    /// `is_string($x)`-shaped calls: the callee is an unqualified or
    /// root-qualified name in the table, the first argument is a
    /// subject, no spread. (PHP resolves unqualified function names
    /// through the namespace with a global fallback; the `is_*` names
    /// are never redeclared in practice — and if one is, the wrong
    /// narrowing is over-narrowing on working code, which the plan-8
    /// corpus gate would catch. Recorded stance.)
    fn type_check_facts(
        &mut self,
        condition: ExpressionId,
    ) -> Option<(NarrowingSubject, TypeId<'db>)> {
        let db = self.db();
        let ir = self.context.ir;
        let Some(BodyExpression::Call { callee, arguments }) = ir.expression(condition) else {
            return None;
        };
        let Some(BodyExpression::NamedReference { text }) = ir.expression(*callee) else {
            return None;
        };
        let name = text.strip_prefix('\\').unwrap_or(text);
        if name.contains('\\') {
            return None;
        }
        let target = crate::narrowing::type_check_target(db, &name.to_ascii_lowercase())?;
        let first = arguments.first().filter(|argument| !argument.spread)?;
        let subject = subject_of(ir, first.value)?;
        Some((subject, target))
    }

    /// Walks a nested body (a closure or arrow function) with its own
    /// return accumulator, so its `return`/`yield` never leak into the
    /// enclosing body's return type; answers the returns it collected,
    /// whether it yielded, and whether its end is reachable (a
    /// closure's implicit-null fall-through).
    fn nested_returns(
        &mut self,
        walk: impl FnOnce(&mut Self, &mut Environment<'db>),
        environment: &mut Environment<'db>,
    ) -> (Vec<TypeId<'db>>, bool, bool) {
        let saved_returns = std::mem::take(&mut self.returns);
        let saved_yield = std::mem::replace(&mut self.saw_yield, false);
        walk(self, environment);
        let inner_returns = std::mem::replace(&mut self.returns, saved_returns);
        let inner_yield = std::mem::replace(&mut self.saw_yield, saved_yield);
        (inner_returns, inner_yield, environment.is_reachable())
    }

    /// The callable type of a closure or arrow function: parameters
    /// from the written signature (lowered at the declaring site, the
    /// native `= null` implicit nullability included), the declared
    /// return text when present, the inner returns joined otherwise.
    fn closure_type(
        &mut self,
        parameters: &[celerrate_semantics::ParameterSignature],
        return_type_text: Option<&str>,
        returns: Vec<TypeId<'db>>,
        saw_yield: bool,
        end_reachable: bool,
    ) -> TypeId<'db> {
        let db = self.db();
        let site = crate::declared::NameSite::Source {
            namespace: &self.context.namespace,
            tables: &self.context.tables,
        };
        let callable_parameters = parameters
            .iter()
            .map(|parameter| {
                let mut parameter_type = parameter
                    .type_text
                    .as_deref()
                    .and_then(|text| crate::declared::lower_written_text(db, &site, text))
                    .unwrap_or_else(|| TypeId::mixed(db));
                if parameter.default_text.as_deref() == Some("null") {
                    parameter_type = TypeId::union(db, [parameter_type, TypeId::null(db)]);
                }
                crate::representation::CallableParameter {
                    parameter_type,
                    optional: parameter.default_text.is_some(),
                    variadic: parameter.variadic,
                    by_reference: parameter.by_reference,
                }
            })
            .collect();
        let return_type = return_type_text
            .and_then(|text| crate::declared::lower_written_text(db, &site, text))
            .unwrap_or_else(|| {
                if saw_yield {
                    return TypeId::class(db, "Generator", vec![]);
                }
                let joined = returns
                    .into_iter()
                    .reduce(|left, right| join(db, left, right));
                match (joined, end_reachable) {
                    (Some(joined), true) => join(db, joined, TypeId::null(db)),
                    (None, true) => TypeId::null(db),
                    (Some(joined), false) => joined,
                    (None, false) => TypeId::never(db),
                }
            });
        TypeId::callable(db, callable_parameters, return_type)
    }

    /// Seeds a closure or arrow function's own parameters into its
    /// inner environment: the closure-side sibling of the per-body
    /// `seeded_parameters` in `inference.rs` — written texts, not
    /// declared queries, because a closure has no `FunctionQuery`
    /// identity of its own.
    fn seed_written_parameters(
        &self,
        parameters: &[celerrate_semantics::ParameterSignature],
        environment: &mut Environment<'db>,
    ) {
        let db = self.db();
        let site = crate::declared::NameSite::Source {
            namespace: &self.context.namespace,
            tables: &self.context.tables,
        };
        for parameter in parameters {
            let mut of = parameter
                .type_text
                .as_deref()
                .and_then(|text| crate::declared::lower_written_text(db, &site, text))
                .unwrap_or_else(|| TypeId::mixed(db));
            if parameter.default_text.as_deref() == Some("null") {
                of = TypeId::union(db, [of, TypeId::null(db)]);
            }
            if parameter.variadic {
                of = TypeId::list(db, of);
            }
            environment.bind(
                NarrowingSubject::Local {
                    name: parameter.name.clone(),
                },
                of,
            );
        }
    }

    /// Task 9(e): a callee's declared signature projected into a
    /// callable type, shared by every first-class-callable form. The
    /// return type funnels through `member_boundary_type` (decision
    /// 1): a `self`/`static`/`parent` placeholder substitutes against
    /// the receiver and the receiver's class arguments bind its
    /// class-level templates; the parameters are never substituted (a
    /// later task revisits generic callables).
    fn projected_callable(
        &mut self,
        signature: Option<DeclaredSignature<'db>>,
        receiver: Option<TypeId<'db>>,
        owner: Option<&str>,
        return_fallback: TypeId<'db>,
    ) -> TypeId<'db> {
        let db = self.db();
        let Some(signature) = signature else {
            return TypeId::mixed(db);
        };
        let parameters = signature
            .parameters
            .iter()
            .map(|parameter| crate::representation::CallableParameter {
                parameter_type: parameter
                    .parameter_type
                    .unwrap_or_else(|| TypeId::mixed(db)),
                optional: parameter.optional,
                variadic: parameter.variadic,
                by_reference: parameter.by_reference,
            })
            .collect();
        let mut return_type = if self.declared_present(&signature) {
            self.edge_counts.declared_return_edges += 1;
            signature.value_type
        } else {
            return_fallback
        };
        if let Some(receiver) = receiver {
            return_type = self.member_boundary_type(return_type, owner, receiver);
        }
        TypeId::callable(db, parameters, return_type)
    }

    /// A named function's declared signature, projected (`g(...)`).
    fn projected_callable_of_function(&mut self, key: &str, source_exists: bool) -> TypeId<'db> {
        let db = self.db();
        let signature = declared_function_signature(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            crate::declared::FunctionQuery::new(db, key.to_owned()),
        );
        // The fallback is consulted only when a signature exists but
        // carries no usable declared return — `projected_callable`'s own
        // condition. Compute the inferred return, and count its edge,
        // exactly then: an eager call would over-count the instrument and
        // spin the fixpoint for a result the declared tier discards.
        let uses_fallback = signature
            .as_ref()
            .is_some_and(|signature| !self.declared_present(signature));
        // Mirrors `function_call_result` (the direct-call precedent): a
        // first-class callable of a free function reaches the same two
        // `edge_counts` increment sites `projected_callable` below counts
        // (declared here, inferred in the fallback branch), so the same
        // dependency must be recorded beside each — otherwise a body
        // forming `g(...)` records the edge count but no identity for
        // task 8's live-demand validator to re-check.
        if signature
            .as_ref()
            .is_some_and(|signature| self.declared_present(signature))
        {
            self.dependencies.functions.insert(key.to_owned());
        }
        let return_fallback = if uses_fallback && source_exists {
            self.edge_counts.inferred_return_edges += 1;
            // Decision 4: raw, pre-substitution — a free function has no
            // member boundary to substitute against, matching
            // `function_call_result`'s own capture.
            let raw = crate::inference::inferred_function_return(
                db,
                self.context.files,
                self.context.stubs,
                self.context.configuration,
                crate::declared::FunctionQuery::new(db, key.to_owned()),
            );
            self.dependencies
                .inferred_functions
                .push((key.to_owned(), raw));
            // Plan 9a task 10: also record the callee's declared-
            // signature-guarding dependency (mirrors `function_call_
            // result`'s own fix) — a docblock `@return` later appearing
            // on `key` must be visible to revalidation even though only
            // the inferred tier answered this first-class-callable
            // projection.
            self.dependencies.functions.insert(key.to_owned());
            raw
        } else {
            TypeId::mixed(db)
        };
        self.projected_callable(signature, None, None, return_fallback)
    }

    /// A method's declared signature on a resolved receiver, projected
    /// (`$obj->method(...)`); `mixed` for an opaque or union receiver.
    fn projected_callable_of_method(
        &mut self,
        receiver_type: TypeId<'db>,
        name: &str,
    ) -> TypeId<'db> {
        let db = self.db();
        let keys = match self.receiver_parts(receiver_type.without_null(db)) {
            Some(keys) => keys,
            None => return TypeId::mixed(db),
        };
        self.projected_callable_of_keys(&keys, receiver_type, name)
    }

    /// A method's declared signature across resolved keys, projected
    /// (`Foo::method(...)`, `self::method(...)`); a union receiver's
    /// differing signatures is ambiguous and answers `mixed`.
    fn projected_callable_of_keys(
        &mut self,
        keys: &[String],
        receiver: TypeId<'db>,
        name: &str,
    ) -> TypeId<'db> {
        let db = self.db();
        let mut signatures = self.method_signatures(keys, name);
        // One key, one signature: more is a union receiver, silence.
        // The owner comes from that same one key, never a different
        // one — there is exactly one candidate here, so there is no
        // hoisting hazard to begin with (Finding 3 is about the
        // multi-signature callers above).
        let resolved = (signatures.len() == 1).then(|| signatures.remove(0));
        let (signature, owner) = match resolved {
            Some((key, signature)) => (
                Some(signature),
                self.member_owner(&key, MemberKind::Method, name),
            ),
            None => (None, None),
        };
        self.projected_callable(
            signature,
            Some(receiver),
            owner.as_deref(),
            TypeId::mixed(db),
        )
    }

    fn string_parts(&mut self, parts: &[StringPart], environment: &mut Environment<'db>) {
        for part in parts {
            if let StringPart::Interpolation { expression } = part {
                self.expression(*expression, environment);
            }
        }
    }

    /// An array literal: a shape when every entry has a statically
    /// known key (or none — positional) and no spread; otherwise the
    /// general array of the joined keys and values.
    fn array_literal(
        &mut self,
        entries: &[ArrayEntry],
        environment: &mut Environment<'db>,
    ) -> TypeId<'db> {
        let db = self.db();
        let mut fields: Vec<crate::representation::ShapeField<'db>> = Vec::new();
        let mut next_index: i64 = 0;
        let mut shape_holds = true;
        let mut joined_key: Option<TypeId<'db>> = None;
        let mut joined_value: Option<TypeId<'db>> = None;
        let mut is_list = true;
        for entry in entries {
            let ArrayEntry::Element {
                key,
                value,
                spread,
                by_reference: _,
            } = entry
            else {
                continue; // a destructuring hole never appears in a literal read
            };
            let key_type = key.map(|key| self.expression(key, environment));
            let value_type = self.expression(*value, environment);
            if *spread {
                shape_holds = false;
                is_list = false;
                let spread_key = value_type
                    .array_key(db)
                    .unwrap_or_else(|| TypeId::union(db, [TypeId::int(db), TypeId::string(db)]));
                let spread_value = value_type
                    .array_value(db)
                    .unwrap_or_else(|| TypeId::mixed(db));
                joined_key = Some(joined_key.map_or(spread_key, |k| join(db, k, spread_key)));
                joined_value =
                    Some(joined_value.map_or(spread_value, |v| join(db, v, spread_value)));
                continue;
            }
            let shape_key = match key_type {
                None => {
                    let index = next_index;
                    next_index += 1;
                    Some(crate::representation::ShapeKey::Integer(index))
                }
                Some(of) => {
                    is_list = false;
                    of.int_literal_value(db)
                        .map(crate::representation::ShapeKey::Integer)
                        .or_else(|| {
                            of.string_literal_value(db)
                                .map(crate::representation::ShapeKey::String)
                        })
                }
            };
            match shape_key {
                Some(shape_key) if shape_holds => fields.push(crate::representation::ShapeField {
                    key: shape_key,
                    optional: false,
                    value: value_type,
                }),
                _ => shape_holds = false,
            }
            let this_key = key_type.unwrap_or_else(|| TypeId::int(db));
            joined_key = Some(joined_key.map_or(this_key, |k| join(db, k, this_key)));
            joined_value = Some(joined_value.map_or(value_type, |v| join(db, v, value_type)));
        }
        if shape_holds && !fields.is_empty() {
            return TypeId::shape(db, fields);
        }
        match (joined_key, joined_value) {
            (Some(key), Some(value)) => {
                if is_list {
                    TypeId::non_empty_list(db, value)
                } else {
                    TypeId::non_empty_array(db, key, value)
                }
            }
            // The empty literal.
            _ => TypeId::shape(db, vec![]),
        }
    }

    /// One assignment: propagate to the target's subject, updating
    /// array bases on index writes and destructuring element-wise.
    /// Answers the expression's own type (the assigned value; the
    /// computed value for compound forms).
    fn assignment(
        &mut self,
        operator: SyntaxKind,
        by_reference: bool,
        target: ExpressionId,
        _value: ExpressionId,
        value_type: TypeId<'db>,
        environment: &mut Environment<'db>,
    ) -> TypeId<'db> {
        let db = self.db();
        if operator == SyntaxKind::QuestionQuestionEquals {
            // `$x ??= v` reduces to `$x = $x ?? v`: the same
            // gated-widen-then-union combination as the `??` arm (see
            // its comment) so a same-family literal absorbs
            // (`?int $x; $x ??= 0;` answers `int`) while a genuinely
            // different alternative — or a pre-existing union — survives
            // instead of collapsing. The value operand was already
            // walked unconditionally by the `Assignment` arm above — its
            // environment effects apply on both paths, a recorded
            // conservative approximation.
            let current = self.recorded(target);
            let assigned = TypeId::union(
                db,
                [
                    widen_if_literal(db, current.without_null(db)),
                    widen_if_literal(db, value_type),
                ],
            );
            self.assign_target(target, assigned, environment);
            return assigned;
        }
        if by_reference {
            // `$b = &$a`: aliased locals are unknowable without alias
            // analysis — both sides degrade to mixed (decision 10).
            if let Some(subject) = subject_of(self.context.ir, target) {
                if let NarrowingSubject::Local { name } = &subject {
                    environment.kill_call_results_involving(name);
                }
                environment.bind(subject, TypeId::mixed(db));
            }
            if let Some(subject) = subject_of(self.context.ir, _value) {
                if let NarrowingSubject::Local { name } = &subject {
                    environment.kill_call_results_involving(name);
                }
                environment.bind(subject, TypeId::mixed(db));
            }
            return TypeId::mixed(db);
        }
        let assigned = match compound_base(operator) {
            Some(base) => {
                let current = self.recorded(target);
                operators::binary_type(db, base, current, value_type)
            }
            None => value_type,
        };
        self.assign_target(target, assigned, environment);
        assigned
    }

    fn assign_target(
        &mut self,
        target: ExpressionId,
        value_type: TypeId<'db>,
        environment: &mut Environment<'db>,
    ) {
        let db = self.db();
        match self.context.ir.expression(target).cloned() {
            // Destructuring: `[$a, $b] = ...`, `['k' => $v] = ...`.
            Some(BodyExpression::Array { entries }) => {
                let mut next_index: i64 = 0;
                for entry in &entries {
                    let ArrayEntry::Element { key, value, .. } = entry else {
                        next_index += 1;
                        continue;
                    };
                    let key_type = match key {
                        Some(key) => Some(self.recorded(*key)),
                        None => {
                            let index = next_index;
                            next_index += 1;
                            Some(TypeId::int_literal(db, index))
                        }
                    };
                    let element = operators::index_type(db, value_type, key_type);
                    self.assign_target(*value, element, environment);
                }
            }
            // An index write rebinds the base array.
            Some(BodyExpression::Index { subject, index }) => {
                if let Some(base) = subject_of(self.context.ir, subject) {
                    let current = environment.binding(&base);
                    let key_type = index.map(|index| self.recorded(index));
                    let updated = updated_array(db, current, key_type, value_type);
                    if let NarrowingSubject::Local { name } = &base {
                        environment.kill_call_results_involving(name);
                    }
                    environment.bind(base, updated);
                }
            }
            _ => {
                if let Some(subject) = subject_of(self.context.ir, target) {
                    if let NarrowingSubject::Local { name } = &subject {
                        environment.kill_call_results_involving(name);
                    }
                    environment.bind(subject, value_type);
                }
            }
        }
    }
}

/// Widen `of` to its general type only when it is a single-value
/// narrowing literal; anything else (a plain scalar, a class, or a
/// pre-existing union) passes through untouched. The `??` and `??=`
/// result types union their two operands after this gate, so a
/// same-family literal still absorbs (`?string ?? 'd'` → `string`)
/// while a multi-literal union survives (`(1|2|null) ?? 3` → `1|2|int`)
/// rather than collapsing the way widening the whole operand would.
fn widen_if_literal<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> TypeId<'db> {
    if crate::narrowing::is_narrowing_literal(db, of) {
        widened_literals(db, of)
    } else {
        of
    }
}

/// `$a op= $b` reduces to `op`; `None` for plain `=` (and for `??=`,
/// which Task 5 handles in the walker).
fn compound_base(operator: SyntaxKind) -> Option<SyntaxKind> {
    Some(match operator {
        SyntaxKind::PlusEquals => SyntaxKind::Plus,
        SyntaxKind::MinusEquals => SyntaxKind::Minus,
        SyntaxKind::StarEquals => SyntaxKind::Star,
        SyntaxKind::SlashEquals => SyntaxKind::Slash,
        SyntaxKind::DotEquals => SyntaxKind::Dot,
        SyntaxKind::PercentEquals => SyntaxKind::Percent,
        SyntaxKind::StarStarEquals => SyntaxKind::StarStar,
        SyntaxKind::AmpersandEquals => SyntaxKind::Ampersand,
        SyntaxKind::PipeEquals => SyntaxKind::Pipe,
        SyntaxKind::CaretEquals => SyntaxKind::Caret,
        SyntaxKind::LessLessEquals => SyntaxKind::LessLess,
        SyntaxKind::GreaterGreaterEquals => SyntaxKind::GreaterGreater,
        _ => return None,
    })
}

/// The new type of an array base after `$a[k] = v`: a shape upserts
/// the field when the key is a known literal, an array joins key and
/// value, anything else becomes an array from this write.
fn updated_array<'db>(
    db: &'db dyn salsa::Database,
    current: Option<TypeId<'db>>,
    key_type: Option<TypeId<'db>>,
    value_type: TypeId<'db>,
) -> TypeId<'db> {
    use crate::representation::{ShapeField, ShapeKey};
    let literal_key = key_type.and_then(|key| {
        key.int_literal_value(db)
            .map(ShapeKey::Integer)
            .or_else(|| key.string_literal_value(db).map(ShapeKey::String))
    });
    if let Some(current) = current {
        if let Some(mut fields) = current.shape_fields(db) {
            match (&literal_key, key_type) {
                (Some(wanted), _) => {
                    fields.retain(|field| field.key != *wanted);
                    fields.push(ShapeField {
                        key: wanted.clone(),
                        optional: false,
                        value: value_type,
                    });
                    return TypeId::shape(db, fields);
                }
                (None, None) => {
                    // `$a[] = v`: the next free integer key.
                    let next = fields
                        .iter()
                        .filter_map(|field| match &field.key {
                            ShapeKey::Integer(index) => Some(*index + 1),
                            ShapeKey::String(_) => None,
                        })
                        .max()
                        .unwrap_or(0);
                    fields.push(ShapeField {
                        key: ShapeKey::Integer(next),
                        optional: false,
                        value: value_type,
                    });
                    return TypeId::shape(db, fields);
                }
                (None, Some(_)) => {
                    // A dynamic key on a shape: degrade to the array
                    // of the joined parts.
                    let (key, value) = shape_join(db, &fields);
                    let key_join = key_type.map_or(key, |of| join(db, key, of));
                    return TypeId::array(db, key_join, join(db, value, value_type));
                }
            }
        }
        if let (Some(key), Some(value)) = (current.array_key(db), current.array_value(db)) {
            let pushed_key = key_type.unwrap_or_else(|| TypeId::int(db));
            return TypeId::non_empty_array(
                db,
                join(db, key, pushed_key),
                join(db, value, value_type),
            );
        }
    }
    // Anything else (absent, mixed, scalar): the write makes it an
    // array from here on.
    match (literal_key, key_type) {
        (Some(key), _) => TypeId::shape(
            db,
            vec![ShapeField {
                key,
                optional: false,
                value: value_type,
            }],
        ),
        (None, None) => TypeId::non_empty_list(db, value_type),
        (None, Some(key)) => TypeId::non_empty_array(db, key, value_type),
    }
}

fn shape_join<'db>(
    db: &'db dyn salsa::Database,
    fields: &[crate::representation::ShapeField<'db>],
) -> (TypeId<'db>, TypeId<'db>) {
    use crate::representation::ShapeKey;
    let mut key: Option<TypeId<'db>> = None;
    let mut value: Option<TypeId<'db>> = None;
    for field in fields {
        let field_key = match &field.key {
            ShapeKey::Integer(index) => TypeId::int_literal(db, *index),
            ShapeKey::String(text) => TypeId::string_literal(db, text),
        };
        key = Some(key.map_or(field_key, |k| join(db, k, field_key)));
        value = Some(value.map_or(field.value, |v| join(db, v, field.value)));
    }
    (
        key.unwrap_or_else(|| TypeId::union(db, [TypeId::int(db), TypeId::string(db)])),
        value.unwrap_or_else(|| TypeId::mixed(db)),
    )
}
