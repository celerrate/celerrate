//! `--output=sarif`: SARIF 2.1.0, the honest subset. What SARIF cannot
//! carry (the needs-review confidence, the engine notes) rides in
//! `properties`, never twisted into a standard field.

use std::collections::BTreeMap;
use std::fmt::Write as _;
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
        "runs": [run(report)]
    })
}

/// The run's `properties` bag carries `baselinedHidden`: nothing else in
/// the document can express it, and without it a run with no results and
/// a dozen baselined findings is indistinguishable from a genuinely clean
/// project.
fn run(report: &MachineReport) -> Value {
    json!({
        "tool": { "driver": driver(report) },
        "columnKind": "unicodeCodePoints",
        "invocations": [invocation(report)],
        "results": results(report),
        "properties": { "baselinedHidden": report.summary.baselined_hidden },
    })
}

/// The tool component: the rule descriptors this run's identifiers
/// referenced, plus the notification descriptors its internal-error
/// kinds referenced. `notifications` is omitted entirely when the run
/// hit none, mirroring how `rules` never ships a dead catalogue.
fn driver(report: &MachineReport) -> Value {
    let mut driver = json!({
        "name": "celerrate",
        "version": env!("CARGO_PKG_VERSION"),
        "informationUri": "https://github.com/celerrate/celerrate",
        "rules": rules(report),
    });
    let descriptors = notification_descriptors(report);
    if !descriptors.is_empty()
        && let Some(object) = driver.as_object_mut()
    {
        object.insert("notifications".to_owned(), json!(descriptors));
    }
    driver
}

/// Notification descriptors for exactly the internal-error kinds this run
/// hit, sorted and unique: a notification's `descriptor.id` resolves
/// against this array, so leaving it empty would dangle the reference
/// instead of merely omitting a nice-to-have.
fn notification_descriptors(report: &MachineReport) -> Vec<Value> {
    let mut kinds: Vec<&str> = report
        .internal_errors
        .iter()
        .map(|error| error.kind.as_str())
        .collect();
    kinds.sort_unstable();
    kinds.dedup();
    kinds
        .into_iter()
        .map(|kind| json!({ "id": kind }))
        .collect()
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
    let owning_rule = owning_rule_by_identifier(report);
    identifiers
        .into_iter()
        .map(|text| {
            let mut descriptor = match described(text) {
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
            };
            if let Some(rule) = owning_rule.get(text)
                && let Some(object) = descriptor.as_object_mut()
            {
                object.insert("properties".to_owned(), json!({ "rule": rule }));
            }
            descriptor
        })
        .collect()
}

/// Identifier to owning rule name, sourced from the diagnostics that
/// reference it: the relation is constant per identifier, so the
/// descriptor is the economical place to carry it once rather than
/// repeating it on every result. Identifiers no rule owns (syntax,
/// project, configuration, and every notice-only identifier) are simply
/// absent.
fn owning_rule_by_identifier(report: &MachineReport) -> BTreeMap<&str, &str> {
    let mut owning_rule = BTreeMap::new();
    for diagnostic in &report.diagnostics {
        if let Some(rule) = &diagnostic.rule {
            owning_rule
                .entry(diagnostic.id.as_str())
                .or_insert(rule.as_str());
        }
    }
    owning_rule
}

/// One registry lookup, not two: the previous version called
/// `find_identifier` to resolve `text` to an id and then re-scanned
/// `REGISTRY` a second time to find the very entry that first scan had
/// already located, just to reach `family` and `explain.why`. Matching
/// directly on `entry.id.as_str() == text` gets both fields from the one
/// entry in a single pass.
fn described(text: &str) -> Option<(&'static str, &'static str)> {
    let entry = celerrate_diagnostics::REGISTRY
        .iter()
        .find(|entry| entry.id.as_str() == text)?;
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
        let suggestions: Vec<Value> = needs_review
            .iter()
            .map(|suggestion| needs_review_suggestion(suggestion))
            .collect();
        properties.insert("needsReviewSuggestions".to_owned(), json!(suggestions));
    }
    if !diagnostic.notes.is_empty() {
        properties.insert("notes".to_owned(), json!(diagnostic.notes));
    }
    if !properties.is_empty() {
        object.insert("properties".to_owned(), Value::Object(properties));
    }
    value
}

