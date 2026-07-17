//! The argument family (design section 8): per-argument assignability
//! against exactly one resolved declared signature, under the calling
//! file's coercion posture. `Proof::Fails` reports; `Holds` and
//! `CannotProve` are silence — which is how weak-mode coercions stay
//! unreported (task 7 upgrades a coercible `Fails` to `CannotProve`).
//! `mixed` and partially fitting unions are guarded **before** the
//! judgment (decision 10): the shipped `judge`/`subtype_of` refutes
//! them set-theoretically (a `mixed` candidate answers `Fails`, a union
//! candidate folds through `Proof::all` so one failing constituent
//! fails the whole union), so their silence is this walk's own
//! structural job, never the `Proof` value's.
//!
//! Resolution (decision 11) answers `None` unless exactly one declared
//! signature resolves: a named free function or stub function; a
//! method, static call, or constructor whose receiver decomposes to
//! exactly one class or enum-case atom (a union receiver, or a
//! genuinely undecidable one, is silent — a recorded stance). The
//! **declared tier only** — providers compute returns, never parameter
//! contracts. `ResolvedCall`/`resolved_call_signature` is the interface
//! task 9's arity and named-argument checks reuse, `source_body` in
//! particular for its `func_get_args` probe.

use celerrate_db::SourceFile;
use celerrate_semantics::{
    BodyExpression, BodyQuery, CallArgument, ClassReference, ExpressionId, MemberKind, MemberQuery,
    MemberReference, MemberResolution, SymbolQuery, SymbolSpace, analyzed_file_index,
    anonymous_class_key, body_ir, file_strict_types, folded_member_key,
    lookup_function_declaration, lookup_member,
};

use crate::TypeId;
use crate::declared::{
    DeclaredParameter, DeclaredSignature, FunctionQuery, declared_function_signature,
    declared_member_signature,
};
use crate::flow::resolved_function_key;
use crate::judgments::{CoercionMode, Proof, assignable_to};

use super::members::{resolve_scoped_class_key, scoped_subject_keys};
use super::receivers::{ReceiverAtom, atoms_of, written_type_display};
use super::{ArgumentLabel, CheckContext, TypedVerdict, TypedVerdictKind};

/// Exactly one declared signature for a call's callee, or `None`
/// (decision 11), plus enough to reach into that callee's own body when
/// it is source code. `pub(crate)`: task 9 reuses both. `source_body`
/// feeds `captures_arguments`'s `func_get_args` probe, which silences
/// the excess-arguments check (CEL0037) for a source callee that reads
/// its arguments dynamically.
pub(crate) struct ResolvedCall<'db> {
    pub callee_display: String,
    pub signature: DeclaredSignature<'db>,
    pub source_body: Option<(SourceFile, BodyQuery<'db>)>,
}

pub(crate) fn check(context: &CheckContext<'_, '_>, verdicts: &mut Vec<TypedVerdict>) {
    let mode = if file_strict_types(context.db, context.file) {
        CoercionMode::Strict
    } else {
        CoercionMode::Weak
    };
    for (index, expression) in context.ir.expressions.iter().enumerate() {
        let Some(id) = ExpressionId::from_index(index) else {
            continue;
        };
        let arguments = match expression {
            BodyExpression::Call { arguments, .. } | BodyExpression::New { arguments, .. } => {
                arguments
            }
            _ => continue,
        };
        let Some(resolved) = resolved_call_signature(context, id) else {
            continue;
        };
        check_argument_types(context, verdicts, &resolved, arguments, mode);
        check_arity(context, verdicts, id, &resolved, arguments);
    }
}

