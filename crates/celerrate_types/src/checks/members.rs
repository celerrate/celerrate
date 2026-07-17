//! The unknown-member family, method kind (CEL0030): every called
//! member access — `$x->m()`, `Foo::m()`, `$x->m(...)` — judged
//! through the ternary receiver surface (task 3). `Missing` reports;
//! `Exists` and `PossiblyExists` are silence (the guillotine).
//! Property, class-constant, and enum-case kinds are tasks 5-6's beat.

use std::collections::HashSet;

use celerrate_semantics::{
    BodyExpression, BodyIr, ExpressionId, MemberKind, MemberReference, SymbolSources, SymbolSpace,
    folded_symbol_key, resolve_candidates, resolve_name,
};

use super::receivers::{
    MemberExistence, atom_existence, member_existence, owner_class_key, parent_key,
    receiver_display,
};
use super::{CheckContext, TypedVerdict, TypedVerdictKind};

pub(crate) fn check(context: &CheckContext<'_, '_>, verdicts: &mut Vec<TypedVerdict>) {
    let called = called_member_accesses(context.ir);
    for (index, expression) in context.ir.expressions.iter().enumerate() {
        let Some(id) = ExpressionId::from_index(index) else {
            continue;
        };
        if !called.contains(&id) {
            continue;
        }
        match expression {
            BodyExpression::MemberAccess {
                receiver,
                member: MemberReference::Named { name },
                ..
            } => check_instance_method(context, verdicts, id, *receiver, name),
            // The `::class` guard: a scoped member named `class`
            // (case-insensitive) is PHP syntax, never a member.
            BodyExpression::ScopedAccess {
                subject,
                member: MemberReference::Named { name },
            } if !name.eq_ignore_ascii_case("class") => {
                check_scoped_method(context, verdicts, id, *subject, name);
            }
            _ => {}
        }
    }
}

/// Every expression consumed as a callee: `Call.callee` and
/// `CallableReference.callee`, so task 5 can treat the remaining
/// `MemberAccess`/`ScopedAccess` expressions as property/constant
/// reads rather than calls.
pub(crate) fn called_member_accesses(ir: &BodyIr) -> HashSet<ExpressionId> {
    ir.expressions
        .iter()
        .filter_map(|expression| match expression {
            BodyExpression::Call { callee, .. } | BodyExpression::CallableReference { callee } => {
                Some(*callee)
            }
            _ => None,
        })
        .collect()
}

/// `$x->m()`/`$x->m(...)`: the receiver's inferred type judged through
/// the ternary surface. An unresolved receiver type (no inference
/// answer for this expression, e.g. dead or unreachable code) is
/// silent — there is nothing to judge against.
fn check_instance_method(
    context: &CheckContext<'_, '_>,
    verdicts: &mut Vec<TypedVerdict>,
    id: ExpressionId,
    receiver: ExpressionId,
    name: &str,
) {
    let Some(receiver_type) = context.inferred.expression_type(receiver) else {
        return;
    };
    if member_existence(context, receiver_type, MemberKind::Method, name, false)
        == MemberExistence::Missing
    {
        verdicts.push(TypedVerdict {
            body: context.body,
            expression: id,
            kind: TypedVerdictKind::UnknownMethod {
                member: name.to_owned(),
                receiver: receiver_display(context, receiver_type),
            },
        });
    }
}

/// `Foo::m()`/`Foo::m(...)`: the scoped subject folds to a set of
/// class keys (rarely more than one, but a placeholder resolution
/// keeps the shape uniform with the union rule below); missing on
/// every one of them reports. An unresolvable subject (decision 5: an
/// unknown class name is `CEL0018`'s beat, a dynamic subject is always
/// silent) never reaches the judgment at all.
fn check_scoped_method(
    context: &CheckContext<'_, '_>,
    verdicts: &mut Vec<TypedVerdict>,
    id: ExpressionId,
    subject: ExpressionId,
    name: &str,
) {
    let Some(keys) = scoped_subject_keys(context, subject) else {
        return;
    };
    let all_missing = !keys.is_empty()
        && keys.iter().all(|key| {
            atom_existence(context, key, MemberKind::Method, name, true) == MemberExistence::Missing
        });
    if !all_missing {
        return;
    }
    let Some(first) = keys.first() else {
        return;
    };
    verdicts.push(TypedVerdict {
        body: context.body,
        expression: id,
        kind: TypedVerdictKind::UnknownMethod {
            member: name.to_owned(),
            receiver: written_class_display(context, subject, first),
        },
    });
}

