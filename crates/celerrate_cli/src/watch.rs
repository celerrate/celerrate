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
        if render::render_cycle(output, session, &outcome, reanalyzed, started.elapsed()).is_err() {
            return Outcome::InternalError;
        }

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

/// The `notify` watcher, the channel it reports through, and the walk
/// roots that both the registrations and the rewrite table were built
/// from.
///
/// The three are one object because they are one decision. The
/// registrations and the rewrite table are two views of the same set of
/// roots, and discovery can replace that set in the middle of a session: a
/// manifest whose autoload section grows a directory declares a root that
/// nothing watches and nothing maps, so every edit under it is silently
/// ignored; a manifest that loses one leaves a root still watched and
/// still mapped after the walk dropped its files, so an edit under it
/// arrives, misses the analyzed set, and puts the file straight back into
/// the set `load` had just dropped it from. Both are silent wrong results,
/// and both come from letting the two views drift apart. Nothing here is
/// rebuilt alone.
pub struct Watch {
    /// Dropping it ends the watch and closes the channel, so it is held
    /// for as long as the watch lives even though nothing calls it again.
    _watcher: RecommendedWatcher,
    events: Receiver<PathBuf>,
    /// The walk roots the watcher above is registered over, and the roots
    /// its rewrite table was built from. Keeping them is what makes "is
    /// the watch still watching what the project declares?" answerable,
    /// and answerable without asking discovery to remember that it re-ran:
    /// these roots are the truth about the watcher, and comparing them
    /// with what discovery declares now is the whole of the decision to
    /// respawn.
    registered: Vec<PathBuf>,
}

/// What a resynchronization did. The loop does not branch on it, but the
/// distinction is a contract worth pinning: a respawn drops every event
/// already queued on the channel it replaces, so a manifest save that
/// leaves the declared roots alone must not cause one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resynchronized {
    Unchanged,
    Respawned,
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
        for root in &session.discovery.project_walk_roots {
            let _ = watcher.watch(root, RecursiveMode::Recursive);
        }
        for manifest in ["composer.json", "composer.lock"] {
            let path = session.discovery.root.join(manifest);
            let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
        }
        Ok(Self {
            _watcher: watcher,
            events: receiver,
            registered: session.discovery.project_walk_roots.clone(),
        })
    }

    /// The channel the watch reports through. It is replaced by every
    /// respawn, so nothing may hold it across one.
    pub fn events(&self) -> &Receiver<PathBuf> {
        &self.events
    }

    /// Makes the watch observe exactly the roots the project declares now.
    ///
    /// Called after every absorption, because absorbing a manifest change
    /// re-runs discovery, and discovery reads the autoload section from
    /// disk: the walk roots of a session are not fixed at startup. When
    /// they have not moved this is a comparison and nothing else, which is
    /// what a `composer.json` save that touches only the PHP version must
    /// cost.
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
        if self.registered == session.discovery.project_walk_roots {
            return Ok(Resynchronized::Unchanged);
        }
        // The new watch is built before the old one is dropped, so a
        // failure here leaves the old watch running and the loop able to
        // report it rather than blind.
        *self = Self::spawn(session)?;
        Ok(Resynchronized::Respawned)
    }
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
        InputMutation, Resynchronized, Watch, WatchedRoot, as_the_project_names_it, reconcile,
        watched_roots,
    };
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
        assert_eq!(watcher.registered, vec![source.clone()]);

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
            watcher.registered,
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
        assert_eq!(watcher.registered, vec![library.clone(), source.clone()]);
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
        assert_eq!(watcher.registered, vec![source.clone()]);

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
        assert_eq!(watcher.registered, vec![root.path().join("src")]);
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
