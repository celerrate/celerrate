//! `--watch`: the loop, and the reconciliation from filesystem changes to
//! input mutations that feeds it.
//!
//! The reconciliation is split out so it can be tested without the
//! operating system: it is a pure function of a VFS diff and the set
//! currently analyzed. The loop around it is where cancellation stops
//! being a claim: the analysis runs on its own thread over its own
//! database handle, so the main thread keeps `&mut Session` and can set an
//! input the moment a change lands. That setter raises `salsa::Cancelled`
//! in every in-flight query on every other handle, and the loop reads it
//! as a restart signal rather than an error.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use celerrate_source::FileId;
use celerrate_vfs::ChangedFile;
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::analysis::{AnalysisOutcome, Cancelled, analyze};
use crate::session::{InternalError, Session};
use crate::{Outcome, render};

/// How long a burst of events is collected before a cycle starts. Editors
/// write in bursts: a save is often a truncate, a write, and a rename.
const BURST_WINDOW: Duration = Duration::from_millis(30);

/// How often the loop looks up from the channel to see whether the
/// analysis it started has finished.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// One mutation of the salsa inputs, resolved from a VFS change against
/// the file set currently analyzed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMutation {
    SetBytes { file: FileId, bytes: Vec<u8> },
    AddFile { file: FileId, bytes: Vec<u8> },
    RemoveFile { file: FileId },
}

/// Maps a VFS diff onto input mutations. Pure: no database, no
/// filesystem, no clock.
pub fn reconcile(changes: &[ChangedFile], analyzed: &BTreeSet<FileId>) -> Vec<InputMutation> {
    changes
        .iter()
        .filter_map(|change| {
            let member = analyzed.contains(&change.file_id);
            match (&change.contents, member) {
                (Some(bytes), true) => Some(InputMutation::SetBytes {
                    file: change.file_id,
                    bytes: bytes.clone(),
                }),
                (Some(bytes), false) => Some(InputMutation::AddFile {
                    file: change.file_id,
                    bytes: bytes.clone(),
                }),
                (None, true) => Some(InputMutation::RemoveFile {
                    file: change.file_id,
                }),
                (None, false) => None,
            }
        })
        .collect()
}

/// Watches, analyzes, reprints, forever. Returns only when the watcher
/// itself cannot be created, or when the output stream is gone.
pub fn watch(session: &mut Session, output: &mut dyn Write) -> Outcome {
    let (_watcher, events) = match spawn_watcher(session) {
        Ok(pair) => pair,
        Err(error) => {
            let _ = writeln!(output, "error: cannot watch the project: {error}");
            return Outcome::UsageError;
        }
    };

    let mut reanalyzed = session.sources.len();
    loop {
        let started = Instant::now();
        // Every cycle re-analyzes, so every cycle also recomputes what the
        // analysis can go wrong about. Last cycle's panics are dropped
        // before this one speaks: the picture is always complete, never a
        // stale log of past edits, and that has to hold for the
        // internal-error block too.
        session.forget_analysis_errors();
        let outcome = cycle(session, &events);
        session.absorb_outcome(&outcome);
        if render::render_cycle(output, session, &outcome, reanalyzed, started.elapsed()).is_err() {
            return Outcome::InternalError;
        }

        let changed = wait_for_a_burst(&events);
        if changed.is_empty() {
            // The channel holds its sender inside the watcher's event
            // handler, and the watcher outlives this loop, so a
            // disconnection cannot happen while the watch is alive: this
            // arm exists only so the loop is total. If it is ever reached
            // the run stops, and it stops on the state it actually
            // rendered. Returning `Outcome::Clean` unconditionally would
            // report success over a screen full of diagnostics, which is
            // the one thing the fixed exit codes forbid.
            return Outcome::of(outcome.diagnostics.len(), session.internal_errors.len());
        }
        session.absorb(&changed);
        reanalyzed = changed.len();
    }
}

/// The watcher observes the project walk roots plus `composer.json` and
/// `composer.lock`. It never watches `vendor/`: thousands of files that
/// only move when the lockfile does, and a lockfile change triggers full
/// re-discovery anyway.
pub fn spawn_watcher(session: &Session) -> notify::Result<(RecommendedWatcher, Receiver<PathBuf>)> {
    let (sender, receiver) = channel();
    let root = session.discovery.root.clone();
    let real_root = real_path(&root);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            for path in event.paths {
                let _ = sender.send(as_the_project_names_it(&root, &real_root, path));
            }
        }
    })?;
    for root in &session.discovery.project_walk_roots {
        let _ = watcher.watch(root, RecursiveMode::Recursive);
    }
    for manifest in ["composer.json", "composer.lock"] {
        let path = session.discovery.root.join(manifest);
        let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
    }
    Ok((watcher, receiver))
}

