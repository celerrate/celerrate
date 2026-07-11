//! The stub compiler: turns the pinned phpstorm-stubs snapshot into
//! the committed blob. Compiled only for tests and under the
//! `compiler` feature — the runtime never parses PHP.

pub mod extract;
