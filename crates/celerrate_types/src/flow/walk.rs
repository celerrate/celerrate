//! The walk itself: statements in order, expressions by form. Every
//! arm delegates to a cluster module — the member boundary, the call
//! machinery, iteration, branching, callables, assignment — so this
//! file is the traversal, not the typing rules. `expression_value`'s
//! match moves here whole, deliberately unsplit (spec decision 2).

use super::assignment::widen_if_literal;
use super::*;

impl<'db> Walker<'db, '_, '_> {
    pub(super) fn statements(&mut self, list: &[StatementId], environment: &mut Environment<'db>) {
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
                        environment.kill_call_results_for_subject(&subject);
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
                        environment.kill_call_results_for_subject(&subject);
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

    pub(super) fn expression(
        &mut self,
        id: ExpressionId,
        environment: &mut Environment<'db>,
    ) -> TypeId<'db> {
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
    pub(super) fn expression_value(
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
}