/// The arity family (design section 8, decision 12): a required
/// parameter bound neither positionally nor by name (CEL0036), a
/// positional argument past a non-variadic parameter list (CEL0037),
/// and a named argument matching no declared parameter (CEL0038). Any
/// spread argument silences all three (missing and excess become
/// undecidable); a trailing variadic parameter silences the
/// unknown-name and excess checks (PHP 8.0 collects unknown names and
/// excess positionals into it); a source callee that calls
/// `func_get_args` silences excess alone (a variadic-by-capture
/// function called with extra arguments is working code). Duplicate
/// binding (`pair(1, a: 2)`) stays silent — a PHP `Error` this preview
/// does not own (task 13's ledger).
fn check_arity(
    context: &CheckContext<'_, '_>,
    verdicts: &mut Vec<TypedVerdict>,
    call: ExpressionId,
    resolved: &ResolvedCall<'_>,
    arguments: &[CallArgument],
) {
    if arguments.iter().any(|argument| argument.spread) {
        return; // undecidable in both directions (decision 12)
    }
    let parameters = &resolved.signature.parameters;
    let positional = arguments.iter().filter(|a| a.label.is_none()).count();
    let named: Vec<&String> = arguments.iter().filter_map(|a| a.label.as_ref()).collect();
    let variadic = parameters
        .last()
        .is_some_and(|parameter| parameter.variadic);
    // Unknown names first: each is its own verdict. A trailing
    // variadic accepts any named argument (PHP 8.0 collects unknown
    // names into it; decision 12), so the whole loop is silenced.
    if !variadic {
        for name in &named {
            if !parameters.iter().any(|parameter| parameter.name == **name) {
                verdicts.push(TypedVerdict {
                    body: context.body,
                    expression: call,
                    kind: TypedVerdictKind::UnknownNamedArgument {
                        callee: resolved.callee_display.clone(),
                        name: (*name).clone(),
                    },
                });
            }
        }
    }
    // Excess: positional arguments past a non-variadic list.
    if !variadic && positional > parameters.len() && !captures(context, resolved) {
        verdicts.push(TypedVerdict {
            body: context.body,
            expression: call,
            kind: TypedVerdictKind::TooManyArguments {
                callee: resolved.callee_display.clone(),
                given: positional,
                accepted: parameters.len(),
            },
        });
    }
    // Missing: a required parameter bound neither by position nor name.
    let required = parameters
        .iter()
        .filter(|parameter| !parameter.optional && !parameter.variadic)
        .count();
    let unbound = parameters
        .iter()
        .enumerate()
        .filter(|(index, parameter)| {
            !parameter.optional
                && !parameter.variadic
                && *index >= positional
                && !named.iter().any(|name| **name == parameter.name)
        })
        .count();
    if unbound > 0 {
        verdicts.push(TypedVerdict {
            body: context.body,
            expression: call,
            kind: TypedVerdictKind::TooFewArguments {
                callee: resolved.callee_display.clone(),
                given: arguments.len(),
                required,
            },
        });
    }
}

/// Whether the source callee captures its arguments with
/// `func_get_args` — a variadic-by-capture function called with extra
/// arguments is working code (the guillotine forbids reporting it).
fn captures(context: &CheckContext<'_, '_>, resolved: &ResolvedCall<'_>) -> bool {
    resolved
        .source_body
        .is_some_and(|(file, body)| captures_arguments(context.db, file, body))
}

/// Tracked per body: any call whose callee text folds to
/// `func_get_args` (bare or fully qualified).
#[salsa::tracked]
pub(crate) fn captures_arguments<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> bool {
    let Some(ir) = body_ir(db, file, body).as_ref() else {
        return false;
    };
    ir.expressions.iter().any(|expression| {
        let BodyExpression::Call { callee, .. } = expression else {
            return false;
        };
        let Some(BodyExpression::NamedReference { text }) = ir.expression(*callee) else {
            return false;
        };
        text.trim_start_matches('\\')
            .eq_ignore_ascii_case("func_get_args")
    })
}

