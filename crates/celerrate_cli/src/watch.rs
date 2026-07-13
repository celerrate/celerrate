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

/// Watches, analyzes, reprints, forever. Returns only when the watch
/// itself cannot be established or re-established, or when the output
/// stream is gone.
pub fn watch(session: &mut Session, output: &mut dyn Write) -> Outcome {
    let mut watcher = match Watch::spawn(session) {
        Ok(watcher) => watcher,
        Err(error) => return unwatchable(output, &error),
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
        let outcome = match cycle(session, &mut watcher) {
            Ok(outcome) => outcome,
            Err(error) => return unwatchable(output, &error),
        };
        session.absorb_outcome(&outcome);
        // What the watch is not observing is part of the picture, and it is
        // read from the watch that is in place now: `cycle` may have
        // respawned it, and the picture must describe the watch the next
        // burst will come from, not the one this cycle started with.
        watcher.report_unwatchable_paths(session);
        if render::render_cycle(output, session, &outcome, reanalyzed, started.elapsed()).is_err() {
            return Outcome::InternalError;
        }
        crate::cache::persist(session, &outcome);

        let changed = wait_for_a_burst(watcher.events());
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
        // The burst may have carried a manifest change, and a manifest
        // change re-runs discovery, and discovery may declare different
        // walk roots. The watch follows them here, before the next cycle
        // reads the channel again: the next read must come from the roots
        // the project declares now, not the ones it declared when the
        // session started.
        if let Err(error) = watcher.resynchronize(session) {
            return unwatchable(output, &error);
        }
        reanalyzed = changed.len();
    }
}

/// The watch cannot be established, or cannot be re-established over the
/// roots the project now declares. Both are the same failure, and it is
/// the operating system's: the resources a watch needs were refused. The
/// run stops and says so, rather than carrying on over a picture it can no
/// longer keep complete.
fn unwatchable(output: &mut dyn Write, error: &notify::Error) -> Outcome {
    let _ = writeln!(output, "error: cannot watch the project: {error}");
    Outcome::UsageError
}

/// The `notify` watcher, the channel it reports through, the walk roots
/// that both the registrations and the rewrite table were built from, and
/// the paths the operating system refused.
///
/// They are one object because they are one decision. The registrations
/// and the rewrite table are two views of the same set of roots, and
/// discovery can replace that set in the middle of a session: a manifest
/// whose autoload section grows a directory declares a root that nothing
/// watches and nothing maps, so every edit under it is silently ignored; a
/// manifest that loses one leaves a root still watched and still mapped
/// after the walk dropped its files, so an edit under it arrives, misses
/// the analyzed set, and puts the file straight back into the set `load`
/// had just dropped it from. Both are silent wrong results, and both come
/// from letting the two views drift apart. Nothing here is rebuilt alone.
pub struct Watch {
    /// Dropping it ends the watch and closes the channel, so it is held
    /// for as long as the watch lives even though nothing calls it again.
    _watcher: RecommendedWatcher,
    events: Receiver<PathBuf>,
    /// The walk roots the watcher above was built over, and the roots its
    /// rewrite table was built from. Declared, not necessarily observed:
    /// `unwatchable` names the ones the operating system refused. Keeping
    /// them is what makes "is the watch still built over what the project
    /// declares?" answerable, and answerable without asking discovery to
    /// remember that it re-ran: these roots are the truth about the
    /// watcher, and comparing them with what discovery declares now is
    /// half of the decision to respawn.
    declared: Vec<PathBuf>,
    /// The paths the operating system refused to observe. They are the
    /// other half of the decision to respawn, and they are what the picture
    /// says about a watch that is only partly alive.
    unwatchable: Vec<UnwatchablePath>,
}

/// What a resynchronization did. The loop does not branch on it, but the
/// distinction is a contract worth pinning: a respawn drops every event
/// already queued on the channel it replaces, so a manifest save that
/// leaves the declared roots alone, over a watch that refused nothing it
/// could now accept, must not cause one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resynchronized {
    Unchanged,
    Respawned,
}

/// A path the watch asked the operating system to observe, and the
/// operating system refused.
///
/// Both halves of the record matter. `reason` is what the user is told:
/// the alternative is a tool that prints `watching for changes...` over a
/// watch that is partly dead, and then reports a picture that never
/// updates, which is the silent wrong result this whole module exists to
/// forbid.
///
/// `existed` is what keeps the retry from becoming a livelock. A path
/// refused while it was there was refused for a reason no respawn can
/// change (an exhausted watch budget is exhausted for the next watcher
/// too), so retrying it would tear the watch down and rebuild it on every
/// cycle, forever, dropping the queued events each time. A path refused
/// because it was not there can start being there, and that refusal, and
/// only that one, is worth retrying.
///
/// `declared` is what decides whether the refusal is worth *saying*, which
/// is a different question and must stay one. A walk root comes from the
/// autoload section, so a project asked for it and its absence is news.
/// `composer.json` and `composer.lock` are asked for by nobody, and a
/// project with no lockfile is ordinary, so a refusal that only reports
/// their absence tells the user nothing they do not know, and would print
/// on every cycle forever.
///
/// Every refusal is recorded either way. Filtering the unreportable ones
/// out at record time is what left a `composer.lock` written mid-session
/// by an ordinary `composer install` unwatched for the life of the
/// process: nothing was left for `can_be_retried` to see, so the watch
/// never picked it up, and no lockfile change ever re-ran discovery again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct UnwatchablePath {
    path: PathBuf,
    reason: String,
    existed: bool,
    declared: bool,
}