/// The operating system reports a watched path in its own terms, and they
/// are not the project's. macOS resolves every symlink before it notifies
/// (`/var` really is `/private/var`), and a project given by a relative
/// path is notified about absolutely. The `Vfs` interns by path, so a path
/// spelled two ways is two files: without this, editing a walked file
/// would look like the arrival of a brand new one, the analyzed set would
/// grow on every save, and the original would keep its stale bytes
/// forever.
///
/// The prefix is rewritten rather than the path canonicalized, because a
/// deletion must survive the trip: the file is already gone by the time
/// the event arrives, and `canonicalize` needs it to exist.
fn as_the_project_names_it(root: &Path, real_root: &Path, path: PathBuf) -> PathBuf {
    match path.strip_prefix(real_root) {
        Ok(relative) => root.join(relative),
        Err(_) => path,
    }
}

/// The root with every symlink resolved, which is how the operating system
/// will name it back to us. A root that cannot be resolved is its own real
/// path as far as this is concerned: there is then nothing to rewrite.
fn real_path(root: &Path) -> PathBuf {
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

/// Runs one analysis, restarting whenever a change overtakes it.
///
/// The analysis runs on its own thread over its own database handle, so
/// the main thread keeps `&mut Session` and can set an input the moment a
/// change lands. That setter is what raises `salsa::Cancelled` in the
/// worker's in-flight queries: cancellation is the mechanism, not a
/// bolted-on check, and the loop reads it as a restart signal.
///
/// The setter blocks until every other handle over the storage is
/// released. It is called while the worker still runs, and that is the
/// point: the worker's in-flight queries see the cancellation flag, unwind
/// with `salsa::Cancelled`, and drop the handles they hold, which is what
/// lets the setter proceed. Joining first would make the setter cheap and
/// the cancellation dead code, at the price of always paying for an
/// analysis whose answer is already stale.
fn cycle(session: &mut Session, events: &Receiver<PathBuf>) -> AnalysisOutcome {
    loop {
        let inputs = session.inputs();
        let worker = std::thread::spawn(move || analyze(&inputs));

        let mut changed: Vec<PathBuf> = Vec::new();
        loop {
            match events.recv_timeout(POLL_INTERVAL) {
                Ok(path) => changed.push(path),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if !changed.is_empty() {
                drain_burst(events, &mut changed);
                break;
            }
            if worker.is_finished() {
                break;
            }
        }

        if !changed.is_empty() {
            // Cancels the worker: it observes the flag inside a query and
            // unwinds, which is what lets this setter return.
            session.absorb(&changed);
        }
        let result = worker.join();
        if !changed.is_empty() {
            continue;
        }
        match result {
            Ok(Ok(outcome)) => return outcome,
            Ok(Err(Cancelled)) => continue,
            Err(_) => {
                session
                    .internal_errors
                    .push(InternalError::AnalysisPanicked);
                return AnalysisOutcome::default();
            }
        }
    }
}

/// Blocks until something changes, then collects the rest of the burst.
fn wait_for_a_burst(events: &Receiver<PathBuf>) -> Vec<PathBuf> {
    let mut changed = Vec::new();
    match events.recv() {
        Ok(path) => changed.push(path),
        Err(_) => return changed,
    }
    drain_burst(events, &mut changed);
    changed
}

/// Collects everything that arrives within the burst window, then
/// deduplicates: an editor's save is several events about one file.
fn drain_burst(events: &Receiver<PathBuf>, changed: &mut Vec<PathBuf>) {
    while let Ok(path) = events.recv_timeout(BURST_WINDOW) {
        changed.push(path);
    }
    changed.sort();
    changed.dedup();
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use celerrate_source::FileId;
    use celerrate_vfs::ChangedFile;

    use super::{InputMutation, as_the_project_names_it, reconcile};
    use crate::analysis::{Cancelled, analyze};
    use crate::session::Session;

    /// The invariant the whole loop is built on, and the one the umbrella
    /// design called unretrofittable: a setter on the main thread's `&mut`
    /// handle raises `salsa::Cancelled` in an analysis already in flight
    /// on another thread, and the setter returns once that analysis has
    /// unwound and released the handles it held.
    ///
    /// This is also the deadlock regression test, and it is why it is
    /// worth its cost. `analyze` clones the database once per file, so the
    /// worker holds hundreds of handles, not one. If a single one of them
    /// outlived the unwind, the setter here would wait forever for a
    /// worker that is itself waiting for nothing, and this test would hang
    /// rather than fail.
    ///
    /// Whether a given attempt catches the analysis mid-flight is a race,
    /// so the test retries. What it asserts is that cancellation really is
    /// observed, not that it lands on any particular attempt.
    #[test]
    fn a_setter_cancels_an_analysis_that_is_already_running() {
        let root = tempfile::tempdir().unwrap();
        let total = 400;
        for index in 0..total {
            std::fs::write(
                root.path().join(format!("Service{index}.php")),
                format!(
                    "<?php class Service{index} extends Service{} {{}}",
                    (index + 1) % total,
                ),
            )
            .unwrap();
        }
        let mut session = Session::start(root.path());
        assert_eq!(session.sources.len(), total);

        let edited = root.path().join("Service0.php");
        for attempt in 0..20 {
            let inputs = session.inputs();
            let worker = std::thread::spawn(move || analyze(&inputs));

            // The mutation lands while the worker is mid-fan-out. This
            // setter is what raises the cancellation, and it cannot return
            // until every handle the worker holds has been dropped.
            std::fs::write(
                &edited,
                format!("<?php class Service0 {{ public int $x = {attempt}; }}"),
            )
            .unwrap();
            session.absorb(std::slice::from_ref(&edited));

            if matches!(worker.join(), Ok(Err(Cancelled))) {
                return;
            }
        }
        panic!("the analysis was never caught in flight: cancellation was never observed");
    }

    fn analyzed(ids: &[u32]) -> BTreeSet<FileId> {
        ids.iter().copied().map(FileId::new).collect()
    }

    #[test]
    fn a_modified_member_sets_its_bytes() {
        let changes = [ChangedFile {
            file_id: FileId::new(0),
            contents: Some(b"<?php echo 2;".to_vec()),
        }];
        assert_eq!(
            reconcile(&changes, &analyzed(&[0])),
            vec![InputMutation::SetBytes {
                file: FileId::new(0),
                bytes: b"<?php echo 2;".to_vec(),
            }],
        );
    }

    #[test]
    fn a_file_that_appears_joins_the_set() {
        let changes = [ChangedFile {
            file_id: FileId::new(1),
            contents: Some(b"<?php class New {}".to_vec()),
        }];
        assert_eq!(
            reconcile(&changes, &analyzed(&[0])),
            vec![InputMutation::AddFile {
                file: FileId::new(1),
                bytes: b"<?php class New {}".to_vec(),
            }],
        );
    }

    #[test]
    fn a_file_that_vanishes_leaves_the_set_rather_than_becoming_a_tombstone() {
        // `SourceFile` has no deleted state, and empty bytes would leave
        // the set lying about what it contains.
        let changes = [ChangedFile {
            file_id: FileId::new(0),
            contents: None,
        }];
        assert_eq!(
            reconcile(&changes, &analyzed(&[0, 1])),
            vec![InputMutation::RemoveFile {
                file: FileId::new(0),
            }],
        );
    }

    #[test]
    fn a_file_that_was_never_analyzed_and_is_gone_is_nothing() {
        let changes = [ChangedFile {
            file_id: FileId::new(9),
            contents: None,
        }];
        assert_eq!(reconcile(&changes, &analyzed(&[0])), Vec::new());
    }

    /// The `Vfs` interns by path, so a file the walk knows as
    /// `/var/project/a.php` and the operating system reports as
    /// `/private/var/project/a.php` would be two files: the edit would
    /// enter the set as a new arrival and the original would keep its
    /// stale bytes. Every reported path comes back in the project's own
    /// spelling.
    #[test]
    fn a_reported_path_comes_back_in_the_projects_own_spelling() {
        let root = Path::new("/var/project");
        let real_root = Path::new("/private/var/project");
        assert_eq!(
            as_the_project_names_it(
                root,
                real_root,
                PathBuf::from("/private/var/project/src/A.php"),
            ),
            PathBuf::from("/var/project/src/A.php"),
        );
    }

    /// A deletion is reported after the file is gone, so it can never be
    /// canonicalized: rewriting the prefix is what keeps it addressable.
    /// The path is a pure string operation, and the file deliberately does
    /// not exist.
    #[test]
    fn a_vanished_file_is_still_renamed_into_the_projects_spelling() {
        assert_eq!(
            as_the_project_names_it(
                Path::new("/var/project"),
                Path::new("/private/var/project"),
                PathBuf::from("/private/var/project/gone.php"),
            ),
            PathBuf::from("/var/project/gone.php"),
        );
    }

    /// A root that needs no rewriting is left exactly as it is, and so is
    /// a path from outside the project.
    #[test]
    fn a_path_that_does_not_start_at_the_real_root_is_left_alone() {
        assert_eq!(
            as_the_project_names_it(
                Path::new("/var/project"),
                Path::new("/private/var/project"),
                PathBuf::from("/elsewhere/b.php"),
            ),
            PathBuf::from("/elsewhere/b.php"),
        );
    }

    #[test]
    fn a_burst_reconciles_in_one_pass() {
        let changes = [
            ChangedFile {
                file_id: FileId::new(0),
                contents: Some(b"a".to_vec()),
            },
            ChangedFile {
                file_id: FileId::new(1),
                contents: None,
            },
            ChangedFile {
                file_id: FileId::new(2),
                contents: Some(b"c".to_vec()),
            },
        ];
        let mutations = reconcile(&changes, &analyzed(&[0, 1]));
        assert_eq!(mutations.len(), 3);
        assert!(matches!(mutations[0], InputMutation::SetBytes { .. }));
        assert!(matches!(mutations[1], InputMutation::RemoveFile { .. }));
        assert!(matches!(mutations[2], InputMutation::AddFile { .. }));
    }
}
