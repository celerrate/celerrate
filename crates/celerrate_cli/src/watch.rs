//! `--watch`: re-analyze on every change, keep reporting.
//!
//! Stubbed for Task 8, so `run` compiles and a `--watch` invocation is a
//! well-defined usage error rather than a missing arm. Task 9 and Task 10
//! build the real watch loop; this stub is deleted then.

use std::io::Write;

use crate::Outcome;
use crate::session::Session;

pub fn watch(_session: &mut Session, _output: &mut dyn Write) -> Outcome {
    Outcome::UsageError
}
