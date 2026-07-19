//! The member boundary: `member_boundary_type` is the ONE funnel
//! every member read, method call, callable projection, and `new`
//! result passes through (issue #39). It is where a member's declared
//! type crosses from its defining class into the receiver's context:
//! placeholders are substituted there, once, against the receiver's
//! solved arguments. Calling `crate::substitution` from anywhere else
//! in `flow/` needs a justification against this invariant and a
//! deliberate entry in the allowlist of
//! `tests/substitution_funnel_guard.rs`.

use super::*;

impl<'db> Walker<'db, '_, '_> {
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
    pub(super) fn member_value_type(
        &mut self,
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
    pub(super) fn method_signatures(
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
    pub(super) fn declared_present(&self, signature: &DeclaredSignature<'db>) -> bool {
        signature.value_trust != Trust::NativeOnly || !signature.value_type.is_mixed(self.db())
    }

    /// Every member boundary funnels through here (decision 1): the
    /// declared or inferred member type is resolved against the
    /// declaring `owner` and the `receiver` — late-static-binding
    /// placeholders substitute (`self` → owner, `parent` → the owner's
    /// first `Extends` ancestor, `static` → the receiver, which may
    /// itself be a placeholder and forward, decision 2) — and the
    /// receiver's class arguments bind its class-level templates.
    pub(super) fn member_boundary_type(
        &mut self,
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
    pub(super) fn member_owner(
        &mut self,
        key: &str,
        kind: MemberKind,
        name: &str,
    ) -> Option<String> {
        let db = self.db();
        // Task 3: this is the walker's own direct `lookup_member`
        // consultation site — recorded here, regardless of whether it
        // resolves, so every caller (receiver resolution, property
        // types, the iteration-protocol members) shares one recording
        // point.
        self.dependencies.classes.insert(key.to_owned());
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
    pub(super) fn scope_keyword_class(&self, name: &str) -> Option<TypeId<'db>> {
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

    /// The first `extends` edge of the defining class's ancestry (the
    /// body owner's own class — one of task 3's four illustrative
    /// consultation categories, recorded through
    /// [`Self::parent_class_key_of`]).
    pub(super) fn parent_class_key(&mut self) -> Option<String> {
        let owner = self.context.owner_class_key.clone()?;
        self.parent_class_key_of(&owner)
    }

    /// The first `extends` edge of `class_key`'s own ancestry — the
    /// same walk [`Self::parent_class_key`] runs for the defining
    /// class, generalized so `member_boundary_type` can resolve a
    /// `parent` placeholder against any declaring owner, not only the
    /// body's own.
    pub(super) fn parent_class_key_of(&mut self, class_key: &str) -> Option<String> {
        let db = self.db();
        // Task 3: the walker's own direct `linearized_class`
        // consultation site.
        self.dependencies.classes.insert(class_key.to_owned());
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
}