/// A needs-review suggestion, projected camelCase and reusing the same
/// `region` encoding every result location already carries, instead of
/// serializing `ReportedSuggestion` verbatim: the derived `Serialize`
/// shape is snake_case field names (`byte_end`, `start_line`) wrapped in
/// a `location` object, a second naming convention and a second location
/// encoding next to the sibling `region` a few lines above it in the same
/// result. The array lives under `needsReviewSuggestions` precisely
/// because it is not a real SARIF `fix`: it needs a human to look at it
/// first, so its `confidence` (needs-review, by construction of this
/// partition) is not repeated on every entry.
fn needs_review_suggestion(suggestion: &ReportedSuggestion) -> Value {
    let edits: Vec<Value> = suggestion
        .edits
        .iter()
        .map(|edit| {
            json!({
                "artifactLocation": { "uri": path_to_uri(&edit.location.path) },
                "region": region(&edit.location),
                "replacement": edit.replacement,
            })
        })
        .collect();
    json!({
        "message": suggestion.message,
        "edits": edits,
    })
}

/// The model's `path` is a project-relative filesystem path, correct
/// verbatim for the JSON writer, but SARIF's `artifactLocation.uri`
/// reinterprets the same string as a URI reference. A raw `#` would open
/// a fragment, a raw `?` a query, and a raw `%` an escape, so a file
/// named with a space or any of those characters, or with a non-ASCII
/// character, would misparse under a strict consumer (GitHub code
/// scanning resolves this URI against the repository tree). Every
/// segment is percent-encoded, byte by byte, leaving `/` alone as the
/// separator; only the URI stays encoded, the JSON `path` field must
/// keep the raw path.
fn path_to_uri(path: &str) -> String {
    path.split('/')
        .map(encode_uri_segment)
        .collect::<Vec<_>>()
        .join("/")
}

/// Percent-encodes everything outside the URI "unreserved" set (RFC
/// 3986: ASCII letters, digits, `-`, `.`, `_`, `~`), one byte at a time.
/// Deliberately conservative: encoding a byte that need not have been
/// encoded is harmless, but leaving one that should have been encoded
/// unescaped is a misparse.
fn encode_uri_segment(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}

fn physical_location(location: &SpanLocation) -> Value {
    json!({
        "physicalLocation": {
            "artifactLocation": { "uri": path_to_uri(&location.path) },
            "region": region(location)
        }
    })
}

/// The `region` shape shared by every location this writer emits: a
/// result's own location, a related location, and (via
/// `needs_review_suggestion`) a needs-review edit. One encoding, so a
/// needs-review suggestion's edit reads exactly like every other
/// location in the document instead of inventing a second one.
fn region(location: &SpanLocation) -> Value {
    json!({
        "startLine": location.start_line,
        "startColumn": location.start_column,
        "endLine": location.end_line,
        "endColumn": location.end_column,
        "byteOffset": location.byte_start,
        "byteLength": location.byte_end.saturating_sub(location.byte_start),
    })
}

