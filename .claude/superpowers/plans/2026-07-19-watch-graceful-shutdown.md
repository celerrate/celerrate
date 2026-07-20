# Watch Graceful Shutdown Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An interactive Ctrl+C (and SIGTERM) ends `--watch` through the graceful-exit persist, and the watch loop becomes drivable in tests (issue #52), per `.claude/superpowers/specs/2026-07-19-watch-graceful-shutdown-design.md`.

**Architecture:** On branch `fix-52-watch-graceful-shutdown`: the watch channel widens from `PathBuf` to `WatchEvent::Changed(PathBuf) | Shutdown`; a `ctrlc` handler sends `Shutdown` through a shared sender cell (`Arc<Mutex<Sender<WatchEvent>>>`) that channel respawns update — a plain clone would go stale, because `resynchronize` replaces the channel (`watch.rs:412-416`). The loop body extracts into one `ControlFlow`-returning iteration. Shutdown semantics: **no new work after shutdown; in-flight work completes** — a shutdown observed while a cycle runs lets the cycle finish and then exits; one observed while collecting a burst discards the not-yet-analyzed burst and exits. A second Ctrl+C exits immediately (code 130), so the user keeps an escape hatch during the final ~60ms persist or a long cold cycle.

**Tech Stack:** Rust 1.94, `ctrlc` 3.x with the `termination` feature (MIT OR Apache-2.0, passes `deny.toml`; covers SIGINT+SIGTERM on Unix, Ctrl+C/console-close on Windows, and encapsulates the platform `unsafe` the workspace forbids), std `mpsc` (as today).

## Global Constraints

- Zero panic lints at deny; `unsafe_code` forbidden; test modules may locally `#[allow]`.
- TDD: failing test before implementation for every new behavior.
- No change to cycle mechanics, burst debouncing (30ms window), quiet-cycle-only persistence, or cancellation.
- Commits: gitmoji + Conventional Commits.
- Corpus gates: zero delta (no analysis semantics touched).

---

### Task 1: The event vocabulary and the burst reader

**Files:**
- Modify: `crates/celerrate_cli/src/watch.rs` — the channel type (`Watch.events`, `watch.rs:254`; `Watch::spawn`, `:366-410`; `events()`, `:414`), the burst readers (`wait_for_a_burst` `:700`, `burst_starting_with` `:711`, `drain_burst` `:719`), `persist_unless_a_burst_is_already_waiting` (`:154`), and `cycle`'s poll loop (`:646-696`)

**Interfaces:**
- Consumes: existing machinery.
- Produces:
  - `enum WatchEvent { Changed(PathBuf), Shutdown }` (module-private).
  - `enum BurstOutcome { Changes(Vec<PathBuf>), Shutdown, Disconnected }` — what the readers now answer.
  - `wait_for_a_burst(&Receiver<WatchEvent>) -> BurstOutcome`, `burst_starting_with(&Receiver<WatchEvent>, PathBuf) -> BurstOutcome`.
  - `cycle(...) -> notify::Result<(AnalysisOutcome, bool)>` — the bool is "a shutdown was observed mid-cycle".
  - `persist_unless_a_burst_is_already_waiting(...) -> Option<WatchEvent>` — persists unless a `Changed` is queued (a queued `Shutdown` does not skip the persist: the run is about to end on it).

- [ ] **Step 1: Write the failing burst-reader tests**

In the watch test module (reusing its fixture style; the channel ends
are plain `std::sync::mpsc::channel()` values in tests):

```rust
#[test]
fn a_shutdown_while_idle_ends_the_wait() {
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(WatchEvent::Shutdown).unwrap();
    assert!(matches!(wait_for_a_burst(&receiver), BurstOutcome::Shutdown));
}

#[test]
fn a_shutdown_inside_a_burst_discards_the_burst() {
    // No new work after shutdown: the not-yet-analyzed burst is
    // dropped, the last completed state is what the exit persists.
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(WatchEvent::Changed(PathBuf::from("src/a.php"))).unwrap();
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
        sender.send(WatchEvent::Changed(PathBuf::from(path))).unwrap();
    }
    let BurstOutcome::Changes(changed) = wait_for_a_burst(&receiver) else {
        panic!("expected changes");
    };
    assert_eq!(
        changed,
        vec![PathBuf::from("src/a.php"), PathBuf::from("src/b.php")],
    );
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli --lib watch`
Expected: FAIL to compile — `WatchEvent`/`BurstOutcome` do not exist.

- [ ] **Step 3: Implement the vocabulary and readers**

```rust
/// What travels on the watch channel: filesystem changes from the
/// notify callback, and the shutdown request from the signal handler
/// (issue #52). One channel, because the loop's only wake-up mechanism
/// is this channel's blocking read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WatchEvent {
    Changed(PathBuf),
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
```

