//! The mixed-rate instrument: a hidden, internal counter stream
//! measuring how much `mixed` remains once inference has run over
//! stub-only call boundaries. This never ships a diagnostic: no
//! `CEL####` identifier, no rendering change. `cargo xtask mixed-rate`
//! byte-compares the exact report format below against a committed
//! baseline (the corpus-snapshot pattern — plain counters, no
//! classification column), so nothing here may change it casually.
//!
//! **Report format**: line 1 `expressions <total>\tmixed <count>`,
//! the whole-corpus expression-typing residual, every body's
//! `expression_types` folded together (this line includes stub
//! *method* calls, the class-refinement channel, which move this
//! counter but never enter the per-callee table below, by deliberate
//! scope), then line 2
//! `element-positions <total>\telement-mixed <count>`, the
//! element-level mixed metric (issue #45): every
//! reported expression's `TypeId::element_positions` folded together,
//! counting structural constituent slots (array/list key and value,
//! shape field values, union constituents recursed but the union node
//! itself never a position) rather than whole expressions, additive
//! alongside line 1 and never a reclassification of it — then one
//! `<callee>\t<mixed>\t<total>` line per stub free-function callee,
//! sorted by callee, trailing newline.

use std::io::{self, Write};

use celerrate_semantics::{AstId, BodyQuery, MemberKind, member_tree};
use celerrate_types::{StubCallRecord, inferred_body_types};

use crate::analysis::AnalysisInputs;
use crate::database::AnalysisDatabase;
use crate::session::Session;

/// Runs the instrument over an already-started session: every
/// reported file's `member_tree`, its free functions and its
/// classes' own methods, each body folded into the running totals
/// through `inferred_body_types` — the same loader and the same body
/// enumeration `ground_truth::run` uses, never a parallel loader. Prints the rendered report and exits clean; there
/// is no analyzable input this walks that can panic, but the output
/// stream itself can fail, mirroring `ground_truth::run`'s
/// precedent rather than swallowing the error.
pub fn run(session: &Session, output: &mut dyn Write) -> io::Result<()> {
    let inputs = session.inputs();
    let database = &inputs.database;

    let mut expressions = 0usize;
    let mut mixed = 0usize;
    let mut element_total = 0usize;
    let mut element_mixed = 0usize;
    let mut calls: Vec<StubCallRecord> = Vec::new();

    for &file in inputs.reported.iter() {
        let tree = member_tree(database, file);

        for function in &tree.functions {
            accumulate(
                database,
                &inputs,
                file,
                function.ast_id,
                &mut expressions,
                &mut mixed,
                &mut element_total,
                &mut element_mixed,
                &mut calls,
            );
        }

        for class in &tree.classes {
            for member in &class.members {
                if member.kind != MemberKind::Method {
                    continue;
                }
                accumulate(
                    database,
                    &inputs,
                    file,
                    member.ast_id,
                    &mut expressions,
                    &mut mixed,
                    &mut element_total,
                    &mut element_mixed,
                    &mut calls,
                );
            }
        }
    }

    write!(
        output,
        "{}",
        render_report(expressions, mixed, element_total, element_mixed, &calls)
    )
}

/// One body's contribution to the running totals: absent when the
/// identity carries no body, mirroring `inferred_body_types`'s own
/// `None` case directly — unlike `ground_truth::run`, this instrument
/// has no annotation gate to check first, so every function and
/// method body in the reported set is folded in.
#[allow(clippy::too_many_arguments)]
fn accumulate(
    database: &AnalysisDatabase,
    inputs: &AnalysisInputs,
    file: celerrate_db::SourceFile,
    ast_id: AstId,
    expressions: &mut usize,
    mixed: &mut usize,
    element_total: &mut usize,
    element_mixed: &mut usize,
    calls: &mut Vec<StubCallRecord>,
) {
    let Some(inferred) = inferred_body_types(
        database,
        inputs.files,
        inputs.stubs,
        inputs.configuration,
        file,
        BodyQuery::new(database, ast_id),
    ) else {
        return;
    };
    *expressions += inferred.expression_types.len();
    *mixed += inferred
        .expression_types
        .iter()
        .filter(|of| of.is_mixed(database))
        .count();
    for &of in &inferred.expression_types {
        let positions = of.element_positions(database);
        *element_total += positions.total;
        *element_mixed += positions.mixed;
    }
    calls.extend(inferred.stub_calls.iter().cloned());
}

