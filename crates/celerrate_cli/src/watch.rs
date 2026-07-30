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
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use celerrate_source::FileId;
use celerrate_vfs::ChangedFile;
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::analysis::{AnalysisOutcome, Cancelled, analyze};
use crate::session::{InternalError, Session};
use crate::{ColorMode, Outcome, render};

/// How long a burst of events is collected before a cycle starts. Editors
/// write in bursts: a save is often a truncate, a write, and a rename.
const BURST_WINDOW: Duration = Duration::from_millis(30);

/// How often the loop looks up from the channel to see whether the
/// analysis it started has finished.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// What travels on the watch channel: filesystem changes from the notify
/// callback, and the shutdown request from the signal handler (issue
/// #52). One channel, because the loop's only wake-up mechanism is this
/// channel's blocking read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WatchEvent {
    Changed(PathBuf),
    // Constructed by the tests below and, in production, by
    // `install_shutdown_handler` (issue #52's Ctrl+C handling), sent
    // through the cell `Watch::shutdown_sender` hands out.
    Shutdown,
}

/// What one blocking read of the channel amounts to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BurstOutcome {
    /// A burst of changed paths, sorted and deduplicated.
    Changes(Vec<PathBuf>),
    /// A shutdown request: no new work starts after it — a burst
    /// collected before it arrived is deliberately discarded, and the
    /// exit persists the last completed state.
    Shutdown,
    /// The channel's sender is gone (the pre-#52 exit branch).
    Disconnected,
}

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

/// One complete watch iteration up to and including the persist.
/// Extracted from the loop so a test can drive exactly what an
/// iteration does to the packs on disk, the "rewritten
/// after every completed analysis, including every `--watch` iteration"
/// clause, without needing a channel event to stop
/// the loop.
///
/// Answers the completed outcome alongside a change already queued on
/// the burst channel, when persisting this cycle was skipped because of
/// one (see below) — `watch`'s own loop folds that path into the very
/// next burst rather than losing it to a blocking `wait_for_a_burst`
/// call that would never have blocked anyway.
fn completed_cycle(
    session: &mut Session,
    watcher: &mut Watch,
    output: &mut dyn Write,
    reanalyzed: usize,
    color: ColorMode,
    mode: crate::baseline::Mode,
) -> Result<(AnalysisOutcome, Option<WatchEvent>, bool), Outcome> {
    let started = Instant::now();
    // Every cycle re-analyzes, so every cycle also recomputes what the
    // analysis can go wrong about. Last cycle's panics are dropped
    // before this one speaks: the picture is always complete, never a
    // stale log of past edits, and that has to hold for the
    // internal-error block too.
    session.forget_analysis_errors();
    let (outcome, shutdown) = match cycle(session, watcher) {
        Ok(result) => result,
        Err(error) => return Err(unwatchable(output, &error)),
    };
    session.absorb_outcome(&outcome);
    // What the watch is not observing is part of the picture, and it is
    // read from the watch that is in place now: `cycle` may have
    // respawned it, and the picture must describe the watch the next
    // burst will come from, not the one this cycle started with.
    watcher.report_unwatchable_paths(session);
    // Read fresh every cycle, outside every query, so the analysis stays
    // a pure function of its inputs (determinism): the terminal can be
    // resized between cycles, and a stale height would cap against a
    // frame the user no longer has.
    let height =
        terminal_size::terminal_size().map(|(_, terminal_size::Height(rows))| rows as usize);
    // The baseline is presentation, exactly as in the single-pass check:
    // applied to a cloned copy, never to the `outcome` the persisted
    // verdicts read. `Mode::Record` cannot reach the watch loop at all:
    // clap rejects `--baseline --watch` as a usage error before either
    // session exists, so only `Apply` ever hides anything here.
    // `Ignore` (and the unreachable `Record`) leave `presented`
    // untouched, exactly like a session with no baseline file.
    let mut presented = outcome.clone();
    let baseline_outcome = if let crate::baseline::Mode::Apply = mode {
        crate::baseline::apply(session, &mut presented.diagnostics)
    } else {
        crate::baseline::BaselineOutcome::default()
    };
    // Configuration diagnostics are part of every picture, exactly as in
    // a single check: merged into a presentation copy, never into the
    // outcome the persisted verdicts read. The baseline filter runs
    // before this, and configuration diagnostics stay exempt from it,
    // exactly as in the single-pass path.
    let _ = crate::configuration::merge_diagnostics(session, &mut presented);
    if render::render_cycle(
        output,
        session,
        &presented,
        reanalyzed,
        started.elapsed(),
        color,
        height,
        &baseline_outcome,
    )
    .is_err()
    {
        return Err(Outcome::InternalError);
    }
    let pending = persist_unless_a_burst_is_already_waiting(session, watcher, &outcome);
    session.statistics.report();
    Ok((outcome, pending, shutdown))
}

/// Per-cycle-persist economics: a real
/// corpus measurement (symfony/demo, 9341 files, release build) found
/// persist's own entry-collection cost — `collect_entries` and
/// `collect_signature_entries` walk every reported file every call, not
/// just the one a single-line edit touched — holding steady around
/// 50-60ms per cycle, several times over the ~13ms median warm cycle the
/// same edits reanalyzed in. That is well past the 10% ceiling, so
/// per-cycle persist does not keep the crash-window guarantee as
/// cheaply as hoped, and this is the recorded fallback: skip the write
/// when a change is already queued on the burst channel, because
/// `wait_for_a_burst` would not have blocked at all in that case —
/// another cycle is already about to start, and this write would be
/// redundant with the one that cycle attempts in turn. `try_recv` both
/// checks and consumes; the consumed path is handed back to the caller
/// rather than dropped, so `watch`'s own loop can fold it into the very
/// next burst.
///
/// This trades away part of the crash-window guarantee: any termination
/// mid-burst loses every cycle since the last quiet persist, not just
/// one. `iteration`'s graceful exit arm (issue #52) persists once more
/// on its way out, along both the branch reached when the burst channel
/// disconnects and the one reached by a `WatchEvent::Shutdown` —
/// including a shutdown that arrives while a burst is already being
/// collected, now that `drain_burst`'s own shutdown flag reaches this
/// same exit instead of being silently dropped. What remains outside
/// this module's control is who actually sends that event: the signal
/// handler that routes an interactive Ctrl+C onto the channel is a
/// follow-up task, and once it is wired an ordinary SIGINT or SIGTERM
/// closes the crash window exactly like any other graceful exit. Only a
/// hard kill — `SIGKILL`, a crash, a power loss — still takes the
/// process down mid-cycle with no destructor run and no chance to
/// persist, losing every cycle back to the last quiet write.
///
/// Split out from `completed_cycle` so this decision — and the path it
/// hands back — is pinned directly against a channel a test controls,
/// rather than against `cycle`'s own real-time analysis polling of that
/// same channel (which, by construction, drains any message already
/// queued before analysis even starts, so a message pre-seeded for a
/// `completed_cycle` test would never survive to reach this check at
/// all).
///
/// Issue #52's vocabulary only widens the "already queued" question, it
/// does not weaken it: a queued `WatchEvent::Shutdown` does not skip the
/// persist the way a queued `WatchEvent::Changed` does, because no cycle
/// is coming after it to persist in turn — this IS the graceful exit's
/// persist, reached when the shutdown lands during a busy cycle's
/// render rather than during the idle wait between cycles.
fn persist_unless_a_burst_is_already_waiting(
    session: &mut Session,
    watcher: &Watch,
    outcome: &AnalysisOutcome,
) -> Option<WatchEvent> {
    let pending = watcher.events().try_recv().ok();
    if !matches!(pending, Some(WatchEvent::Changed(_))) {
        crate::cache::persist(session, outcome);
    }
    pending
}

/// Watches, analyzes, reprints, forever. Returns only when the watch
/// itself cannot be established or re-established, or when the output
/// stream is gone.
pub fn watch(
    session: &mut Session,
    output: &mut dyn Write,
    color: ColorMode,
    mode: crate::baseline::Mode,
) -> Outcome {
    let mut watcher = match Watch::spawn(session) {
        Ok(watcher) => watcher,
        Err(error) => return unwatchable(output, &error),
    };
    // Issue #52: routes Ctrl+C (and `kill`) into the graceful exit. Taken
    // once, over the first spawn's cell; every respawn inside
    // `resynchronize` keeps writing into this same cell, so the handler
    // stays live across the life of the process, not just until the first
    // walk-root change.
    install_shutdown_handler(watcher.shutdown_sender());

    let mut reanalyzed = session.sources.len();
    loop {
        match iteration(session, &mut watcher, output, reanalyzed, color, mode) {
            ControlFlow::Continue(next) => reanalyzed = next,
            ControlFlow::Break(outcome) => return outcome,
        }
    }
}

