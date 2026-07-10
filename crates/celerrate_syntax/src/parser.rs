//! The hand-written recursive-descent parser. Event-based: it reads a
//! trivia-free view of the token stream and records [`Event`]s; the
//! tree builder replays them. The parser never fails and never touches
//! the tree.

mod event;

pub(crate) use event::Event;
