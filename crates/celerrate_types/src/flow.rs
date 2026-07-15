//! The flow walk of one body: expression typing with an environment
//! of narrowing subjects threaded through statements. Branches join,
//! divergence (return, throw, break, goto) marks the path
//! unreachable, and loops run an inner join-ascent fixpoint with a
//! budget — the interprocedural discipline in miniature (design
//! section 6). Absence is silence: a subject missing from the
//! environment reads as its wide type, `mixed` for locals.

// Interim scaffolding: `files`, `stubs`, `configuration`,
// `owner_class_key`, and `method_is_static` are read starting Task 6
// (member and call resolution) and Task 8 (property widening); this
// task only threads them through the context so later tasks do not
// have to change the walker's construction.
#![allow(dead_code)]

use std::collections::BTreeMap;

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{
    AncestorRelation, ArrayEntry, BodyExpression, BodyIr, BodyStatement, ClassQuery,
    ClassReference, ExpressionId, MemberKind, MemberQuery, MemberReference, StatementId,
    StringPart, UseTables, folded_member_key, linearized_class,
};
use celerrate_stubs::StubIndexInput;
use celerrate_syntax::SyntaxKind;

use crate::declared::{DeclaredSignature, Trust, declared_member_signature};
use crate::inference::InterproceduralEdgeCounts;
use crate::narrowing::{NarrowingSubject, subject_of};
use crate::operators;
use crate::representation::TypeId;
use crate::widening::{join, widened_literals};

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
}

pub(crate) struct FlowResult<'db> {
    pub expression_types: Vec<TypeId<'db>>,
    pub return_type: TypeId<'db>,
    pub edge_counts: InterproceduralEdgeCounts,
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

pub(crate) struct Walker<'db, 'body, 'context> {
    context: &'context FlowContext<'db, 'body>,
    types: Vec<TypeId<'db>>,
    returns: Vec<TypeId<'db>>,
    saw_yield: bool,
    edge_counts: InterproceduralEdgeCounts,
}

