//! The nullability family (CEL0034): a dereference — property read,
//! property write, or method call, all the same `MemberAccess` arena
//! node — whose receiver's type explicitly contains `null`.
//! `contains_null` is the whole predicate: `mixed` and templates
//! answer `false`, so the design's undecidable-receiver silence is
//! structural. `?->` accesses never report (the chain rule types the
//! short-circuited suffix non-null; the chain end re-acquires `|null`
//! and a later real dereference of it reports there). Guarded
//! property reads never report either (decision 9): `isset()`,
//! `empty()`, and the `??`/`??=` left side evaluate a null property
//! chain without a fatal error — a method call inside them still
//! throws and still reports.

use std::collections::HashSet;

use celerrate_semantics::{BodyExpression, BodyIr, ExpressionId, MemberReference};
use celerrate_syntax::SyntaxKind;

use super::members::called_member_accesses;
use super::receivers::written_type_display;
use super::{CheckContext, TypedVerdict, TypedVerdictKind};

pub(crate) fn check(context: &CheckContext<'_, '_>, verdicts: &mut Vec<TypedVerdict>) {
    let called = called_member_accesses(context.ir);
    let guarded = null_guarded_property_reads(context.ir);
    for (index, expression) in context.ir.expressions.iter().enumerate() {
        let Some(id) = ExpressionId::from_index(index) else {
            continue;
        };
        // `MemberReference::Computed`/`Variable` (a dynamic member
        // name) never matches this arm: debt ledger, dynamic member
        // names are silent across every family (`members.rs` carries
        // the same note).
        let BodyExpression::MemberAccess {
            receiver,
            member: MemberReference::Named { name },
            null_safe: false,
        } = expression
        else {
            continue;
        };
        // The decision-9 exemption covers property positions only:
        // a guarded expression consumed as a callee is a real call.
        if guarded.contains(&id) && !called.contains(&id) {
            continue;
        }
        // A receiver that is itself a call result carries its `|null`
        // from a return type the plan-5/6 narrowing floor does not
        // track: `subject_of` narrows variables and `$this->prop`
        // chains, never a call. So the floor cannot see the guards PHP
        // routinely writes around such a receiver (`if ($x->get() &&
        // $x->get()->y())`, a local re-bind), and reporting one is a
        // false-positive class — the guillotine stays silent (design
        // section 8: an undecidable receiver is never a guess). A
        // variable or property receiver holding a nullable call result
        // (`$p = $u?->profile(); $p->refresh();`) is narrowable and
        // still reports.
        if matches!(
            context.ir.expression(*receiver),
            Some(BodyExpression::Call { .. })
        ) {
            continue;
        }
        let Some(receiver_type) = context.inferred.expression_type(*receiver) else {
            continue;
        };
        if receiver_type.contains_null(context.db) {
            verdicts.push(TypedVerdict {
                body: context.body,
                expression: id,
                kind: TypedVerdictKind::NullDereference {
                    member: name.clone(),
                    receiver: written_type_display(context, receiver_type),
                },
            });
        }
    }
}