/// Routes SIGINT/SIGTERM (Ctrl+C, kill) into the watch channel so the
/// loop exits through the graceful persist (issue #52). The second
/// signal exits the process immediately (130, the shell convention):
/// the graceful path must never cost the user their escape hatch. An
/// installation failure degrades to the pre-#52 behavior — the watch
/// still runs, shutdown is just abrupt again.
///
/// Known, accepted residue: a shutdown sent in the exact instant a
/// respawn swaps the channel can be lost — `resynchronize` holds the
/// cell's lock only for the length of the swap, but a signal landing in
/// that narrow window still finds the cell either mid-swap (behind the
/// same lock, so this call simply waits) or, in the case where the
/// signal is delivered and handled between the respawn's `spawn` and its
/// write into the cell, sends into a sender whose receiver is about to
/// be dropped. Either way, the second Ctrl+C covers it: this handler
/// exits the process outright on the second signal regardless of
/// whether the first one was delivered.
///
/// Tests never install this: `set_handler` is process-global (one
/// handler for the whole binary, and `ctrlc` refuses a second
/// installation), so tests inject `WatchEvent::Shutdown` directly
/// through a cell they hold, exercising `iteration` and `resynchronize`
/// without touching the process-wide signal state.
fn install_shutdown_handler(cell: Arc<Mutex<Sender<WatchEvent>>>) {
    let already_requested = std::sync::atomic::AtomicBool::new(false);
    let _ = ctrlc::set_handler(move || {
        if already_requested.swap(true, std::sync::atomic::Ordering::SeqCst) {
            std::process::exit(130);
        }
        if let Ok(sender) = cell.lock() {
            let _ = sender.send(WatchEvent::Shutdown);
        }
    });
}