fn check_argument_types<'db>(
    context: &CheckContext<'db, '_>,
    verdicts: &mut Vec<TypedVerdict>,
    resolved: &ResolvedCall<'db>,
    arguments: &[CallArgument],
    mode: CoercionMode,
) {
    let parameters = &resolved.signature.parameters;
    let mut position = 0usize;
    for argument in arguments {
        if argument.spread {
            // Spread makes later positional matching undecidable;
            // arguments before the first spread were already checked.
            break;
        }
        let (parameter, label) = match &argument.label {
            Some(name) => {
                let Some(parameter) = parameters.iter().find(|parameter| parameter.name == *name)
                else {
                    continue; // task 9 reports the unknown name
                };
                (parameter, ArgumentLabel::Named(name.clone()))
            }
            None => {
                position += 1;
                let Some(parameter) = parameter_at(parameters, position) else {
                    continue; // task 9 reports the excess
                };
                (parameter, ArgumentLabel::Positional(position))
            }
        };
        if parameter.by_reference {
            continue; // the `preg_match` exemption (design section 6)
        }
        let Some(parameter_type) = parameter.parameter_type else {
            continue; // the empty-intersection stub guard (plan 3)
        };
        let Some(argument_type) = context.inferred.expression_type(argument.value) else {
            continue;
        };
        // Decision 10's pre-judgment guards, structural rather than
        // `Proof`-based: `mixed` passes everywhere, and a union reports
        // only when every constituent fails assignability on its own
        // (a non-union scalar source is the one-constituent case of the
        // same fold).
        if argument_type.is_mixed(context.db) {
            continue;
        }
        let every_constituent_fails =
            argument_type
                .constituents(context.db)
                .into_iter()
                .all(|part| {
                    assignable_to(
                        context.db,
                        context.files,
                        context.stubs,
                        context.configuration,
                        part,
                        parameter_type,
                        mode,
                    ) == Proof::Fails
                });
        if every_constituent_fails {
            verdicts.push(TypedVerdict {
                body: context.body,
                expression: argument.value,
                kind: TypedVerdictKind::ArgumentType {
                    label,
                    callee: resolved.callee_display.clone(),
                    expected: written_type_display(context, parameter_type),
                    given: written_type_display(context, argument_type),
                },
            });
        }
    }
}

/// The parameter a 1-based position binds: the last variadic absorbs
/// everything past the list.
fn parameter_at<'a, 'db>(
    parameters: &'a [DeclaredParameter<'db>],
    position: usize,
) -> Option<&'a DeclaredParameter<'db>> {
    match parameters.get(position.saturating_sub(1)) {
        Some(parameter) => Some(parameter),
        None => parameters.last().filter(|parameter| parameter.variadic),
    }
}

/// Exactly one declared signature for a call's or instantiation's
/// callee (decision 11); `None` otherwise. `expression` is the `Call`
/// or `New` expression itself, not its callee sub-expression.
pub(crate) fn resolved_call_signature<'db>(
    context: &CheckContext<'db, '_>,
    expression: ExpressionId,
) -> Option<ResolvedCall<'db>> {
    match context.ir.expression(expression)? {
        BodyExpression::Call { callee, .. } => resolved_callee_signature(context, *callee),
        BodyExpression::New { class, .. } => resolved_constructor_signature(context, class),
        _ => None,
    }
}

/// A `Call`'s callee, decomposed by written shape (mirrors `flow.rs`'s
/// own call-boundary matching): a named free function, an instance
/// method, or a static/scoped call. Any other callee shape (a callable
/// value, a closure result) is undecidable here and answers `None`.
fn resolved_callee_signature<'db>(
    context: &CheckContext<'db, '_>,
    callee: ExpressionId,
) -> Option<ResolvedCall<'db>> {
    match context.ir.expression(callee)? {
        BodyExpression::NamedReference { text } => resolved_named_function_call(context, text),
        BodyExpression::MemberAccess {
            receiver,
            member: MemberReference::Named { name },
            ..
        } => resolved_method_call(context, *receiver, name),
        BodyExpression::ScopedAccess {
            subject,
            member: MemberReference::Named { name },
        } => resolved_scoped_call(context, *subject, name),
        _ => None,
    }
}

