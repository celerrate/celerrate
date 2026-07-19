//! Condition splitting: one condition expression in, a
//! (true-environment, false-environment) pair out — instanceof,
//! null checks, truthiness, comparisons against literals, type-check
//! functions, and the call-assertion facts drained from
//! `pending_condition_facts`.

use super::*;

impl<'db> Walker<'db, '_, '_> {
    /// Types `condition` (side effects included) and answers the two
    /// environments its truth and falsity establish. Composition is
    /// structural: `!` swaps, `&&` chains the true side and joins the
    /// false sides, `||` is the dual (design section 6: negation and
    /// boolean composition are in the floor — early returns are
    /// vacuous without them).
    pub(super) fn branch_environments(
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

    pub(super) fn narrowed_to(&self, current: TypeId<'db>, target: TypeId<'db>) -> TypeId<'db> {
        crate::narrowing::narrow_to(
            self.db(),
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            current,
            target,
        )
    }

    pub(super) fn removed_type(&self, current: TypeId<'db>, target: TypeId<'db>) -> TypeId<'db> {
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
}