/// One pass of the watch loop: a completed cycle, then either the start
/// of the next burst or the graceful exit. Extracted from `watch` (issue
/// #52) so a test can drive exactly one pass by injecting events on a
/// held sender, rather than only observing the loop from the outside
/// through its infinite iteration.
///
/// `Break` carries the final `Outcome`, reached only after the graceful
/// exit's own persist has run; `Continue` carries the `reanalyzed` count
/// the next iteration reports against.
fn iteration(
    session: &mut Session,
    watcher: &mut Watch,
    output: &mut dyn Write,
    reanalyzed: usize,
    color: ColorMode,
    mode: crate::baseline::Mode,
) -> ControlFlow<Outcome, usize> {
    let (outcome, pending, shutdown) =
        match completed_cycle(session, watcher, output, reanalyzed, color, mode) {
            Ok(result) => result,
            Err(ended) => return ControlFlow::Break(ended),
        };

    // Issue #52: no new work starts after a shutdown is observed. A
    // shutdown seen mid-cycle (the flag `cycle` threads back through
    // `completed_cycle`) skips the burst wait entirely — there is
    // nothing left to collect, and falling through to the graceful exit
    // arm below is the whole of what is left to do.
    //
    // A change already queued (the fallback in
    // `persist_unless_a_burst_is_already_waiting`) starts the next burst
    // instead of blocking for one that has, in effect, already arrived.
    // A shutdown queued the same way reaches the same graceful exit
    // instead: no new work starts after it either.
    let burst = if shutdown {
        BurstOutcome::Shutdown
    } else {
        match pending {
            Some(WatchEvent::Changed(path)) => burst_starting_with(watcher.events(), path),
            Some(WatchEvent::Shutdown) => BurstOutcome::Shutdown,
            None => wait_for_a_burst(watcher.events()),
        }
    };
    let changed = match burst {
        BurstOutcome::Changes(changed) => changed,
        BurstOutcome::Shutdown | BurstOutcome::Disconnected => {
            // The graceful exit (issue #52): a shutdown request, or the
            // disconnect that "cannot happen while the watch is alive"
            // (kept because the loop must be total). Whatever the last
            // busy cycle skipped is flushed before the process returns —
            // a no-op write when that cycle's own persist already ran,
            // since `write_when_changed` compares before writing.
            //
            // The exit count is computed on a clone, exactly as the
            // rendered picture was: `outcome` itself must reach `persist`
            // un-filtered, so the cache never absorbs the baseline's
            // presentation-only hiding. `apply` already removes the
            // hidden diagnostics from `final_diagnostics` in place, so its
            // length IS the post-baseline count with no separate
            // subtraction needed.
            let mut final_diagnostics = outcome.diagnostics.clone();
            if let crate::baseline::Mode::Apply = mode {
                crate::baseline::apply(session, &mut final_diagnostics);
            }
            crate::cache::persist(session, &outcome);
            return ControlFlow::Break(Outcome::of(
                final_diagnostics.len() + crate::configuration::diagnostic_count(session),
                session.internal_errors.len(),
            ));
        }
    };
    session.absorb(&changed);
    // The burst may have carried a manifest change, and a manifest
    // change re-runs discovery, and discovery may declare different
    // walk roots. The watch follows them here, before the next cycle
    // reads the channel again: the next read must come from the roots
    // the project declares now, not the ones it declared when the
    // session started.
    if let Err(error) = watcher.resynchronize(session) {
        return ControlFlow::Break(unwatchable(output, &error));
    }
    ControlFlow::Continue(changed.len())
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
    events: Receiver<WatchEvent>,
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
    /// The live sender for `events`, shared with the signal handler
    /// installed by `install_shutdown_handler` (issue #52) through an
    /// `Arc` clone taken once, before any respawn.
    ///
    /// A respawn inside `resynchronize` replaces `events` with a whole new
    /// channel, and a plain cloned `Sender` held by the handler would go
    /// stale the moment that happens: it would keep sending into a channel
    /// nothing reads from again. The cell exists so the respawn can instead
    /// write the new sender *into this same `Mutex`*, leaving every `Arc`
    /// clone anyone already holds pointing at a cell that always contains
    /// the current, live sender.
    shutdown_sender: Arc<Mutex<Sender<WatchEvent>>>,
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

/// The pieces `Watch::build` assembles, before its two callers decide what
/// to do with the sender: `spawn` mints a brand new `shutdown_sender` cell
/// around it, `resynchronize`'s respawn writes it into a cell that already
/// exists.
struct Built {
    watcher: RecommendedWatcher,
    events: Receiver<WatchEvent>,
    unwatchable: Vec<UnwatchablePath>,
    sender: Sender<WatchEvent>,
}

impl Watch {
    /// The watcher observes the project walk roots plus `composer.json`,
    /// `composer.lock`, `celerrate.toml`, and `celerrate-baseline.toml`.
    /// The vendor walk roots are never watched on their own: thousands of
    /// files that only move when the lockfile does, and a lockfile change
    /// triggers full re-discovery anyway.
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
        let built = Self::build(session)?;
        Ok(Self {
            _watcher: built.watcher,
            events: built.events,
            declared: session.discovery.project_walk_roots.clone(),
            unwatchable: built.unwatchable,
            shutdown_sender: Arc::new(Mutex::new(built.sender)),
        })
    }

    /// The watcher, channel, and refusals a fresh registration over
    /// `session`'s declared roots produces, plus the raw sender that feeds
    /// the channel. Shared by `spawn` (which mints a brand new
    /// `shutdown_sender` cell around the sender: there is no existing cell
    /// yet) and `resynchronize`'s respawn (which instead writes the sender
    /// into the cell it already has, so a handler installed over the
    /// pre-respawn `Watch` keeps sending into the channel the respawned
    /// one actually reads).
    fn build(session: &Session) -> notify::Result<Built> {
        let (sender, receiver) = channel();
        let roots = watched_roots(session);
        let watcher_sender = sender.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event {
                    if !changes_content(&event.kind) {
                        return;
                    }
                    for path in event.paths {
                        let _ = watcher_sender
                            .send(WatchEvent::Changed(as_the_project_names_it(&roots, path)));
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
        for manifest in [
            "composer.json",
            "composer.lock",
            "celerrate.toml",
            crate::baseline::BASELINE_FILE_NAME,
        ] {
            let path = session.discovery.root.join(manifest);
            // A project may perfectly well have none of these files: a
            // missing manifest is already a notice, and a project with no
            // lockfile, no `celerrate.toml`, or no recorded baseline is
            // ordinary. None of them is declared by anyone, so no refusal
            // is reported while the file is absent. The refusal is still
            // *recorded*, because that is a different question: it is what
            // lets a lockfile, a `celerrate.toml`, or a baseline file that
            // appears (or is deleted and rewritten) mid-session be picked
            // up. Watching it is what lets a `celerrate check --baseline`
            // run outside the watch, or an edit made directly to the file,
            // reach `Session::absorb`'s `is_project_manifest` routing to
            // `rediscover` and reload it.
            unwatchable.extend(register(
                &mut watcher,
                &path,
                RecursiveMode::NonRecursive,
                Declared::ByNobody,
            ));
        }
        Ok(Built {
            watcher,
            events: receiver,
            unwatchable,
            sender,
        })
    }

    /// The channel the watch reports through. It is replaced by every
    /// respawn, so nothing may hold it across one.
    ///
    /// Not `pub`: `WatchEvent` is module-private, and nothing outside this
    /// module needs the channel directly (mirrors `resynchronize` below,
    /// which is private for the same reason it returns `Resynchronized`).
    fn events(&self) -> &Receiver<WatchEvent> {
        &self.events
    }

    /// An `Arc` clone of the cell holding this watch's current sender.
    /// `install_shutdown_handler` takes it once, and it stays valid across
    /// every respawn `resynchronize` performs afterward: see the field
    /// doc on `shutdown_sender` for why a plain cloned `Sender` would not.
    ///
    /// Not `pub`, for the same reason `events` is not: `WatchEvent` is
    /// module-private, and nothing outside this module needs the cell
    /// directly — `watch()` is the one production caller, in this same
    /// module.
    fn shutdown_sender(&self) -> Arc<Mutex<Sender<WatchEvent>>> {
        Arc::clone(&self.shutdown_sender)
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
    /// the same root it was given, so the three manifest watches, which
    /// hang off that root, cannot move.
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
        let built = Self::build(session)?;
        // Issue #52: a signal handler installed over the pre-respawn
        // `Watch` holds an `Arc` clone of `self.shutdown_sender`, taken
        // long before this call. Writing the new sender into that SAME
        // cell — instead of letting the respawn mint its own, the way
        // `spawn` does for a first-time `Watch` — is what keeps that
        // clone pointing at the channel the respawned watch actually
        // reads. A poisoned lock degrades to no signal handling rather
        // than a panic: the watch itself is unaffected either way.
        if let Ok(mut guard) = self.shutdown_sender.lock() {
            *guard = built.sender;
        }
        self._watcher = built.watcher;
        self.events = built.events;
        self.declared = session.discovery.project_walk_roots.clone();
        self.unwatchable = built.unwatchable;
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

/// Whether an event can change what analysis reads. Access events cannot:
/// they report reads, and on Linux inotify reports the reads the session
/// itself performs. `Session::absorb` reading an edited file raises one on
/// the watched path, the cycle would treat that as a change and absorb it,
/// absorbing it reads the file again, and the watch would feed on its own
/// absorption forever. macOS and Windows never report reads, which is why
/// only Linux ever looped. Filtering here, at the source, means no consumer
/// downstream has to remember why.
fn changes_content(kind: &notify::EventKind) -> bool {
    !matches!(kind, notify::EventKind::Access(_))
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
///
/// The returned `bool` is issue #52's addition: whether a shutdown request
/// was observed while this cycle ran, on either of two routes. The first
/// is a shutdown that arrives while nothing has changed yet, in which
/// case the worker is joined exactly as on the channel-disconnected path
/// — the in-flight analysis completes (warm cycles are ~13ms) rather
/// than being cancelled, because nothing was absorbed to cancel it with.
/// The second is a shutdown that arrives while a burst is already being
/// collected (`drain_burst`'s own return value): that live change still
/// cancels this attempt and restarts analysis over the absorbed edit, so
/// the flag cannot simply be returned in the same breath it is set — it
/// is carried above the restart loop instead, and read only once this
/// cycle finally settles on a completed outcome.
fn cycle(session: &mut Session, watcher: &mut Watch) -> notify::Result<(AnalysisOutcome, bool)> {
    // Hoisted above the outer loop, not redeclared per restart: a live
    // change that arrives alongside a shutdown (line below) cancels this
    // attempt and `continue`s to a fresh one, and a `shutdown` scoped to
    // one iteration would be reset to `false` right there, losing
    // exactly the request this bool exists to carry. It accumulates
    // across every restart within this one `cycle` call and is read only
    // once, at whichever `return` below finally settles on a completed
    // outcome.
    let mut shutdown = false;
    loop {
        let inputs = session.inputs();
        let worker = std::thread::spawn(move || analyze(&inputs));

        let mut changed: Vec<PathBuf> = Vec::new();
        loop {
            match watcher.events().recv_timeout(POLL_INTERVAL) {
                Ok(WatchEvent::Changed(path)) => changed.push(path),
                Ok(WatchEvent::Shutdown) => {
                    shutdown = true;
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if !changed.is_empty() {
                // Issue #52's must-fix: a shutdown arriving while a burst
                // is already being collected here must not be lost —
                // `drain_burst`'s own return value is exactly that
                // signal, and it folds into this cycle's shutdown flag
                // the same way the direct `Ok(WatchEvent::Shutdown)` arm
                // above does.
                shutdown |= drain_burst(watcher.events(), &mut changed);
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
            Ok(Ok(outcome)) => return Ok((outcome, shutdown)),
            Ok(Err(Cancelled)) => continue,
            Err(_) => {
                session
                    .internal_errors
                    .push(InternalError::AnalysisPanicked);
                return Ok((AnalysisOutcome::default(), shutdown));
            }
        }
    }
}

/// Blocks until something changes, then collects the rest of the burst.
fn wait_for_a_burst(events: &Receiver<WatchEvent>) -> BurstOutcome {
    match events.recv() {
        Ok(WatchEvent::Changed(path)) => burst_starting_with(events, path),
        Ok(WatchEvent::Shutdown) => BurstOutcome::Shutdown,
        Err(_) => BurstOutcome::Disconnected,
    }
}

/// Collects the rest of a burst that has already started with `first`,
/// either `wait_for_a_burst`'s own blocking read, or a change
/// `completed_cycle` already found queued on the channel while deciding
/// whether to persist.
fn burst_starting_with(events: &Receiver<WatchEvent>, first: PathBuf) -> BurstOutcome {
    let mut changed = vec![first];
    if drain_burst(events, &mut changed) {
        // No new work after shutdown: the burst collected so far is
        // deliberately discarded, not carried forward as changes.
        BurstOutcome::Shutdown
    } else {
        BurstOutcome::Changes(changed)
    }
}

/// Collects everything that arrives within the burst window, then
/// deduplicates: an editor's save is several events about one file.
/// Answers whether a shutdown request arrived inside the window.
fn drain_burst(events: &Receiver<WatchEvent>, changed: &mut Vec<PathBuf>) -> bool {
    let mut shutdown = false;
    while let Ok(event) = events.recv_timeout(BURST_WINDOW) {
        match event {
            WatchEvent::Changed(path) => changed.push(path),
            WatchEvent::Shutdown => {
                shutdown = true;
                break;
            }
        }
    }
    changed.sort();
    changed.dedup();
    shutdown
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use std::collections::BTreeSet;
    use std::ops::ControlFlow;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use celerrate_project::PhpVersion;
    use celerrate_source::FileId;
    use celerrate_vfs::ChangedFile;

    use super::{
        BurstOutcome, InputMutation, Resynchronized, UnwatchablePath, Watch, WatchEvent,
        WatchedRoot, as_the_project_names_it, changes_content, reconcile, wait_for_a_burst,
        watched_roots,
    };
    use crate::analysis::{AnalysisOutcome, Cancelled, analyze};
    use crate::render;
    use crate::session::{InternalError, Session};
    use crate::{ColorMode, Outcome};

    /// A `Watch` over a channel the test itself controls, with the
    /// sender kept alive so it can inject `WatchEvent`s (issue #52's
    /// `iteration` tests) or be dropped on purpose to drive the
    /// `Disconnected` branch. Nothing is registered with the operating
    /// system.
    ///
    /// For a test that mutates the session directly (`session.absorb`)
    /// rather than through a real filesystem edit, this is what keeps
    /// the persist-skip check deterministic: a real `Watch::spawn`
    /// over a temporary directory really does observe the test's own
    /// `std::fs::write` calls, and the OS event's arrival time relative
    /// to `completed_cycle`'s own `try_recv` peek is a race this helper
    /// removes rather than accepts.
    fn watch_with_held_sender(session: &Session) -> (Watch, Sender<WatchEvent>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let watcher = Watch {
            _watcher: notify::recommended_watcher(|_event: notify::Result<notify::Event>| {})
                .unwrap(),
            events: receiver,
            declared: session.discovery.project_walk_roots.clone(),
            unwatchable: Vec::new(),
            shutdown_sender: Arc::new(Mutex::new(sender.clone())),
        };
        (watcher, sender)
    }

    /// A `Watch` whose channel never receives anything: the sender from
    /// `watch_with_held_sender` is dropped immediately, so `try_recv`
    /// always answers `Disconnected` and `recv_timeout` never actually
    /// waits out its timeout.
    fn silent_watch(session: &Session) -> Watch {
        watch_with_held_sender(session).0
    }

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
    fn reported_until(events: &Receiver<WatchEvent>, wanted: &Path) -> BTreeSet<PathBuf> {
        let deadline = Duration::from_secs(5);
        let quiet = Duration::from_millis(500);
        let started = Instant::now();
        let mut seen = BTreeSet::new();
        while started.elapsed() < deadline {
            match events.recv_timeout(Duration::from_millis(100)) {
                Ok(WatchEvent::Changed(path)) => {
                    let found = path == wanted;
                    seen.insert(path);
                    if found {
                        break;
                    }
                }
                Ok(WatchEvent::Shutdown) | Err(_) => continue,
            }
        }
        while let Ok(WatchEvent::Changed(path)) = events.recv_timeout(quiet) {
            seen.insert(path);
        }
        seen
    }

    /// The one test that touches the operating system's real notification
    /// mechanism rather than driving the watch through direct calls to
    /// `Session::absorb`. It lives here, not in `tests/`, because `events()`
    /// and `WatchEvent` are both module-private (issue #52's channel now
    /// carries the shutdown vocabulary alongside changes, and that
    /// vocabulary is an internal implementation detail, not a public API):
    /// an external integration test could no longer name either.
    ///
    /// Deliberately tolerant: filesystem notification is platform-specific,
    /// coalesced, and reordered, so it asserts that the adapter reports the
    /// paths at all, not how many events each edit produced.
    mod the_os_touching_notify_adapter {
        use super::{BTreeSet, Duration, Instant, PathBuf, Receiver, Session, Watch, WatchEvent};

        /// How long the channel must stay silent before the events of an
        /// edit are taken to be all in. Generous, because it is paid once
        /// and a leftover event is what would make the deletion assertion a
        /// lie.
        const QUIET: Duration = Duration::from_millis(500);

        /// Collects reported paths until `wanted` have all appeared, or the
        /// deadline passes.
        fn collect_until(
            events: &Receiver<WatchEvent>,
            wanted: &BTreeSet<PathBuf>,
            deadline: Duration,
        ) -> BTreeSet<PathBuf> {
            let started = Instant::now();
            let mut seen = BTreeSet::new();
            while started.elapsed() < deadline {
                match events.recv_timeout(Duration::from_millis(100)) {
                    Ok(WatchEvent::Changed(path)) => {
                        seen.insert(path);
                        if wanted.iter().all(|path| seen.contains(path)) {
                            return seen;
                        }
                    }
                    Ok(WatchEvent::Shutdown) | Err(_) => continue,
                }
            }
            seen
        }

        /// Empties the channel of everything the edits so far produced,
        /// queued or still in flight, by waiting the queue out until it
        /// falls silent.
        ///
        /// Emptying only what is already queued would not be enough:
        /// `collect_until` returns the instant it has seen what it wanted,
        /// and the rest of that burst is still on its way.
        fn drain(events: &Receiver<WatchEvent>) {
            while events.recv_timeout(QUIET).is_ok() {}
        }

        #[test]
        #[ignore = "spawns a real filesystem watcher and waits on operating system notifications, which costs seconds per test; the continuous integration suite runs it with --include-ignored"]
        fn the_adapter_reports_creation_modification_and_deletion() {
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();

            let session = Session::start(root.path());
            let watcher = Watch::spawn(&session).unwrap();
            let events = watcher.events();

            let created = root.path().join("b.php");
            std::fs::write(&created, "<?php class B {}").unwrap();
            let modified = root.path().join("a.php");
            std::fs::write(&modified, "<?php class A { public int $x = 1; }").unwrap();

            let wanted: BTreeSet<PathBuf> =
                [created.clone(), modified.clone()].into_iter().collect();
            let seen = collect_until(events, &wanted, Duration::from_secs(5));
            assert!(
                seen.contains(&created) && seen.contains(&modified),
                "the watcher reported {seen:?}",
            );

            // One `write` on a new file commonly yields several events
            // about it, and `collect_until` above stopped at the first
            // that completed the set. Every leftover event about `b.php`
            // predates its removal, and any one of them would satisfy the
            // assertion below just as well as a deletion would: without
            // this drain the test would pass unchanged on a platform that
            // never reports deletions at all. Draining is what makes the
            // assertion real, and the deletion is precisely the case that
            // justifies rewriting a reported path as a string rather than
            // canonicalizing it, since by then the file is gone.
            drain(events);

            std::fs::remove_file(&created).unwrap();
            let wanted: BTreeSet<PathBuf> = [created.clone()].into_iter().collect();
            let seen = collect_until(events, &wanted, Duration::from_secs(5));
            assert!(
                seen.contains(&created),
                "the deletion was reported: {seen:?}"
            );
        }
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
    #[ignore = "spawns a real filesystem watcher and waits on operating system notifications, which costs seconds per test; the continuous integration suite runs it with --include-ignored"]
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

    /// The signal handler (issue #52) takes `shutdown_sender()` once, well
    /// before any respawn. `resynchronize` replaces the whole channel on a
    /// respawn, so if it also minted a fresh cell, the handler's `Arc`
    /// clone would keep locking a cell nothing reads from again, and every
    /// Ctrl+C after the first respawn would silently do nothing. This
    /// drives the exact respawn `a_walk_root_the_manifest_grows_is_watched_
    /// and_mapped` does, then proves the handler's own path: send
    /// `Shutdown` through the cell taken *before* the respawn, and observe
    /// it on the watch's *current*, post-respawn receiver.
    #[test]
    #[ignore = "spawns a real filesystem watcher and waits on operating system notifications, which costs seconds per test; the continuous integration suite runs it with --include-ignored"]
    fn a_respawn_updates_the_shared_sender_cell() {
        let root = project_with_an_undeclared_directory();
        let manifest = root.path().join("composer.json");

        let mut session = Session::start(root.path());
        let mut watcher = Watch::spawn(&session).unwrap();
        let cell = watcher.shutdown_sender();

        std::fs::write(
            &manifest,
            r#"{"autoload": {"psr-4": {"App\\": "src", "Lib\\": "lib"}}}"#,
        )
        .unwrap();
        session.absorb(std::slice::from_ref(&manifest));
        assert_eq!(
            watcher.resynchronize(&session).unwrap(),
            Resynchronized::Respawned,
            "the respawn this test needs to prove the cell survives really happened",
        );

        // The cell still sends into the channel the respawned watch is reading.
        cell.lock().unwrap().send(WatchEvent::Shutdown).unwrap();

        match watcher.events().recv_timeout(Duration::from_secs(5)) {
            Ok(WatchEvent::Shutdown) => {}
            other => {
                panic!("expected Shutdown through the watch's post-respawn receiver, got {other:?}")
            }
        }
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
    #[ignore = "spawns a real filesystem watcher and waits on operating system notifications, which costs seconds per test; the continuous integration suite runs it with --include-ignored"]
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
    #[ignore = "spawns a real filesystem watcher and waits on operating system notifications, which costs seconds per test; the continuous integration suite runs it with --include-ignored"]
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
        std::fs::write(root.path().join("celerrate.toml"), "").unwrap();
        std::fs::write(root.path().join(crate::baseline::BASELINE_FILE_NAME), "").unwrap();

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
    /// The lockfile, `celerrate.toml`, and the baseline file are all
    /// written so that the only refusal these tests see is the one they
    /// are about. A project without any one of them records a refusal for
    /// it too, which is what lets one created mid-session be picked up;
    /// the lockfile case has its own test at this registration-retry
    /// level. `celerrate.toml`'s mid-session appearance is instead
    /// covered at the higher reconfiguration level, by
    /// `a_configuration_saved_mid_watch_reconfigures_the_next_cycle`,
    /// which drives injected watch events rather than the registration
    /// retry mechanism itself.
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
        std::fs::write(root.path().join("celerrate.toml"), "").unwrap();
        std::fs::write(root.path().join(crate::baseline::BASELINE_FILE_NAME), "").unwrap();
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
    #[ignore = "spawns a real filesystem watcher and waits on operating system notifications, which costs seconds per test; the continuous integration suite runs it with --include-ignored"]
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
        render::render_check(
            &mut output,
            &mut session,
            &AnalysisOutcome::default(),
            ColorMode::Plain,
        )
        .unwrap();
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
    #[ignore = "spawns a real filesystem watcher and waits on operating system notifications, which costs seconds per test; the continuous integration suite runs it with --include-ignored"]
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
    #[ignore = "spawns a real filesystem watcher and waits on operating system notifications, which costs seconds per test; the continuous integration suite runs it with --include-ignored"]
    fn a_lockfile_created_mid_session_is_watched_from_then_on() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/A.php"), "<?php class A {}").unwrap();
        std::fs::write(
            root.path().join("composer.json"),
            r#"{"autoload": {"psr-4": {"App\\": "src"}}}"#,
        )
        .unwrap();
        // Present from the start so the only refusal this test is about is
        // the lockfile's: a project with no `celerrate.toml` or no
        // recorded baseline is exactly as ordinary, and their own
        // mid-session appearance is covered elsewhere.
        std::fs::write(root.path().join("celerrate.toml"), "").unwrap();
        std::fs::write(root.path().join(crate::baseline::BASELINE_FILE_NAME), "").unwrap();
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
    #[ignore = "spawns a real filesystem watcher and waits on operating system notifications, which costs seconds per test; the continuous integration suite runs it with --include-ignored"]
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
    #[ignore = "spawns a real filesystem watcher and waits on operating system notifications, which costs seconds per test; the continuous integration suite runs it with --include-ignored"]
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

    /// The persist clause at the watch level: after a cycle absorbs an
    /// edit, the packs on disk carry the
    /// cycle's results — proven by decoding the diagnostics pack and
    /// finding the edited content's hash keyed in it, not by reading the
    /// source of `watch`.
    #[test]
    fn a_cycle_rewrites_the_packs_with_its_results() {
        use crate::cache::pack::{Pack, PackHeader, decode};
        use crate::cache::stored::StoredVerdict;

        let root = tempfile::tempdir().unwrap();
        let edited = root.path().join("a.php");
        std::fs::write(&edited, "<?php class A {}").unwrap();
        let mut session = Session::start(root.path());
        let mut watcher = silent_watch(&session);
        let mut output = Vec::new();

        let (first, pending, shutdown) = super::completed_cycle(
            &mut session,
            &mut watcher,
            &mut output,
            1,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        )
        .unwrap();
        assert!(
            first.diagnostics.is_empty(),
            "sanity: the initial state is clean"
        );
        assert!(
            pending.is_none(),
            "the silent watch never has anything queued, so this cycle persists",
        );
        assert!(!shutdown, "the silent watch never sends a shutdown");
        let diagnostics_pack = root.path().join(".celerrate/cache/diagnostics.bin");
        let after_first = std::fs::read(&diagnostics_pack).unwrap();

        let edited_source = "<?php class A {} new Missing();";
        std::fs::write(&edited, edited_source).unwrap();
        session.absorb(std::slice::from_ref(&edited));
        let (second, _, _) = super::completed_cycle(
            &mut session,
            &mut watcher,
            &mut output,
            1,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        )
        .unwrap();
        assert_eq!(second.diagnostics.len(), 1, "the cycle sees the edit");

        let after_second = std::fs::read(&diagnostics_pack).unwrap();
        assert_ne!(
            after_first, after_second,
            "the cycle's persist rewrote the pack"
        );

        let header = PackHeader::current(
            session.configuration.php_version_range(&session.database),
            session.plugin_set_digest,
            session.configuration_digest,
        );
        let pack: Pack<Vec<([u8; 32], StoredVerdict)>> = decode(&after_second, &header).unwrap();
        assert!(
            pack.entries
                .iter()
                .any(|(key, _)| key == blake3::hash(edited_source.as_bytes()).as_bytes()),
            "the pack on disk is keyed by the edited content",
        );
    }

    /// The persist-skip fallback: a measured corpus run
    /// (symfony/demo, 9341 files, release build) found per-cycle persist
    /// costing several times the ~13ms median warm cycle it was folded
    /// into, well past the 10% ceiling — so a busy cycle, one where a
    /// change is already queued on the burst channel by the time this
    /// cycle would persist, skips the write instead, and the very next
    /// quiet cycle persists what the busy one deferred. This pins the
    /// trade directly on `completed_cycle`: nothing is dropped, only
    /// deferred, and the queued path is threaded back rather than lost —
    /// `persist_written`/`persist_skipped` move on neither pack during
    /// the busy cycle and do move on the quiet one that follows.
    #[test]
    fn a_skipped_persist_lands_on_the_next_quiet_cycle() {
        use std::sync::atomic::Ordering;

        // `super::persist_unless_a_burst_is_already_waiting` directly:
        // `cycle`'s own inner analysis loop polls the very same channel
        // this decision reads, and drains any message already queued
        // before analysis even starts (that is what lets it cancel and
        // restart an in-flight analysis at all) — so a message
        // pre-seeded ahead of a full `completed_cycle` call would never
        // survive to reach this decision, and only a message that truly
        // arrives in the narrow window between `cycle` returning and
        // this check would exercise it, which no synchronous test can
        // land deterministically. The decision is a small, self-
        // contained function precisely so it can be pinned without that
        // race.
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();
        let mut session = Session::start(root.path());
        let outcome = AnalysisOutcome::default();

        let persisted = |session: &Session| -> u64 {
            session.statistics.persist_written.load(Ordering::Relaxed)
                + session.statistics.persist_skipped.load(Ordering::Relaxed)
        };

        // Warm the pack once on a channel with nothing queued, so the
        // busy cycle below has an unchanged entry set to compare
        // against and skip a rewrite of, exactly as an ordinary quiet
        // cycle would.
        let watcher = silent_watch(&session);
        let pending =
            super::persist_unless_a_burst_is_already_waiting(&mut session, &watcher, &outcome);
        assert!(pending.is_none(), "sanity: nothing was ever queued yet");
        let written_before = persisted(&session);
        assert!(written_before > 0, "sanity: the first cycle did persist");

        // A change lands on the channel exactly as if the user kept
        // editing while this cycle's analysis ran — `wait_for_a_burst`
        // would not have blocked at all had this cycle reached it.
        let mut busy_watcher = silent_watch(&session);
        let (sender, receiver) = std::sync::mpsc::channel();
        busy_watcher.events = receiver;
        let queued = root.path().join("a.php");
        sender.send(WatchEvent::Changed(queued.clone())).unwrap();

        let pending =
            super::persist_unless_a_burst_is_already_waiting(&mut session, &busy_watcher, &outcome);
        assert_eq!(
            pending,
            Some(WatchEvent::Changed(queued)),
            "the queued change is threaded back, never dropped",
        );
        assert_eq!(
            persisted(&session),
            written_before,
            "the busy cycle skips persist entirely: neither written nor skipped moves",
        );

        // The channel is quiet now — the queued path was consumed by
        // the `try_recv` peek above and nothing replaced it — so the
        // very next cycle persists, landing what the busy cycle
        // deferred.
        let pending =
            super::persist_unless_a_burst_is_already_waiting(&mut session, &busy_watcher, &outcome);
        assert!(pending.is_none(), "nothing is queued on a quiet channel");
        assert!(
            persisted(&session) > written_before,
            "the next quiet cycle actually persists",
        );
    }

    /// The no-provisional pin: "no provisional value served or
    /// persisted". `completed_cycle`
    /// calls `crate::cache::persist` exactly once, AFTER `cycle` settles
    /// on a completed `AnalysisOutcome` — `cycle`'s own internal restart
    /// loop, entered whenever a change lands mid-analysis and cancels
    /// the in-flight `analyze` (the same primitive
    /// `a_setter_cancels_an_analysis_that_is_already_running` above
    /// pins), never calls `persist` itself. This drives that exact
    /// cancellation primitive directly — not through the OS `notify`
    /// watcher, which the other test already establishes is sufficient
    /// to reach the same `Cancelled` arm inside `cycle`'s own loop, so
    /// re-deriving it through real filesystem events here would only add
    /// timing flakiness for no additional coverage — and asserts persist
    /// genuinely never ran during the cancelled attempt: the packs on
    /// disk stay byte-identical, mtime included, to the prior COMPLETED
    /// cycle's own persist, right up until the NEXT cycle actually
    /// completes and persists in turn.
    #[test]
    fn a_cancelled_cycle_persists_nothing() {
        use crate::cache::pack::{Pack, PackHeader, decode};
        use crate::cache::stored::StoredVerdict;

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
        let mut watcher = silent_watch(&session);
        let mut output = Vec::new();

        // One completed cycle: the packs exist, and this is the "prior
        // completed cycle's" state the cancelled attempt below must
        // leave untouched.
        let (first, _, _) = super::completed_cycle(
            &mut session,
            &mut watcher,
            &mut output,
            total,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        )
        .unwrap();
        assert!(
            first.diagnostics.is_empty(),
            "sanity: the initial circular-inheritance fixture is clean"
        );

        let diagnostics_pack = root.path().join(".celerrate/cache/diagnostics.bin");
        let item_trees_pack = root.path().join(".celerrate/cache/item_trees.bin");
        let after_first_diagnostics = std::fs::read(&diagnostics_pack).unwrap();
        let after_first_trees = std::fs::read(&item_trees_pack).unwrap();
        let mtime_before = std::fs::metadata(&diagnostics_pack)
            .unwrap()
            .modified()
            .unwrap();

        // The exact cancellation primitive `a_setter_cancels_an_analysis_
        // that_is_already_running` above pins: a mutation lands on the
        // main thread's `&mut Session` handle while a worker thread is
        // mid-fan-out over `analyze`, raising `salsa::Cancelled` in the
        // worker. Neither `absorb_outcome` nor `persist` is reachable
        // from this loop at all — both live only inside `completed_
        // cycle`, downstream of a settled `cycle()` result — so this is
        // the whole point under test: cancellation cannot persist
        // anything by construction, and this asserts that empirically
        // too, on the actual bytes on disk.
        let edited = root.path().join("Service0.php");
        let mut cancelled = false;
        for attempt in 0..20 {
            let inputs = session.inputs();
            let worker = std::thread::spawn(move || analyze(&inputs));

            std::fs::write(
                &edited,
                format!("<?php class Service0 {{ public int $x = {attempt}; }} new Missing();"),
            )
            .unwrap();
            session.absorb(std::slice::from_ref(&edited));

            if matches!(worker.join(), Ok(Err(Cancelled))) {
                cancelled = true;
                break;
            }
        }
        assert!(
            cancelled,
            "the analysis was never caught in flight: cancellation was never observed",
        );

        assert_eq!(
            std::fs::read(&diagnostics_pack).unwrap(),
            after_first_diagnostics,
            "a cancelled analysis must persist nothing: the diagnostics pack bytes are unchanged",
        );
        assert_eq!(
            std::fs::read(&item_trees_pack).unwrap(),
            after_first_trees,
            "a cancelled analysis must persist nothing: the item-tree pack bytes are unchanged",
        );
        assert_eq!(
            std::fs::metadata(&diagnostics_pack)
                .unwrap()
                .modified()
                .unwrap(),
            mtime_before,
            "a cancelled analysis must not even rewrite the pack with identical bytes",
        );

        // The next COMPLETED cycle sees the settled edit (real bytes on
        // disk: `new Missing();`) and persists it — proving the
        // comparison above was not vacuous, since the packs on disk CAN
        // and DO change once a cycle actually completes.
        let (second, _, _) = super::completed_cycle(
            &mut session,
            &mut watcher,
            &mut output,
            1,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        )
        .unwrap();
        assert!(
            second
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Missing")),
            "the next completed cycle sees the settled edit: {:?}",
            second.diagnostics,
        );
        assert_ne!(
            std::fs::read(&diagnostics_pack).unwrap(),
            after_first_diagnostics,
            "the next completed cycle's persist does rewrite the pack",
        );

        let header = PackHeader::current(
            session.configuration.php_version_range(&session.database),
            session.plugin_set_digest,
            session.configuration_digest,
        );
        let after_second = std::fs::read(&diagnostics_pack).unwrap();
        let pack: Pack<Vec<([u8; 32], StoredVerdict)>> = decode(&after_second, &header).unwrap();
        assert!(
            pack.entries.iter().any(|(_, verdict)| verdict
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Missing"))),
            "the persisted pack carries the settled edit's own finding, never a cancelled one",
        );
    }

    /// On Linux, inotify reports `IN_OPEN` as `EventKind::Access(_)`, and
    /// `Session::absorb` reading a changed file raises exactly one of
    /// those on the path it just read. An access event never changes
    /// content, so it must not be forwarded: forwarding it is what turned
    /// the watch's own read of an edit into another event, feeding the
    /// cycle its own reads forever. macOS and Windows never emit these,
    /// which is why only Linux ever looped.
    #[test]
    fn only_events_that_can_change_content_pass_the_filter() {
        use notify::EventKind;
        use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};

        assert!(!changes_content(&EventKind::Access(AccessKind::Any)));
        assert!(!changes_content(&EventKind::Access(AccessKind::Open(
            notify::event::AccessMode::Any
        ))));
        assert!(changes_content(&EventKind::Modify(ModifyKind::Any)));
        assert!(changes_content(&EventKind::Create(CreateKind::Any)));
        assert!(changes_content(&EventKind::Remove(RemoveKind::Any)));
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

    /// Issue #52's vocabulary: the watch channel carries a shutdown
    /// request alongside filesystem changes, and the burst readers answer
    /// what one blocking read amounts to.
    #[test]
    fn a_shutdown_while_idle_ends_the_wait() {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(WatchEvent::Shutdown).unwrap();
        assert!(matches!(
            wait_for_a_burst(&receiver),
            BurstOutcome::Shutdown
        ));
    }

    #[test]
    fn a_shutdown_inside_a_burst_discards_the_burst() {
        // No new work after shutdown: the not-yet-analyzed burst is
        // dropped, the last completed state is what the exit persists.
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(WatchEvent::Changed(PathBuf::from("src/a.php")))
            .unwrap();
        sender.send(WatchEvent::Shutdown).unwrap();
        assert!(matches!(
            wait_for_a_burst(&receiver),
            BurstOutcome::Shutdown
        ));
    }

    #[test]
    fn a_dropped_sender_reads_as_disconnected() {
        let (_, receiver) = {
            let (sender, receiver) = std::sync::mpsc::channel::<WatchEvent>();
            drop(sender);
            ((), receiver)
        };
        assert!(matches!(
            wait_for_a_burst(&receiver),
            BurstOutcome::Disconnected
        ));
    }

    #[test]
    fn a_plain_burst_still_collects_sorts_and_dedups() {
        let (sender, receiver) = std::sync::mpsc::channel();
        for path in ["src/b.php", "src/a.php", "src/b.php"] {
            sender
                .send(WatchEvent::Changed(PathBuf::from(path)))
                .unwrap();
        }
        let BurstOutcome::Changes(changed) = wait_for_a_burst(&receiver) else {
            panic!("expected changes");
        };
        assert_eq!(
            changed,
            vec![PathBuf::from("src/a.php"), PathBuf::from("src/b.php")],
        );
    }

    /// Issue #52's `iteration`: a shutdown queued ahead of the call is
    /// observed by `completed_cycle`'s own `persist_unless_a_burst_is_
    /// already_waiting` peek, so it never reaches the burst wait at all —
    /// `iteration` must still take the graceful exit, and the exit's own
    /// persist must be the one that lands on disk (not the busy cycle's,
    /// which this fixture never runs in the first place).
    #[test]
    fn a_shutdown_event_exits_through_the_graceful_persist() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();
        let mut session = Session::start(root.path());
        let (mut watcher, sender) = watch_with_held_sender(&session);
        let mut output = Vec::new();

        let diagnostics_pack = root.path().join(".celerrate/cache/diagnostics.bin");
        assert!(!diagnostics_pack.exists(), "sanity: nothing persisted yet",);

        sender.send(WatchEvent::Shutdown).unwrap();
        let outcome = match super::iteration(
            &mut session,
            &mut watcher,
            &mut output,
            1,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        ) {
            ControlFlow::Break(outcome) => outcome,
            ControlFlow::Continue(next) => {
                panic!("a shutdown must break the loop, not continue with {next}")
            }
        };
        assert_eq!(outcome, Outcome::Clean, "the fixture project is clean");
        assert!(
            diagnostics_pack.exists(),
            "the graceful exit persists the cycle it completed before the shutdown",
        );
    }

    /// Issue #52's `iteration`: a change delivered only once the loop is
    /// genuinely idle (a background thread sends it after a short delay,
    /// long after the fixture's trivial cycle has settled) is picked up
    /// by the burst wait between cycles, absorbed, and the loop
    /// continues rather than exiting.
    #[test]
    fn a_burst_event_continues_the_loop() {
        let root = tempfile::tempdir().unwrap();
        let edited = root.path().join("a.php");
        std::fs::write(&edited, "<?php class A {}").unwrap();
        let mut session = Session::start(root.path());
        let (mut watcher, sender) = watch_with_held_sender(&session);
        let mut output = Vec::new();

        // The edit lands on disk before the event is sent: `Session::
        // absorb` reads the file's current contents when the path
        // arrives, so what proves the session absorbed the change is the
        // very next cycle seeing it, not the burst itself.
        let edited_source = "<?php class A {} new Missing();";
        std::fs::write(&edited, edited_source).unwrap();

        let sent = edited.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            sender.send(WatchEvent::Changed(sent)).unwrap();
        });

        let next = match super::iteration(
            &mut session,
            &mut watcher,
            &mut output,
            1,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        ) {
            ControlFlow::Continue(next) => next,
            ControlFlow::Break(outcome) => {
                panic!("a plain change must not break the loop: {outcome:?}")
            }
        };
        assert_eq!(
            next, 1,
            "one path changed, so the next cycle reports one reanalyzed file",
        );

        let (outcome, _, shutdown) = super::completed_cycle(
            &mut session,
            &mut watcher,
            &mut output,
            next,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        )
        .unwrap();
        assert!(!shutdown, "no shutdown was ever sent");
        assert_eq!(
            outcome.diagnostics.len(),
            1,
            "the session absorbed the edit the burst carried, and the next cycle sees it",
        );
    }

    /// Issue #52's `iteration`: the `Disconnected` branch the module's
    /// own comment calls unreachable "while the watch is alive" — kept
    /// only so the loop is total — is, with a held sender, finally
    /// something a test can drive on purpose. Dropping it must still
    /// reach the graceful exit and its persist.
    #[test]
    fn a_disconnected_channel_exits_through_the_graceful_persist() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();
        let mut session = Session::start(root.path());
        let (mut watcher, sender) = watch_with_held_sender(&session);
        drop(sender);
        // `watch_with_held_sender` also seeds the shutdown-sender cell
        // with a clone: issue #52's Ctrl+C plumbing keeps one live
        // sender for the watch's whole life, which is exactly why
        // `Disconnected` cannot occur in production. Orphan that clone
        // too, or the channel never disconnects and the loop's blocking
        // read waits forever instead of taking the branch under test.
        {
            let (orphan, _) = std::sync::mpsc::channel();
            *watcher.shutdown_sender.lock().unwrap() = orphan;
        }
        let mut output = Vec::new();

        let diagnostics_pack = root.path().join(".celerrate/cache/diagnostics.bin");
        assert!(!diagnostics_pack.exists(), "sanity: nothing persisted yet",);

        let outcome = match super::iteration(
            &mut session,
            &mut watcher,
            &mut output,
            1,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        ) {
            ControlFlow::Break(outcome) => outcome,
            ControlFlow::Continue(next) => {
                panic!("a disconnected channel must break the loop, not continue with {next}")
            }
        };
        assert_eq!(outcome, Outcome::Clean, "the fixture project is clean");
        assert!(
            diagnostics_pack.exists(),
            "the graceful exit persists the cycle it completed before the disconnect",
        );
    }

    /// `iteration`'s
    /// `BurstOutcome::Shutdown | Disconnected` arm computes the watch
    /// loop's own exit code from a `final_diagnostics` clone it applies
    /// the baseline to itself, independently of the presentation copy
    /// `completed_cycle` already built for rendering. Both existing
    /// shutdown tests above (`a_shutdown_event_exits_through_the_
    /// graceful_persist`, `a_disconnected_channel_exits_through_the_
    /// graceful_persist`) run on a project with nothing to baseline, so
    /// `final_diagnostics.len()` is 0 whether or not the exit arm's own
    /// `baseline::apply` call runs at all -- neither one would notice if
    /// those four lines were deleted outright.
    ///
    /// This drives the one fixture that can tell the difference:
    /// `project_with_a_recorded_baseline` has exactly one finding, and it
    /// is fully baselined. A shutdown delivered before any burst starts
    /// takes the very same route `a_shutdown_event_exits_through_the_
    /// graceful_persist` pins, so the only thing left to vary is the
    /// mode. Asserting `Mode::Ignore` still exits `DiagnosticsReported`
    /// over the identical fixture is what proves the `Mode::Apply` result
    /// below is the exit arm's own doing, and not some accident of the
    /// fixture itself.
    #[test]
    fn a_shutdown_over_a_fully_baselined_project_exits_clean() {
        let root = project_with_a_recorded_baseline();

        let exit_outcome = |mode: crate::baseline::Mode| -> Outcome {
            let mut session = Session::start(root.path());
            let (mut watcher, sender) = watch_with_held_sender(&session);
            let mut output = Vec::new();
            sender.send(WatchEvent::Shutdown).unwrap();
            match super::iteration(
                &mut session,
                &mut watcher,
                &mut output,
                1,
                ColorMode::Plain,
                mode,
            ) {
                ControlFlow::Break(outcome) => outcome,
                ControlFlow::Continue(next) => {
                    panic!("a shutdown must break the loop, not continue with {next}")
                }
            }
        };

        assert_eq!(
            exit_outcome(crate::baseline::Mode::Apply),
            Outcome::Clean,
            "the recorded baseline hides the project's one finding, and the graceful exit's \
             own arithmetic must apply it exactly like the cycle that ran just before it",
        );
        assert_eq!(
            exit_outcome(crate::baseline::Mode::Ignore),
            Outcome::DiagnosticsReported,
            "sanity: --ignore-baseline over the identical fixture still reports the finding, \
             which is what proves the Apply case above is the exit arm's own doing rather than \
             a quirk of the fixture",
        );
    }

    /// The part 2 wiring promise: a `celerrate.toml` saved mid-watch
    /// reconfigures the very next cycle. The project fires CEL0034; a
    /// saved configuration disabling `null-dereference` must make the next
    /// picture clean, and deleting the file must bring the diagnostic
    /// back.
    ///
    /// Driven with a held sender exactly like `a_burst_event_continues_
    /// the_loop` above, and for the same reason that test delivers its
    /// event from a delayed background thread rather than sending it
    /// synchronously before the call: `cycle`'s own inner loop polls the
    /// channel while analysis is in flight, and a message already queued
    /// by the time `iteration` is called can be caught and absorbed by
    /// that inner cancel-and-restart mechanism before `iteration`'s own
    /// burst-wait logic ever sees anything queued — which would then
    /// block forever in `wait_for_a_burst` with no second message ever
    /// coming. Delaying the send until after the fixture's own trivial
    /// cycle has settled guarantees the event is instead picked up by
    /// `iteration`'s own logic, exactly like the existing test's fixture.
    /// What that absorption changed is only visible in the cycle after,
    /// so every reconfiguring step here is a call to `iteration` (to
    /// absorb) followed by a call to `completed_cycle` (to see the
    /// picture that absorption produced).
    #[test]
    fn a_configuration_saved_mid_watch_reconfigures_the_next_cycle() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(
            root.path().join("composer.json"),
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        )
        .unwrap();
        std::fs::write(
            root.path().join("src/Consumer.php"),
            r#"<?php
declare(strict_types=1);
namespace App;

class User
{
    public function save(): void
    {
    }
}

class Consumer
{
    public function run(?User $user): void
    {
        $user->save();
    }
}
"#,
        )
        .unwrap();

        let mut session = Session::start(root.path());
        let (mut watcher, sender) = watch_with_held_sender(&session);
        let mut output = Vec::new();

        // The first picture, over the project as it stands: the possibly-
        // null dereference fires.
        let (first, _, _) = super::completed_cycle(
            &mut session,
            &mut watcher,
            &mut output,
            1,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        )
        .unwrap();
        assert_eq!(
            first.diagnostics.len(),
            1,
            "the fixture fires exactly the null dereference: {:?}",
            first.diagnostics,
        );
        let first_text = String::from_utf8(output.clone()).unwrap();
        assert!(
            first_text.contains("CEL0034"),
            "the initial picture reports the null dereference: {first_text}",
        );

        // A `celerrate.toml` disabling the rule is saved mid-watch, and the
        // watch is told about it exactly as the real notify adapter would:
        // the event is delivered from a delayed background thread, after
        // the fixture's own trivial cycle has settled (see the doc comment
        // above for why a synchronous send races `cycle`'s own inner
        // loop).
        let configuration = root.path().join("celerrate.toml");
        std::fs::write(
            &configuration,
            "[rules.null-dereference]\nenabled = false\n",
        )
        .unwrap();
        let sent = configuration.clone();
        let deliver = sender.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            deliver.send(WatchEvent::Changed(sent)).unwrap();
        });

        let next = match super::iteration(
            &mut session,
            &mut watcher,
            &mut output,
            1,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        ) {
            ControlFlow::Continue(next) => next,
            ControlFlow::Break(outcome) => {
                panic!("a configuration save must not break the loop: {outcome:?}")
            }
        };

        output.clear();
        let (second, _, _) = super::completed_cycle(
            &mut session,
            &mut watcher,
            &mut output,
            next,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        )
        .unwrap();
        assert!(
            second.diagnostics.is_empty(),
            "the disabled rule no longer fires: {:?}",
            second.diagnostics,
        );
        let second_text = String::from_utf8(output.clone()).unwrap();
        assert!(
            !second_text.contains("CEL0034"),
            "the next picture is clean: {second_text}",
        );
        for identifier in [
            "CEL0043", "CEL0044", "CEL0045", "CEL0046", "CEL0047", "CEL0048", "CEL0049",
        ] {
            assert!(
                !second_text.contains(identifier),
                "a valid celerrate.toml carries no configuration diagnostic of its own \
                 ({identifier}): {second_text}",
            );
        }

        // Deleting the file returns the session to defaults, and the
        // diagnostic the file had silenced comes back. Same delayed
        // delivery, same reason.
        std::fs::remove_file(&configuration).unwrap();
        let sent = configuration.clone();
        let deliver = sender.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            deliver.send(WatchEvent::Changed(sent)).unwrap();
        });

        let next = match super::iteration(
            &mut session,
            &mut watcher,
            &mut output,
            next,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        ) {
            ControlFlow::Continue(next) => next,
            ControlFlow::Break(outcome) => {
                panic!("a configuration deletion must not break the loop: {outcome:?}")
            }
        };

        output.clear();
        let (third, _, _) = super::completed_cycle(
            &mut session,
            &mut watcher,
            &mut output,
            next,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        )
        .unwrap();
        assert_eq!(
            third.diagnostics.len(),
            1,
            "deleting the file returns the rule to active: {:?}",
            third.diagnostics,
        );
        let third_text = String::from_utf8(output.clone()).unwrap();
        assert!(
            third_text.contains("CEL0034"),
            "the diagnostic is back: {third_text}",
        );
    }

    /// A project with a single finding (`a.php` contains exactly one
    /// `new Missing()`, CEL0018) and a `celerrate-baseline.toml` that
    /// already records it, recorded from a first session's own analysis.
    ///
    /// Shared by the watch-cycle baseline tests below, which differ only
    /// in what they do with the fixture afterward: a fresh
    /// `Session::start` over the returned root is what picks up the file
    /// `Session::start` reads once, up front, mirroring how
    /// `a_cycle_rewrites_the_packs_with_its_results` above builds its own
    /// fixture, and why the configuration-reload test restarts nothing:
    /// here, unlike there, the baseline must exist on disk before the
    /// very first cycle runs.
    fn project_with_a_recorded_baseline() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php class A {} new Missing();").unwrap();

        let recording_session = Session::start(root.path());
        let inputs = recording_session.inputs();
        let outcome = analyze(&inputs).unwrap();
        assert_eq!(
            outcome.diagnostics.len(),
            1,
            "sanity: exactly one finding to baseline: {:?}",
            outcome.diagnostics,
        );
        let recorded = crate::baseline::record(&recording_session, &outcome.diagnostics).unwrap();
        assert!(recorded.is_some(), "a fresh finding is always recorded");

        root
    }

    /// The watch's own part of the baseline promise: a recorded entry
    /// hides its finding from a watch cycle exactly as it does from a
    /// single `check`, and the hiding is presentation-only. `Mode::Record`
    /// cannot reach the watch loop at all: clap rejects `--baseline
    /// --watch` before either session exists, so `Mode::Apply` is the
    /// only mode this cycle is ever asked to honor a baseline file under.
    #[test]
    fn a_cycle_applies_the_baseline_and_reports_hidden_findings() {
        let root = project_with_a_recorded_baseline();

        let mut session = Session::start(root.path());
        let mut watcher = silent_watch(&session);
        let mut output = Vec::new();

        let (result, pending, shutdown) = super::completed_cycle(
            &mut session,
            &mut watcher,
            &mut output,
            1,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        )
        .unwrap();
        assert!(
            pending.is_none(),
            "the silent watch never has anything queued",
        );
        assert!(!shutdown, "the silent watch never sends a shutdown");
        assert_eq!(
            result.diagnostics.len(),
            1,
            "the baseline hides the finding from presentation only: the outcome the cache \
             reads still carries it: {:?}",
            result.diagnostics,
        );

        let text = String::from_utf8(output).unwrap();
        assert!(
            text.contains("1 baselined diagnostic hidden"),
            "the frame reports what it hid: {text}",
        );
        assert!(
            !text.contains("CEL0018"),
            "the hidden finding's identifier is absent from the frame: {text}",
        );
        assert!(
            !text.contains("Missing"),
            "the hidden finding's own message is absent from the frame: {text}",
        );
    }

    /// The reload half of the same promise: `Session::absorb` already
    /// routes an edit of `celerrate-baseline.toml` to `rediscover` (task
    /// 5's extension of `is_project_manifest`), and `rediscover` reloads
    /// `loaded_baseline` from whatever is on disk now. Deleting the file
    /// mid-watch must therefore bring the finding back on the very next
    /// cycle, exactly as deleting `celerrate.toml` brings its silenced
    /// diagnostic back in `a_configuration_saved_mid_watch_reconfigures_
    /// the_next_cycle` above, driven the same way, for the same reason:
    /// a synchronous send would race `cycle`'s own inner polling loop, so
    /// the event is delivered from a delayed background thread instead.
    #[test]
    fn deleting_the_baseline_file_mid_watch_reports_the_finding_again() {
        let root = project_with_a_recorded_baseline();

        let mut session = Session::start(root.path());
        let (mut watcher, sender) = watch_with_held_sender(&session);
        let mut output = Vec::new();

        let (first, _, _) = super::completed_cycle(
            &mut session,
            &mut watcher,
            &mut output,
            1,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        )
        .unwrap();
        assert_eq!(first.diagnostics.len(), 1, "the finding is still analyzed");
        let first_text = String::from_utf8(output.clone()).unwrap();
        assert!(
            first_text.contains("1 baselined diagnostic hidden"),
            "sanity: the first cycle hides it: {first_text}",
        );

        let baseline_file = root.path().join(crate::baseline::BASELINE_FILE_NAME);
        std::fs::remove_file(&baseline_file).unwrap();
        let sent = baseline_file.clone();
        let deliver = sender.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            deliver.send(WatchEvent::Changed(sent)).unwrap();
        });

        let next = match super::iteration(
            &mut session,
            &mut watcher,
            &mut output,
            1,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        ) {
            ControlFlow::Continue(next) => next,
            ControlFlow::Break(outcome) => {
                panic!("deleting the baseline file must not break the loop: {outcome:?}")
            }
        };

        output.clear();
        let (second, _, _) = super::completed_cycle(
            &mut session,
            &mut watcher,
            &mut output,
            next,
            ColorMode::Plain,
            crate::baseline::Mode::Apply,
        )
        .unwrap();
        assert_eq!(
            second.diagnostics.len(),
            1,
            "the finding is still analyzed once the baseline that hid it is gone",
        );
        let second_text = String::from_utf8(output.clone()).unwrap();
        assert!(
            second_text.contains("CEL0018"),
            "the finding is reported again once the baseline that hid it is gone: {second_text}",
        );
        assert!(
            !second_text.contains("hidden"),
            "nothing is baselined anymore: {second_text}",
        );
    }

    /// `Mode::Ignore` (`--ignore-baseline`) must not apply a baseline even
    /// when one is recorded and present on disk: the finding stays
    /// reported, and the frame carries no "hidden" line at all. Pins that
    /// folding `Ignore` into the same do-nothing arm as the unreachable
    /// `Record` really does skip `baseline::apply` altogether, rather
    /// than applying an empty or vacuous baseline that happens to hide
    /// nothing.
    #[test]
    fn ignore_mode_does_not_apply_a_present_baseline() {
        let root = project_with_a_recorded_baseline();

        let mut session = Session::start(root.path());
        let mut watcher = silent_watch(&session);
        let mut output = Vec::new();

        let (result, _, _) = super::completed_cycle(
            &mut session,
            &mut watcher,
            &mut output,
            1,
            ColorMode::Plain,
            crate::baseline::Mode::Ignore,
        )
        .unwrap();
        assert_eq!(
            result.diagnostics.len(),
            1,
            "the finding is still analyzed under --ignore-baseline",
        );

        let text = String::from_utf8(output).unwrap();
        assert!(
            text.contains("CEL0018"),
            "ignore mode reports the finding even though a baseline recorded it: {text}",
        );
        assert!(
            !text.contains("hidden"),
            "nothing is baselined under --ignore-baseline: {text}",
        );
    }
}