/// The expressions decision 9 exempts: the targets of `isset()` and
/// `empty()`, the left operand of `??`, and the target of `??=`,
/// expanded along their receiver chains — through `MemberAccess`
/// receivers and `Index` subjects, stopping at anything else (a call
/// in particular is a hard boundary).
pub(crate) fn null_guarded_property_reads(ir: &BodyIr) -> HashSet<ExpressionId> {
    let mut seeds: Vec<ExpressionId> = Vec::new();
    for expression in &ir.expressions {
        match expression {
            BodyExpression::Isset { targets } => seeds.extend(targets.iter().copied()),
            BodyExpression::Empty { target } => seeds.push(*target),
            BodyExpression::Binary { operator, lhs, .. }
                if *operator == SyntaxKind::QuestionQuestion =>
            {
                seeds.push(*lhs);
            }
            BodyExpression::Assignment {
                operator, target, ..
            } if *operator == SyntaxKind::QuestionQuestionEquals => {
                seeds.push(*target);
            }
            _ => {}
        }
    }
    let mut guarded: HashSet<ExpressionId> = HashSet::new();
    while let Some(id) = seeds.pop() {
        if !guarded.insert(id) {
            continue;
        }
        match ir.expression(id) {
            Some(BodyExpression::MemberAccess { receiver, .. }) => seeds.push(*receiver),
            Some(BodyExpression::Index { subject, .. }) => seeds.push(*subject),
            _ => {}
        }
    }
    guarded
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use super::super::TypedVerdictKind;
    use super::super::test_support::family_verdicts;

    #[test]
    fn a_possibly_null_dereference_reports() {
        let verdicts = family_verdicts(
            r#"<?php
class User { public string $name = ''; public function save(): void {} }
function f(?User $u): void {
    $u->save();
    $u->name;
    $u->name = 'x';
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![
                TypedVerdictKind::NullDereference {
                    member: "save".to_owned(),
                    receiver: "User|null".to_owned(),
                },
                TypedVerdictKind::NullDereference {
                    member: "name".to_owned(),
                    receiver: "User|null".to_owned(),
                },
                TypedVerdictKind::NullDereference {
                    member: "name".to_owned(),
                    receiver: "User|null".to_owned(),
                },
            ],
        );
    }

    #[test]
    fn narrowing_and_the_null_safe_operator_silence() {
        let verdicts = family_verdicts(
            r#"<?php
class Address { public string $city = ''; }
class User {
    public ?Address $address = null;
    public function save(): void {}
}
function f(?User $u, mixed $anything): void {
    if ($u !== null) {
        $u->save();                    // narrowed: silent
    }
    $u?->save();                       // null-safe: silent
    $u?->address?->city;               // whole-chain short circuit: silent
    if ($u === null) {
        return;
    }
    $u->save();                        // early return narrowed: silent
    $u->address->city;                 // ?Address un-narrowed: reports
    $anything->whatever();             // mixed: silent by construction
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::NullDereference {
                member: "city".to_owned(),
                receiver: "Address|null".to_owned(),
            }],
        );
    }

    #[test]
    fn a_chain_end_re_acquires_null_and_a_real_dereference_of_it_reports() {
        // Plan 5's chain rule: only the final chain result re-acquires
        // `|null`. Dereferencing that end without narrowing is a real
        // possible-null dereference.
        let verdicts = family_verdicts(
            r#"<?php
class Profile { public function refresh(): void {} }
class User { public function profile(): Profile { return new Profile(); } }
function f(?User $u): void {
    $profile = $u?->profile();
    $profile->refresh();
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::NullDereference {
                member: "refresh".to_owned(),
                receiver: "Profile|null".to_owned(),
            }],
        );
    }

    #[test]
    fn guarded_property_reads_are_exempt_but_calls_still_report() {
        // Decision 9: isset(), empty(), and the ??/??= left side are
        // the idiomatic guards themselves — property reads there are
        // non-fatal and warning-suppressed. A call is a hard boundary:
        // it still throws on a null receiver and still reports.
        let verdicts = family_verdicts(
            r#"<?php
class Box {
    public ?Box $inner = null;
    public function get(): ?Box { return null; }
}
function f(?Box $b, array $bag): void {
    isset($b->inner);                  // exempt
    empty($b->inner);                  // exempt
    $x = $b->inner ?? null;            // exempt
    $y = $b->inner->inner ?? null;     // the whole chain is exempt
    $z = $bag[0]->inner ?? null;       // Index subjects thread too
    $b->inner ??= null;                // exempt (recorded stance)
    $w = $b->get() ?? null;            // a call boundary: reports
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::NullDereference {
                member: "get".to_owned(),
                receiver: "Box|null".to_owned(),
            }],
        );
    }

    #[test]
    fn a_call_result_receiver_is_not_a_tracked_dereference() {
        // The corpus idiom `if ($e->getCommand() && $e->getCommand()->getName())`:
        // the second `getCommand()` is a fresh call whose `?Command` the
        // narrowing floor cannot track, so the `&&` guard is invisible to
        // it and reporting the `getName()` receiver would be a false
        // positive. A variable receiver holding the same nullable value
        // (`$c`) stays reported, so the silence is specifically about the
        // call-result receiver shape, not a vacuous fixture.
        let verdicts = family_verdicts(
            r#"<?php
class Command { public function getName(): string { return ''; } }
class Event { public function getCommand(): ?Command { return null; } }
function f(Event $e): void {
    $e->getCommand()->getName();       // call-result receiver: silent
}
function g(?Command $c): void {
    $c->getName();                     // variable receiver: reports
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::NullDereference {
                member: "getName".to_owned(),
                receiver: "Command|null".to_owned(),
            }],
        );
    }

    #[test]
    fn index_subjects_thread_through_the_guard_chain() {
        // The `??` LHS `$h->bag[0]->leaf` expands: the outer
        // `MemberAccess` (`...->leaf`) receiver is the `Index`
        // expression `$h->bag[0]`; that `Index`'s subject is the
        // `MemberAccess` `$h->bag`, whose OWN receiver `$h` is
        // `Holder|null`. Only the `Index { subject, .. }` expansion arm
        // reaches `$h->bag` and marks it guarded — without it, `$h->bag`
        // would be a real, un-exempted `MemberAccess` on a nullable
        // receiver and would report. `leaf`'s own receiver (an untyped
        // array's element) is `mixed`, so it never reports regardless of
        // guard status: this test is about `bag`, not `leaf`.
        let verdicts = family_verdicts(
            r#"<?php
class Holder { public array $bag = []; }
function g(?Holder $h): void {
    $x = $h->bag[0]->leaf ?? null;
}
"#,
        );
        assert_eq!(verdicts, vec![]);
    }
}