/// `foo(...)`: resolved through the same candidate order the flow
/// walk's own call boundary uses (`crate::flow::resolved_function_key`,
/// the fallback-to-global rule folded in), against the declared tier
/// only. `callee_display` is the written text's last segment (`App\foo`
/// displays as `foo`, matching `flow.rs`'s own convention for messages
/// that name a callee).
fn resolved_named_function_call<'db>(
    context: &CheckContext<'db, '_>,
    text: &str,
) -> Option<ResolvedCall<'db>> {
    let db = context.db;
    let (key, _) = resolved_function_key(
        db,
        context.files,
        context.stubs,
        context.configuration,
        &context.namespace,
        &context.tables,
        text,
    );
    let signature = declared_function_signature(
        db,
        context.files,
        context.stubs,
        context.configuration,
        FunctionQuery::new(db, key.clone()),
    )?;
    Some(ResolvedCall {
        callee_display: last_segment(text).to_owned(),
        signature,
        source_body: source_function_body(context, &key),
    })
}

/// `(file, BodyQuery)` for a resolved function key, when it names a
/// source declaration — the `inferred_function_return` idiom
/// (`analyzed_file_index` bridges the declaration's `AstId::file` back
/// to the salsa `SourceFile` handle). `None` for a stub-only or
/// unresolved key.
fn source_function_body<'db>(
    context: &CheckContext<'db, '_>,
    key: &str,
) -> Option<(SourceFile, BodyQuery<'db>)> {
    let db = context.db;
    let query = SymbolQuery::new(db, SymbolSpace::Function, key.to_owned());
    let ast_id = lookup_function_declaration(db, context.files, query)?;
    let index = analyzed_file_index(db, context.files);
    let position = index
        .binary_search_by_key(&ast_id.file, |(id, _)| *id)
        .ok()?;
    let &(_, file) = index.get(position)?;
    Some((file, BodyQuery::new(db, ast_id)))
}

/// `$receiver->name(...)`: the receiver's inferred type must decompose
/// to exactly one class or enum-case atom, non-null constituents
/// dropped (a union receiver, or an otherwise undecidable one, is
/// silent — decision 11's recorded stance).
fn resolved_method_call<'db>(
    context: &CheckContext<'db, '_>,
    receiver: ExpressionId,
    name: &str,
) -> Option<ResolvedCall<'db>> {
    let receiver_type = context.inferred.expression_type(receiver)?;
    let key = single_class_atom_key(context, receiver_type)?;
    resolved_member_call(context, &key, name)
}

/// The one class or enum-case key a receiver names, after dropping
/// `Null` atoms — `None` when zero or more than one atom remains, or
/// when the sole remaining atom is itself undecidable.
fn single_class_atom_key<'db>(
    context: &CheckContext<'db, '_>,
    receiver: TypeId<'db>,
) -> Option<String> {
    let mut atoms = atoms_of(context, receiver)
        .into_iter()
        .filter(|atom| !matches!(atom, ReceiverAtom::Null));
    let atom = atoms.next()?;
    if atoms.next().is_some() {
        return None;
    }
    match atom {
        ReceiverAtom::Class { key } | ReceiverAtom::Case { enum_key: key } => Some(key),
        ReceiverAtom::Null | ReceiverAtom::Undecidable => None,
    }
}

/// `Subject::name(...)`: `scoped_subject_keys` (task 4) must answer
/// exactly one key (`self`/`static`/`parent` already fold to the
/// owner's key there; any other name resolves through the global
/// symbol index) — a union or unresolvable subject is silent.
fn resolved_scoped_call<'db>(
    context: &CheckContext<'db, '_>,
    subject: ExpressionId,
    name: &str,
) -> Option<ResolvedCall<'db>> {
    let keys = scoped_subject_keys(context, subject)?;
    let [key] = keys.as_slice() else {
        return None;
    };
    resolved_member_call(context, key, name)
}