`wait_for_a_burst` / `burst_starting_with` / `drain_burst` adapt:
`drain_burst` keeps its 30ms-window collect-sort-dedup shape over
`Changed` payloads and reports (as its new return value) whether a
`Shutdown` arrived inside the window; the two callers map that to
`BurstOutcome::Shutdown`, an empty first read to `Disconnected`
(`recv()` error) and everything else to `Changes`. The notify callback
in `Watch::spawn` (`watch.rs:376`) wraps its sends:
`sender.send(WatchEvent::Changed(as_the_project_names_it(&roots, path)))`.

In `cycle`'s poll loop (`watch.rs:653-657`), the `Ok(path)` arm becomes
two arms: `Ok(WatchEvent::Changed(path)) => changed.push(path)`, and
`Ok(WatchEvent::Shutdown)` sets a local `shutdown = true` and breaks —
the worker is then joined as on the disconnect path (in-flight work
completes; warm cycles are ~13ms) and the outcome returns with the
flag. `cycle` returns `(AnalysisOutcome, bool)`;
`completed_cycle` threads the flag through to its caller (extend its
`Ok` tuple to carry it).

`persist_unless_a_burst_is_already_waiting` becomes:

```rust
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
```

(A queued `Shutdown` persists here — this IS the graceful exit's
persist when the shutdown lands during a busy cycle's render.)

- [ ] **Step 4: Run the suite**

Run: `cargo test -p celerrate_cli --lib watch`
Expected: the new tests PASS; existing watch tests still compile and
pass (the fixture helpers adapt mechanically to `WatchEvent::Changed`
where they send paths).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli/src/watch.rs
git commit -m "✨ feat(cli): the watch channel carries a shutdown event (#52)"
```

---

### Task 2: The loop becomes one drivable iteration, shutdown-aware

**Files:**
- Modify: `crates/celerrate_cli/src/watch.rs:166-224` (`watch()` and the extracted iteration), the stale-comment block at `:189-211`, and the crash-window doc at `:128-145`

**Interfaces:**
- Consumes: Task 1's vocabulary.
- Produces: `fn iteration(session: &mut Session, watcher: &mut Watch, output: &mut dyn Write, reanalyzed: usize) -> ControlFlow<Outcome, usize>` (module-private; `Break` carries the final outcome after a graceful persist, `Continue` carries the next `reanalyzed`); `watch()` reduces to spawn + handler install (Task 3) + `loop { iteration }`.

- [ ] **Step 1: Write the failing iteration tests**

Generalize the test module's `silent_watch` helper (`watch.rs:760`) into
one that keeps the sender:

```rust
fn watch_with_held_sender(session: &Session) -> (Watch, Sender<WatchEvent>) {
    // Same construction as silent_watch, but the sender survives so
    // the test can inject events.
}
```

Then, reusing the pack-observation fixtures of
`a_cycle_rewrites_the_packs_with_its_results` (`watch.rs:1534`):

```rust
#[test]
fn a_shutdown_event_exits_through_the_graceful_persist() {
    // Fixture: a session over a small project, a watch with a held
    // sender, packs cleared. Send Shutdown, run one iteration:
    // it must Break with the outcome of the completed cycle AND the
    // packs must exist on disk (the final persist ran).
}

#[test]
fn a_burst_event_continues_the_loop() {
    // Send Changed(<fixture file>), run one iteration: it must
    // Continue with reanalyzed == 1 and the session must have
    // absorbed the change.
}

#[test]
fn a_disconnected_channel_exits_through_the_graceful_persist() {
    // Drop the sender, run one iteration: Break, packs on disk —
    // the formerly untestable watch.rs:189-211 branch, now driven.
}
```

Write the three bodies against the real fixture machinery of the
existing tests in this module (they build sessions over `tempfile`
projects; follow `a_cycle_rewrites_the_packs_with_its_results` for the
pack-path assertions).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli --lib watch::tests`
Expected: FAIL to compile — no `iteration`, no `watch_with_held_sender`.

- [ ] **Step 3: Extract the iteration**

The body of today's `loop` (`watch.rs:177-222`) moves into `iteration`
with three changes:

1. The `completed_cycle` result now carries the mid-cycle shutdown
   flag; when set, fall through to the graceful exit arm below
   (skipping the burst wait: shutdown already observed).
2. The `pending` dispatch matches Task 1's types:
   `Some(WatchEvent::Changed(path))` → `burst_starting_with`;
   `Some(WatchEvent::Shutdown)` → the graceful exit arm;
   `None` → `wait_for_a_burst`.
3. One graceful exit arm replaces the `changed.is_empty()` block, taken
   on `BurstOutcome::Shutdown`, `BurstOutcome::Disconnected`, and the
   two shutdown routes above:

```rust
        // The graceful exit (issue #52): a shutdown request, or the
        // disconnect that "cannot happen while the watch is alive"
        // (kept because the loop must be total). Whatever the last
        // busy cycle skipped is flushed before the process returns —
        // a no-op write when that cycle's own persist already ran,
        // since `write_when_changed` compares before writing.
        crate::cache::persist(session, &outcome);
        return ControlFlow::Break(Outcome::of(
            outcome.diagnostics.len(),
            session.internal_errors.len(),
        ));
```

`watch()` becomes:

```rust
pub fn watch(session: &mut Session, output: &mut dyn Write) -> Outcome {
    let mut watcher = match Watch::spawn(session) {
        Ok(watcher) => watcher,
        Err(error) => return unwatchable(output, &error),
    };
    let mut reanalyzed = session.sources.len();
    loop {
        match iteration(session, &mut watcher, output, reanalyzed) {
            ControlFlow::Continue(next) => reanalyzed = next,
            ControlFlow::Break(outcome) => return outcome,
        }
    }
}
```

Rewrite the `watch.rs:128-145` doc paragraph: the SIGINT/SIGTERM
follow-up it declared out of scope is now implemented; the crash
window applies to hard kills only.

- [ ] **Step 4: Run the suite**

Run: `cargo test -p celerrate_cli`
Expected: PASS — new iteration tests and every existing watch test.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli/src/watch.rs
git commit -m "♻️ refactor(cli): the watch loop is one drivable iteration (#52)"
```

---

### Task 3: The signal handler

**Files:**
- Modify: `Cargo.toml` (workspace deps: `ctrlc = { version = "3", features = ["termination"] }`)
- Modify: `crates/celerrate_cli/Cargo.toml` (`ctrlc.workspace = true`)
- Modify: `crates/celerrate_cli/src/watch.rs` (`Watch` gains the shared sender cell; `Watch::spawn` and the respawn path in `resynchronize` update it; `watch()` installs the handler)

**Interfaces:**
- Consumes: Tasks 1-2.
- Produces: `Watch::shutdown_sender(&self) -> Arc<Mutex<Sender<WatchEvent>>>`; `fn install_shutdown_handler(cell: Arc<Mutex<Sender<WatchEvent>>>)`.

- [ ] **Step 1: Write the failing cell test**

```rust
#[test]
fn a_respawn_updates_the_shared_sender_cell() {
    // Fixture: session + watch. Take shutdown_sender(), force a
    // respawn (the grown-roots path the existing respawn tests drive,
    // e.g. a_walk_root_the_manifest_grows_is_watched_and_mapped),
    // then send Shutdown through the CELL and assert the watch's
    // current receiver gets it: the handler survives respawns.
}
```

Write the body against the respawn fixtures already in the module.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli --lib a_respawn_updates`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`Watch` gains `shutdown_sender: Arc<Mutex<Sender<WatchEvent>>>`,
initialized by `spawn` with the channel's sender clone; every place
that rebuilds the channel (the respawn inside `resynchronize`) stores
the new sender into the existing cell instead of minting a new cell
(`if let Ok(mut guard) = cell.lock() { *guard = new_sender; }` — a
poisoned lock degrades to no signal handling, never a panic). Handler
installation, called once from `watch()` after the first spawn:

```rust
/// Routes SIGINT/SIGTERM (Ctrl+C, kill) into the watch channel so the
/// loop exits through the graceful persist (issue #52). The second
/// signal exits the process immediately (130, the shell convention):
/// the graceful path must never cost the user their escape hatch. An
/// installation failure degrades to the pre-#52 behavior — the watch
/// still runs, shutdown is just abrupt again.
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
```

Known, accepted residue (document it on the function): a shutdown sent
in the instant a respawn swaps the channel can be lost; the second
Ctrl+C covers it. Tests never install the handler (they inject
`Shutdown` directly), so the process-global `set_handler` is exercised
manually, not in CI.

- [ ] **Step 4: Run the suite**

Run: `cargo test -p celerrate_cli`
Expected: PASS.

- [ ] **Step 5: Manual verification (the verify practice)**

Build release, run `celerrate check --watch` on a fixture project
(for example the corpus checkout), let one cycle complete, edit a file,
Ctrl+C during the quiet wait. Observe: the process exits promptly, and
a rerun starts warm (the cache statistics line shows served verdicts).
Then once more with Ctrl+C pressed twice rapidly during a cold first
cycle: immediate exit.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/celerrate_cli
git commit -m "✨ feat(cli): Ctrl+C ends the watch through the graceful persist (#52)"
```

---

### Task 4: Verification and PR

**Files:** `CHANGELOG.md`.

- [ ] **Step 1: Full local gates**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

Expected: all clean (`cargo deny check` newly covers `ctrlc` and its
`nix` subtree — both MIT OR Apache-2.0).

- [ ] **Step 2: Corpus gates**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: zero delta.

- [ ] **Step 3: Changelog and PR**

Unreleased entry, user-visible: `--watch` now persists its cache on
Ctrl+C/SIGTERM, so the next run starts warm; a second Ctrl+C exits
immediately (#52).

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the watch graceful shutdown (#52)"
git push -u origin fix-52-watch-graceful-shutdown
gh pr create --title "✨ feat(cli): graceful watch shutdown and a testable loop (#52)" --body "Implements .claude/superpowers/specs/2026-07-19-watch-graceful-shutdown-design.md: WatchEvent on the existing channel, ctrlc (termination) through a respawn-safe shared sender cell, second signal exits 130, and the loop body is one ControlFlow iteration driven by three new tests (including the formerly unreachable disconnect branch). Closes #52."
```