/// The folded class keys of a scoped subject: `self`/`static` resolve
/// to the owner, `parent` to the owner's parent (task 3's
/// `parent_key`), any other `NamedReference` through `resolve_name` in
/// the body's namespace and use tables (`None` when unknown — an
/// unresolved class name is `CEL0018`'s beat, silence here); any
/// non-`NamedReference` subject (a variable, an expression) answers
/// `None` (decision 5: dynamic subjects are silent).
pub(crate) fn scoped_subject_keys(
    context: &CheckContext<'_, '_>,
    subject: ExpressionId,
) -> Option<Vec<String>> {
    let BodyExpression::NamedReference { text } = context.ir.expression(subject)? else {
        return None;
    };
    match text.to_ascii_lowercase().as_str() {
        "self" | "static" => owner_class_key(context).map(|key| vec![key]),
        "parent" => owner_class_key(context)
            .and_then(|key| parent_key(context, &key))
            .map(|key| vec![key]),
        _ => resolve_scoped_class_key(context, text).map(|key| vec![key]),
    }
}

/// A written class name resolved through the global symbol index, the
/// same candidate set `resolve_name` consults: `None` when it resolves
/// to no declaration.
fn resolve_scoped_class_key(context: &CheckContext<'_, '_>, written: &str) -> Option<String> {
    let candidate = resolve_candidates(
        written,
        SymbolSpace::ClassLike,
        &context.namespace,
        &context.tables,
    )
    .into_iter()
    .next()?;
    let sources = SymbolSources {
        files: context.files,
        stubs: context.stubs,
        configuration: context.configuration,
    };
    resolve_name(
        context.db,
        sources,
        &context.namespace,
        &context.tables,
        written,
        SymbolSpace::ClassLike,
    )?;
    Some(folded_symbol_key(SymbolSpace::ClassLike, &candidate))
}

