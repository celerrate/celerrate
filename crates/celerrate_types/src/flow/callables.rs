//! Closures and first-class callables: closure/arrow-function typing
//! (including nested-return collection and written-parameter
//! seeding) and the projection of `f(...)` / `$o->m(...)` /
//! `C::m(...)` into callable signatures.

use super::*;

impl<'db> Walker<'db, '_, '_> {
    /// Walks a nested body (a closure or arrow function) with its own
    /// return accumulator, so its `return`/`yield` never leak into the
    /// enclosing body's return type; answers the returns it collected,
    /// whether it yielded, and whether its end is reachable (a
    /// closure's implicit-null fall-through).
    pub(super) fn nested_returns(
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
    pub(super) fn closure_type(
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
    pub(super) fn seed_written_parameters(
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
    pub(super) fn projected_callable_of_function(
        &mut self,
        key: &str,
        source_exists: bool,
    ) -> TypeId<'db> {
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
    pub(super) fn projected_callable_of_method(
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
    pub(super) fn projected_callable_of_keys(
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
}
