# Watch Graceful Shutdown and Testable Loop — Design

Date: 2026-07-19
Status: Approved (issue #52)

## Problem

`--watch` persists the derived cache on quiet cycles only (plan 9a's
measured trade: ~57.5ms persist against a ~13.5ms median warm cycle).
The accepted cost was a crash window; what makes the window the common
case:

- No signal handler exists anywhere in `celerrate_cli`. An interactive
  Ctrl+C gets the operating system's default handling: immediate
  termination, `watch()` never returns.
- The loop's final flush persist lives only on the channel-disconnected
  exit branch (`watch.rs:189-211`), which the module's own comment says
  cannot happen while the watch is alive.

So essentially every real way a watch session ends — Ctrl+C included,
which is how most sessions end — loses every cycle back to the last
quiet persist. Never a correctness loss (the next run recomputes), but
it undoes audit finding I6's "a crash loses at most one cycle" property
for ordinary shutdowns. Second gap, same surface: the `watch()` loop
itself (`watch.rs:176-223`) has no test — its factored-out pieces do
(`completed_cycle`, `persist_unless_a_burst_is_already_waiting`,
`reconcile`, the cancellation primitive), but not the loop that
assembles them, including the flush-on-exit branch.

## Design

### 1. A shutdown event on the existing channel

The watch channel widens from `Receiver<PathBuf>` to a small event
enum, `WatchEvent::Changed(PathBuf) | WatchEvent::Shutdown`. The notify
callback keeps sending `Changed`; a signal handler sends `Shutdown`
through a cloned sender. This reuses the loop's one wake-up mechanism
instead of adding a second (an atomic flag cannot wake a blocking
`recv()`), and it makes shutdown an ordinary, testable event: a test
sends `Shutdown` like any other message.

On `Shutdown`, the loop takes the same graceful-exit shape the
disconnect branch already has: final `crate::cache::persist`, then
`Outcome::of(...)` from the last completed cycle. If a cycle is in
flight, the existing cancellation primitive applies (the shutdown event
is drained exactly where bursts are), the in-flight results are
discarded, and the persist covers the last completed state — restoring
"at most one cycle lost" for ordinary shutdowns. The disconnect branch
stays as the belt-and-suspenders it is.

### 2. The signal handler: `ctrlc` with the `termination` feature

`ctrlc` (MIT OR Apache-2.0, passes `deny.toml`) is the handler crate:
it covers SIGINT and SIGTERM on Unix and Ctrl+C/console-close on
Windows (tier 2 is in the CI matrix), and it encapsulates the platform
`unsafe` the workspace itself forbids. The handler does one thing:
send `WatchEvent::Shutdown`. Installation happens in `watch()` only —
a single `check` keeps today's behavior — and an installation failure
degrades to today's behavior (the watch runs, Ctrl+C is abrupt) rather
than failing the session: the handler is an improvement, not a
precondition. A second Ctrl+C during the final ~60ms persist is not
specially handled in this iteration; the persist is atomic per pack
(`write_atomically`), so a torn shutdown corrupts nothing.

### 3. The loop body becomes one drivable iteration

The `loop { }` body is extracted into a function with the shape
"one iteration: pending-or-wait → burst or shutdown → absorb →
resynchronize (or exit with a final persist)", returning
`ControlFlow<Outcome, ...>` so `watch()` reduces to installing the
handler and iterating. Tests construct a `Watch` whose sender they
keep (the existing `silent_watch` helper generalizes), and drive:

- a `Changed` burst → the iteration cycles and continues;
- a `Shutdown` → the iteration persists and exits with the outcome —
  the graceful path is pinned, not hoped;
- sender dropped → the disconnect branch persists and exits (the
  formerly untestable branch becomes reachable in tests).

No behavioral change to cycle mechanics, burst debouncing, quiet-cycle
persistence, or cancellation: those functions and their tests carry
over unchanged.

## Testing

- New loop-iteration tests as above (shutdown persists; disconnect
  persists; a burst continues the loop).
- Existing watch suite passes unchanged.
- Manual verification per the repo's verify practice: run `--watch` on
  a fixture project, Ctrl+C, observe the persist log line and warm
  restart.
- Corpus gates: zero delta (no analysis semantics touched).

## Out of scope

- Signal handling for single-`check` runs (persist already runs at the
  end of `run`; a mid-pass Ctrl+C there loses only that pass).
- Exit-code conventions for interrupted runs (130-style codes): the
  watch returns the same `Outcome` mapping the disconnect branch uses
  today.
- Any persistence-economics change: quiet-cycle-only persist stays
  exactly as plan 9a set it.
