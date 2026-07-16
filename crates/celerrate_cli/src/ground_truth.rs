//! The ground-truth channel (design section 10, harness 1): a hidden,
//! internal record stream that confronts the inference engine with the
//! docblock annotations it is not allowed to see while inferring.
//!
//! For every source function and method whose docblock-annotated
//! return exists and whose body exists, the inferred return is
//! measured against the annotated one under the compatibility
//! relation: `Proof::Fails` is a divergence (printed), `Proof::
//! CannotProve` passes in silence (inference-only generics make
//! precision asymmetric by design, per decision 13). This never ships
//! a diagnostic: no `CEL####` identifier, no rendering change. `cargo
//! xtask ground-truth` (task 12) pins the exact record format below
//! against a committed baseline, so nothing here may change it
//! casually.

use std::io::Write;

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
/// the sorted records, then exactly one summary line. Never fails:
/// there is no analyzable input this walks that can panic, so there is
/// no error path to report.
pub fn run(session: &Session, output: &mut dyn Write) {
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
        let _ = writeln!(output, "{record}");
    }
    let _ = writeln!(output, "checked {checked}, divergences {divergences}");
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
    *checked += 1;

    let Some(declared) = declared_function_signature(
        database,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
        FunctionQuery::new(database, key.clone()),
    ) else {
        return;
    };
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
            inferred.display(database),
            annotated.display(database),
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
    *checked += 1;

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
            inferred.display(database),
            annotated.display(database),
        ));
    }
}
