//! The application engine (design section 7): a single pass in the
//! total diagnostic order, expressed against the original snapshot
//! coordinates, planned per file and applied atomically. A fix whose
//! edits overlap an already-applied fix is skipped and reported —
//! deterministically, the first wins — never silently merged. No
//! fixpoint: re-running `check` after application shows what remains.

use std::collections::BTreeMap;
use std::path::Path;

use celerrate_diagnostics::{Confidence, Diagnostic};
use celerrate_edit::find_conflict;
use celerrate_source::{FileId, TextEdit};

use crate::session::{InternalError, Session};

/// What the two flags admit: every suggestion at or below the
/// threshold in the `Confidence` order (`Safe < NeedsReview`).
/// `--fix` alone is `Safe` only — and at closure of this sub-project
/// every shipped fix is `NeedsReview`, so `--fix` applies nothing;
/// that is the design's owned consequence, stated, not hidden.
pub fn fix_threshold(fix: bool, fix_suggestions: bool) -> Option<Confidence> {
    if fix_suggestions {
        Some(Confidence::NeedsReview)
    } else if fix {
        Some(Confidence::Safe)
    } else {
        None
    }
}

/// Why a fix could not join the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// An edit overlaps one an earlier fix already claimed (or the
    /// fix's own edits overlap each other). The first fix wins.
    Overlap,
    /// An edit targets a file other than the diagnostic's own.
    /// Cross-file suggestion edits are out of scope (design section 3).
    ForeignFile,
}

/// One fix that was skipped, in encounter order, for the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFix {
    pub file: FileId,
    pub message: String,
    pub reason: SkipReason,
}

/// The plan: per-file accepted edits in original-snapshot coordinates,
/// and everything skipped. One fix is one suggestion, whole: all its
/// edits enter or none do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlannedFixes {
    pub edits_by_file: BTreeMap<FileId, Vec<TextEdit>>,
    pub accepted: usize,
    pub skipped: Vec<SkippedFix>,
}

/// Plans the single application pass. `diagnostics` must already be in
/// the total diagnostic order (the analysis outcome is), so "first
/// wins" is deterministic without re-sorting anything here.
pub fn plan(diagnostics: &[Diagnostic], threshold: Confidence) -> PlannedFixes {
    let mut planned = PlannedFixes::default();
    for diagnostic in diagnostics {
        let Some((file, _)) = diagnostic.span() else {
            continue;
        };
        for suggestion in &diagnostic.suggestions {
            if suggestion.confidence > threshold || suggestion.edits.is_empty() {
                continue;
            }
            if suggestion.edits.iter().any(|edit| edit.file != file) {
                planned.skipped.push(SkippedFix {
                    file,
                    message: suggestion.message.clone(),
                    reason: SkipReason::ForeignFile,
                });
                continue;
            }
            let mut trial: Vec<TextEdit> = planned
                .edits_by_file
                .get(&file)
                .cloned()
                .unwrap_or_default();
            trial.extend(suggestion.edits.iter().cloned());
            trial.sort();
            if find_conflict(&trial).is_some() {
                planned.skipped.push(SkippedFix {
                    file,
                    message: suggestion.message.clone(),
                    reason: SkipReason::Overlap,
                });
                continue;
            }
            planned.edits_by_file.insert(file, trial);
            planned.accepted += 1;
        }
    }
    planned
}

/// The outcome of writing a plan to disk.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AppliedFixes {
    pub files_written: usize,
}

/// Applies the planned edits: per file, one splice of all accepted
/// edits against the exact decoded text the analysis read, written to
/// disk and echoed into the `Vfs` so the session's effective state
/// stays true. Every failure is an internal error the run reports and
/// survives; nothing panics, and no file is ever partially written.
pub fn apply_to_disk(session: &mut Session, planned: &PlannedFixes) -> AppliedFixes {
    let mut applied = AppliedFixes::default();
    for (&file, edits) in &planned.edits_by_file {
        let Some(&source) = session.sources.get(&file) else {
            session.internal_errors.push(InternalError::FixUnappliable {
                file,
                reason: "the file is no longer in the analyzed set".to_owned(),
            });
            continue;
        };
        let original = match celerrate_db::source_text(&session.database, source).as_ref() {
            Ok(text) => text.text().to_owned(),
            Err(_) => {
                session.internal_errors.push(InternalError::FixUnappliable {
                    file,
                    reason: "the source text could not be decoded".to_owned(),
                });
                continue;
            }
        };
        let patched = match celerrate_edit::apply(&original, edits) {
            Ok(patched) => patched,
            Err(error) => {
                session.internal_errors.push(InternalError::FixUnappliable {
                    file,
                    reason: format!("{error:?}"),
                });
                continue;
            }
        };
        let Some(path) = session.vfs.path(file).map(Path::to_path_buf) else {
            session.internal_errors.push(InternalError::FixUnappliable {
                file,
                reason: "the file has no path in the VFS".to_owned(),
            });
            continue;
        };
        if let Err(reason) = std::fs::write(&path, patched.as_bytes()) {
            session.internal_errors.push(InternalError::FixWriteFailed {
                path,
                reason: reason.to_string(),
            });
            continue;
        }
        session
            .vfs
            .set_file_contents(&path, Some(patched.into_bytes()));
        applied.files_written += 1;
    }
    applied
}

