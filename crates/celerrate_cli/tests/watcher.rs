//! The one integration test that touches the operating system. Everything
//! else about `--watch` is the pure reconciliation, tested in the unit
//! suite. This one is deliberately tolerant: filesystem notification is
//! platform-specific, coalesced, and reordered, so it asserts that the
//! adapter reports the paths at all, not how many events each edit
//! produced.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use celerrate_cli::session::Session;
use celerrate_cli::watch::spawn_watcher;

/// How long the channel must stay silent before the events of an edit are
/// taken to be all in. Generous, because it is paid once and a leftover
/// event is what would make the deletion assertion a lie.
const QUIET: Duration = Duration::from_millis(500);

/// Collects reported paths until `wanted` have all appeared, or the
/// deadline passes.
fn collect_until(
    events: &Receiver<PathBuf>,
    wanted: &BTreeSet<PathBuf>,
    deadline: Duration,
) -> BTreeSet<PathBuf> {
    let started = Instant::now();
    let mut seen = BTreeSet::new();
    while started.elapsed() < deadline {
        match events.recv_timeout(Duration::from_millis(100)) {
            Ok(path) => {
                seen.insert(path);
                if wanted.iter().all(|path| seen.contains(path)) {
                    return seen;
                }
            }
            Err(_) => continue,
        }
    }
    seen
}

/// Empties the channel of everything the edits so far produced, queued or
/// still in flight, by waiting the queue out until it falls silent.
///
/// Emptying only what is already queued would not be enough: `collect_until`
/// returns the instant it has seen what it wanted, and the rest of that
/// burst is still on its way.
fn drain(events: &Receiver<PathBuf>) {
    while events.recv_timeout(QUIET).is_ok() {}
}

#[test]
fn the_adapter_reports_creation_modification_and_deletion() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.php"), "<?php class A {}").unwrap();

    let session = Session::start(root.path());
    let (_watcher, events) = spawn_watcher(&session).unwrap();

    let created = root.path().join("b.php");
    std::fs::write(&created, "<?php class B {}").unwrap();
    let modified = root.path().join("a.php");
    std::fs::write(&modified, "<?php class A { public int $x = 1; }").unwrap();

    let wanted: BTreeSet<PathBuf> = [created.clone(), modified.clone()].into_iter().collect();
    let seen = collect_until(&events, &wanted, Duration::from_secs(5));
    assert!(
        seen.contains(&created) && seen.contains(&modified),
        "the watcher reported {seen:?}",
    );

    // One `write` on a new file commonly yields several events about it,
    // and `collect_until` above stopped at the first that completed the
    // set. Every leftover event about `b.php` predates its removal, and any
    // one of them would satisfy the assertion below just as well as a
    // deletion would: without this drain the test would pass unchanged on a
    // platform that never reports deletions at all. Draining is what makes
    // the assertion real, and the deletion is precisely the case that
    // justifies rewriting a reported path as a string rather than
    // canonicalizing it, since by then the file is gone.
    drain(&events);

    std::fs::remove_file(&created).unwrap();
    let wanted: BTreeSet<PathBuf> = [created.clone()].into_iter().collect();
    let seen = collect_until(&events, &wanted, Duration::from_secs(5));
    assert!(
        seen.contains(&created),
        "the deletion was reported: {seen:?}"
    );
}
