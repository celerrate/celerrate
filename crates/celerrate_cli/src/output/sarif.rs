//! `--output=sarif`: SARIF 2.1.0, the honest subset. What SARIF cannot
//! carry (the needs-review confidence, the engine notes) rides in
//! `properties`, never twisted into a standard field.

use std::io::{self, Write};

use serde_json::{Value, json};

use super::model::{
    MachineReport, ReportedAnchor, ReportedDiagnostic, ReportedSeverity, ReportedSuggestion,
    SpanLocation,
};

pub fn write(output: &mut dyn Write, report: &MachineReport) -> io::Result<()> {
    let document = document(report);
    serde_json::to_writer_pretty(&mut *output, &document).map_err(io::Error::from)?;
    writeln!(output)
}

fn document(report: &MachineReport) -> Value {
    json!({
        "$schema": "https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "celerrate",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/celerrate/celerrate",
                    "rules": rules(report),
                }
            },
            "columnKind": "unicodeCodePoints",
            "invocations": [invocation(report)],
            "results": results(report),
        }]
    })
}

/// The run's single invocation: the exit code and whether the run
/// survived cleanly, plus the tool's own internal errors as
/// `toolExecutionNotifications`, the standard place for a problem the
/// tool itself hit rather than a finding about the analyzed code. The
/// key is omitted entirely when the run hit none.
fn invocation(report: &MachineReport) -> Value {
    let mut invocation = json!({
        "executionSuccessful": report.summary.internal_errors == 0,
        "exitCode": report.summary.exit_code,
    });
    if !report.internal_errors.is_empty()
        && let Some(object) = invocation.as_object_mut()
    {
        object.insert(
            "toolExecutionNotifications".to_owned(),
            json!(
                report
                    .internal_errors
                    .iter()
                    .map(|error| json!({
                        "level": "error",
                        "message": { "text": error.message },
                        "descriptor": { "id": error.kind },
                    }))
                    .collect::<Vec<Value>>()
            ),
        );
    }
    invocation
}

/// Reporting descriptors for exactly the identifiers this run referenced,
/// sorted and unique: deterministic output, no dead catalogue.
fn rules(report: &MachineReport) -> Vec<Value> {
    let mut identifiers: Vec<&str> = report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.id.as_str())
        .chain(report.notices.iter().map(|notice| notice.id.as_str()))
        .collect();
    identifiers.sort_unstable();
    identifiers.dedup();
    identifiers
        .into_iter()
        .map(|text| match described(text) {
            Some((family, why)) => json!({
                "id": text,
                "shortDescription": { "text": family },
                "fullDescription": { "text": why },
                "help": {
                    "text": format!("Run `celerrate explain {text}` for the full page."),
                },
            }),
            // Resilience: an identifier outside the registry still gets
            // a descriptor, never a crash.
            None => json!({ "id": text }),
        })
        .collect()
}

fn described(text: &str) -> Option<(&'static str, &'static str)> {
    let id = celerrate_diagnostics::find_identifier(text)?;
    let entry = celerrate_diagnostics::REGISTRY
        .iter()
        .find(|entry| entry.id == id)?;
    Some((entry.family, entry.explain.why))
}

fn results(report: &MachineReport) -> Vec<Value> {
    let mut results = Vec::new();
    for notice in &report.notices {
        results.push(json!({
            "ruleId": notice.id,
            "level": "note",
            "message": { "text": notice.message },
        }));
    }
    for diagnostic in &report.diagnostics {
        results.push(result(diagnostic));
    }
    results
}

fn result(diagnostic: &ReportedDiagnostic) -> Value {
    let level = match diagnostic.severity {
        ReportedSeverity::Error => "error",
        ReportedSeverity::Warning => "warning",
    };
    let mut value = json!({
        "ruleId": diagnostic.id,
        "level": level,
        "message": { "text": diagnostic.message },
    });
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    if let ReportedAnchor::Span(location) = &diagnostic.anchor {
        object.insert("locations".to_owned(), json!([physical_location(location)]));
    }
    if !diagnostic.labels.is_empty() {
        let related: Vec<Value> = diagnostic
            .labels
            .iter()
            .map(|label| {
                let mut related = physical_location(&label.location);
                if let Some(object) = related.as_object_mut() {
                    object.insert("message".to_owned(), json!({ "text": label.message }));
                }
                related
            })
            .collect();
        object.insert("relatedLocations".to_owned(), json!(related));
    }
    let (safe, needs_review): (Vec<&ReportedSuggestion>, Vec<&ReportedSuggestion>) = diagnostic
        .suggestions
        .iter()
        .partition(|suggestion| suggestion.confidence == super::model::ReportedConfidence::Safe);
    if !safe.is_empty() {
        let fixes: Vec<Value> = safe.iter().map(|suggestion| fix(suggestion)).collect();
        object.insert("fixes".to_owned(), json!(fixes));
    }
    let mut properties = serde_json::Map::new();
    if !needs_review.is_empty() {
        properties.insert(
            "needsReviewSuggestions".to_owned(),
            serde_json::to_value(&needs_review).unwrap_or(Value::Null),
        );
    }
    if !diagnostic.notes.is_empty() {
        properties.insert(
            "notes".to_owned(),
            serde_json::to_value(&diagnostic.notes).unwrap_or(Value::Null),
        );
    }
    if !properties.is_empty() {
        object.insert("properties".to_owned(), Value::Object(properties));
    }
    value
}

fn physical_location(location: &SpanLocation) -> Value {
    json!({
        "physicalLocation": {
            "artifactLocation": { "uri": location.path },
            "region": {
                "startLine": location.start_line,
                "startColumn": location.start_column,
                "endLine": location.end_line,
                "endColumn": location.end_column,
                "byteOffset": location.byte_start,
                "byteLength": location.byte_end.saturating_sub(location.byte_start),
            }
        }
    })
}

fn fix(suggestion: &ReportedSuggestion) -> Value {
    let changes: Vec<Value> = suggestion
        .edits
        .iter()
        .map(|edit| {
            json!({
                "artifactLocation": { "uri": edit.location.path },
                "replacements": [{
                    "deletedRegion": {
                        "byteOffset": edit.location.byte_start,
                        "byteLength": edit.location.byte_end
                            .saturating_sub(edit.location.byte_start),
                    },
                    "insertedContent": { "text": edit.replacement },
                }]
            })
        })
        .collect();
    json!({
        "description": { "text": suggestion.message },
        "artifactChanges": changes,
    })
}