impl UnwatchablePath {
    /// A refusal a respawn could overturn: the path was not there when the
    /// registration was attempted, and it is there now.
    ///
    /// Retrying is bounded by construction. The respawn it asks for either
    /// registers the path, which drops the refusal, or refuses it again,
    /// and that second refusal is over a path that does exist, so it is
    /// never retried. Each path therefore costs at most one extra respawn
    /// in the life of a set of declared roots.
    fn can_be_retried(&self) -> bool {
        !self.existed && self.path.exists()
    }

    /// A refusal worth printing: the path is there and could not be watched
    /// anyway, which is a watch gone partly dead; or the project declared
    /// it and it is not there, which the user can act on.
    ///
    /// What is left is the absence of a manifest nobody declared, and that
    /// is not news.
    fn is_worth_reporting(&self) -> bool {
        self.existed || self.declared
    }
}

impl Watch {
    /// The watcher observes the project walk roots plus `composer.json`
    /// and `composer.lock`. The vendor walk roots are never watched on
    /// their own: thousands of files that only move when the lockfile
    /// does, and a lockfile change triggers full re-discovery anyway.
    ///
    /// That is not a promise that `vendor/` goes unwatched. When a
    /// manifest declares no autoload, or none that resolves,
    /// `celerrate_project` falls back to the project root as the single
    /// walk root, and the recursive watch placed on it reaches `vendor/`
    /// like any other subdirectory. The fallback analyzes the whole root
    /// in that case too, so watching it is exactly coherent with what is
    /// analyzed.
    ///
    /// A registration the operating system refuses is kept, never
    /// discarded. The walk roots are lexical: `AutoloadRules::walk_roots`
    /// normalizes what the manifest declares and stats none of it, so an
    /// ordinary scaffold, a `composer.json` declaring `"Tests\\": "tests/"`
    /// before `tests/` has been written, yields a walk root that cannot be
    /// registered. Discarding that failure would leave the tool blind to
    /// the directory for the life of the process, and blind in silence.
    /// The same goes for an exhausted watch budget, which refuses a
    /// directory that is there. Both are reported; the first is retried.
    pub fn spawn(session: &Session) -> notify::Result<Self> {
        let (sender, receiver) = channel();
        let roots = watched_roots(session);
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event {
                    for path in event.paths {
                        let _ = sender.send(as_the_project_names_it(&roots, path));
                    }
                }
            })?;
        let mut unwatchable = Vec::new();
        for root in &session.discovery.project_walk_roots {
            unwatchable.extend(register(
                &mut watcher,
                root,
                RecursiveMode::Recursive,
                Declared::ByTheProject,
            ));
        }
        for manifest in ["composer.json", "composer.lock"] {
            let path = session.discovery.root.join(manifest);
            // A project may perfectly well have neither file: a missing
            // manifest is already a notice, and a project with no lockfile
            // is ordinary. Neither is declared by anyone, so neither
            // refusal is reported while the file is absent. The refusal is
            // still *recorded*, because that is a different question: it is
            // what lets a lockfile that appears mid-session be picked up.
            unwatchable.extend(register(
                &mut watcher,
                &path,
                RecursiveMode::NonRecursive,
                Declared::ByNobody,
            ));
        }
        Ok(Self {
            _watcher: watcher,
            events: receiver,
            declared: session.discovery.project_walk_roots.clone(),
            unwatchable,
        })
    }

    /// The channel the watch reports through. It is replaced by every
    /// respawn, so nothing may hold it across one.
    pub fn events(&self) -> &Receiver<PathBuf> {
        &self.events
    }

    /// Tells the session which paths the watch is not observing, so that
    /// the next picture says so.
    ///
    /// This is the honest channel: a declared directory that does not exist
    /// yet, and an operating system that will not extend its watch budget,
    /// are the environment's condition and not a bug in Celerrate, which is
    /// exactly the distinction the internal-error report already draws
    /// around `FileUnreadable`. So the refusal is reported there, in the
    /// block that names what went wrong and invites no bug report for
    /// something that is not one.
    ///
    /// The report replaces the previous one rather than adding to it, for
    /// the same reason `load` replaces its unreadable files: every cycle
    /// reprints the whole picture, so a path that is watched now must stop
    /// being reported, and a path still refused must be reported once per
    /// picture, not once per save.
    /// The filter is here, at report time, and not where the refusal is
    /// recorded: "not worth reporting" and "not worth retrying" are two
    /// questions, and answering the first by discarding the record answered
    /// the second by accident.
    fn report_unwatchable_paths(&self, session: &mut Session) {
        session
            .internal_errors
            .retain(|error| !matches!(error, InternalError::PathUnwatchable { .. }));
        for refused in self
            .unwatchable
            .iter()
            .filter(|refusal| refusal.is_worth_reporting())
        {
            session
                .internal_errors
                .push(InternalError::PathUnwatchable {
                    path: refused.path.clone(),
                    reason: refused.reason.clone(),
                });
        }
    }

    /// Makes the watch observe exactly the roots the project declares now,
    /// and every one of them it can.
    ///
    /// Called after every absorption, because absorbing a manifest change
    /// re-runs discovery, and discovery reads the autoload section from
    /// disk: the walk roots of a session are not fixed at startup. It
    /// respawns when the declared roots have moved, and when a root that
    /// could not be registered could be registered now, because a declared
    /// root is lexical and can be created long after it is declared:
    /// creating it moves nothing that discovery can see, so comparing the
    /// declared roots alone would leave the tool blind to it forever. When
    /// neither holds, this is a comparison and a stat and nothing else,
    /// which is what a `composer.json` save that touches only the PHP
    /// version must cost. Over a watch that refused nothing, which is the
    /// ordinary case, it is the comparison alone.
    ///
    /// The stat is a filesystem read, and it is allowed: this runs on the
    /// main thread, after the join, outside every salsa query.
    ///
    /// The project root itself is not compared: `rediscover` rediscovers
    /// the same root it was given, so the two manifest watches, which hang
    /// off that root, cannot move.
    ///
    /// A respawn replaces the channel, and the events already queued on
    /// the old one are dropped with it. That is deliberate. A respawn
    /// happens only just after `rediscover`, which re-walks the project
    /// and re-reads every file the walk finds, so a queued event about a
    /// file the project still declares says nothing that load did not
    /// already read from disk, and a queued event about a file the project
    /// no longer declares must not be replayed at all: it would arrive
    /// after `load` dropped the file, miss the analyzed set, and be
    /// classified as an arrival, resurrecting exactly what the walk just
    /// removed. What is left is a change landing in the instant between
    /// that re-read and the new registrations, and that window is the same
    /// one the session already accepts at startup, between the first walk
    /// and the first registration.
    fn resynchronize(&mut self, session: &Session) -> notify::Result<Resynchronized> {
        let declared_the_same = self.declared == session.discovery.project_walk_roots;
        let nothing_to_retry = !self.unwatchable.iter().any(UnwatchablePath::can_be_retried);
        if declared_the_same && nothing_to_retry {
            return Ok(Resynchronized::Unchanged);
        }
        // The new watch is built before the old one is dropped, so a
        // failure here leaves the old watch running and the loop able to
        // report it rather than blind.
        *self = Self::spawn(session)?;
        Ok(Resynchronized::Respawned)
    }
}

