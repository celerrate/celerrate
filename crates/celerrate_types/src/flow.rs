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
    MemberResolution, StatementId, StringPart, UseTables, folded_member_key, linearized_class,
    lookup_member,
};
use celerrate_stubs::StubIndexInput;
use celerrate_syntax::SyntaxKind;

use crate::declared::{
    DeclaredSignature, Trust, declared_function_signature, declared_member_signature,
};
use crate::inference::InterproceduralEdgeCounts;
use crate::narrowing::{NarrowingSubject, subject_of};
use crate::operators;
use crate::representation::{TypeData, TypeId};
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
    /// `mixed` for a local, the declared type for a property subject
    /// (decision 9's fallback; a dropped or never-narrowed property
    /// still reads as its declaration).
    fn subject_type(
        &self,
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
                self.context
                    .owner_class_key
                    .as_ref()
                    .and_then(|key| {
                        self.member_value_type(
                            std::slice::from_ref(key),
                            MemberKind::Property,
                            name,
                            self.current_static_type(),
                        )
                    })
                    .unwrap_or_else(|| TypeId::mixed(db))
            }
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
    fn receiver_parts(&self, of: TypeId<'db>) -> Option<Vec<String>> {
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

    /// A member's declared value across the receiver keys: the
    /// alternatives a union receiver's resolving constituents answer,
    /// preserved with `TypeId::union` rather than widened away — which
    /// constituent the receiver actually is at runtime decides which
    /// answer applies, the same control-flow-alternative shape as a
    /// branch join (see the module's `join`-vs-`union` convention);
    /// `None` when no key answers. Every resolving key's value funnels
    /// through `member_boundary_type` against its *own* declaring
    /// owner and `receiver` (decision 1): a `self`/`static`/`parent`-
    /// typed property or constant substitutes exactly like a method
    /// return. The owner is resolved per key, not once for the whole
    /// call — a union receiver `A|B` where both declare the member
    /// answers each key against its own class, never both against
    /// whichever key happened to resolve first (Finding 3).
    fn member_value_type(
        &self,
        keys: &[String],
        kind: MemberKind,
        name: &str,
        receiver: TypeId<'db>,
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
                .map(|signature| {
                    let owner = self.member_owner(key, kind, name);
                    self.member_boundary_type(signature.value_type, owner.as_deref(), receiver)
                })
            })
            .reduce(|left, right| TypeId::union(db, [left, right]))
    }

    /// The declared method signatures across the receiver keys, each
    /// paired with the key that resolved it (some keys in `keys` may
    /// not resolve at all, so a plain positional zip against `keys`
    /// would misalign once that happens — the pairing keeps a
    /// per-key owner lookup honest), in key order (empty when none
    /// resolves).
    fn method_signatures(
        &self,
        keys: &[String],
        name: &str,
    ) -> Vec<(String, DeclaredSignature<'db>)> {
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
                .map(|signature| (key.clone(), signature))
            })
            .collect()
    }

    /// The declared-present gate (decision 4).
    fn declared_present(&self, signature: &DeclaredSignature<'db>) -> bool {
        signature.value_trust != Trust::NativeOnly || !signature.value_type.is_mixed(self.db())
    }

    /// Every member boundary funnels through here (decision 1): the
    /// declared or inferred member type is resolved against the
    /// declaring `owner` and the `receiver` — late-static-binding
    /// placeholders substitute (`self` → owner, `parent` → the owner's
    /// first `Extends` ancestor, `static` → the receiver, which may
    /// itself be a placeholder and forward, decision 2) — and the
    /// receiver's class arguments bind its class-level templates.
    fn member_boundary_type(
        &self,
        of: TypeId<'db>,
        owner: Option<&str>,
        receiver: TypeId<'db>,
    ) -> TypeId<'db> {
        let db = self.db();
        let mut map = crate::substitution::Substitution::default();
        if let Some(name) = receiver.class_name(db) {
            let arguments = receiver.class_arguments(db);
            if !arguments.is_empty() {
                let class = ClassQuery::new(db, name.clone());
                let templates = &crate::inheritance::class_annotations(
                    db,
                    self.context.files,
                    self.context.stubs,
                    self.context.configuration,
                    class,
                )
                .templates;
                for (position, template) in templates.iter().enumerate() {
                    if let Some(argument) = arguments.get(position) {
                        map.bind(&name, &template.name, *argument);
                    }
                }
            }
        }
        let resolution = crate::substitution::PlaceholderResolution {
            owner: owner.map(str::to_owned),
            parent: owner.and_then(|key| self.parent_class_key_of(key)),
            receiver: Some(receiver),
        };
        crate::substitution::substitute(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            of,
            &map,
            Some(&resolution),
        )
    }

    /// The declaring owner of `key`'s own member — `self` and `parent`
    /// placeholders substitute against it. A per-key fact, deliberately
    /// not a per-call one (Finding 3): a union receiver's keys may
    /// each declare the member on a different ancestor, so a single
    /// owner hoisted out of a per-key loop and reused for every key
    /// would substitute every key's `self`/`parent` against
    /// whichever key happened to resolve first, a wrong concrete
    /// answer rather than conservative silence.
    ///
    /// Task 7's trait boundary fix, anchored by task 7b: a
    /// `Trait`-origin resolution's `owner` (from `lookup_member`) names
    /// the trait itself — the class that lexically declares the method —
    /// but decision 5 analyzes a trait body *for the using class*, so
    /// PHP's `self` and `parent` inside it are bound to the class that
    /// wrote `use`, not the trait. Substituting against the trait here
    /// would silently answer the wrong concrete class (the trait) for
    /// every using class alike, rather than conservative silence:
    /// `SelfPlaceholder`/`ParentPlaceholder` substitution
    /// (`substitution.rs`) has no scope key to fall back through the
    /// way `Template` does, so an untrue owner is not a safe default.
    ///
    /// The using class is the origin's `anchor`, not `key`: they
    /// coincide only for a direct use. Queried through a subclass of the
    /// user, or through a chain of traits using traits, `key` is the
    /// subclass — answering it would trade the trait's wrong concrete
    /// class for the receiver's, since `self` in a trait does not follow
    /// late static binding. Only the linearization knows which class the
    /// trait was pasted into, so it carries the answer here.
    fn member_owner(&self, key: &str, kind: MemberKind, name: &str) -> Option<String> {
        let db = self.db();
        let query = MemberQuery::new(db, key.to_owned(), kind, folded_member_key(kind, name));
        match lookup_member(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            query,
        ) {
            Some(MemberResolution::Source {
                origin: MemberOrigin::Trait { anchor },
                ..
            }) => Some(anchor),
            Some(
                MemberResolution::Source { owner, .. }
                | MemberResolution::Stub { owner, .. }
                | MemberResolution::Virtual { owner, .. },
            ) => Some(owner),
            None => None,
        }
    }

    /// The class a `self`/`static`/`parent` keyword names in the
    /// defining context (decision 1): each carries its own
    /// late-static-binding placeholder rather than an immediately
    /// resolved class — `None` for any other name (an ordinary class,
    /// which the caller resolves through `class_type_of_written`). An
    /// absent owner answers `mixed`, never a bogus qualified
    /// `self`/`parent` class. No static-method gate: the
    /// `self`/`static`/`parent` *class keyword* is available in a
    /// static method, only the `$this` *value* is not
    /// ([`Self::this_type`]'s gate).
    fn scope_keyword_class(&self, name: &str) -> Option<TypeId<'db>> {
        let db = self.db();
        let has_owner = self.context.owner_class_key.is_some();
        match name.to_ascii_lowercase().as_str() {
            "self" => Some(if has_owner {
                TypeId::self_placeholder(db)
            } else {
                TypeId::mixed(db)
            }),
            "static" => Some(self.current_static_type()),
            "parent" => Some(if has_owner {
                TypeId::parent_placeholder(db)
            } else {
                TypeId::mixed(db)
            }),
            _ => None,
        }
    }

    /// The first `extends` edge of the defining class's ancestry.
    fn parent_class_key(&self) -> Option<String> {
        let owner = self.context.owner_class_key.as_ref()?;
        self.parent_class_key_of(owner)
    }

    /// The first `extends` edge of `class_key`'s own ancestry — the
    /// same walk [`Self::parent_class_key`] runs for the defining
    /// class, generalized so `member_boundary_type` can resolve a
    /// `parent` placeholder against any declaring owner, not only the
    /// body's own.
    fn parent_class_key_of(&self, class_key: &str) -> Option<String> {
        let db = self.db();
        let linearized = linearized_class(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            ClassQuery::new(db, class_key.to_owned()),
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

    /// Decision 10: any call, instantiation, closure creation,
    /// `yield`, `eval`, `include`, or shell-exec may run arbitrary
    /// code that rewrites object state: every property binding dies.
    /// Over-killing is the conservative direction — a dropped binding
    /// reads as the declared type (`subject_type`'s fallback). Locals
    /// survive: they are not addressable through arbitrary aliasing
    /// the way `$this`/`self::` state is.
    fn kill_property_bindings(&mut self, environment: &mut Environment<'db>) {
        for subject in environment.subjects() {
            if !matches!(subject, NarrowingSubject::Local { .. }) {
                environment.remove(&subject);
            }
        }
    }

    /// The by-reference rules (design section 6): an argument bound to
    /// a by-reference parameter takes the parameter's declared type
    /// after the call (the general write-back; plan 7's stdlib
    /// provider refines `$matches`), which also invalidates its
    /// narrowing. Named labels resolve by name; a spread ends the
    /// positional mapping (conservative).
    fn apply_by_reference(
        &mut self,
        parameters: &[crate::declared::DeclaredParameter<'db>],
        arguments: &[celerrate_semantics::CallArgument],
        environment: &mut Environment<'db>,
    ) {
        let db = self.db();
        for (index, argument) in arguments.iter().enumerate() {
            if argument.spread {
                break;
            }
            let parameter = match &argument.label {
                Some(label) => parameters.iter().find(|parameter| parameter.name == *label),
                None => parameters
                    .get(index)
                    .or_else(|| parameters.last().filter(|parameter| parameter.variadic)),
            };
            let Some(parameter) = parameter else {
                continue;
            };
            if !parameter.by_reference {
                continue;
            }
            if let Some(subject) = subject_of(self.context.ir, argument.value) {
                environment.bind(
                    subject,
                    parameter
                        .parameter_type
                        .unwrap_or_else(|| TypeId::mixed(db)),
                );
            }
        }
    }

    /// The (declared parameter type, argument type) pairs the call-site
    /// solver (task 8, decision 10) matches constraints against. The
    /// same alignment `apply_by_reference` and `apply_call_assertions`
    /// already use: a labeled argument matches the parameter of that
    /// name; an unlabeled argument matches positionally by its own
    /// index in `arguments`, falling to the last parameter when it is
    /// variadic (surplus positional arguments); a spread argument ends
    /// alignment (conservative — its element-wise contents are
    /// unknown without evaluating it). A parameter with no declared
    /// type (`None`, the stub-range degenerate case) contributes no
    /// pair, silently.
    fn solver_pairs(
        &self,
        parameters: &[crate::declared::DeclaredParameter<'db>],
        arguments: &[celerrate_semantics::CallArgument],
        types: &[TypeId<'db>],
    ) -> Vec<(TypeId<'db>, TypeId<'db>)> {
        let mut pairs = Vec::new();
        for (index, argument) in arguments.iter().enumerate() {
            if argument.spread {
                break;
            }
            let Some(argument_type) = types.get(index).copied() else {
                continue;
            };
            let parameter = match &argument.label {
                Some(label) => parameters.iter().find(|parameter| parameter.name == *label),
                None => parameters
                    .get(index)
                    .or_else(|| parameters.last().filter(|parameter| parameter.variadic)),
            };
            let Some(parameter) = parameter else {
                continue;
            };
            let Some(declared_type) = parameter.parameter_type else {
                continue;
            };
            pairs.push((declared_type, argument_type));
        }
        pairs
    }

    /// Solves any template still present in `result` from the call's
    /// (declared parameter, argument) pairs, then finalizes whatever
    /// the solver left unbound to its bound, then `mixed` (task 8,
    /// decision 10). The provider tier needs no special exemption here:
    /// a provider answers a concrete type, already widened at the
    /// consumption boundary, so `contains_symbolic` is already false
    /// for it and this is a costless no-op. Skipped entirely when
    /// `result` carries nothing symbolic, so a call with an ordinary,
    /// template-free signature never pays for pair alignment.
    fn solved_call_result(
        &self,
        result: TypeId<'db>,
        parameters: &[crate::declared::DeclaredParameter<'db>],
        arguments: &[celerrate_semantics::CallArgument],
        argument_types: &[TypeId<'db>],
    ) -> TypeId<'db> {
        let db = self.db();
        if !crate::substitution::contains_symbolic(db, result) {
            return result;
        }
        let pairs = self.solver_pairs(parameters, arguments, argument_types);
        let solved = crate::solver::solve(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            &pairs,
        );
        let result = crate::substitution::substitute(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            result,
            &solved,
            None,
        );
        crate::solver::finalize_return(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            result,
        )
    }

    /// Constructor inference (decision 11, task 9): `new Foo(...)`
    /// solves `Foo`'s class-level templates from its `__construct`
    /// parameters, reusing `solver_pairs`/`solve` exactly as an
    /// ordinary call does — the constructor is simply the call whose
    /// result is the class itself rather than a declared return.
    /// `of` is the plain class already resolved for `class` (a
    /// concrete class name, not a placeholder — `new self()`/`new
    /// static()` resolved to their owner before this runs); returned
    /// unchanged when it names no class, `class_annotations` finds no
    /// `@template`, or the call passes no arguments.
    ///
    /// Bound under the class's *own* scope key — `class_annotations`'s
    /// own convention (the bare class key, no member suffix) — because
    /// that is exactly the scope `member_boundary_type`'s
    /// receiver-argument zip binds a receiver's `class_arguments`
    /// against later (it keys on the receiver's class name alone);
    /// solving under any other scope (the constructor's own
    /// `<class>::__construct` member scope, say) would silently never
    /// match there, and every solved argument would fall through to
    /// its bound then `mixed`.
    ///
    /// The `any_bound` guard keeps an unconstrained `new Box()` the
    /// plain class `of` already is, rather than minting a `Box<mixed>`
    /// spelling of the same thing — the canonical receiver everywhere
    /// else in the corpus.
    fn constructor_solved_class(
        &self,
        of: TypeId<'db>,
        arguments: &[celerrate_semantics::CallArgument],
        argument_types: &[TypeId<'db>],
    ) -> TypeId<'db> {
        let db = self.db();
        if arguments.is_empty() {
            return of;
        }
        let Some(key) = of.class_name(db) else {
            return of;
        };
        let templates = &crate::inheritance::class_annotations(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            ClassQuery::new(db, key.clone()),
        )
        .templates;
        if templates.is_empty() {
            return of;
        }
        let Some(signature) = declared_member_signature(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            MemberQuery::new(
                db,
                key.clone(),
                MemberKind::Method,
                folded_member_key(MemberKind::Method, "__construct"),
            ),
        ) else {
            return of;
        };
        let pairs = self.solver_pairs(&signature.parameters, arguments, argument_types);
        let solved = crate::solver::solve(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            &pairs,
        );
        let mut any_bound = false;
        let arguments: Vec<TypeId<'db>> = templates
            .iter()
            .map(|template| match solved.binding(&key, &template.name) {
                Some(argument) => {
                    any_bound = true;
                    argument
                }
                None => template.bound.unwrap_or_else(|| TypeId::mixed(db)),
            })
            .collect();
        if any_bound {
            TypeId::class(db, &key, arguments)
        } else {
            of
        }
    }

    /// The named inline `@var` entries (decision 11) one docblock text
    /// parses to, at the body's own declaring scope: `Collection<User>
    /// $c` and the like, read straight from `ParsedAnnotations.variables`
    /// — never from trivia or a syntax tree, `text` already came from
    /// `BodyIr.annotations`. Mirrors `member_annotations`'s own site
    /// construction (`declared.rs`), except the declaring scope is the
    /// body's own (`FlowContext::scope_key`), not a member looked up
    /// fresh by key.
    fn inline_variables(&self, text: &str) -> Vec<(String, TypeId<'db>)> {
        let db = self.db();
        let tables = &self.context.tables;
        let site = crate::declared::NameSite::Source {
            namespace: &self.context.namespace,
            tables,
        };
        let owner_docblock =
            self.context.owner_class_key.as_deref().and_then(|owner| {
                crate::declared::owner_class_docblock(db, self.context.files, owner)
            });
        let context = crate::type_syntax::AnnotationContext {
            declaring_scope: &self.context.scope_key,
            enclosing_class_scope: self.context.owner_class_key.as_deref(),
            enclosing_class_docblock: owner_docblock.as_deref(),
        };
        crate::type_syntax::annotations_for_docblock(db, &site, &context, text).variables
    }

    /// Binds every named inline `@var` entry anchored at `anchor` into
    /// `environment` (decision 11): `None` for a body-entry
    /// declaration (bound once, before any statement), `Some(id)` for
    /// one anchored to statement `id` — the caller applies this both
    /// immediately before and immediately after walking that
    /// statement, so the declaration survives the statement's own
    /// assignment.
    fn bind_inline_variables(
        &self,
        anchor: Option<StatementId>,
        environment: &mut Environment<'db>,
    ) {
        let Some(texts) = self.inline_variable_texts.get(&anchor).cloned() else {
            return;
        };
        for text in texts {
            for (name, of) in self.inline_variables(text) {
                environment.bind(NarrowingSubject::Local { name }, of);
            }
        }
    }

    /// Applies a callee's assertion tags at this call site (decision
    /// 17): `$name` subjects map through the declared parameters to
    /// the argument's subject; `$this->name` maps to the caller's
    /// property subject when the receiver is the caller's `$this`;
    /// other subject shapes are ignored (recorded). `Always` applies
    /// now; `IfTrue`/`IfFalse` queue for the condition consumer.
    fn apply_call_assertions(
        &mut self,
        call: ExpressionId,
        assertions: &[crate::type_syntax::ParsedAssertion<'db>],
        parameters: &[crate::declared::DeclaredParameter<'db>],
        receiver_is_this: bool,
        arguments: &[celerrate_semantics::CallArgument],
        environment: &mut Environment<'db>,
    ) {
        use crate::type_syntax::AssertionPolarity;
        for assertion in assertions {
            let subject = if let Some(property) = assertion.subject.strip_prefix("$this->") {
                receiver_is_this.then(|| NarrowingSubject::ThisProperty {
                    name: property.to_owned(),
                })
            } else if let Some(name) = assertion.subject.strip_prefix('$') {
                let position = parameters
                    .iter()
                    .position(|parameter| parameter.name == name);
                position.and_then(|position| {
                    // A named argument matches by label regardless of
                    // order; an unlabeled positional match halts at the
                    // first spread, exactly as `apply_by_reference`
                    // does ("a spread ends the positional mapping").
                    let argument =
                        arguments
                            .iter()
                            .enumerate()
                            .find_map(|(index, argument)| match &argument.label {
                                Some(label) if label == name => Some(argument),
                                Some(_) => None,
                                None if argument.spread => None,
                                None if index == position => Some(argument),
                                None => None,
                            });
                    // Reject a positional match that a preceding spread
                    // has already invalidated (labeled matches survive).
                    let argument = argument.filter(|argument| {
                        argument.label.is_some()
                            || !arguments
                                .iter()
                                .take(position)
                                .any(|earlier| earlier.spread)
                    })?;
                    subject_of(self.context.ir, argument.value)
                })
            } else {
                None
            };
            let Some(subject) = subject else {
                continue;
            };
            match assertion.polarity {
                AssertionPolarity::Always => {
                    let current = self.subject_type(environment, &subject);
                    let narrowed = if assertion.negated {
                        self.removed_type(current, assertion.asserted)
                    } else {
                        self.narrowed_to(current, assertion.asserted)
                    };
                    environment.bind(subject, narrowed);
                }
                AssertionPolarity::IfTrue | AssertionPolarity::IfFalse => {
                    self.pending_condition_facts.push(PendingAssertion {
                        origin: call,
                        subject,
                        asserted: assertion.asserted,
                        polarity: assertion.polarity,
                        negated: assertion.negated,
                    });
                }
            }
        }
    }

    /// The folded Function-space key a written callee resolves to,
    /// and whether a source declaration exists (the inferred-return
    /// gate: only source bodies can be inferred). Mirrors the
    /// reference checks' own resolution (`resolve_name`): the
    /// namespaced spelling first, the global fallback last, the first
    /// existing candidate wins (source, then stubs), the last
    /// candidate as the never-resolves fallback (so a provider claim
    /// on an undeclared helper still matches a deterministic key).
    /// Every candidate is folded before the lookup: `SymbolQuery`'s key
    /// (like `FunctionQuery`'s) is pre-folded, and `resolve_candidates`
    /// itself answers case-preserved spellings.
    fn resolved_function_key(&self, written: &str) -> (String, bool) {
        let db = self.db();
        let candidates = celerrate_semantics::resolve_candidates(
            written,
            celerrate_semantics::SymbolSpace::Function,
            &self.context.namespace,
            &self.context.tables,
        );
        let folded: Vec<String> = candidates
            .iter()
            .map(|candidate| {
                celerrate_semantics::folded_symbol_key(
                    celerrate_semantics::SymbolSpace::Function,
                    candidate,
                )
            })
            .collect();
        for key in &folded {
            let query = celerrate_semantics::SymbolQuery::new(
                db,
                celerrate_semantics::SymbolSpace::Function,
                key.clone(),
            );
            if celerrate_semantics::lookup_function_declaration(db, self.context.files, query)
                .is_some()
            {
                return (key.clone(), true);
            }
        }
        for key in &folded {
            if celerrate_semantics::stub_symbol_table(
                db,
                self.context.stubs,
                self.context.configuration,
            )
            .lookup(celerrate_semantics::SymbolSpace::Function, key)
            .is_some()
            {
                return (key.clone(), false);
            }
        }
        (
            folded
                .into_iter()
                .last()
                .unwrap_or_else(|| written.trim_start_matches('\\').to_ascii_lowercase()),
            false,
        )
    }

    /// Decision 3, first tier: a provider that claims the callee
    /// answers, widened at the consumption boundary — a plugin never
    /// controls termination.
    fn provider_return(
        &mut self,
        claim: crate::dynamic_type_provider::SymbolClaim,
        receiver_type: Option<TypeId<'db>>,
        argument_types: &[TypeId<'db>],
    ) -> Option<TypeId<'db>> {
        let db = self.db();
        let registry = crate::dynamic_type_provider::DynamicTypeProviderRegistry::try_get(db)?;
        for registration in registry.registrations(db) {
            if !registration.provider.claims().contains(&claim) {
                continue;
            }
            let invocation = crate::dynamic_type_provider::Invocation {
                claim: claim.clone(),
                receiver_type,
                argument_types: argument_types.to_vec(),
            };
            if let Some(answer) = registration.provider.return_type(db, &invocation) {
                self.edge_counts.provider_edges += 1;
                return Some(crate::widening::capped_child(db, answer));
            }
        }
        None
    }

    /// Decision 3, tiers two and three, for a named function call.
    /// Task 10 replaces the `mixed` fallback with the fixpoint.
    fn function_call_result(&mut self, key: &str, source_exists: bool) -> TypeId<'db> {
        let db = self.db();
        let declared = declared_function_signature(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            crate::declared::FunctionQuery::new(db, key.to_owned()),
        );
        if let Some(signature) = &declared
            && self.declared_present(signature)
        {
            self.edge_counts.declared_return_edges += 1;
            return signature.value_type;
        }
        if source_exists {
            self.edge_counts.inferred_return_edges += 1;
            return crate::inference::inferred_function_return(
                db,
                self.context.files,
                self.context.stubs,
                self.context.configuration,
                crate::declared::FunctionQuery::new(db, key.to_owned()),
            );
        }
        TypeId::mixed(db)
    }

    /// Task 9(f): a provider consulted for a method call before the
    /// declared tier, when the receiver resolves to exactly one key
    /// (an ambiguous union receiver never reaches a provider — the
    /// same conservative stance the by-reference write-back channel
    /// already takes). A `Some` answer wins the call outright; the
    /// declared-tier edge is not also counted.
    fn method_call_result_for_keys_with_provider(
        &mut self,
        keys: &[String],
        receiver: TypeId<'db>,
        name: &str,
        argument_types: &[TypeId<'db>],
    ) -> (TypeId<'db>, Option<DeclaredSignature<'db>>) {
        if keys.len() == 1
            && let Some(class_key) = keys.first().cloned()
        {
            let method_key = folded_member_key(MemberKind::Method, name);
            if let Some(answer) = self.provider_return(
                crate::dynamic_type_provider::SymbolClaim::Method {
                    class_key,
                    method_key,
                },
                Some(receiver),
                argument_types,
            ) {
                let mut signatures = self.method_signatures(keys, name);
                let signature = (signatures.len() == 1).then(|| signatures.remove(0).1);
                return (answer, signature);
            }
        }
        self.method_call_result_for_keys(keys, receiver, name)
    }

    /// An instance method call's result on a resolved receiver type,
    /// with the single resolved signature when exactly one receiver
    /// key answered (the by-reference write-back channel; `None` for
    /// an opaque or union receiver — conservative, recorded).
    /// Provider-aware: resolves the receiver's keys itself.
    fn method_call_result_with_provider(
        &mut self,
        receiver: TypeId<'db>,
        name: &str,
        argument_types: &[TypeId<'db>],
    ) -> (TypeId<'db>, Option<DeclaredSignature<'db>>) {
        match self.receiver_parts(receiver) {
            Some(keys) => self.method_call_result_for_keys_with_provider(
                &keys,
                receiver,
                name,
                argument_types,
            ),
            None => (TypeId::mixed(self.db()), None),
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
    /// expected `"int|string"`). The gate failing on a key drops that
    /// key to the method-inferred tier (decision 3's fourth tier,
    /// `inferred_method_return`): the callee's body answers, and
    /// `mixed` remains only when even that is silent.
    /// Placeholders substitute against each signature's *own*
    /// declaring owner and the receiver through `member_boundary_type`
    /// (decision 1) — the owner is resolved per key, not once for the
    /// whole call: for a union receiver `A|B` where both declare the
    /// member, a hoisted single owner would substitute both keys'
    /// `self` against whichever key resolved first, e.g. `app\a|app\a`
    /// instead of `app\a|app\b` — a wrong concrete answer, not the
    /// conservative silence the design otherwise holds to (Finding 3).
    fn method_call_result_for_keys(
        &mut self,
        keys: &[String],
        receiver: TypeId<'db>,
        name: &str,
    ) -> (TypeId<'db>, Option<DeclaredSignature<'db>>) {
        let db = self.db();
        let signatures = self.method_signatures(keys, name);
        if signatures.is_empty() {
            return (TypeId::mixed(db), None);
        }
        let mut result: Option<TypeId<'db>> = None;
        let mut any_declared = false;
        for (key, signature) in &signatures {
            let value = if self.declared_present(signature) {
                any_declared = true;
                let owner = self.member_owner(key, MemberKind::Method, name);
                self.member_boundary_type(signature.value_type, owner.as_deref(), receiver)
            } else {
                // Decision 3's fourth tier: no usable declared return,
                // so the callee's own body answers, through the
                // fixpoint. The result is a *body-relative* type — its
                // `self`/`static` placeholders and the owner's class
                // templates are unresolved — so it funnels through the
                // one member boundary exactly like a declared return
                // does (decision 1), against this key's own declaring
                // owner and the call's receiver.
                self.edge_counts.inferred_return_edges += 1;
                let method = crate::inference::MethodQuery::new(
                    db,
                    key.clone(),
                    folded_member_key(MemberKind::Method, name),
                );
                let inferred = crate::inference::inferred_method_return(
                    db,
                    self.context.files,
                    self.context.stubs,
                    self.context.configuration,
                    method,
                );
                let owner = self.member_owner(key, MemberKind::Method, name);
                self.member_boundary_type(inferred, owner.as_deref(), receiver)
            };
            result = Some(match result {
                Some(previous) => TypeId::union(db, [previous, value]),
                None => value,
            });
        }
        if any_declared {
            self.edge_counts.declared_return_edges += 1;
        }
        let of = result.unwrap_or_else(|| TypeId::mixed(db));
        // Exactly one receiver key: the signature is unambiguous, the
        // by-reference write-back channel a caller may use. A union
        // receiver's differing signatures write back nothing.
        let signature = if keys.len() == 1 {
            signatures
                .into_iter()
                .next()
                .map(|(_, signature)| signature)
        } else {
            None
        };
        (of, signature)
    }

    /// The protocol interfaces the iteration chain recognizes by
    /// their folded, unqualified name: `Generator`, `Iterator`,
    /// `IteratorAggregate`, `Traversable`. Every name in this crate's
    /// class-key convention is folded lowercase with no leading
    /// separator (`construction.rs`'s `class_names_fold_at_construction`),
    /// so a root-namespace `\Iterator` folds to exactly `"iterator"`.
    const ITERATION_PROTOCOL: [&'static str; 4] =
        ["generator", "iterator", "iteratoraggregate", "traversable"];

    /// Element and key types through the iteration protocol chain
    /// (decision 12). Precedence per subject constituent: array forms
    /// answer directly (a shape's key/value are eager over
    /// `TypeId::key_of`/`value_of`, an array carries them already); a
    /// class carrying two or more arguments whose own name is one of
    /// the protocol interfaces answers them directly (`Generator<K,
    /// V>`, `Iterator<K, V>`, ...); otherwise a class funnels through
    /// `class_iteration_types` (the `getIterator` unwrap, the
    /// threaded-ancestor-arguments-else-`current`/`key` split); a
    /// union joins its constituents, skipping `null` and `false`
    /// (iterating them yields nothing, so they contribute no
    /// alternative); a template recurses through its bound. Everything
    /// else — including a plain object, whose property iteration is a
    /// recorded stance — answers `mixed`. `depth` is the recursion
    /// guard (capped at 8, decision 12): the `IteratorAggregate`
    /// unwrap is the only arm that can recurse on a *different*
    /// subject through a class's own declared return, so it is the one
    /// the guard exists to bound — a `getIterator` returning `$this`,
    /// or two classes whose `getIterator`s return each other, would
    /// otherwise recurse forever.
    fn iteration_types(&mut self, subject: TypeId<'db>, depth: u32) -> (TypeId<'db>, TypeId<'db>) {
        let db = self.db();
        let mixed = (TypeId::mixed(db), TypeId::mixed(db));
        if depth > 8 {
            return mixed;
        }
        match subject.data(db) {
            TypeData::Array { key, value, .. } => (*key, *value),
            TypeData::Shape { .. } => (TypeId::key_of(db, subject), TypeId::value_of(db, subject)),
            TypeData::Union { constituents } => {
                let mut keys = Vec::new();
                let mut values = Vec::new();
                for constituent in constituents {
                    if matches!(
                        constituent.data(db),
                        TypeData::Null
                            | TypeData::Bool {
                                literal: Some(false)
                            }
                    ) {
                        continue;
                    }
                    let (key, value) = self.iteration_types(*constituent, depth + 1);
                    keys.push(key);
                    values.push(value);
                }
                if values.is_empty() {
                    return mixed;
                }
                (TypeId::union(db, keys), TypeId::union(db, values))
            }
            TypeData::Template { bound, .. } => self.iteration_types(*bound, depth + 1),
            TypeData::Class { name, arguments } => {
                self.class_iteration_types(name, arguments, subject, depth)
            }
            _ => mixed,
        }
    }

    /// Whether `name`'s linearized ancestry genuinely resolves an
    /// `implements`/`extends` edge to one of the iteration protocol
    /// interfaces (`Iterator`, `IteratorAggregate`, `Traversable`).
    /// Decision 12 gates the `getIterator` unwrap and the
    /// `current`/`key` fallback on the class actually implementing the
    /// protocol — a `getIterator()` helper or a `current()`/`key()`
    /// pair declared on a class that implements nothing is not
    /// iterable in PHP; the language falls back to plain property
    /// iteration, decision 12's `mixed`/`mixed` default. Answering the
    /// method's element type there would be a guessed concrete answer
    /// where the spec mandates conservative silence.
    ///
    /// `ancestor_arguments` (task 3) is not usable for this check: it
    /// only records an ancestor when the target's own docblock threads
    /// at least one fixed argument (`inheritance.rs`'s `if
    /// !fixed.is_empty()`), so a class implementing `\Iterator` with no
    /// generic threading at all is silently absent from its answer.
    /// `linearized_class`'s `ancestry` records every edge the walk
    /// crosses regardless of whether it carries arguments, so it is
    /// the only query that can answer "implements", full stop. An edge
    /// resolves through either `resolved` (a source class-like) or
    /// `stub` (a compiled one) — exactly one is `Some` on a resolved
    /// edge (`AncestorEdge`'s own invariant).
    ///
    /// `ancestry` alone still misses a whole class of real implementors,
    /// though: the linearization walk only ever pushes a stub ancestor's
    /// *direct* edge there (`linearize.rs`'s `AncestorAnswer::Stub` arm),
    /// then expands the transitive stub frontier — the compiled parents
    /// a stub class-like's own surface names — into the separate
    /// `stub_ancestors` field. A class that `extends \ArrayObject`
    /// records only `ancestry = [edge{stub: "arrayobject"}]`; the fact
    /// that `ArrayObject` itself implements `IteratorAggregate` lives
    /// only in `stub_ancestors`, already folded, sorted, and deduped by
    /// the same walk. Checking both fields is what makes this query
    /// answer "implements" for a stub-inherited protocol, not just a
    /// directly-named one.
    fn implements_iteration_protocol(&self, name: &str) -> bool {
        let db = self.db();
        let Some(linearized) = linearized_class(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            ClassQuery::new(db, name.to_owned()),
        )
        .as_ref() else {
            return false;
        };
        linearized.ancestry.iter().any(|edge| {
            edge.resolved
                .as_deref()
                .or(edge.stub.as_deref())
                .is_some_and(|key| Self::ITERATION_PROTOCOL.contains(&key))
        }) || linearized
            .stub_ancestors
            .iter()
            .any(|key| Self::ITERATION_PROTOCOL.contains(&key.as_str()))
    }

    /// The class arm of [`Self::iteration_types`]: the protocol's own
    /// two arguments when `name` is itself one of the interfaces; else,
    /// for a genuine implementor (guarded by
    /// [`Self::implements_iteration_protocol`]), a `getIterator`
    /// (declared or inherited) unwraps its declared-or-inferred return
    /// recursively — the standard method result path
    /// (`method_call_result_for_keys`), substitution included, exactly
    /// as any other call answers, so `self`/`static` and the
    /// receiver's class arguments resolve the same way a direct call
    /// site would; else the threaded protocol-ancestor arguments (task
    /// 3's `ancestor_arguments`) when the linearized ancestry actually
    /// composed one for a protocol interface, substituted against
    /// `subject` through `member_boundary_type` (decision 1) exactly
    /// like any other member boundary; else, still gated on genuine
    /// implementation, lacking both, `current`/`key` declared or
    /// inherited answer through the same method result path; else
    /// `mixed` — including a class that merely declares
    /// `getIterator`/`current`/`key` without implementing any protocol
    /// interface at all, whose runtime iteration is plain property
    /// iteration, not this chain.
    fn class_iteration_types(
        &mut self,
        name: &str,
        arguments: &[TypeId<'db>],
        subject: TypeId<'db>,
        depth: u32,
    ) -> (TypeId<'db>, TypeId<'db>) {
        let db = self.db();
        let mixed = (TypeId::mixed(db), TypeId::mixed(db));
        // The protocol interfaces themselves, carrying their
        // arguments: `Generator<int, User>`, `Iterator<string, User>`,
        // ...
        if Self::ITERATION_PROTOCOL.contains(&name)
            && let (Some(key), Some(value)) = (arguments.first(), arguments.get(1))
        {
            return (*key, *value);
        }
        if !self.implements_iteration_protocol(name) {
            return mixed;
        }
        let keys = vec![name.to_owned()];
        // A genuine implementor declaring or inheriting `getIterator`:
        // unwrap its declared-or-inferred return through the standard
        // method result path (substitution included), then recurse
        // under the depth guard.
        if self
            .member_owner(name, MemberKind::Method, "getIterator")
            .is_some()
        {
            let (inner, _) = self.method_call_result_for_keys(&keys, subject, "getIterator");
            return self.iteration_types(inner, depth + 1);
        }
        // Threaded protocol-ancestor arguments:
        // `@implements Iterator<string, User>` composed by task 3.
        let class = ClassQuery::new(db, name.to_owned());
        let threaded = crate::inheritance::ancestor_arguments(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            class,
        )
        .iter()
        .find(|(ancestor, _)| Self::ITERATION_PROTOCOL.contains(&ancestor.as_str()))
        .map(|(_, fixed)| fixed.clone());
        if let Some(fixed) = threaded
            && let (Some(key), Some(value)) = (fixed.first(), fixed.get(1))
        {
            let key = self.member_boundary_type(*key, Some(name), subject);
            let value = self.member_boundary_type(*value, Some(name), subject);
            return (key, value);
        }
        // The `current`/`key` protocol members, declared or inherited
        // by a genuine implementor.
        if self
            .member_owner(name, MemberKind::Method, "current")
            .is_some()
            && self.member_owner(name, MemberKind::Method, "key").is_some()
        {
            let (value, _) = self.method_call_result_for_keys(&keys, subject, "current");
            let (key, _) = self.method_call_result_for_keys(&keys, subject, "key");
            return (key, value);
        }
        mixed
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
                for subject in environment.subjects() {
                    if matches!(subject, NarrowingSubject::Local { .. }) {
                        environment.remove(&subject);
                    }
                }
                self.kill_property_bindings(environment);
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
                        let of = self
                            .provider_return(
                                crate::dynamic_type_provider::SymbolClaim::Function {
                                    key: key.clone(),
                                },
                                None,
                                &argument_types,
                            )
                            .unwrap_or_else(|| self.function_call_result(&key, source_exists));
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
                    // Recorded debt (decision 14): anonymous-class
                    // receivers (`new class { }`) stay `mixed` — no
                    // folded key exists yet for an anonymous class
                    // literal, and the expression-to-key path belongs
                    // with the checks' receiver-resolution surface
                    // (plan 8), not here: no diagnostic consumes
                    // receiver types before plan 8, so building the
                    // path here would ship untested-by-consumer
                    // machinery. `Missing` (a malformed `new` with no
                    // class reference at all) shares the same silent
                    // fallback.
                    ClassReference::Anonymous { .. } | ClassReference::Missing => TypeId::mixed(db),
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
        let return_fallback = if uses_fallback && source_exists {
            self.edge_counts.inferred_return_edges += 1;
            crate::inference::inferred_function_return(
                db,
                self.context.files,
                self.context.stubs,
                self.context.configuration,
                crate::declared::FunctionQuery::new(db, key.to_owned()),
            )
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