/// The subject's written text as the message receiver (`Tool`, not the
/// folded `tool`); falls back to the folded key when the subject is
/// not a plain `NamedReference` (defensive: every caller reaches this
/// only after `scoped_subject_keys` confirmed it is one).
pub(crate) fn written_class_display(
    context: &CheckContext<'_, '_>,
    subject: ExpressionId,
    key: &str,
) -> String {
    match context.ir.expression(subject) {
        Some(BodyExpression::NamedReference { text }) => text.clone(),
        _ => key.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use celerrate_semantics::{
        PluginIdentity, VirtualMember, VirtualMemberKind, VirtualSymbolProvider,
        VirtualSymbolRegistration, VirtualSymbolRegistry,
    };

    use super::super::test_support::{fixture, handle_of};
    use super::super::{TypedVerdictKind, typed_file_verdicts};

    /// A provider that recognizes the literal PHPDoc `@method` tag
    /// convention (`@method <ReturnType> <name>(...)`) — just enough to
    /// exercise the virtual-member integration this walker's guillotine
    /// leans on (design section 8: `@method` "counts as existing").
    /// The real dialect lives in `celerrate_phpdoc_bridge`, a crate
    /// above this one in the dependency DAG that cannot be depended on
    /// from here; a duplicated minimal parser is the codebase's own
    /// precedent for this gap (`celerrate_semantics`'s own test modules
    /// carry an equivalent `FakeProvider`, keyed on a bare `@fake`
    /// marker since none of them exercise the real tag text).
    #[derive(Debug)]
    struct DocblockMethodProvider;

    impl VirtualSymbolProvider for DocblockMethodProvider {
        fn virtual_members(&self, class_docblock: &str) -> Vec<VirtualMember> {
            class_docblock
                .split("@method")
                .skip(1)
                .filter_map(|rest| {
                    let name_token = rest.split_whitespace().find(|token| token.contains('('))?;
                    let name = name_token.split('(').next()?;
                    (!name.is_empty()).then(|| VirtualMember {
                        kind: VirtualMemberKind::Method,
                        name: name.to_owned(),
                        is_static: false,
                        type_text: None,
                        parameters: Vec::new(),
                    })
                })
                .collect()
        }
    }

    fn method_verdicts(source: &str) -> Vec<TypedVerdictKind> {
        let fixture = fixture(&[source]);
        let _ = VirtualSymbolRegistry::builder(vec![VirtualSymbolRegistration {
            identity: PluginIdentity {
                name: "test-docblock-method".to_owned(),
                version: "0.0.0".to_owned(),
                configuration: String::new(),
            },
            provider: std::sync::Arc::new(DocblockMethodProvider),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
        typed_file_verdicts(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            handle_of(&fixture, 0),
        )
        .verdicts
        .iter()
        .map(|verdict| verdict.kind.clone())
        .collect()
    }

    #[test]
    fn an_unknown_instance_method_reports() {
        let verdicts = method_verdicts(
            r#"<?php
class User { public function save(): void {} }
function f(User $u): void { $u->svae(); }
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::UnknownMethod {
                member: "svae".to_owned(),
                receiver: "User".to_owned(),
            }],
        );
    }

    #[test]
    fn known_inherited_virtual_and_magic_receivers_are_silent() {
        let verdicts = method_verdicts(
            r#"<?php
class Base { public function up(): void {} }
/** @method void annotated() */
class User extends Base {
    public function save(): void {}
    public function all(): void {
        $this->save();
        $this->up();
        $this->annotated();
        static::save();
        parent::up();
        self::save();
    }
}
class Magic { public function __call(string $n, array $a): mixed {} }
function f(User $u, Magic $m, mixed $x): void {
    $u->save();
    $u->up();
    $u->annotated();
    $m->whatever();
    $x->anything();
    $u->save(...);
}
"#,
        );
        assert_eq!(verdicts, vec![]);
    }

    #[test]
    fn union_receivers_report_only_when_missing_everywhere() {
        let verdicts = method_verdicts(
            r#"<?php
class A { public function shared(): void {} public function onlyA(): void {} }
class B { public function shared(): void {} }
function f(A|B $either, ?A $nullable): void {
    $either->shared();
    $either->onlyA();      // possibly undefined: future family, silent
    $either->nowhere();    // missing on both: reports
    $nullable->nowhere();  // missing on the non-null part: reports
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![
                TypedVerdictKind::UnknownMethod {
                    member: "nowhere".to_owned(),
                    receiver: "A|B".to_owned(),
                },
                TypedVerdictKind::UnknownMethod {
                    member: "nowhere".to_owned(),
                    receiver: "A".to_owned(),
                },
            ],
        );
    }

    // Signpost: task 6's nullability walker will additionally report a
    // `NullDereference` for `$nullable->nowhere()` above (the receiver
    // carries null); task 6 extends this expectation when it lands —
    // that update is expected, not a regression.

    #[test]
    fn trait_owned_bodies_are_not_checked() {
        // Decision 3: plan 6 analyzes trait bodies per using class;
        // judged against the trait's own surface, this call would be a
        // false positive.
        let verdicts = method_verdicts(
            r#"<?php
trait Caching {
    public function warm(): void { $this->providedByTheUsingClass(); }
}
"#,
        );
        assert_eq!(verdicts, vec![]);
    }

    #[test]
    fn scoped_calls_resolve_their_subject_symbolically() {
        let verdicts = method_verdicts(
            r#"<?php
class Tool { public static function make(): static { return new static(); } }
function f(string $class): void {
    Tool::make();
    Tool::nowhere();
    Ghost::anything();     // unknown class: CEL0018's beat, silent here
    $class::dynamic();     // dynamic subject: silent
    Tool::class;           // the ::class constant is never a member
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::UnknownMethod {
                member: "nowhere".to_owned(),
                receiver: "Tool".to_owned(),
            }],
        );
    }

    #[test]
    fn an_anonymous_class_receiver_is_checked() {
        let verdicts = method_verdicts(
            r#"<?php
function f(): void {
    $listener = new class { public function handle(): void {} };
    $listener->handle();
    $listener->nowhere();
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::UnknownMethod {
                member: "nowhere".to_owned(),
                receiver: "class@anonymous".to_owned(),
            }],
        );
    }
}