fn fix(suggestion: &ReportedSuggestion) -> Value {
    let changes: Vec<Value> = suggestion
        .edits
        .iter()
        .map(|edit| {
            json!({
                "artifactLocation": { "uri": path_to_uri(&edit.location.path) },
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

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use serde_json::Value;

    use super::document;
    use crate::output::model::{
        MachineReport, ReportedAnchor, ReportedConfidence, ReportedDiagnostic, ReportedEdit,
        ReportedInternalError, ReportedSeverity, ReportedSuggestion, ResolvedReportLabel,
        SpanLocation, Summary,
    };

    /// One diagnostic carrying a `Safe` suggestion with an edit (exercises
    /// `fix`, otherwise untouched by any test) and a resolved secondary
    /// label (exercises the `relatedLocations` branch, otherwise untouched
    /// by any test), plus a `rule` and repeated internal-error kinds so
    /// the driver's `rules` and `notifications` descriptors both have
    /// something real to describe.
    fn sample_report() -> MachineReport {
        let anchor_location = SpanLocation {
            path: "src/Example.php".to_owned(),
            start_line: 3,
            start_column: 1,
            end_line: 3,
            end_column: 8,
            byte_start: 7,
            byte_end: 14,
        };
        let label_location = SpanLocation {
            path: "src/User.php".to_owned(),
            start_line: 5,
            start_column: 2,
            end_line: 5,
            end_column: 6,
            byte_start: 20,
            byte_end: 24,
        };
        let diagnostic = ReportedDiagnostic {
            id: "CEL0019".to_owned(),
            severity: ReportedSeverity::Error,
            rule: Some("unknown-symbols".to_owned()),
            anchor: ReportedAnchor::Span(anchor_location.clone()),
            message: "unknown function `strlenn`".to_owned(),
            labels: vec![ResolvedReportLabel {
                location: label_location,
                message: "declared here".to_owned(),
            }],
            notes: Vec::new(),
            suggestions: vec![ReportedSuggestion {
                message: "did you mean `strlen`?".to_owned(),
                confidence: ReportedConfidence::Safe,
                edits: vec![ReportedEdit {
                    location: anchor_location,
                    replacement: "strlen".to_owned(),
                }],
            }],
        };
        MachineReport {
            schema_version: 1,
            summary: Summary {
                errors: 1,
                warnings: 0,
                notices: 0,
                baselined_hidden: 3,
                internal_errors: 3,
                exit_code: 2,
            },
            notices: Vec::new(),
            internal_errors: vec![
                ReportedInternalError {
                    kind: "file-unreadable".to_owned(),
                    message: "src/Locked.php could not be read: permission denied".to_owned(),
                    bug: false,
                },
                ReportedInternalError {
                    kind: "analysis-panicked".to_owned(),
                    message: "the analysis loop panicked".to_owned(),
                    bug: true,
                },
                // Same kind as the first entry: proves the notification
                // descriptors deduplicate rather than repeating one per
                // occurrence.
                ReportedInternalError {
                    kind: "file-unreadable".to_owned(),
                    message: "src/OtherLocked.php could not be read: permission denied".to_owned(),
                    bug: false,
                },
            ],
            diagnostics: vec![diagnostic],
        }
    }

    /// Validates a produced document against the committed official
    /// schema: the shapes this module exercises (fixes, related
    /// locations, run properties, notification descriptors) must never
    /// ship a field the gate would reject.
    fn assert_valid(document: &Value) {
        let schema: Value =
            serde_json::from_str(include_str!("../../../../schemas/sarif-2.1.0.schema.json"))
                .unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        let errors: Vec<String> = validator
            .iter_errors(document)
            .map(|error| error.to_string())
            .collect();
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }

    #[test]
    fn a_safe_suggestion_becomes_a_fix_with_an_artifact_change() {
        let report = sample_report();
        let document = document(&report);
        let result = &document["runs"][0]["results"][0];
        let fixes = result["fixes"].as_array().unwrap();
        assert_eq!(fixes.len(), 1);
        let change = &fixes[0]["artifactChanges"][0];
        assert_eq!(change["artifactLocation"]["uri"], "src/Example.php");
        let replacement = &change["replacements"][0];
        assert_eq!(replacement["deletedRegion"]["byteOffset"], 7);
        assert_eq!(replacement["deletedRegion"]["byteLength"], 7);
        assert_eq!(replacement["insertedContent"]["text"], "strlen");
        assert_valid(&document);
    }

    /// A needs-review suggestion must project into
    /// `properties.needsReviewSuggestions` in camelCase, reusing the same
    /// `region` fields (`startLine`, `endColumn`, `byteOffset`,
    /// `byteLength`) the sibling `region` object carries, rather than the
    /// derived `Serialize` shape (`start_line`, `byte_end`, a nested
    /// `location` object) that would otherwise leak snake_case into an
    /// camelCase document. `confidence` must not repeat: the array is
    /// needs-review by construction.
    #[test]
    fn a_needs_review_suggestion_becomes_a_camel_case_property_without_confidence() {
        let mut report = sample_report();
        report.diagnostics[0].suggestions = vec![ReportedSuggestion {
            message: "consider renaming to `strlen`".to_owned(),
            confidence: ReportedConfidence::NeedsReview,
            edits: vec![ReportedEdit {
                location: SpanLocation {
                    path: "src/Example.php".to_owned(),
                    start_line: 3,
                    start_column: 1,
                    end_line: 3,
                    end_column: 8,
                    byte_start: 7,
                    byte_end: 14,
                },
                replacement: "strlen".to_owned(),
            }],
        }];
        let document = document(&report);
        let result = &document["runs"][0]["results"][0];
        assert!(result.get("fixes").is_none(), "not a real, applicable fix");
        let suggestions = result["properties"]["needsReviewSuggestions"]
            .as_array()
            .unwrap();
        assert_eq!(suggestions.len(), 1);
        let suggestion = &suggestions[0];
        assert_eq!(suggestion["message"], "consider renaming to `strlen`");
        assert!(
            suggestion.get("confidence").is_none(),
            "the array is needs-review by construction, confidence is redundant",
        );
        let edit = &suggestion["edits"][0];
        assert_eq!(edit["artifactLocation"]["uri"], "src/Example.php");
        assert_eq!(edit["region"]["startLine"], 3);
        assert_eq!(edit["region"]["endColumn"], 8);
        assert_eq!(edit["region"]["byteOffset"], 7);
        assert_eq!(edit["region"]["byteLength"], 7);
        assert_eq!(edit["replacement"], "strlen");
        assert!(
            edit.get("start_line").is_none() && edit.get("byte_end").is_none(),
            "no snake_case field must leak into the projected shape: {edit}",
        );
        assert!(edit.get("location").is_none(), "no nested location object");
        assert_valid(&document);
    }

    /// A path containing a space and a `#` must not survive verbatim into
    /// `artifactLocation.uri`: a raw `#` opens a fragment and a raw space
    /// is not a legal URI character, so a strict consumer (GitHub code
    /// scanning, resolving the URI against the repository tree) would
    /// misparse it. The JSON `path` field, unlike the URI, must keep the
    /// raw string.
    #[test]
    fn a_path_with_a_space_and_a_hash_is_percent_encoded_in_the_uri_but_raw_in_json() {
        let mut report = sample_report();
        report.diagnostics[0].anchor = ReportedAnchor::Span(SpanLocation {
            path: "src/My Service#2.php".to_owned(),
            start_line: 3,
            start_column: 1,
            end_line: 3,
            end_column: 8,
            byte_start: 7,
            byte_end: 14,
        });
        let document = document(&report);
        let result = &document["runs"][0]["results"][0];
        let uri = result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
            .as_str()
            .unwrap();
        assert_eq!(uri, "src/My%20Service%232.php");
        assert_valid(&document);

        let json_value = serde_json::to_value(&report).unwrap();
        assert_eq!(
            json_value["diagnostics"][0]["anchor"]["path"], "src/My Service#2.php",
            "the JSON path field must stay a raw path, never a URI",
        );
    }

    #[test]
    fn a_secondary_label_becomes_a_related_location() {
        let report = sample_report();
        let document = document(&report);
        let result = &document["runs"][0]["results"][0];
        let related = result["relatedLocations"].as_array().unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(
            related[0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/User.php",
        );
        assert_eq!(related[0]["message"]["text"], "declared here");
        assert_valid(&document);
    }

    /// `summary.baselined_hidden` rides in the run's `properties` bag,
    /// the only place in the document that can carry it. Without it a
    /// run with no results and several baselined findings would be
    /// indistinguishable from a genuinely clean project.
    #[test]
    fn the_run_carries_the_baselined_hidden_count_in_its_properties() {
        let report = sample_report();
        let document = document(&report);
        assert_eq!(document["runs"][0]["properties"]["baselinedHidden"], 3);
        assert_valid(&document);
    }

    /// The owning rule name goes on the matching `reportingDescriptor`
    /// under `tool.driver.rules`, not repeated on every result.
    #[test]
    fn a_rule_descriptor_carries_its_owning_rule_when_one_owns_the_identifier() {
        let report = sample_report();
        let document = document(&report);
        let rules = document["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap();
        let described = rules.iter().find(|rule| rule["id"] == "CEL0019").unwrap();
        assert_eq!(described["properties"]["rule"], "unknown-symbols");
        assert_valid(&document);
    }

    /// A notification's `descriptor.id` resolves against
    /// `tool.driver.notifications`, so every referenced kind must be
    /// described there, sorted and deduplicated exactly like `rules`.
    #[test]
    fn internal_error_kinds_are_described_under_driver_notifications_sorted_and_deduplicated() {
        let report = sample_report();
        let document = document(&report);
        let notifications = document["runs"][0]["tool"]["driver"]["notifications"]
            .as_array()
            .unwrap();
        let ids: Vec<&str> = notifications
            .iter()
            .map(|notification| notification["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["analysis-panicked", "file-unreadable"]);
        assert_valid(&document);
    }

    /// This key must be entirely absent, not an empty array, when the
    /// run hit no internal error: the common case stays visually clean.
    #[test]
    fn the_notifications_key_is_omitted_when_there_are_no_internal_errors() {
        let mut report = sample_report();
        report.internal_errors = Vec::new();
        report.summary.internal_errors = 0;
        let document = document(&report);
        assert!(
            document["runs"][0]["tool"]["driver"]
                .get("notifications")
                .is_none()
        );
        assert_valid(&document);
    }
}