/// The pure aggregation, unit-tested ahead of the wiring above: sums
/// every stub callee's mixed and total call counts and renders the
/// report in the exact documented format, sorted by callee for
/// byte-reproducibility (`BTreeMap` iterates in key order — the
/// `--bless`/byte-compare pattern this instrument's `xtask` gate
/// depends on).
pub(crate) fn render_report(
    expressions: usize,
    mixed: usize,
    element_total: usize,
    element_mixed: usize,
    calls: &[StubCallRecord],
) -> String {
    use std::collections::BTreeMap;
    let mut per_callee: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for record in calls {
        let entry = per_callee.entry(record.callee.as_str()).or_insert((0, 0));
        entry.1 += 1;
        if record.mixed {
            entry.0 += 1;
        }
    }
    let mut report = format!("expressions {expressions}\tmixed {mixed}\n");
    report.push_str(&format!(
        "element-positions {element_total}\telement-mixed {element_mixed}\n"
    ));
    for (callee, (mixed_calls, total)) in per_callee {
        report.push_str(&format!("{callee}\t{mixed_calls}\t{total}\n"));
    }
    report
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use std::io;

    use celerrate_types::StubCallRecord;

    use super::{render_report, run};
    use crate::session::Session;

    #[test]
    fn the_report_sorts_by_callee_and_sums_per_callee() {
        let report = render_report(
            7,
            3,
            11,
            5,
            &[
                StubCallRecord {
                    callee: "b".to_owned(),
                    mixed: true,
                },
                StubCallRecord {
                    callee: "a".to_owned(),
                    mixed: false,
                },
                StubCallRecord {
                    callee: "b".to_owned(),
                    mixed: false,
                },
            ],
        );
        assert_eq!(
            report,
            "expressions 7\tmixed 3\n\
             element-positions 11\telement-mixed 5\n\
             a\t0\t1\n\
             b\t1\t2\n",
        );
    }

    #[test]
    fn an_empty_call_set_still_prints_the_summary_line() {
        let report = render_report(0, 0, 0, 0, &[]);
        assert_eq!(
            report,
            "expressions 0\tmixed 0\n\
             element-positions 0\telement-mixed 0\n",
        );
    }

    /// The production entry point, invoked end to end: a stub-only
    /// free-function call in a real project is folded into both the
    /// whole-corpus counters and the per-callee table `run` prints —
    /// pinning that `run` actually walks bodies and calls
    /// `inferred_body_types`, not merely that `render_report` can
    /// format a hand-built vector.
    #[test]
    fn run_walks_a_real_project_and_prints_a_stub_call() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("code.php"),
            "<?php\nfunction consume(): void {\n    unserialize('x');\n}\n",
        )
        .unwrap();
        let session = Session::start(project.path());
        let mut output = Vec::new();

        run(&session, &mut output).unwrap();

        let text = String::from_utf8(output).unwrap();
        let mut lines = text.lines();
        let summary = lines.next().unwrap();
        assert!(summary.starts_with("expressions "), "{summary}");
        assert!(
            text.contains("unserialize\t1\t1\n"),
            "the stub call must be recorded and printed: {text}",
        );
    }

    /// A writer that fails on its very first byte, so a truncated
    /// stream is observable without depending on real I/O exhaustion
    /// (mirrors `ground_truth`'s precedent).
    struct FailingWriter;

    impl io::Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("simulated write failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_write_failure_is_propagated_rather_than_swallowed() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("code.php"), "<?php\nfunction f() {}\n").unwrap();
        let session = Session::start(project.path());
        let mut writer = FailingWriter;

        assert!(
            run(&session, &mut writer).is_err(),
            "a truncated report stream must surface as an error, not a silent success",
        );
    }
}
