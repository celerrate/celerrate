//! The unknown-member family (CEL0030-CEL0033): every member access —
//! called (`$x->m()`, `Foo::m()`, `$x->m(...)`) or not (`$x->p`,
//! `Foo::$p`, `Foo::CONST`, `Foo::Case`) — judged through the ternary
//! receiver surface (task 3). `Missing` reports; `Exists` and
//! `PossiblyExists` are silence (the guillotine). A nullable receiver's
//! own null-dereference beat (`NullDereference`) is task 6's.

use std::collections::HashSet;

use celerrate_semantics::{
    BodyExpression, BodyIr, ExpressionId, MemberKind, MemberReference, SymbolSources, SymbolSpace,
    folded_symbol_key, resolve_candidates, resolve_name,
};

use super::receivers::{
    MemberExistence, atom_existence, is_enum_key, member_existence, owner_class_key, parent_key,
    receiver_display,
};
use super::{CheckContext, TypedVerdict, TypedVerdictKind};

pub(crate) fn check(context: &CheckContext<'_, '_>, verdicts: &mut Vec<TypedVerdict>) {
    let called = called_member_accesses(context.ir);
    for (index, expression) in context.ir.expressions.iter().enumerate() {
        let Some(id) = ExpressionId::from_index(index) else {
            continue;
        };
        match expression {
            BodyExpression::MemberAccess {
                receiver,
                member: MemberReference::Named { name },
                ..
            } => {
                if called.contains(&id) {
                    check_instance_method(context, verdicts, id, *receiver, name);
                } else {
                    check_instance_property(context, verdicts, id, *receiver, name);
                }
            }
            // `Foo::$prop`: always a static-property read, whether or
            // not it is itself the callee of a call (`Foo::$prop()`
            // invokes the callable the property holds — that is still a
            // property access, never a scoped method call).
            BodyExpression::ScopedAccess {
                subject,
                member: MemberReference::Variable { name },
            } => check_scoped_property(context, verdicts, id, *subject, name),
            // The `::class` guard: a scoped member named `class`
            // (case-insensitive) is PHP syntax, never a member.
            BodyExpression::ScopedAccess {
                subject,
                member: MemberReference::Named { name },
            } if !name.eq_ignore_ascii_case("class") => {
                if called.contains(&id) {
                    check_scoped_method(context, verdicts, id, *subject, name);
                } else {
                    check_scoped_constant(context, verdicts, id, *subject, name);
                }
            }
            // `MemberReference::Computed` (`$x->$m()`, a dynamic member
            // name) falls through here: debt ledger, dynamic member
            // names are silent across every family — there is no
            // written name to judge against.
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

/// `$x->p`/`$x->p = ...`: the read and the write share the same
/// `MemberAccess` node kind, so both positions reach this and both
/// report — same shape as `check_instance_method`, judged as a
/// property.
fn check_instance_property(
    context: &CheckContext<'_, '_>,
    verdicts: &mut Vec<TypedVerdict>,
    id: ExpressionId,
    receiver: ExpressionId,
    name: &str,
) {
    let Some(receiver_type) = context.inferred.expression_type(receiver) else {
        return;
    };
    if member_existence(context, receiver_type, MemberKind::Property, name, false)
        == MemberExistence::Missing
    {
        verdicts.push(TypedVerdict {
            body: context.body,
            expression: id,
            kind: TypedVerdictKind::UnknownProperty {
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

/// `Foo::$prop`: same union rule as `check_scoped_method`, judged as a
/// static property.
fn check_scoped_property(
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
            atom_existence(context, key, MemberKind::Property, name, true)
                == MemberExistence::Missing
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
        kind: TypedVerdictKind::UnknownProperty {
            member: name.to_owned(),
            receiver: written_class_display(context, subject, first),
        },
    });
}

/// `Foo::CONST`/`Foo::Case`: the dual lookup — missing as both a class
/// constant and an enum case, on every folded subject key, reports;
/// `is_enum_key` on the first key then picks the message (an enum
/// spells its missing member `UnknownEnumCase`, anything else
/// `UnknownClassConstant`). `is_enum_key`, not `class_kind`, because a
/// stub-only enum (a PHP built-in, never a source declaration) must
/// still classify as an enum here — `class_kind` alone answers `None`
/// for it.
fn check_scoped_constant(
    context: &CheckContext<'_, '_>,
    verdicts: &mut Vec<TypedVerdict>,
    id: ExpressionId,
    subject: ExpressionId,
    name: &str,
) {
    let Some(keys) = scoped_subject_keys(context, subject) else {
        return;
    };
    let missing = |key: &String| {
        atom_existence(context, key, MemberKind::ClassConstant, name, true)
            == MemberExistence::Missing
            && atom_existence(context, key, MemberKind::EnumCase, name, true)
                == MemberExistence::Missing
    };
    if keys.is_empty() || !keys.iter().all(missing) {
        return;
    }
    let Some(first) = keys.first() else {
        return;
    };
    let receiver = written_class_display(context, subject, first);
    let kind = if is_enum_key(context, first) {
        TypedVerdictKind::UnknownEnumCase {
            member: name.to_owned(),
            receiver,
        }
    } else {
        TypedVerdictKind::UnknownClassConstant {
            member: name.to_owned(),
            receiver,
        }
    };
    verdicts.push(TypedVerdict {
        body: context.body,
        expression: id,
        kind,
    });
}

/// The folded class keys of a scoped subject: `self`/`static` resolve
/// to the owner, `parent` to the owner's parent (task 3's
/// `parent_key`), any other `NamedReference` through `resolve_name` in
/// the body's namespace and use tables (`None` when unknown — an
/// unresolved class name is `CEL0018`'s beat, silence here); any
/// non-`NamedReference` subject (a variable, an expression) answers
/// `None` (decision 5: dynamic subjects are silent). Debt ledger: a
/// variable typed `class-string<Foo>` is a non-`NamedReference`
/// subject too, so a scoped call through it (`$class::method()`) is
/// silent even though the class is statically known.
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
/// to no declaration. `pub(crate)`: task 8's `New { class: Named }`
/// constructor resolution (`checks::arguments`) resolves its written
/// class name through this exact same lookup.
pub(crate) fn resolve_scoped_class_key(
    context: &CheckContext<'_, '_>,
    written: &str,
) -> Option<String> {
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

    use super::super::test_support::{
        family_verdicts, fixture_with_stub_class, fixture_with_stub_enum,
        fixture_with_stub_enum_interfaces, handle_of,
    };
    use super::super::{TypedVerdictKind, typed_file_verdicts};

    /// `family_verdicts` over a fixture whose stub input is a synthetic
    /// `StubIndex::from_symbols` carrying a single `stdClass` class
    /// symbol — the shared `fixture_with_stub_class` idiom, so a source
    /// class can `extends \stdClass` and record it as a stub ancestor
    /// (design section 2: `json_decode`'s dynamic-property surface).
    fn method_verdicts_with_stub_stdclass(source: &str) -> Vec<TypedVerdictKind> {
        let fixture = fixture_with_stub_class(&[source], "stdClass");
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

    /// `family_verdicts` over the shared `fixture_with_stub_enum_interfaces`
    /// fixture, carrying `UnitEnum`/`BackedEnum` surfaces so decision 7's
    /// implicit enum edges are synthesized end to end through this walk.
    fn method_verdicts_with_stub_enum_interfaces(source: &str) -> Vec<TypedVerdictKind> {
        let fixture = fixture_with_stub_enum_interfaces(&[source]);
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

    /// `family_verdicts` over the shared `fixture_with_stub_enum`
    /// fixture: an enum declared only in the compiled stub surface, with
    /// no matching source declaration at all — a PHP built-in enum, the
    /// stub-only-enum classification regression's shape.
    fn method_verdicts_with_stub_enum(
        source: &str,
        name: &str,
        cases: &[&str],
    ) -> Vec<TypedVerdictKind> {
        let fixture = fixture_with_stub_enum(&[source], name, cases);
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
        let verdicts = family_verdicts(
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
        let verdicts = family_verdicts(
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
        let verdicts = family_verdicts(
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
                // Task 6's nullability walker: `$nullable->nowhere()`'s
                // receiver carries `null`, so the same `MemberAccess`
                // node also earns a `NullDereference` beat, appended
                // after every members-family verdict (`body_typed_verdicts`
                // runs `members::check` then `nullability::check`).
                TypedVerdictKind::NullDereference {
                    member: "nowhere".to_owned(),
                    receiver: "A|null".to_owned(),
                },
            ],
        );
    }

    #[test]
    fn trait_owned_bodies_are_not_checked() {
        // Decision 3: plan 6 analyzes trait bodies per using class;
        // judged against the trait's own surface, this call would be a
        // false positive.
        let verdicts = family_verdicts(
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
        let verdicts = family_verdicts(
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
        let verdicts = family_verdicts(
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

    #[test]
    fn unknown_properties_report_with_their_suppressions() {
        let verdicts = family_verdicts(
            r#"<?php
class User { public string $name = ''; }
class Getter { public function __get(string $n): mixed {} }
#[AllowDynamicProperties]
class Bag {}
/** @property string $virtual */
class Annotated {}
function f(User $u, Getter $g, Bag $b, Annotated $a): void {
    $u->name;
    $u->nmae;          // reports
    $u->nmae = 'x';    // the write position is the same node kind
    $g->anything;
    $b->anything;
    $a->virtual;
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![
                TypedVerdictKind::UnknownProperty {
                    member: "nmae".to_owned(),
                    receiver: "User".to_owned(),
                },
                TypedVerdictKind::UnknownProperty {
                    member: "nmae".to_owned(),
                    receiver: "User".to_owned(),
                },
            ],
        );
    }

    #[test]
    fn stdclass_descendants_accept_dynamic_properties() {
        // `json_decode` alone makes this mandatory on any real corpus
        // (design section 2). `method_verdicts_with_stub_stdclass` is
        // the task-4 helper over a fixture whose stub input is a
        // synthetic `StubIndex::from_symbols` carrying a `stdClass`
        // class — mirror the synthetic-stub fixtures the member-lookup
        // tests already build.
        let verdicts = method_verdicts_with_stub_stdclass(
            r#"<?php
class Payload extends \stdClass {}
function f(Payload $p, \stdClass $raw): void {
    $p->anything;
    $raw->anything;
}
"#,
        );
        assert_eq!(verdicts, vec![]);
    }

    #[test]
    fn static_properties_constants_and_cases_report() {
        let verdicts = family_verdicts(
            r#"<?php
class Config {
    public static int $limit = 10;
    public const RETRIES = 3;
}
enum Status: string {
    case Active = 'active';
    public const DEFAULT = self::Active;
}
function f(): void {
    Config::$limit;
    Config::$limti;        // reports CEL0031
    Config::RETRIES;
    Config::MISSING;       // reports CEL0032
    Status::Active;
    Status::DEFAULT;       // constants on enums resolve
    Status::Draft;         // reports CEL0033
    Config::class;         // never a member
    Status::Active->value; // enum-case receiver: backing property
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![
                TypedVerdictKind::UnknownProperty {
                    member: "limti".to_owned(),
                    receiver: "Config".to_owned(),
                },
                TypedVerdictKind::UnknownClassConstant {
                    member: "MISSING".to_owned(),
                    receiver: "Config".to_owned(),
                },
                TypedVerdictKind::UnknownEnumCase {
                    member: "Draft".to_owned(),
                    receiver: "Status".to_owned(),
                },
            ],
        );
    }

    #[test]
    fn the_implicit_enum_surface_is_silent_and_its_ghosts_still_report() {
        let verdicts = method_verdicts_with_stub_enum_interfaces(
            r#"<?php
enum Status: string { case Active = 'active'; }
function f(Status $s): void {
    Status::cases();
    Status::from('active');
    Status::tryFrom('x');
    $s->value;
    $s->name;
    Status::ghost();       // the surface stays closed: reports
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::UnknownMethod {
                member: "ghost".to_owned(),
                receiver: "Status".to_owned(),
            }],
        );
    }

    #[test]
    fn a_missing_case_on_a_stub_only_enum_reports_as_an_enum_case() {
        // `Suit` is declared only in the compiled stub surface — no
        // matching source declaration at all, mirroring a PHP built-in
        // enum. `class_kind` alone answers `None` for it (source
        // declarations only), which would mislabel the report
        // `UnknownClassConstant` (CEL0032); the dual lookup must
        // classify through `is_enum_key` instead, which also consults
        // the stub symbol table, so the report comes out
        // `UnknownEnumCase` (CEL0033).
        let verdicts = method_verdicts_with_stub_enum(
            r#"<?php
function f(): void {
    Suit::Hearts;
    Suit::Diamonds;    // reports CEL0033, not CEL0032
}
"#,
            "Suit",
            &["Hearts"],
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::UnknownEnumCase {
                member: "Diamonds".to_owned(),
                receiver: "Suit".to_owned(),
            }],
        );
    }
}
