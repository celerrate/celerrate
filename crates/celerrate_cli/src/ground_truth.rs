//! The ground-truth channel: a hidden,
//! internal record stream that confronts the inference engine with the
//! docblock annotations it is not allowed to see while inferring.
//!
//! For every source function and method whose docblock-annotated
//! return exists and whose body exists, the inferred return is
//! measured against the annotated one under the compatibility
//! relation: `Proof::Fails` is a divergence (printed), `Proof::
//! CannotProve` passes in silence (inference-only generics make
//! precision asymmetric by design). This never ships
//! a diagnostic: no `CEL####` identifier, no rendering change. `cargo
//! xtask ground-truth` pins the exact record format below
//! against a committed baseline, so nothing here may change it
//! casually.
//!
//! **Record format**: one line per divergence,
//! `<symbol>\t<inferred display>\t<annotated display>`, sorted by the
//! full line, followed by exactly one summary line
//! `checked <N>, divergences <M>`. `<symbol>` is `<class key>::
//! <member key>` for methods, the folded function key for functions.
//!
//! **Escaping**: `display.rs`'s literal-string rendering
//! (`StringConstraint::Literal`) is unescaped by design — it is shared
//! rendering used elsewhere, and this channel may not change it (no
//! rendering change). A literal like `'a<TAB>b<LF>c'` would otherwise
//! inject a raw tab or newline straight into a tab-separated,
//! one-record-per-line stream, breaking both invariants. So the
//! inferred and annotated display fields (never the symbol, which
//! cannot contain these bytes) are escaped at the record boundary,
//! here, before being joined: backslash first (`\` becomes `\\`, so
//! the scheme round-trips unambiguously), then tab (`\t`), newline
//! (`\n`) and carriage return (`\r`) become their familiar
//! two-character escapes. This guarantees one record is always
//! exactly one line and every field always splits cleanly on a
//! literal tab, deterministically and stably, since `cargo xtask
//! ground-truth` commits the output to a baseline file and diffs
//! against it.

use std::io::{self, Write};

use celerrate_semantics::{
    BodyQuery, FreeFunction, Member, MemberKind, MemberQuery, SymbolSpace, body_ir,
    folded_member_key, folded_symbol_key, fully_qualified_name, member_tree,
};
use celerrate_types::{
    FunctionQuery, MethodQuery, Proof, declared_function_signature, declared_member_signature,
    function_annotations, inferred_function_return, inferred_method_return, member_annotations,
    subtype_of,
};

use crate::analysis::AnalysisInputs;
use crate::database::AnalysisDatabase;
use crate::session::Session;

/// Runs the channel over an already-started session: every reported
/// file's `member_tree`, its free functions and its classes' own
/// methods, each either a silent pass or a divergence record. Prints
/// the sorted records, then exactly one summary line. There is no
/// analyzable input this walks that can panic, but the output stream
/// itself can fail (a truncated pipe, a full disk): that failure is
/// propagated, mirroring `render_check`'s precedent, rather than
/// swallowed into a false `Outcome::Clean`.
pub fn run(session: &Session, output: &mut dyn Write) -> io::Result<()> {
    let inputs = session.inputs();
    let database = &inputs.database;

    let mut checked = 0usize;
    let mut records = Vec::new();

    for &file in inputs.reported.iter() {
        let tree = member_tree(database, file);

        for function in &tree.functions {
            check_function(
                database,
                &inputs,
                file,
                function,
                &mut checked,
                &mut records,
            );
        }

        for class in &tree.classes {
            // Anonymous class-likes have no key: there is no stable
            // symbol to print a record against, so they are skipped,
            // not guessed at.
            let Some(class_name) = class.name.as_deref() else {
                continue;
            };
            let class_key = folded_symbol_key(
                SymbolSpace::ClassLike,
                &fully_qualified_name(&class.namespace, class_name),
            );
            for member in &class.members {
                if member.kind != MemberKind::Method {
                    continue;
                }
                check_method(
                    database,
                    &inputs,
                    file,
                    &class_key,
                    member,
                    &mut checked,
                    &mut records,
                );
            }
        }
    }

    records.sort();
    let divergences = records.len();
    for record in &records {
        writeln!(output, "{record}")?;
    }
    writeln!(output, "checked {checked}, divergences {divergences}")
}