/// Walks one body from its seeded parameter environment.
pub(crate) fn walk_body<'db>(context: &FlowContext<'db, '_>) -> FlowResult<'db> {
    let db = context.db;
    let mut walker = Walker {
        context,
        types: vec![TypeId::mixed(db); context.ir.expressions.len()],
        returns: Vec::new(),
        saw_yield: false,
        edge_counts: InterproceduralEdgeCounts::default(),
    };
    let mut environment = Environment::new();
    for (name, seeded) in &context.parameters {
        environment.bind(NarrowingSubject::Local { name: name.clone() }, *seeded);
    }
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
    FlowResult {
        expression_types: walker.types,
        return_type,
        edge_counts: walker.edge_counts,
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
    /// `mixed` in this task (Task 8 widens property subjects to their
    /// declared type).
    fn subject_type(
        &self,
        environment: &Environment<'db>,
        subject: &NarrowingSubject,
    ) -> TypeId<'db> {
        environment
            .binding(subject)
            .unwrap_or_else(|| TypeId::mixed(self.db()))
    }

    /// `$this`: the defining class in a non-static method; `mixed`
    /// (silence) everywhere else. Plan 6 replaces this with the
    /// symbolic late-static-binding placeholder (decision 5).
    fn this_type(&self) -> TypeId<'db> {
        match (&self.context.owner_class_key, self.context.method_is_static) {
            (Some(key), false) => TypeId::class(self.db(), key, vec![]),
            _ => TypeId::mixed(self.db()),
        }
    }

    /// The class-like keys a receiver type addresses; `None` when any
    /// part is opaque (mixed, object, a scalar, an unresolvable
    /// shape) — the silent stance of decision 11. Union constituents
    /// must all resolve (null skipped: the nullability family's
    /// business); intersection contributes every resolving part.
    fn receiver_parts(&self, of: TypeId<'db>) -> Option<Vec<String>> {
        let db = self.db();
        if of == TypeId::self_placeholder(db) || of == TypeId::static_placeholder(db) {
            return self.context.owner_class_key.clone().map(|key| vec![key]);
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

    /// A member's declared value across the receiver keys: the
    /// alternatives a union receiver's resolving constituents answer,
    /// preserved with `TypeId::union` rather than widened away — which
    /// constituent the receiver actually is at runtime decides which
    /// answer applies, the same control-flow-alternative shape as a
    /// branch join (see the module's `join`-vs-`union` convention);
    /// `None` when no key answers.
    fn member_value_type(
        &self,
        keys: &[String],
        kind: MemberKind,
        name: &str,
    ) -> Option<TypeId<'db>> {
        let db = self.db();
        keys.iter()
            .filter_map(|key| {
                declared_member_signature(
                    db,
                    self.context.files,
                    self.context.stubs,
                    self.context.configuration,
                    MemberQuery::new(db, key.clone(), kind, folded_member_key(kind, name)),
                )
                .map(|signature| signature.value_type)
            })
            .reduce(|left, right| TypeId::union(db, [left, right]))
    }

    /// The declared method signatures across the receiver keys, in
    /// key order (empty when none resolves).
    fn method_signatures(&self, keys: &[String], name: &str) -> Vec<DeclaredSignature<'db>> {
        let db = self.db();
        keys.iter()
            .filter_map(|key| {
                declared_member_signature(
                    db,
                    self.context.files,
                    self.context.stubs,
                    self.context.configuration,
                    MemberQuery::new(
                        db,
                        key.clone(),
                        MemberKind::Method,
                        folded_member_key(MemberKind::Method, name),
                    ),
                )
            })
            .collect()
    }

    /// The declared-present gate (decision 4).
    fn declared_present(&self, signature: &DeclaredSignature<'db>) -> bool {
        signature.value_trust != Trust::NativeOnly || !signature.value_type.is_mixed(self.db())
    }

    /// Decision 6: `self`/`static` placeholders in a declared return
    /// substitute the receiver's type, top level and one level into
    /// unions; `parent` answers mixed. Plan 6 replaces this with the
    /// forwarding model.
    fn substitute_receiver(&self, of: TypeId<'db>, receiver: TypeId<'db>) -> TypeId<'db> {
        let db = self.db();
        if of == TypeId::self_placeholder(db) || of == TypeId::static_placeholder(db) {
            return receiver;
        }
        if of == TypeId::parent_placeholder(db) {
            return TypeId::mixed(db);
        }
        let constituents = of.constituents(db);
        if constituents.len() > 1 {
            return TypeId::union(
                db,
                constituents
                    .into_iter()
                    .map(|part| self.substitute_receiver(part, receiver)),
            );
        }
        of
    }

    /// The defining class as a type (decision 5): built from
    /// `owner_class_key`, `mixed` when there is no owner (a free
    /// function or an anonymous-class method, decision 12). Unlike
    /// [`Self::this_type`] this carries no static-method gate: the
    /// `self`/`static` *class keyword* is available in a static method,
    /// only the `$this` *value* is not.
    fn defining_class_type(&self) -> TypeId<'db> {
        let db = self.db();
        self.context
            .owner_class_key
            .as_ref()
            .map(|key| TypeId::class(db, key, vec![]))
            .unwrap_or_else(|| TypeId::mixed(db))
    }

    /// The class a `self`/`static`/`parent` keyword names in the
    /// defining context (decision 5); `None` for any other name (an
    /// ordinary class, which the caller resolves through
    /// `class_type_of_written`). An absent owner or parent answers
    /// `mixed`, never a bogus qualified `self`/`parent` class.
    fn scope_keyword_class(&self, name: &str) -> Option<TypeId<'db>> {
        let db = self.db();
        match name.to_ascii_lowercase().as_str() {
            "self" | "static" => Some(self.defining_class_type()),
            "parent" => Some(
                self.parent_class_key()
                    .map(|key| TypeId::class(db, &key, vec![]))
                    .unwrap_or_else(|| TypeId::mixed(db)),
            ),
            _ => None,
        }
    }

    /// The first `extends` edge of the defining class's ancestry.
    fn parent_class_key(&self) -> Option<String> {
        let db = self.db();
        let owner = self.context.owner_class_key.as_ref()?;
        let linearized = linearized_class(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            ClassQuery::new(db, owner.clone()),
        )
        .as_ref()?;
        linearized
            .ancestry
            .iter()
            .find(|edge| edge.relation == AncestorRelation::Extends)
            .and_then(|edge| edge.resolved.clone())
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
                let of = keys
                    .as_ref()
                    .and_then(|keys| keys.first())
                    .map(|key| TypeId::class(db, key, vec![]))
                    .unwrap_or_else(|| TypeId::mixed(db));
                (of, keys)
            }
            _ => {
                let of = self.expression(subject, environment);
                let through_class_string = of.class_string_argument(db).flatten();
                let effective = through_class_string.unwrap_or(of);
                (effective, self.receiver_parts(effective))
            }
        }
    }

    fn typed_arguments(
        &mut self,
        arguments: &[celerrate_semantics::CallArgument],
        environment: &mut Environment<'db>,
    ) -> Vec<TypeId<'db>> {
        arguments
            .iter()
            .map(|argument| self.expression(argument.value, environment))
            .collect()
    }

    /// An instance method call's result on a resolved receiver type.
    fn method_call_result(&mut self, receiver: TypeId<'db>, name: &str) -> TypeId<'db> {
        match self.receiver_parts(receiver) {
            Some(keys) => self.method_call_result_for_keys(&keys, receiver, name),
            None => TypeId::mixed(self.db()),
        }
    }

    /// The declared-return path (decision 3's middle tier): a
    /// declared return per resolving key, preserved as alternatives
    /// with `TypeId::union` — not `crate::widening::join` — because a
    /// union receiver's keys are exclusive control-flow alternatives
    /// (exactly one applies at runtime), the same shape the branch and
    /// loop joins in this module already preserve rather than widen
    /// (the brief's own pseudocode reduces with `join`, but that
    /// collapses e.g. `int` and `string` straight to `mixed`, which
    /// contradicts `union_receivers_join_and_opaque_receivers_stay_silent`'s
    /// expected `"int|string"`). The gate failing on any key answers
    /// mixed for that key (method-inferred returns are plan 6).
    /// Placeholders substitute the receiver (decision 6).
    fn method_call_result_for_keys(
        &mut self,
        keys: &[String],
        receiver: TypeId<'db>,
        name: &str,
    ) -> TypeId<'db> {
        let db = self.db();
        let signatures = self.method_signatures(keys, name);
        if signatures.is_empty() {
            return TypeId::mixed(db);
        }
        let mut result: Option<TypeId<'db>> = None;
        let mut any_declared = false;
        for signature in &signatures {
            let value = if self.declared_present(signature) {
                any_declared = true;
                self.substitute_receiver(signature.value_type, receiver)
            } else {
                TypeId::mixed(db)
            };
            result = Some(match result {
                Some(previous) => TypeId::union(db, [previous, value]),
                None => value,
            });
        }
        if any_declared {
            self.edge_counts.declared_return_edges += 1;
        }
        result.unwrap_or_else(|| TypeId::mixed(db))
    }

    fn statements(&mut self, list: &[StatementId], environment: &mut Environment<'db>) {
        for &statement in list {
            if !environment.is_reachable() {
                // Dead code still gets its expressions typed (the
                // table covers the whole arena), against a throwaway
                // empty environment — reachable locally so nested
                // joins behave, discarded so it cannot resurrect the
                // real path.
                let mut scratch = Environment::new();
                self.statement(statement, &mut scratch);
                continue;
            }
            self.statement(statement, environment);
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
                self.expression(subject, environment);
                self.looped(environment, |walker, env| {
                    // Iteration typing is plan 6: key and value are
                    // mixed, honestly.
                    if let Some(key) = key
                        && let Some(subject) = subject_of(walker.context.ir, key)
                    {
                        walker.expression(key, env);
                        env.bind(subject, TypeId::mixed(walker.db()));
                    }
                    walker.expression(value, env);
                    if let Some(subject) = subject_of(walker.context.ir, value) {
                        env.bind(subject, TypeId::mixed(walker.db()));
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
                // eval can rewrite every local: forget them all.
                let locals: Vec<NarrowingSubject> = environment
                    .bindings
                    .keys()
                    .filter(|subject| matches!(subject, NarrowingSubject::Local { .. }))
                    .cloned()
                    .collect();
                for subject in locals {
                    environment.remove(&subject);
                }
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
                TypeId::mixed(db)
            }
            BodyExpression::Include { operand, .. } => {
                self.expression(operand, environment);
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
                // Task 7 refines the null_safe receiver; until then
                // resolve through the non-null part unconditionally —
                // identical member answers either way.
                let resolving = receiver_type.without_null(db);
                let _ = null_safe;
                match member {
                    MemberReference::Named { name } => self
                        .receiver_parts(resolving)
                        .and_then(|keys| self.member_value_type(&keys, MemberKind::Property, &name))
                        .unwrap_or_else(|| TypeId::mixed(db)),
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
                let _ = subject_type;
                match member {
                    MemberReference::Named { name } => {
                        if name.eq_ignore_ascii_case("class") {
                            // `Foo::class`, `self::class`, `static::class`.
                            let argument = keys
                                .as_ref()
                                .and_then(|keys| keys.first())
                                .map(|key| TypeId::class(db, key, vec![]));
                            TypeId::class_string(db, argument)
                        } else {
                            keys.as_ref()
                                .and_then(|keys| {
                                    self.member_value_type(keys, MemberKind::ClassConstant, &name)
                                        .or_else(|| {
                                            self.member_value_type(
                                                keys,
                                                MemberKind::EnumCase,
                                                &name,
                                            )
                                        })
                                })
                                .unwrap_or_else(|| TypeId::mixed(db))
                        }
                    }
                    // `Foo::$prop`: a static property is a Property.
                    MemberReference::Variable { name } => keys
                        .as_ref()
                        .and_then(|keys| self.member_value_type(keys, MemberKind::Property, &name))
                        .unwrap_or_else(|| TypeId::mixed(db)),
                    MemberReference::Computed { expression } => {
                        self.expression(expression, environment);
                        TypeId::mixed(db)
                    }
                    MemberReference::Missing => TypeId::mixed(db),
                }
            }
            BodyExpression::NullSafeChain { chain } => {
                // Task 7 implements the whole-chain rule.
                self.expression(chain, environment)
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
                        let receiver_type = self.expression(receiver, environment);
                        let resolving = receiver_type.without_null(db);
                        let _ = null_safe;
                        self.record(callee, TypeId::mixed(db));
                        // Task 9's provider tier reads the argument
                        // types; until then they type for the table.
                        let _argument_types = self.typed_arguments(&arguments, environment);
                        self.method_call_result(resolving, &name)
                    }
                    Some(BodyExpression::ScopedAccess {
                        subject,
                        member: MemberReference::Named { name },
                    }) => {
                        let (subject_type, keys) = self.scoped_subject(subject, environment);
                        self.record(callee, TypeId::mixed(db));
                        let _argument_types = self.typed_arguments(&arguments, environment);
                        match keys {
                            Some(keys) => {
                                let receiver = subject_type;
                                self.method_call_result_for_keys(&keys, receiver, &name)
                            }
                            None => TypeId::mixed(db),
                        }
                    }
                    _ => {
                        // Task 9: named function calls and callable
                        // values. Until then: walk and stay silent.
                        self.expression(callee, environment);
                        self.typed_arguments(&arguments, environment);
                        TypeId::mixed(db)
                    }
                }
            }
            BodyExpression::CallableReference { callee } => {
                self.expression(callee, environment);
                TypeId::mixed(db)
            }
            BodyExpression::New { class, arguments } => {
                let of = match &class {
                    // Decision 5: `new self()`/`new static()` type as the
                    // defining class, `new parent()` as the parent — the
                    // same resolution `scoped_subject` applies to
                    // `self::`/`static::`/`parent::`. Not
                    // `class_type_of_written` (which would qualify these
                    // keywords into bogus class names `self`/`parent`),
                    // and not `this_type` (whose static-method gate is
                    // `$this`'s alone; the `self`/`static` class keyword is
                    // available in a static method too).
                    ClassReference::Named { name } => self
                        .scope_keyword_class(name)
                        .unwrap_or_else(|| self.class_type_of_written(name)),
                    ClassReference::StaticKeyword => self.defining_class_type(),
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
                    // Decision 12: no folded key exists yet.
                    ClassReference::Anonymous { .. } | ClassReference::Missing => TypeId::mixed(db),
                };
                for argument in &arguments {
                    self.expression(argument.value, environment);
                }
                of
            }
            BodyExpression::Index { subject, index } => {
                let subject_type = self.expression(subject, environment);
                let index_type = index.map(|index| self.expression(index, environment));
                operators::index_type(db, subject_type, index_type)
            }
            BodyExpression::Closure { body, .. } => {
                // Task 9 types closures and seeds captures; the inner
                // body is walked now so the table covers its arena.
                let mut inner = Environment::new();
                self.statements_nested(&body, &mut inner);
                TypeId::mixed(db)
            }
            BodyExpression::ArrowFunction { body, .. } => {
                let mut inner = environment.clone();
                let _ = self.expression_nested(body, &mut inner);
                TypeId::mixed(db)
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
                    .map(|subject| self.subject_type(environment, subject))
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
                self.expression_value(condition, environment);
                if let Some((subject, target)) = self.type_check_facts(condition) {
                    let current = self.subject_type(environment, &subject);
                    let mut when_true = environment.clone();
                    let mut when_false = environment.clone();
                    when_true.bind(subject.clone(), self.narrowed_to(current, target));
                    when_false.bind(subject, self.removed_type(current, target));
                    return (when_true, when_false);
                }
                let subject = subject_of(ir, condition);
                let current = subject
                    .as_ref()
                    .map(|subject| self.subject_type(environment, subject))
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

    /// Walks a nested body (closure) without contributing its
    /// `return` statements to the enclosing body's return type.
    fn statements_nested(&mut self, list: &[StatementId], environment: &mut Environment<'db>) {
        let saved_returns = std::mem::take(&mut self.returns);
        let saved_yield = self.saw_yield;
        self.statements(list, environment);
        self.returns = saved_returns;
        self.saw_yield = saved_yield;
    }

    fn expression_nested(
        &mut self,
        id: ExpressionId,
        environment: &mut Environment<'db>,
    ) -> TypeId<'db> {
        let saved_returns = std::mem::take(&mut self.returns);
        let saved_yield = self.saw_yield;
        let of = self.expression(id, environment);
        self.returns = saved_returns;
        self.saw_yield = saved_yield;
        of
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
                environment.bind(subject, TypeId::mixed(db));
            }
            if let Some(subject) = subject_of(self.context.ir, _value) {
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
                    environment.bind(base, updated);
                }
            }
            _ => {
                if let Some(subject) = subject_of(self.context.ir, target) {
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