#[cfg(test)]
mod tests {
    //! `unwrap`/`expect`/indexing are fine here: failing loudly is
    //! what a test should do.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use celerrate_diagnostics::{Confidence, Diagnostic, DiagnosticId, Severity, Suggestion};
    use celerrate_source::{FileId, TextEdit, TextRange, TextSize};

    use super::{SkipReason, fix_threshold, plan};

    fn suggestion(confidence: Confidence, file: u32, start: u32, end: u32) -> Suggestion {
        Suggestion {
            message: format!("did you mean `x{start}`?"),
            confidence,
            edits: vec![TextEdit {
                file: FileId::new(file),
                range: TextRange::new(TextSize::from(start), TextSize::from(end)),
                replacement: format!("x{start}"),
            }],
        }
    }

    fn diagnostic(file: u32, start: u32, end: u32, suggestions: Vec<Suggestion>) -> Diagnostic {
        let mut diagnostic = Diagnostic::spanned(
            DiagnosticId::new("CEL0030"),
            Severity::Error,
            FileId::new(file),
            TextRange::new(TextSize::from(start), TextSize::from(end)),
            "unknown method `m` on `T`".to_owned(),
        );
        diagnostic.suggestions = suggestions;
        diagnostic
    }

    #[test]
    fn the_threshold_maps_the_two_flags_onto_the_confidence_order() {
        assert_eq!(fix_threshold(false, false), None);
        assert_eq!(fix_threshold(true, false), Some(Confidence::Safe));
        assert_eq!(fix_threshold(false, true), Some(Confidence::NeedsReview));
        assert_eq!(fix_threshold(true, true), Some(Confidence::NeedsReview));
    }

    #[test]
    fn fix_applies_safe_only_and_fix_suggestions_applies_both() {
        let diagnostics = vec![
            diagnostic(0, 0, 4, vec![suggestion(Confidence::Safe, 0, 0, 4)]),
            diagnostic(
                0,
                10,
                14,
                vec![suggestion(Confidence::NeedsReview, 0, 10, 14)],
            ),
        ];
        let safe_only = plan(&diagnostics, Confidence::Safe);
        assert_eq!(safe_only.accepted, 1);
        let both = plan(&diagnostics, Confidence::NeedsReview);
        assert_eq!(both.accepted, 2);
        assert!(both.skipped.is_empty());
    }

    #[test]
    fn the_first_fix_wins_an_overlap_and_the_loser_is_reported() {
        let diagnostics = vec![
            diagnostic(0, 0, 4, vec![suggestion(Confidence::NeedsReview, 0, 0, 4)]),
            diagnostic(0, 2, 6, vec![suggestion(Confidence::NeedsReview, 0, 2, 6)]),
        ];
        let planned = plan(&diagnostics, Confidence::NeedsReview);
        assert_eq!(planned.accepted, 1);
        assert_eq!(planned.skipped.len(), 1);
        assert_eq!(planned.skipped[0].reason, SkipReason::Overlap);
        assert_eq!(planned.skipped[0].file, FileId::new(0));
        // The winner is the first in the given order, so the plan is
        // deterministic by construction.
        let edits = planned.edits_by_file.get(&FileId::new(0)).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(u32::from(edits[0].range.start()), 0);
    }

    #[test]
    fn coordinates_stay_original_snapshot_coordinates_across_a_file() {
        // Two accepted fixes on one file: the second is not shifted by
        // the first — both carry pre-application offsets, and the
        // splice resolves them together.
        let diagnostics = vec![
            diagnostic(0, 0, 4, vec![suggestion(Confidence::NeedsReview, 0, 0, 4)]),
            diagnostic(
                0,
                20,
                24,
                vec![suggestion(Confidence::NeedsReview, 0, 20, 24)],
            ),
        ];
        let planned = plan(&diagnostics, Confidence::NeedsReview);
        let edits = planned.edits_by_file.get(&FileId::new(0)).unwrap();
        assert_eq!(
            edits
                .iter()
                .map(|edit| u32::from(edit.range.start()))
                .collect::<Vec<_>>(),
            vec![0, 20],
        );
    }

    #[test]
    fn a_cross_file_edit_is_skipped_as_foreign() {
        let foreign = Suggestion {
            edits: vec![TextEdit {
                file: FileId::new(9),
                range: TextRange::new(TextSize::from(0), TextSize::from(1)),
                replacement: "x".to_owned(),
            }],
            ..suggestion(Confidence::NeedsReview, 0, 0, 4)
        };
        let planned = plan(
            &[diagnostic(0, 0, 4, vec![foreign])],
            Confidence::NeedsReview,
        );
        assert_eq!(planned.accepted, 0);
        assert_eq!(planned.skipped.len(), 1);
        assert_eq!(planned.skipped[0].reason, SkipReason::ForeignFile);
    }