/// Neutralizes the control characters that would otherwise break the
/// record's own invariants: backslash first, so the scheme round-trips
/// unambiguously, then tab, newline and carriage return. See the
/// module doc's "Escaping" section for why this exists and why
/// `display.rs` itself is not the place to fix it.
fn escape_field(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// One free function: annotation and body presence gate whether it is
/// checked at all; the check itself is `inferred` measured against
/// `declared`'s value type.
fn check_function(
    database: &AnalysisDatabase,
    inputs: &AnalysisInputs,
    file: celerrate_db::SourceFile,
    function: &FreeFunction,
    checked: &mut usize,
    records: &mut Vec<String>,
) {
    let key = folded_symbol_key(
        SymbolSpace::Function,
        &fully_qualified_name(&function.namespace, &function.name),
    );
    let has_annotation = function_annotations(
        database,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
        FunctionQuery::new(database, key.clone()),
    )
    .value
    .is_some();
    if !has_annotation {
        return;
    }
    let has_body = body_ir(database, file, BodyQuery::new(database, function.ast_id)).is_some();
    if !has_body {
        return;
    }

    let Some(declared) = declared_function_signature(
        database,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
        FunctionQuery::new(database, key.clone()),
    ) else {
        return;
    };
    *checked += 1;

    let inferred = inferred_function_return(
        database,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
        FunctionQuery::new(database, key.clone()),
    );
    let annotated = declared.value_type;
    if matches!(
        subtype_of(
            database,
            inputs.files,
            inputs.stubs,
            inputs.configuration,
            inferred,
            annotated,
        ),
        Proof::Fails
    ) {
        records.push(format!(
            "{key}\t{}\t{}",
            escape_field(&inferred.display(database)),
            escape_field(&annotated.display(database)),
        ));
    }
}

/// One class's own method: the symmetric case, keyed by `class_key ::
/// member_key`.
fn check_method(
    database: &AnalysisDatabase,
    inputs: &AnalysisInputs,
    file: celerrate_db::SourceFile,
    class_key: &str,
    member: &Member,
    checked: &mut usize,
    records: &mut Vec<String>,
) {
    let member_key = folded_member_key(MemberKind::Method, &member.name);
    let has_annotation = member_annotations(
        database,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
        MemberQuery::new(
            database,
            class_key.to_owned(),
            MemberKind::Method,
            member_key.clone(),
        ),
    )
    .value
    .is_some();
    if !has_annotation {
        return;
    }
    let has_body = body_ir(database, file, BodyQuery::new(database, member.ast_id)).is_some();
    if !has_body {
        return;
    }

    let Some(declared) = declared_member_signature(
        database,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
        MemberQuery::new(
            database,
            class_key.to_owned(),
            MemberKind::Method,
            member_key.clone(),
        ),
    ) else {
        return;
    };
    *checked += 1;

    let inferred = inferred_method_return(
        database,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
        MethodQuery::new(database, class_key.to_owned(), member_key.clone()),
    );
    let annotated = declared.value_type;
    if matches!(
        subtype_of(
            database,
            inputs.files,
            inputs.stubs,
            inputs.configuration,
            inferred,
            annotated,
        ),
        Proof::Fails
    ) {
        records.push(format!(
            "{class_key}::{member_key}\t{}\t{}",
            escape_field(&inferred.display(database)),
            escape_field(&annotated.display(database)),
        ));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use std::io;

    use super::{escape_field, run};
    use crate::session::Session;

    /// Pinned directly (rather than only indirectly through a fixture
    /// whose display happens to contain one): backslash escapes first,
    /// so `\t` in the output can only ever mean an escaped tab, never
    /// an escaped-then-literal-`t` ambiguity, and the three control
    /// characters the record format forbids all round-trip through a
    /// recognizable two-character form.
    #[test]
    fn escape_field_neutralizes_backslash_and_the_three_control_characters() {
        assert_eq!(escape_field("plain"), "plain");
        assert_eq!(escape_field("a\tb\nc\rd"), "a\\tb\\nc\\rd");
        assert_eq!(escape_field("back\\slash"), "back\\\\slash");
        // The escaping character itself must not be re-escaped by a
        // later pass, and a backslash immediately followed by a
        // control character must stay unambiguous.
        assert_eq!(escape_field("\\\t"), "\\\\\\t");
    }

    /// A writer that fails on its very first byte, so a truncated
    /// stream is observable without depending on real I/O exhaustion.
    struct FailingWriter;

    impl io::Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("simulated write failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// `render_check` escalates a render failure to `Outcome::
    /// InternalError` rather than reporting a clean run over a
    /// truncated stream; this channel must follow the same precedent
    /// instead of swallowing the error behind `let _ = writeln!(...)`.
    #[test]
    fn a_write_failure_is_propagated_rather_than_swallowed() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("code.php"),
            "<?php\nnamespace App;\nfunction f() {}\n",
        )
        .unwrap();
        let session = Session::start(project.path());
        let mut writer = FailingWriter;

        assert!(
            run(&session, &mut writer).is_err(),
            "a truncated record stream must surface as an error, not a silent success",
        );
    }
}