/// Whether a project asked for the path being registered. A walk root comes
/// from the autoload section; `composer.json` and `composer.lock` come from
/// nobody, and their mere absence is not news.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Declared {
    ByTheProject,
    ByNobody,
}

/// Registers one path, and answers with the refusal when the operating
/// system produces one.
///
/// The existence is read before the attempt and not after: a path created
/// in between must look like one that was missing, so the retry picks it
/// up, rather than like one the operating system refused while it was
/// there, which is never retried. Erring that way costs at most one
/// respawn; erring the other way would be permanent blindness.
fn register(
    watcher: &mut RecommendedWatcher,
    path: &Path,
    mode: RecursiveMode,
    declared: Declared,
) -> Option<UnwatchablePath> {
    let existed = path.exists();
    watcher
        .watch(path, mode)
        .err()
        .map(|error| UnwatchablePath {
            path: path.to_path_buf(),
            reason: error.to_string(),
            existed,
            declared: declared == Declared::ByTheProject,
        })
}

/// One watched root under the two names it answers to: `reported` is how
/// the operating system will name it back to us, `spelled` is how the
/// project names it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WatchedRoot {
    reported: PathBuf,
    spelled: PathBuf,
}

/// Every root a reported path can arrive from, each paired with the
/// project's spelling of it.
///
/// A walk root is not necessarily under the project root: it comes from
/// the declared Composer autoload directories, normalized lexically, so a
/// manifest saying `"App\\": "../packages/core/src"` (or naming an
/// absolute directory) puts one outside. Rewriting against the project
/// root alone would leave such an event untouched, and the identity bug
/// that [`as_the_project_names_it`] exists to kill would come straight
/// back for those files. So every watched root maps back on its own terms.
///
/// The project root is in the table even when it is no walk root: it is
/// where `composer.json` and `composer.lock` are watched, and those events
/// must map too.
///
/// Roots are ordered by descending depth so that the most specific one
/// wins: a walk root nested inside another, or reached through a symlink
/// of its own, must not be rewritten by its ancestor's mapping.
fn watched_roots(session: &Session) -> Vec<WatchedRoot> {
    let mut roots: Vec<WatchedRoot> = session
        .discovery
        .project_walk_roots
        .iter()
        .chain(std::iter::once(&session.discovery.root))
        .map(|root| WatchedRoot {
            reported: real_path(root),
            spelled: root.clone(),
        })
        .collect();
    roots.sort_by_key(|root| std::cmp::Reverse(root.reported.components().count()));
    roots
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
/// the event arrives, and `canonicalize` needs it to exist. That keeps
/// this a pure operation on components, which is also why a path under no
/// watched root can only be handed back exactly as it came.
fn as_the_project_names_it(roots: &[WatchedRoot], path: PathBuf) -> PathBuf {
    for root in roots {
        if let Ok(relative) = path.strip_prefix(&root.reported) {
            return root.spelled.join(relative);
        }
    }
    path
}

/// The root with every symlink resolved, which is how the operating system
/// will name it back to us. A root that cannot be resolved is its own real
/// path as far as this is concerned: the mapping degrades to identity,
/// which rewrites nothing rather than rewriting it wrongly.
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
///
/// The only error it returns is a watch that could not be re-established
/// over roots the project changed under it; there is no analysis error,
/// because a panicking analysis is an internal error the run reports and
/// survives.
fn cycle(session: &mut Session, watcher: &mut Watch) -> notify::Result<AnalysisOutcome> {
    loop {
        let inputs = session.inputs();
        let worker = std::thread::spawn(move || analyze(&inputs));

        let mut changed: Vec<PathBuf> = Vec::new();
        loop {
            match watcher.events().recv_timeout(POLL_INTERVAL) {
                Ok(path) => changed.push(path),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if !changed.is_empty() {
                drain_burst(watcher.events(), &mut changed);
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
            // That absorption may have been a manifest change, and a
            // manifest change re-runs discovery, which may declare
            // different walk roots. The watch follows them here, after the
            // worker has unwound and before the restarted analysis begins,
            // so the analysis that is about to run is the first one able to
            // be overtaken by an edit in a root that has only just
            // appeared. Resynchronizing before the join would work too, but
            // it would put a filesystem call between the setter and the
            // unwind it is waiting on, for no gain.
            watcher.resynchronize(session)?;
            continue;
        }
        match result {
            Ok(Ok(outcome)) => return Ok(outcome),
            Ok(Err(Cancelled)) => continue,
            Err(_) => {
                session
                    .internal_errors
                    .push(InternalError::AnalysisPanicked);
                return Ok(AnalysisOutcome::default());
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
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    use celerrate_project::PhpVersion;
    use celerrate_source::FileId;
    use celerrate_vfs::ChangedFile;

    use super::{
        InputMutation, Resynchronized, UnwatchablePath, Watch, WatchedRoot,
        as_the_project_names_it, reconcile, watched_roots,
    };
    use crate::analysis::{AnalysisOutcome, Cancelled, analyze};
    use crate::render;
    use crate::session::{InternalError, Session};

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

    /// A project with a manifest, a `src` walk root holding `A.php`, and a
    /// `lib` directory that exists on disk but that the manifest does not
    /// declare yet.
    fn project_with_an_undeclared_directory() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::create_dir_all(root.path().join("lib")).unwrap();
        std::fs::write(root.path().join("src/A.php"), "<?php class A {}").unwrap();
        std::fs::write(
            root.path().join("composer.json"),
            r#"{"autoload": {"psr-4": {"App\\": "src"}}}"#,
        )
        .unwrap();
        root
    }

    /// Everything the watch reports until `wanted` has been reported, plus
    /// everything that arrives in the quiet period after it.
    ///
    /// The quiet period is what lets a caller assert that something is
    /// *not* reported: notification is asynchronous, and stopping at the
    /// first sight of `wanted` would leave a slower event still in flight,
    /// so an absence proven that way would prove nothing.
    fn reported_until(events: &Receiver<PathBuf>, wanted: &Path) -> BTreeSet<PathBuf> {
        let deadline = Duration::from_secs(5);
        let quiet = Duration::from_millis(500);
        let started = Instant::now();
        let mut seen = BTreeSet::new();
        while started.elapsed() < deadline {
            match events.recv_timeout(Duration::from_millis(100)) {
                Ok(path) => {
                    let found = path == wanted;
                    seen.insert(path);
                    if found {
                        break;
                    }
                }
                Err(_) => continue,
            }
        }
        while let Ok(path) = events.recv_timeout(quiet) {
            seen.insert(path);
        }
        seen
    }

    /// The walk roots of a session are not fixed at startup: a manifest
    /// whose autoload section grows a directory declares a walk root that
    /// the watch, spawned once at the top of the loop, neither watches nor
    /// maps. Every edit under it would then be silently ignored, and the
    /// picture would go stale with no error at all, which is precisely what
    /// the format's central promise forbids.
    ///
    /// This asserts the whole chain, not that a function was called: the
    /// declared root really moves, the registration really follows (the
    /// operating system really reports a file created in the brand new
    /// directory), and the rewrite table really follows too (the report
    /// arrives in the project's own spelling of that directory, not the
    /// operating system's).
    #[test]
    fn a_walk_root_the_manifest_grows_is_watched_and_mapped() {
        let root = project_with_an_undeclared_directory();
        let source = root.path().join("src");
        let library = root.path().join("lib");
        let manifest = root.path().join("composer.json");

        let mut session = Session::start(root.path());
        let mut watcher = Watch::spawn(&session).unwrap();
        assert_eq!(watcher.declared, vec![source.clone()]);

        std::fs::write(
            &manifest,
            r#"{"autoload": {"psr-4": {"App\\": "src", "Lib\\": "lib"}}}"#,
        )
        .unwrap();
        session.absorb(std::slice::from_ref(&manifest));
        assert_eq!(
            session.discovery.project_walk_roots,
            vec![library.clone(), source.clone()],
            "discovery re-ran, and the directory the manifest now declares is a walk root",
        );

        assert_eq!(
            watcher.resynchronize(&session).unwrap(),
            Resynchronized::Respawned,
        );
        assert_eq!(
            watcher.declared,
            vec![library.clone(), source.clone()],
            "the watch is registered over the roots the project declares now",
        );

        let created = library.join("B.php");
        std::fs::write(&created, "<?php class B {}").unwrap();
        let seen = reported_until(watcher.events(), &created);
        assert!(
            seen.contains(&created),
            "a file created in the brand new walk root is watched, and maps back into the \
             project's own spelling: {seen:?}",
        );
    }

    /// The other half of the same bug. A walk root the manifest drops is
    /// still watched and still mapped by a watch that never respawns, so an
    /// edit to a file that has just left the analyzed set still arrives,
    /// misses the set, and `reconcile` classifies it as an arrival: the
    /// file re-enters the very set `load` had just dropped it from.
    ///
    /// The edit inside the root that stayed is the control. Without it, the
    /// assertion that nothing arrives from the dropped root would pass just
    /// as well on a watch that reports nothing at all.
    #[test]
    fn a_walk_root_the_manifest_drops_stops_being_watched() {
        let root = project_with_an_undeclared_directory();
        let source = root.path().join("src");
        let library = root.path().join("lib");
        let manifest = root.path().join("composer.json");
        std::fs::write(library.join("B.php"), "<?php class B {}").unwrap();
        std::fs::write(
            &manifest,
            r#"{"autoload": {"psr-4": {"App\\": "src", "Lib\\": "lib"}}}"#,
        )
        .unwrap();

        let mut session = Session::start(root.path());
        let mut watcher = Watch::spawn(&session).unwrap();
        assert_eq!(watcher.declared, vec![library.clone(), source.clone()]);
        assert_eq!(session.sources.len(), 2);

        std::fs::write(&manifest, r#"{"autoload": {"psr-4": {"App\\": "src"}}}"#).unwrap();
        session.absorb(std::slice::from_ref(&manifest));
        assert_eq!(
            session.sources.len(),
            1,
            "the walk dropped the file of the root the project no longer declares",
        );
        assert_eq!(
            watcher.resynchronize(&session).unwrap(),
            Resynchronized::Respawned,
        );
        assert_eq!(watcher.declared, vec![source.clone()]);

        std::fs::write(
            library.join("B.php"),
            "<?php class B { public int $x = 1; }",
        )
        .unwrap();
        let edited = source.join("A.php");
        std::fs::write(&edited, "<?php class A { public int $x = 1; }").unwrap();

        let seen = reported_until(watcher.events(), &edited);
        assert!(
            seen.contains(&edited),
            "the watch is alive and reports the root that stayed: {seen:?}",
        );
        assert!(
            !seen.iter().any(|path| path.starts_with(&library)),
            "nothing is reported from the root the project dropped, so nothing can resurrect \
             the file the walk removed: {seen:?}",
        );
    }

    /// A respawn drops every event already queued on the channel it
    /// replaces, and saving `composer.json` is a common event: most saves
    /// leave the autoload section exactly as it was. Such a save must cost
    /// a comparison, not a teardown.
    ///
    /// The version assertion is what keeps this honest: discovery really
    /// did re-run, and really did change the session. It is the walk roots
    /// that did not move.
    #[test]
    fn a_manifest_save_that_leaves_the_roots_alone_does_not_respawn() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/A.php"), "<?php class A {}").unwrap();
        let manifest = root.path().join("composer.json");
        std::fs::write(
            &manifest,
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src"}}}"#,
        )
        .unwrap();
        // Every path this watch registers is there, so it refuses nothing:
        // the assertion below is about the roots not moving, and only that.
        std::fs::write(root.path().join("composer.lock"), "{}").unwrap();

        let mut session = Session::start(root.path());
        let mut watcher = Watch::spawn(&session).unwrap();

        std::fs::write(
            &manifest,
            r#"{"require": {"php": "^8.4"}, "autoload": {"psr-4": {"App\\": "src"}}}"#,
        )
        .unwrap();
        session.absorb(std::slice::from_ref(&manifest));
        assert_eq!(
            session
                .configuration
                .php_version_range(&session.database)
                .minimum,
            PhpVersion::new(8, 4),
            "discovery really re-ran: this is not a save that absorbed into nothing",
        );

        assert_eq!(
            watcher.resynchronize(&session).unwrap(),
            Resynchronized::Unchanged,
        );
        assert_eq!(watcher.declared, vec![root.path().join("src")]);
        assert!(
            watcher.unwatchable.is_empty(),
            "every declared root registered, so there is nothing to retry either",
        );
    }

    /// The ordinary shape of a scaffold: `composer.json` declares `tests/`
    /// before anyone has written it. The declared roots are lexical, so the
    /// directory that does not exist is a walk root all the same.
    ///
    /// The lockfile is written so that the only refusal these tests see is
    /// the one they are about. A project without a lockfile records a
    /// refusal for it too (which is what lets one created mid-session be
    /// picked up), and that has its own test.
    fn project_declaring_a_directory_that_does_not_exist() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/A.php"), "<?php class A {}").unwrap();
        std::fs::write(
            root.path().join("composer.json"),
            r#"{"autoload": {"psr-4": {"App\\": "src", "Tests\\": "tests"}}}"#,
        )
        .unwrap();
        std::fs::write(root.path().join("composer.lock"), "{}").unwrap();
        root
    }

    /// A registration the operating system refuses must reach the user. The
    /// declared directory that is not there yet is the common case, and
    /// discarding its refusal leaves the tool blind to that directory for
    /// the life of the process, printing `watching for changes...` over a
    /// watch that is only partly alive: the silent wrong result the whole
    /// module exists to forbid.
    ///
    /// It reaches the user through the internal-error block, which is where
    /// an environment condition that is not a Celerrate bug already goes,
    /// and it must not invite a bug report, exactly as an unreadable file
    /// does not: a directory a project has not created yet is nobody's bug.
    #[test]
    fn a_declared_walk_root_that_does_not_exist_is_reported_not_swallowed() {
        let root = project_declaring_a_directory_that_does_not_exist();
        let tests = root.path().join("tests");

        let mut session = Session::start(root.path());
        assert_eq!(
            session.discovery.project_walk_roots,
            vec![root.path().join("src"), tests.clone()],
            "the declared roots are lexical: nothing stats them, so the missing one is a root",
        );

        let watcher = Watch::spawn(&session).unwrap();
        assert_eq!(
            watcher
                .unwatchable
                .iter()
                .map(|refusal| refusal.path.clone())
                .collect::<Vec<_>>(),
            vec![tests.clone()],
            "the refusal is kept, not discarded",
        );
        assert!(
            !watcher.unwatchable[0].reason.is_empty(),
            "the operating system's own words are kept, so the user can act on them",
        );
        assert!(
            !watcher.unwatchable[0].existed,
            "it was refused because it is not there, which is the refusal a respawn can overturn",
        );

        watcher.report_unwatchable_paths(&mut session);
        assert_eq!(
            session.internal_errors,
            vec![InternalError::PathUnwatchable {
                path: tests.clone(),
                reason: watcher.unwatchable[0].reason.clone(),
            }],
        );
        watcher.report_unwatchable_paths(&mut session);
        assert_eq!(
            session.internal_errors.len(),
            1,
            "reported once per picture, not once per cycle: the picture is never a log",
        );

        let mut output = Vec::new();
        render::render_check(&mut output, &session, &AnalysisOutcome::default()).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(
            text.contains("tests could not be watched"),
            "the path the watch is blind to is named: {text}",
        );
        assert!(
            !text.contains("Please report it"),
            "a directory the project has not created is not a bug in Celerrate: {text}",
        );
    }

    /// The other half: the directory is created, and the watch must pick it
    /// up. Nothing discovery can see has moved (the root was declared all
    /// along, and discovery never stats it), so comparing the declared roots
    /// alone would leave the tool blind to it forever.
    ///
    /// This asserts the whole chain, not that a function was called: the
    /// registration really follows (the operating system really reports a
    /// file created in the directory that has just appeared) and the rewrite
    /// table really follows too (the report arrives in the project's own
    /// spelling of it, not the operating system's). And it asserts the
    /// retry is spent: the cycle after it respawns nothing.
    #[test]
    fn a_walk_root_that_was_missing_and_now_exists_is_watched_and_mapped() {
        let root = project_declaring_a_directory_that_does_not_exist();
        let source = root.path().join("src");
        let tests = root.path().join("tests");

        let mut session = Session::start(root.path());
        let mut watcher = Watch::spawn(&session).unwrap();
        assert_eq!(watcher.unwatchable.len(), 1);

        // The scaffold is completed, and an ordinary edit lands. No declared
        // root moves: only the filesystem changed.
        std::fs::create_dir_all(&tests).unwrap();
        let edited = source.join("A.php");
        std::fs::write(&edited, "<?php class A { public int $x = 1; }").unwrap();
        session.absorb(std::slice::from_ref(&edited));
        assert_eq!(
            session.discovery.project_walk_roots,
            vec![source.clone(), tests.clone()],
            "the declared roots are exactly what they were: creating a directory declares nothing",
        );

        assert_eq!(
            watcher.resynchronize(&session).unwrap(),
            Resynchronized::Respawned,
        );
        assert!(
            watcher.unwatchable.is_empty(),
            "the root that could not be registered is registered now",
        );
        assert_eq!(watcher.declared, vec![source.clone(), tests.clone()]);
        assert_eq!(
            watcher.resynchronize(&session).unwrap(),
            Resynchronized::Unchanged,
            "the retry is spent: one respawn, not one on every cycle from here on",
        );

        let created = tests.join("FooTest.php");
        std::fs::write(&created, "<?php class FooTest {}").unwrap();
        let seen = reported_until(watcher.events(), &created);
        assert!(
            seen.contains(&created),
            "a file created in the root that has just appeared is watched, and maps back into \
             the project's own spelling: {seen:?}",
        );
    }

    /// A `composer.lock` that is not there when the watch spawns is
    /// ordinary: a project may perfectly well have none, and saying so on
    /// every cycle would be noise. So the refusal is not reported.
    ///
    /// It was not *recorded* either, and that conflated "not worth
    /// reporting" with "not worth retrying". Nothing was left for
    /// `can_be_retried` to see, so a lockfile written mid-session by an
    /// ordinary `composer install` was never watched, and no lockfile
    /// change from then on ever re-ran discovery: the project's whole
    /// dependency set could move under the watch, in silence.
    ///
    /// Recording it and filtering at report time closes that for free, and
    /// cannot livelock: while the file is absent `can_be_retried` is false,
    /// and it is true exactly once, when the file appears.
    #[test]
    fn a_lockfile_created_mid_session_is_watched_from_then_on() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/A.php"), "<?php class A {}").unwrap();
        std::fs::write(
            root.path().join("composer.json"),
            r#"{"autoload": {"psr-4": {"App\\": "src"}}}"#,
        )
        .unwrap();
        let lockfile = root.path().join("composer.lock");

        let mut session = Session::start(root.path());
        let mut watcher = Watch::spawn(&session).unwrap();

        assert_eq!(
            watcher
                .unwatchable
                .iter()
                .map(|refusal| refusal.path.clone())
                .collect::<Vec<_>>(),
            vec![lockfile.clone()],
            "the refusal is recorded, which is what lets the retry see it",
        );
        watcher.report_unwatchable_paths(&mut session);
        assert!(
            session.internal_errors.is_empty(),
            "and it is not reported: a project with no lockfile is ordinary, and a refusal that \
             only says so tells the user nothing: {:?}",
            session.internal_errors,
        );

        // An ordinary `composer install`.
        std::fs::write(&lockfile, "{}").unwrap();

        assert_eq!(
            watcher.resynchronize(&session).unwrap(),
            Resynchronized::Respawned,
            "the lockfile is there now, and that is the one refusal a respawn can overturn",
        );
        assert!(
            watcher.unwatchable.is_empty(),
            "the lockfile that could not be registered is registered now",
        );
        assert_eq!(
            watcher.resynchronize(&session).unwrap(),
            Resynchronized::Unchanged,
            "the retry is spent: one respawn, not one on every cycle from here on",
        );

        std::fs::write(&lockfile, r#"{"packages": []}"#).unwrap();
        let seen = reported_until(watcher.events(), &lockfile);
        assert!(
            seen.contains(&lockfile),
            "and a lockfile change is observed from now on, which is what re-runs discovery and \
             rebuilds the configuration: {seen:?}",
        );
    }

    /// A manifest that *is* there and is refused anyway is a watch gone
    /// partly dead, and that is worth saying: the picture would silently
    /// stop following the project's dependencies. Reporting turns on the
    /// path's existence, not on whether the project declared it.
    #[test]
    fn a_manifest_refused_while_it_exists_is_reported() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php echo 1;").unwrap();
        let manifest = root.path().join("composer.json");
        std::fs::write(&manifest, "{}").unwrap();

        let mut session = Session::start(root.path());
        let mut watcher = Watch::spawn(&session).unwrap();

        // No portable test can exhaust a real watch budget, so the refusal
        // is fabricated in the exact shape the operating system produces: a
        // path that is there, refused anyway.
        watcher.unwatchable = vec![UnwatchablePath {
            path: manifest.clone(),
            reason: "OS file watch limit reached.".to_owned(),
            existed: true,
            declared: false,
        }];
        watcher.report_unwatchable_paths(&mut session);

        assert_eq!(
            session.internal_errors,
            vec![InternalError::PathUnwatchable {
                path: manifest,
                reason: "OS file watch limit reached.".to_owned(),
            }],
            "the user is never left with `watching for changes...` over a watch that is dead",
        );
    }

    /// A refusal no respawn can overturn must not respawn the watch.
    ///
    /// Two shapes, and neither may cost a teardown. A root that is still not
    /// there cannot be registered now either. And a root the operating
    /// system refused while it *was* there was refused for a reason a new
    /// watcher meets unchanged: an exhausted watch budget is exhausted for
    /// the next watcher too. Retrying either would rebuild the watch on
    /// every single cycle, forever, dropping every queued event each time.
    ///
    /// The budget refusal is fabricated, because no portable test can
    /// exhaust a real watch budget, but it is fabricated in the exact shape
    /// the operating system produces: a path that is there, refused anyway.
    #[test]
    fn a_root_that_can_never_be_registered_does_not_respawn_the_watch_forever() {
        let root = project_declaring_a_directory_that_does_not_exist();
        let source = root.path().join("src");
        let session = Session::start(root.path());
        let mut watcher = Watch::spawn(&session).unwrap();
        assert_eq!(watcher.unwatchable.len(), 1, "the missing root was refused");

        for _ in 0..5 {
            assert_eq!(
                watcher.resynchronize(&session).unwrap(),
                Resynchronized::Unchanged,
                "a root that is still missing cannot be registered now: nothing is torn down",
            );
        }

        watcher.unwatchable = vec![UnwatchablePath {
            path: source.clone(),
            reason: "OS file watch limit reached.".to_owned(),
            existed: true,
            declared: true,
        }];
        for _ in 0..5 {
            assert_eq!(
                watcher.resynchronize(&session).unwrap(),
                Resynchronized::Unchanged,
                "a refusal over a path that exists is never retried: no livelock",
            );
        }
        assert_eq!(
            watcher.unwatchable.len(),
            1,
            "and it keeps being reported, cycle after cycle: the user is never left blind",
        );
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

    /// The watched roots of a project whose only walk root is its own
    /// source directory, as the operating system would name them back.
    fn project_roots() -> Vec<WatchedRoot> {
        vec![WatchedRoot {
            reported: PathBuf::from("/private/var/project"),
            spelled: PathBuf::from("/var/project"),
        }]
    }

    /// The `Vfs` interns by path, so a file the walk knows as
    /// `/var/project/a.php` and the operating system reports as
    /// `/private/var/project/a.php` would be two files: the edit would
    /// enter the set as a new arrival and the original would keep its
    /// stale bytes. Every reported path comes back in the project's own
    /// spelling.
    #[test]
    fn a_reported_path_comes_back_in_the_projects_own_spelling() {
        assert_eq!(
            as_the_project_names_it(
                &project_roots(),
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
                &project_roots(),
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
            as_the_project_names_it(&project_roots(), PathBuf::from("/elsewhere/b.php")),
            PathBuf::from("/elsewhere/b.php"),
        );
    }

    /// A walk root comes from a declared autoload directory, and nothing
    /// obliges that directory to sit under the project root: a manifest
    /// saying `"App\\": "../packages/core/src"` puts one beside it. The
    /// project root's mapping cannot reach such a path, so the root itself
    /// must map: otherwise every save in that package would arrive under
    /// the operating system's spelling, enter the analyzed set as a brand
    /// new file, and leave the walked one with its stale bytes.
    #[test]
    fn a_walk_root_outside_the_project_root_maps_back_on_its_own_terms() {
        let roots = vec![
            WatchedRoot {
                reported: PathBuf::from("/private/var/workspace/packages/core/src"),
                spelled: PathBuf::from("/var/workspace/packages/core/src"),
            },
            WatchedRoot {
                reported: PathBuf::from("/private/var/workspace/app"),
                spelled: PathBuf::from("/var/workspace/app"),
            },
        ];
        assert_eq!(
            as_the_project_names_it(
                &roots,
                PathBuf::from("/private/var/workspace/packages/core/src/Core.php"),
            ),
            PathBuf::from("/var/workspace/packages/core/src/Core.php"),
        );
    }

    /// The same, end to end: the walk root really is outside the project
    /// root, and the table `Watch::spawn` builds really does map an event
    /// reported from it, and an event reported for the manifest, which is
    /// under no walk root at all.
    #[test]
    fn the_watched_roots_map_a_walk_root_that_is_not_under_the_project_root() {
        let workspace = tempfile::tempdir().unwrap();
        let application = workspace.path().join("app");
        let package = workspace.path().join("packages/core/src");
        std::fs::create_dir_all(&application).unwrap();
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            application.join("composer.json"),
            r#"{"autoload": {"psr-4": {"App\\": "../packages/core/src"}}}"#,
        )
        .unwrap();
        std::fs::write(package.join("Core.php"), "<?php class Core {}").unwrap();

        let session = Session::start(&application);
        assert_eq!(
            session.discovery.project_walk_roots,
            vec![package.clone()],
            "the declared autoload directory is the walk root, and it is outside the project root",
        );
        assert_eq!(session.sources.len(), 1);

        let roots = watched_roots(&session);
        let reported = std::fs::canonicalize(&package).unwrap().join("Core.php");
        assert_eq!(
            as_the_project_names_it(&roots, reported),
            package.join("Core.php"),
            "an event from the outside walk root comes back in the project's spelling",
        );

        let manifest = application.join("composer.json");
        let reported = std::fs::canonicalize(&application)
            .unwrap()
            .join("composer.json");
        assert_eq!(
            as_the_project_names_it(&roots, reported),
            manifest,
            "the manifest is under no walk root, and still maps through the project root",
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