    #[test]
    fn a_suggestion_whose_own_edits_overlap_leaves_no_spurious_file_entry() {
        // The first candidate suggestion touched for a file is itself
        // internally conflicting (its two edits overlap: 0..4 and 2..6).
        // It must be skipped as a whole, and critically, `plan` must not
        // have materialized an empty entry for the file along the way:
        // `entry(file).or_default()` would insert one before the trial is
        // known to fail, which the fix under test must avoid.
        let self_overlapping = Suggestion {
            message: "conflicting rewrite".to_owned(),
            confidence: Confidence::NeedsReview,
            edits: vec![
                TextEdit {
                    file: FileId::new(0),
                    range: TextRange::new(TextSize::from(0), TextSize::from(4)),
                    replacement: "a".to_owned(),
                },
                TextEdit {
                    file: FileId::new(0),
                    range: TextRange::new(TextSize::from(2), TextSize::from(6)),
                    replacement: "b".to_owned(),
                },
            ],
        };
        let planned = plan(
            &[diagnostic(0, 0, 4, vec![self_overlapping])],
            Confidence::NeedsReview,
        );
        assert_eq!(planned.accepted, 0);
        assert_eq!(planned.skipped.len(), 1);
        assert_eq!(planned.skipped[0].reason, SkipReason::Overlap);
        assert!(planned.edits_by_file.is_empty());
    }

    #[test]
    fn an_empty_suggestion_and_a_project_finding_plan_nothing() {
        let empty = Suggestion {
            edits: Vec::new(),
            ..suggestion(Confidence::Safe, 0, 0, 4)
        };
        let project = Diagnostic::project(
            DiagnosticId::new("CEL0025"),
            Severity::Warning,
            "no composer.json found".to_owned(),
        );
        let planned = plan(
            &[diagnostic(0, 0, 4, vec![empty]), project],
            Confidence::NeedsReview,
        );
        assert_eq!(planned.accepted, 0);
        assert!(planned.skipped.is_empty());
        assert!(planned.edits_by_file.is_empty());
    }

    use crate::session::{InternalError, Session};

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (path, contents) in files {
            let path = root.path().join(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }
        root
    }

    #[test]
    fn accepted_edits_are_spliced_once_and_written_through_the_vfs_to_disk() {
        let root = project(&[("src/App.php", "<?php aaaa(); bbbb();")]);
        let mut session = Session::start(root.path());
        let file = *session.sources.keys().next().unwrap();
        // Two original-coordinate edits: `aaaa` (6..10) and `bbbb`
        // (14..18); replacements of different lengths prove the splice
        // resolves both against the pre-application text.
        let diagnostics = vec![
            diagnostic(
                file.as_u32(),
                6,
                10,
                vec![Suggestion {
                    message: "did you mean `a`?".to_owned(),
                    confidence: Confidence::NeedsReview,
                    edits: vec![TextEdit {
                        file,
                        range: TextRange::new(TextSize::from(6), TextSize::from(10)),
                        replacement: "a".to_owned(),
                    }],
                }],
            ),
            diagnostic(
                file.as_u32(),
                14,
                18,
                vec![Suggestion {
                    message: "did you mean `bb`?".to_owned(),
                    confidence: Confidence::NeedsReview,
                    edits: vec![TextEdit {
                        file,
                        range: TextRange::new(TextSize::from(14), TextSize::from(18)),
                        replacement: "bb".to_owned(),
                    }],
                }],
            ),
        ];
        let planned = plan(&diagnostics, Confidence::NeedsReview);
        let applied = super::apply_to_disk(&mut session, &planned);
        assert_eq!(applied.files_written, 1);
        assert!(session.internal_errors.is_empty());
        let patched = std::fs::read_to_string(root.path().join("src/App.php")).unwrap();
        assert_eq!(patched, "<?php a(); bb();");
        // The VFS effective state followed the disk.
        assert_eq!(
            session.vfs.contents(file),
            Some("<?php a(); bb();".as_bytes()),
        );
    }

    #[test]
    fn an_out_of_bounds_edit_is_an_internal_error_not_a_panic_and_other_files_still_write() {
        let root = project(&[
            ("src/Bad.php", "<?php short();"),
            ("src/Good.php", "<?php aaaa();"),
        ]);
        let mut session = Session::start(root.path());
        let mut files = session.sources.keys().copied();
        let bad = files.next().unwrap();
        let good = files.next().unwrap();
        let mut planned = super::PlannedFixes::default();
        planned.edits_by_file.insert(
            bad,
            vec![TextEdit {
                file: bad,
                range: TextRange::new(TextSize::from(500), TextSize::from(504)),
                replacement: "x".to_owned(),
            }],
        );
        planned.edits_by_file.insert(
            good,
            vec![TextEdit {
                file: good,
                range: TextRange::new(TextSize::from(6), TextSize::from(10)),
                replacement: "a".to_owned(),
            }],
        );
        let applied = super::apply_to_disk(&mut session, &planned);
        assert_eq!(applied.files_written, 1, "the good file still writes");
        assert!(matches!(
            session.internal_errors.as_slice(),
            [InternalError::FixUnappliable { file, .. }] if *file == bad,
        ));
    }
}
