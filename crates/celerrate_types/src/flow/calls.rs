//! The call boundary: argument typing, by-reference effects, template
//! solving at the call site, provider consultation, and call-result
//! computation for free functions and methods. `solved_call_result`'s
//! direct substitution call is one of the two deliberate sites in
//! `flow/` (the other is the `flow/boundary.rs` funnel) — see
//! `tests/substitution_funnel_guard.rs`.

use super::*;

/// The folded Function-space key a written callee resolves to, and
/// whether a source declaration exists (the inferred-return gate: only
/// source bodies can be inferred). Mirrors the reference checks' own
/// resolution (`resolve_name`): the namespaced spelling first, the
/// global fallback last, the first existing candidate wins (source,
/// then stubs), the last candidate as the never-resolves fallback (so a
/// provider claim on an undeclared helper still matches a deterministic
/// key). Every candidate is folded before the lookup: `SymbolQuery`'s
/// key (like `FunctionQuery`'s) is pre-folded, and `resolve_candidates`
/// itself answers case-preserved spellings.
///
/// A free function, not a `Walker` method: task 8's argument-type
/// family (`checks::arguments`) resolves a call's `NamedReference`
/// callee through this exact same candidate order, so both call
/// boundaries agree on one implementation rather than two that could
/// silently drift apart.
pub(crate) fn resolved_function_key(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    namespace: &str,
    tables: &UseTables,
    written: &str,
) -> (String, bool) {
    let candidates = celerrate_semantics::resolve_candidates(
        written,
        celerrate_semantics::SymbolSpace::Function,
        namespace,
        tables,
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
        if celerrate_semantics::lookup_function_declaration(db, files, query).is_some() {
            return (key.clone(), true);
        }
    }
    for key in &folded {
        if celerrate_semantics::stub_symbol_table(db, stubs, configuration)
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

impl<'db> Walker<'db, '_, '_> {
    pub(super) fn typed_arguments(
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
    /// the way `$this`/`self::` state is. Call-result fingerprints
    /// survive too: their v1 bases are `$this` and locals (both
    /// call-stable), and their validity is the purity assumption
    /// itself — an intervening call does not undermine "this method
    /// keeps answering the same value" (design
    /// 2026-07-19-call-result-narrowing).
    pub(super) fn kill_property_bindings(&mut self, environment: &mut Environment<'db>) {
        for subject in environment.subjects() {
            if !matches!(
                subject,
                NarrowingSubject::Local { .. } | NarrowingSubject::CallResult { .. }
            ) {
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
    pub(super) fn apply_by_reference(
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
                environment.kill_call_results_for_subject(&subject);
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
    pub(super) fn solver_pairs(
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
    /// decision 10). The provider tier needs no special-cased exemption
    /// here: by convention every `DynamicTypeProvider` answers a
    /// concrete type, already widened at the consumption boundary, so
    /// `contains_symbolic` is already false for it and this is a
    /// costless no-op in the ordinary case. That convention is not
    /// enforced at the trait boundary, though — nothing stops an
    /// implementation from answering something symbolic. Task 11 pins
    /// what happens if one ever does
    /// (`a_symbolic_provider_answer_still_finalizes_to_its_bound_then_mixed`
    /// in `inference.rs`): the same solve/substitute/finalize pipeline
    /// below runs over it exactly as it would an ordinary symbolic
    /// declared return, and an unconstrained template still falls to
    /// its bound then `mixed` — conservative, never a guess, so a
    /// misbehaving provider degrades gracefully rather than leaking a
    /// raw template or crashing. Skipped entirely when `result` carries
    /// nothing symbolic, so a call with an ordinary, template-free
    /// signature never pays for pair alignment.
    pub(super) fn solved_call_result(
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

    /// Applies a callee's assertion tags at this call site (decision
    /// 17): `$name` subjects map through the declared parameters to
    /// the argument's subject; `$this->name` maps to the caller's
    /// property subject when the receiver is the caller's `$this`;
    /// other subject shapes are ignored (recorded). `Always` applies
    /// now; `IfTrue`/`IfFalse` queue for the condition consumer.
    pub(super) fn apply_call_assertions(
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

    /// The folded Function-space key a written callee resolves to, and
    /// whether a source declaration exists. Delegates to
    /// [`resolved_function_key`], the free-function form task 8's
    /// argument-type family (`checks::arguments`) shares, so both the
    /// flow walk's call boundary and the checks layer's callee
    /// resolution agree on exactly one implementation.
    pub(super) fn resolved_function_key(&self, written: &str) -> (String, bool) {
        resolved_function_key(
            self.db(),
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            &self.context.namespace,
            &self.context.tables,
            written,
        )
    }

    /// Decision 3, first tier: a provider that claims the callee
    /// answers, widened at the consumption boundary — a plugin never
    /// controls termination.
    pub(super) fn provider_return(
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
            if let Some(answer) = registration.provider.return_type(
                &crate::dynamic_type_provider::InvocationSite::new(db, &invocation),
            ) {
                self.edge_counts.provider_edges += 1;
                return Some(crate::widening::capped_child(db, answer));
            }
        }
        None
    }

    /// The by-reference sibling of `provider_return`: first claiming
    /// registration wins; every contribution is widened at the
    /// consumption boundary. Applied after `apply_by_reference`, so a
    /// provider refines (overrides) the declared write-back.
    pub(super) fn provider_by_reference(
        &mut self,
        claim: crate::dynamic_type_provider::SymbolClaim,
        receiver_type: Option<TypeId<'db>>,
        argument_types: &[TypeId<'db>],
    ) -> Vec<(usize, TypeId<'db>)> {
        let db = self.db();
        let Some(registry) = crate::dynamic_type_provider::DynamicTypeProviderRegistry::try_get(db)
        else {
            return Vec::new();
        };
        for registration in registry.registrations(db) {
            if !registration.provider.claims().contains(&claim) {
                continue;
            }
            let invocation = crate::dynamic_type_provider::Invocation {
                claim: claim.clone(),
                receiver_type,
                argument_types: argument_types.to_vec(),
            };
            let contributions = registration.provider.by_reference_types(
                &crate::dynamic_type_provider::InvocationSite::new(db, &invocation),
            );
            if !contributions.is_empty() {
                return contributions
                    .into_iter()
                    .map(|(index, of)| (index, crate::widening::capped_child(db, of)))
                    .collect();
            }
        }
        Vec::new()
    }

    /// Binds provider by-reference contributions onto their positional
    /// arguments' subjects. Labeled arguments are skipped (the channel
    /// is positional); a spread ends the mapping, like
    /// `apply_by_reference`.
    ///
    /// Task-12 debt (owner: the by-reference channel). Only the
    /// free-function call site above wires `provider_by_reference`/
    /// `apply_provider_by_reference` in; the method-call arm has no
    /// analogous wiring, so a provider's by-reference contribution
    /// never reaches a method call's arguments. No handler claims a
    /// method today (`StdlibProvider`'s channel is free-function-only,
    /// `preg_match`), so this is a wiring gap with no live symptom yet
    /// — closed only once a method-call claimant exists to demand it.
    pub(super) fn apply_provider_by_reference(
        &mut self,
        contributions: &[(usize, TypeId<'db>)],
        arguments: &[celerrate_semantics::CallArgument],
        environment: &mut Environment<'db>,
    ) {
        for (index, of) in contributions {
            let Some(argument) = arguments.get(*index) else {
                continue;
            };
            if arguments
                .iter()
                .take(*index + 1)
                .any(|argument| argument.spread)
                || argument.label.is_some()
            {
                continue;
            }
            if let Some(subject) = subject_of(self.context.ir, argument.value) {
                environment.bind(subject, *of);
            }
        }
    }

    /// Decision 3, tiers two and three, for a named function call.
    /// Task 10 replaces the `mixed` fallback with the fixpoint.
    pub(super) fn function_call_result(&mut self, key: &str, source_exists: bool) -> TypeId<'db> {
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
            self.dependencies.functions.insert(key.to_owned());
            return signature.value_type;
        }
        if source_exists {
            self.edge_counts.inferred_return_edges += 1;
            // Decision 4: `raw` is the callee-query answer as-is — a
            // free function has no `self`/`static`/receiver boundary to
            // substitute against, so this IS already the value task 8's
            // live validator re-derives; recorded verbatim.
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
            // signature-guarding dependency, keyed on `function_
            // signature_digest`, even though only the inferred tier
            // answered this call. Without this, a docblock `@return`
            // later appearing on `key` (flipping it from `Trust::
            // NativeOnly`/`mixed` to a usable declared return) has no
            // recorded fact for revalidation to notice — it only
            // re-demands `inferred_function_return`, which never reads
            // docblocks, and serves the stale pre-docblock verdict.
            self.dependencies.functions.insert(key.to_owned());
            return raw;
        }
        TypeId::mixed(db)
    }

    /// Task 9(f): a provider consulted for a method call before the
    /// declared tier, when the receiver resolves to exactly one key
    /// (an ambiguous union receiver never reaches a provider — the
    /// same conservative stance the by-reference write-back channel
    /// already takes). A `Some` answer wins the call outright; the
    /// declared-tier edge is not also counted.
    pub(super) fn method_call_result_for_keys_with_provider(
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
    pub(super) fn method_call_result_with_provider(
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
    pub(super) fn method_call_result_for_keys(
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
                // Decision 4, the load-bearing capture: `inferred` here
                // is the RAW callee-query answer — a `static`-typed
                // callee still carries the placeholder at this point.
                // Recorded BEFORE `member_boundary_type` below
                // substitutes it against this call's own owner and
                // receiver, so the recorded edge matches exactly what
                // task 8's live validator re-derives by calling
                // `inferred_method_return` itself, never the call-site-
                // relative value substitution produces.
                self.dependencies.inferred_methods.push((
                    (key.clone(), folded_member_key(MemberKind::Method, name)),
                    inferred,
                ));
                let owner = self.member_owner(key, MemberKind::Method, name);
                // Plan 9a task 10: also record the owning class as a
                // dependency, keyed on `class_surface_digest` (task 2's
                // surface digest resolves each member's declared
                // signature), even though only the inferred tier
                // answered this call. Without this, a docblock
                // `@return` later appearing on this method has no
                // recorded fact for revalidation to notice — it only
                // re-demands `inferred_method_return`, which never
                // reads docblocks, and serves the stale pre-docblock
                // verdict. Recorded against `owner` (the actual
                // declaring class lookup_member resolved), not merely
                // `key` (the receiver key `member_owner` already
                // records), since inheritance can make them differ.
                if let Some(owner_key) = &owner {
                    self.dependencies.classes.insert(owner_key.clone());
                }
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
}