/// The declared method signature `key::name` names, plus its source
/// body when it has one. Shared by the instance-method and scoped-call
/// resolutions above: both fold their receiver down to a single class
/// key before reaching here.
fn resolved_member_call<'db>(
    context: &CheckContext<'db, '_>,
    key: &str,
    name: &str,
) -> Option<ResolvedCall<'db>> {
    let db = context.db;
    let query = MemberQuery::new(
        db,
        key.to_owned(),
        MemberKind::Method,
        folded_member_key(MemberKind::Method, name),
    );
    let signature = declared_member_signature(
        db,
        context.files,
        context.stubs,
        context.configuration,
        query,
    )?;
    Some(ResolvedCall {
        callee_display: name.to_owned(),
        signature,
        source_body: source_method_body(context, key, name),
    })
}

/// `(file, BodyQuery)` for a resolved member key, when `lookup_member`
/// answers `Source` — the `inferred_method_return` idiom
/// (`member.ast_id` bridged through `analyzed_file_index` exactly like
/// the free-function case). `None` for a stub, virtual, or unresolved
/// member.
fn source_method_body<'db>(
    context: &CheckContext<'db, '_>,
    key: &str,
    name: &str,
) -> Option<(SourceFile, BodyQuery<'db>)> {
    let db = context.db;
    let query = MemberQuery::new(
        db,
        key.to_owned(),
        MemberKind::Method,
        folded_member_key(MemberKind::Method, name),
    );
    let MemberResolution::Source { member, .. } = lookup_member(
        db,
        context.files,
        context.stubs,
        context.configuration,
        query,
    )?
    else {
        return None;
    };
    let index = analyzed_file_index(db, context.files);
    let position = index
        .binary_search_by_key(&member.ast_id.file, |(id, _)| *id)
        .ok()?;
    let &(_, file) = index.get(position)?;
    Some((file, BodyQuery::new(db, member.ast_id)))
}

/// `new Name(...)`/`new class { }(...)`: the named class resolved
/// through the same global lookup a scoped subject uses, or the
/// anonymous class's own synthetic key (task 1); no constructor at all
/// (`declared_member_signature` answers `None`) silences the whole call
/// (decision 12) rather than reporting on an assumed empty signature.
/// `new self()`/`new static()`/`new $dynamic()` are not covered:
/// resolving them needs the defining context's own placeholders, out of
/// this family's declared-tier-only scope, so they stay silent.
fn resolved_constructor_signature<'db>(
    context: &CheckContext<'db, '_>,
    class: &ClassReference,
) -> Option<ResolvedCall<'db>> {
    let (key, callee_display) = match class {
        ClassReference::Named { name } => (resolve_scoped_class_key(context, name)?, name.clone()),
        ClassReference::Anonymous { declaration } => (
            anonymous_class_key(*declaration),
            "class@anonymous".to_owned(),
        ),
        ClassReference::StaticKeyword
        | ClassReference::Dynamic { .. }
        | ClassReference::Missing => {
            return None;
        }
    };
    let db = context.db;
    let query = MemberQuery::new(
        db,
        key.clone(),
        MemberKind::Method,
        folded_member_key(MemberKind::Method, "__construct"),
    );
    let signature = declared_member_signature(
        db,
        context.files,
        context.stubs,
        context.configuration,
        query,
    )?;
    Some(ResolvedCall {
        callee_display,
        signature,
        source_body: source_method_body(context, &key, "__construct"),
    })
}

