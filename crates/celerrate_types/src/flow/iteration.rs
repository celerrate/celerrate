//! The iteration protocol (design section 6): what `foreach` yields —
//! array shapes and lists by their parts, `iterable<K, V>` and
//! `Traversable` by their arguments, `Iterator`/`IteratorAggregate`
//! implementors by their `current()`/`key()` signatures.

use super::*;

impl<'db> Walker<'db, '_, '_> {
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
    pub(super) fn iteration_types(
        &mut self,
        subject: TypeId<'db>,
        depth: u32,
    ) -> (TypeId<'db>, TypeId<'db>) {
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
    ///
    /// `linearized_class` itself answers `None` for `name` when `name`
    /// has no SOURCE declaration at all — `linearize.rs`'s root fetch
    /// requires one, by design (plan 6). A genuine stub receiver with
    /// no user subclass in between (`new \ArrayIterator(...)`) falls
    /// into exactly that gap: it is never the ROOT of any source
    /// ancestry walk, so `ancestry`/`stub_ancestors` never exist for it
    /// to check. `stub_ancestors_of` covers that case by walking
    /// `name`'s own compiled surface — the SAME shared stub-frontier
    /// walk (`celerrate_semantics`' `stub_frontier`) that produced the
    /// `stub_ancestors` the arm above reads, so both arms mean the same
    /// thing by construction: only ANCESTORS implement the protocol,
    /// never `name` itself.
    fn implements_iteration_protocol(&mut self, name: &str) -> bool {
        let db = self.db();
        // Task 3: the walker's own direct `linearized_class`
        // consultation site (the iteration-protocol category).
        self.dependencies.classes.insert(name.to_owned());
        match linearized_class(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            ClassQuery::new(db, name.to_owned()),
        )
        .as_ref()
        {
            Some(linearized) => {
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
            None => stub_ancestors_of(stub_signature_table(db, self.context.stubs), name)
                .reached
                .iter()
                .any(|key| Self::ITERATION_PROTOCOL.contains(&key.as_str())),
        }
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
}