/// A written class or function name's last segment: `App\Sub\foo` and
/// `foo` alike display as `foo` — the flow walk's own convention for a
/// message that names a callee.
fn last_segment(written: &str) -> &str {
    written.rsplit('\\').next().unwrap_or(written)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use celerrate_project::PhpVersion;
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubParameter, StubSignature, StubSymbol, StubSymbolKind,
        VersionedTypeText, embedded_stub_index,
    };

    use super::super::test_support::{family_verdicts, fixture_with_stubs, handle_of};
    use super::super::{ArgumentLabel, TypedVerdictKind, typed_file_verdicts};

    const STRICT: &str = "<?php declare(strict_types=1);\n";

    #[test]
    fn a_failing_argument_reports_in_a_strict_file() {
        let verdicts = family_verdicts(&format!(
            "{STRICT}{}",
            r#"
function takes(int $n, string $s = ''): void {}
function f(mixed $anything, int|string $either): void {
    takes(1, 'ok');
    takes('wrong');        // reports: string against int
    takes($anything);      // mixed: guarded before the judgment, silent
    takes($either);        // int|string: one constituent fits, silent
    takes(1, s: 42);       // named argument, reports: int against string
}
"#
        ));
        assert_eq!(
            verdicts,
            vec![
                TypedVerdictKind::ArgumentType {
                    label: ArgumentLabel::Positional(1),
                    callee: "takes".to_owned(),
                    expected: "int".to_owned(),
                    given: "'wrong'".to_owned(),
                },
                TypedVerdictKind::ArgumentType {
                    label: ArgumentLabel::Named("s".to_owned()),
                    callee: "takes".to_owned(),
                    expected: "string".to_owned(),
                    given: "42".to_owned(),
                },
            ],
        );
    }

    #[test]
    fn a_weak_file_does_not_report_runtime_coercions() {
        let verdicts = family_verdicts(
            r#"<?php
function takes(int $n): void {}
class Plain {}
function f(Plain $object): void {
    takes('42');       // weak mode coerces: silent
    takes($object);    // no coercion exists: reports
}
"#,
        );
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::ArgumentType {
                label: ArgumentLabel::Positional(1),
                callee: "takes".to_owned(),
                expected: "int".to_owned(),
                given: "Plain".to_owned(),
            }],
        );
    }

    #[test]
    fn the_exemptions_are_structural() {
        // `fills('x', 1)`'s first argument is a string literal against
        // `array &$out`: definitely non-`mixed` and definitely not
        // assignable to `array`, so this is load-bearing on the
        // by-reference `continue` alone, not on the `mixed` guard
        // (a never-assigned local would infer `mixed` and pass through
        // that guard regardless of the by-reference exemption, proving
        // nothing). Verified: temporarily deleting the `by_reference`
        // `continue` in `check_argument_types` turns this into a
        // reported `ArgumentType { expected: "array", given: "'x'" }`
        // and this test fails; restoring it goes green again.
        let verdicts = family_verdicts(&format!(
            "{STRICT}{}",
            r#"
function fills(array &$out, int $n): void {}
class A { public function m(int $n): void {} }
class B { public function m(int $n): void {} }
function f(A|B $either): void {
    fills('x', 1);           // by-reference parameter: exempt
    $either->m('x');         // union receiver: silent (recorded stance)
}
"#
        ));
        assert_eq!(verdicts, vec![]);
    }

    #[test]
    fn a_constructor_less_class_is_silent_but_a_declared_constructor_still_reports() {
        // Decision 12: a class with no `__construct` at all makes the
        // whole `new` call silent (`resolved_constructor_signature`
        // answers `None`), rather than checking against an assumed
        // empty parameter list. `Typed`'s own mismatched constructor
        // call in the same fixture proves the silence is specifically
        // about "no constructor", not "this test file checks nothing".
        let verdicts = family_verdicts(&format!(
            "{STRICT}{}",
            r#"
class Plain {}
class Typed { public function __construct(int $n) {} }
function f(): void {
    new Plain(1);      // no constructor at all: silent
    new Typed('x');    // reports: string against int
}
"#
        ));
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::ArgumentType {
                label: ArgumentLabel::Positional(1),
                callee: "Typed".to_owned(),
                expected: "int".to_owned(),
                given: "'x'".to_owned(),
            }],
        );
    }

    #[test]
    fn a_spread_argument_halts_positional_matching() {
        // A wrong-typed argument before the first spread still reports
        // (positional matching up to that point is not in question);
        // the spread itself, and every argument after it, are not
        // checked at all — even one that is just as clearly wrong —
        // because a spread makes later positional matching
        // undecidable without evaluating it.
        let verdicts = family_verdicts(&format!(
            "{STRICT}{}",
            r#"
function takes(int $a, int $b, int $c): void {}
function f(array $rest): void {
    takes('wrong', ...$rest, 'also-wrong');
}
"#
        ));
        assert_eq!(
            verdicts,
            vec![TypedVerdictKind::ArgumentType {
                label: ArgumentLabel::Positional(1),
                callee: "takes".to_owned(),
                expected: "int".to_owned(),
                given: "'wrong'".to_owned(),
            }],
        );
    }

    /// A one-parameter stub signature whose forms across the
    /// configured range have no most-restrictive member: `int` at 8.1,
    /// `string` from 8.2 — the design's empty-intersection guard
    /// (plan 3) silences the parameter entirely
    /// (`DeclaredParameter::parameter_type` is `None`), mirroring
    /// `declared.rs`'s own `disjoint_signature` fixture.
    fn disjoint_stub_signature() -> StubSignature {
        StubSignature {
            parameters: vec![StubParameter {
                name: "value".to_owned(),
                type_text: VersionedTypeText {
                    default: Some("int".to_owned()),
                    overrides: vec![(PhpVersion::new(8, 2), "string".to_owned())],
                },
                optional: false,
                by_reference: false,
                variadic: false,
                availability: StubAvailability::ALWAYS,
            }],
            return_type: VersionedTypeText::from_text(Some("void".to_owned())),
            by_reference: false,
        }
    }

    #[test]
    fn a_disjoint_stub_parameter_is_silently_unchecked() {
        // The empty-intersection stub guard (plan 3, consumed here for
        // the first time by `check_argument_types`'s `let Some(parameter_type)
        // = parameter.parameter_type else { continue; }`): a parameter
        // with no single most-restrictive type across the configured
        // PHP range silences the check outright, whatever the argument
        // is — an array literal here, which would fail assignability
        // against either per-version form (`int` or `string`) were the
        // guard not silencing the parameter first.
        let index = StubIndex::new(
            vec![StubSymbol {
                name: "disjoint".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            }],
            vec![("disjoint".to_owned(), disjoint_stub_signature())],
            vec![],
        );
        let fixture = fixture_with_stubs(
            &[r#"<?php
function f(): void {
    disjoint([]);
}
"#],
            index,
        );
        let verdicts: Vec<TypedVerdictKind> = typed_file_verdicts(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            handle_of(&fixture, 0),
        )
        .verdicts
        .iter()
        .map(|verdict| verdict.kind.clone())
        .collect();
        assert_eq!(verdicts, vec![]);
    }

    #[test]
    fn methods_static_calls_constructors_and_variadics_are_checked() {
        let verdicts = family_verdicts(&format!(
            "{STRICT}{}",
            r#"
class Mailer {
    public function __construct(private string $dsn) {}
    public function send(string $to): void {}
    public static function make(string $dsn): static { return new static($dsn); }
}
function f(Mailer $m): void {
    $m->send('a@b');
    $m->send(42);              // reports
    Mailer::make(42);          // reports
    new Mailer(42);            // reports
    variadic('a', 'b', 42);    // reports on the third
}
function variadic(string ...$parts): void {}
"#
        ));
        assert_eq!(
            verdicts,
            vec![
                TypedVerdictKind::ArgumentType {
                    label: ArgumentLabel::Positional(1),
                    callee: "send".to_owned(),
                    expected: "string".to_owned(),
                    given: "42".to_owned(),
                },
                TypedVerdictKind::ArgumentType {
                    label: ArgumentLabel::Positional(1),
                    callee: "make".to_owned(),
                    expected: "string".to_owned(),
                    given: "42".to_owned(),
                },
                TypedVerdictKind::ArgumentType {
                    label: ArgumentLabel::Positional(1),
                    callee: "Mailer".to_owned(),
                    expected: "string".to_owned(),
                    given: "42".to_owned(),
                },
                TypedVerdictKind::ArgumentType {
                    label: ArgumentLabel::Positional(3),
                    callee: "variadic".to_owned(),
                    expected: "string".to_owned(),
                    given: "42".to_owned(),
                },
            ],
        );
    }

    #[test]
    fn arity_reports_missing_excess_and_unknown_names() {
        let verdicts = family_verdicts(&format!(
            "{STRICT}{}",
            r#"
function takes(int $a, string $b = '', int ...$rest): void {}
function pair(int $a, int $b): void {}
function f(): void {
    takes(1);                  // optional + variadic satisfied: silent
    takes(1, 'x', 2, 3);       // variadic absorbs: silent
    pair(1, 2, 3);             // reports CEL0037
    pair(1);                   // reports CEL0036
    pair(b: 2, a: 1);          // named fill: silent
    pair(1, c: 2);             // reports CEL0038 (and CEL0036 for $b)
}
"#
        ));
        assert_eq!(
            verdicts,
            vec![
                TypedVerdictKind::TooManyArguments {
                    callee: "pair".to_owned(),
                    given: 3,
                    accepted: 2,
                },
                TypedVerdictKind::TooFewArguments {
                    callee: "pair".to_owned(),
                    given: 1,
                    required: 2,
                },
                TypedVerdictKind::UnknownNamedArgument {
                    callee: "pair".to_owned(),
                    name: "c".to_owned(),
                },
                TypedVerdictKind::TooFewArguments {
                    callee: "pair".to_owned(),
                    given: 2,
                    required: 2,
                },
            ],
        );
    }

    #[test]
    fn a_variadic_signature_accepts_any_named_argument() {
        // PHP 8.0 collects unknown named arguments into a trailing
        // variadic (decision 12); reporting CEL0038 here would flag
        // working code.
        let verdicts = family_verdicts(&format!(
            "{STRICT}{}",
            r#"
function sink(int $first, int ...$rest): void {}
function f(): void {
    sink(1, extra: 2, more: 3);
}
"#
        ));
        assert_eq!(verdicts, vec![]);
    }

    #[test]
    fn spread_and_argument_capture_silence_arity() {
        // The `\DateTime` line needs the real embedded stub surface
        // (a synthetic single-class fixture has no constructor to
        // resolve against), so this test builds its own fixture
        // rather than going through `family_verdicts`'s minimal stub
        // index — the same `fixture_with_stubs` plus
        // `celerrate_stubs::embedded_stub_index` idiom
        // `a_disjoint_stub_parameter_is_silently_unchecked` above and
        // `inference.rs`'s `fixture_with_embedded_stubs` already use.
        let fixture = fixture_with_stubs(
            &[&format!(
                "{STRICT}{}",
                r#"
function pair(int $a, int $b): void {}
function capturing(): void { $all = func_get_args(); }
function f(array $bag): void {
    pair(...$bag);             // spread: all three arity checks silent
    capturing(1, 2, 3);        // captures arguments: excess silent
    new \DateTime('now');      // stub constructors resolve normally
}
"#
            )],
            embedded_stub_index().unwrap(),
        );
        let verdicts: Vec<TypedVerdictKind> = typed_file_verdicts(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            handle_of(&fixture, 0),
        )
        .verdicts
        .iter()
        .map(|verdict| verdict.kind.clone())
        .collect();
        assert_eq!(verdicts, vec![]);
    }
}
