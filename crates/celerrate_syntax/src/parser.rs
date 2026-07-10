//! The hand-written recursive-descent parser. Event-based: it reads a
//! trivia-free view of the token stream and records [`Event`]s; the
//! tree builder replays them. The parser never fails and never touches
//! the tree.

mod event;
mod grammar;
mod token_source;

pub(crate) use event::Event;

use celerrate_source::{TextRange, TextSize};

use crate::diagnostic::{ParserDiagnostic, ParserDiagnosticKind};
use crate::syntax_kind::SyntaxKind;
use token_source::TokenSource;

/// Runs the parser over a token stream: events for the builder plus
/// structured diagnostics. Never fails; degenerate input yields
/// `ErrorNode` wreckage and diagnostics.
pub(crate) fn run(tokens: &[crate::token::Token]) -> (Vec<Event>, Vec<ParserDiagnostic>) {
    let mut parser = Parser::new(TokenSource::new(tokens));
    grammar::source_file(&mut parser);
    (parser.events, parser.diagnostics)
}

struct Parser {
    source: TokenSource,
    position: usize,
    events: Vec<Event>,
    diagnostics: Vec<ParserDiagnostic>,
    nesting_depth: u32,
}

impl Parser {
    fn new(source: TokenSource) -> Self {
        Self {
            source,
            position: 0,
            events: Vec::new(),
            diagnostics: Vec::new(),
            nesting_depth: 0,
        }
    }

    fn current(&self) -> Option<SyntaxKind> {
        self.source.kind(self.position)
    }

    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == Some(kind)
    }

    fn at_end(&self) -> bool {
        self.position >= self.source.significant_count()
    }

    /// Consumes the current token into the events. A no-op at end of
    /// input, so recovery loops can never run past the stream.
    fn bump(&mut self) {
        if self.at_end() {
            return;
        }
        self.position += 1;
        self.events.push(Event::Token);
    }

    fn start(&mut self) -> Marker {
        let event_index = self.events.len();
        self.events.push(Event::tombstone());
        Marker::new(event_index)
    }

    /// The end of the last consumed token; offset zero before any.
    fn previous_end(&self) -> TextSize {
        self.position
            .checked_sub(1)
            .and_then(|position| self.source.range(position))
            .map(TextRange::end)
            .unwrap_or_default()
    }

    /// Points at the current token, or zero-width after the last one
    /// when at end of input.
    fn diagnose_current(&mut self, kind: ParserDiagnosticKind) {
        let range = self
            .source
            .range(self.position)
            .unwrap_or_else(|| TextRange::empty(self.previous_end()));
        self.diagnostics.push(ParserDiagnostic { kind, range });
    }

    /// Marks something missing: zero-width at the previous token's end.
    fn diagnose_missing(&mut self, kind: ParserDiagnosticKind) {
        self.diagnostics.push(ParserDiagnostic {
            kind,
            range: TextRange::empty(self.previous_end()),
        });
    }

    /// Bounds recursive descent: degenerate nesting (`((((...`,
    /// `$$$$...`) must stay a diagnostic, never a stack overflow. 128
    /// levels is far beyond real code and well inside default stacks.
    const MAXIMUM_NESTING_DEPTH: u32 = 128;

    fn nth(&self, offset: usize) -> Option<SyntaxKind> {
        self.source.kind(self.position + offset)
    }

    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            return true;
        }
        false
    }

    fn expect(&mut self, kind: SyntaxKind) {
        if !self.eat(kind) {
            self.diagnose_missing(ParserDiagnosticKind::Expected(kind));
        }
    }

    /// Returns false (and diagnoses, once per trip) instead of
    /// recursing past the budget. Every recursive expression entry
    /// point pairs this with `leave_nesting`.
    fn enter_nesting(&mut self) -> bool {
        if self.nesting_depth >= Self::MAXIMUM_NESTING_DEPTH {
            self.diagnose_current(ParserDiagnosticKind::NestingTooDeep);
            return false;
        }
        self.nesting_depth += 1;
        true
    }

    fn leave_nesting(&mut self) {
        self.nesting_depth = self.nesting_depth.saturating_sub(1);
    }
}

/// An open node. Must be completed or abandoned; the tripwire makes a
/// forgotten marker fail tests (`debug_assert!` compiles out in
/// release, where the worst case is a tombstone, not a panic).
struct Marker {
    event_index: usize,
    defused: bool,
}

impl Marker {
    fn new(event_index: usize) -> Self {
        Self {
            event_index,
            defused: false,
        }
    }

    fn complete(mut self, parser: &mut Parser, kind: SyntaxKind) -> CompletedMarker {
        self.defused = true;
        if let Some(Event::Start { kind: slot, .. }) = parser.events.get_mut(self.event_index) {
            *slot = Some(kind);
        }
        parser.events.push(Event::Finish);
        CompletedMarker {
            event_index: self.event_index,
        }
    }

    fn abandon(mut self, parser: &mut Parser) {
        self.defused = true;
        if self.event_index + 1 == parser.events.len() {
            parser.events.pop();
        }
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        debug_assert!(self.defused, "a marker must be completed or abandoned");
    }
}

/// A finished node: remembers where its `Start` event lives so a
/// forward parent can wrap it retroactively.
struct CompletedMarker {
    event_index: usize,
}

impl CompletedMarker {
    /// Opens a node that will enclose this completed one: the new
    /// marker's `Start` is appended now, and this node's `Start` gains
    /// a forward parent pointing at it (absolute event index), which
    /// the builder replays outermost-first.
    fn precede(self, parser: &mut Parser) -> Marker {
        let marker = parser.start();
        if let Some(Event::Start { forward_parent, .. }) = parser.events.get_mut(self.event_index) {
            *forward_parent = Some(marker.event_index);
        }
        marker
    }
}

#[cfg(test)]
mod tests {
    use crate::syntax_kind::SyntaxKind;

    use super::*;

    fn parser_over(source: &str) -> Parser {
        let (tokens, _diagnostics) = crate::lexer::lex(source);
        Parser::new(token_source::TokenSource::new(&tokens))
    }

    #[test]
    fn markers_wrap_bumped_tokens_into_a_node() {
        let mut parser = parser_over("<?php echo 1;");
        let marker = parser.start();
        while !parser.at_end() {
            parser.bump();
        }
        marker.complete(&mut parser, SyntaxKind::SourceFile);
        assert_eq!(
            parser.events,
            vec![
                Event::Start {
                    kind: Some(SyntaxKind::SourceFile),
                    forward_parent: None
                },
                Event::Token,
                Event::Token,
                Event::Token,
                Event::Token,
                Event::Finish,
            ],
        );
    }

    #[test]
    fn abandoning_the_last_marker_removes_its_event() {
        let mut parser = parser_over("<?php");
        let marker = parser.start();
        marker.abandon(&mut parser);
        assert!(parser.events.is_empty());
    }

    #[test]
    fn abandoning_an_older_marker_leaves_a_tombstone() {
        let mut parser = parser_over("<?php");
        let outer = parser.start();
        parser.bump();
        outer.abandon(&mut parser);
        assert_eq!(parser.events, vec![Event::tombstone(), Event::Token]);
    }

    #[test]
    fn missing_diagnostics_are_zero_width_after_the_previous_token() {
        let mut parser = parser_over("<?php echo");
        parser.bump(); // <?php
        parser.bump(); // echo
        parser.diagnose_missing(ParserDiagnosticKind::ExpectedSemicolon);
        let diagnostic = parser.diagnostics.first().copied();
        assert!(matches!(
            diagnostic,
            Some(ParserDiagnostic {
                kind: ParserDiagnosticKind::ExpectedSemicolon,
                range,
            }) if range.is_empty() && u32::from(range.start()) == 10
        ));
    }

    #[test]
    fn precede_wraps_a_completed_node_through_a_forward_parent() {
        let mut parser = parser_over("<?php 1");
        parser.bump(); // <?php
        let marker = parser.start();
        parser.bump(); // 1
        let completed = marker.complete(&mut parser, SyntaxKind::Literal);
        let wrapper = completed.precede(&mut parser);
        wrapper.complete(&mut parser, SyntaxKind::ExpressionStatement);
        assert_eq!(
            parser.events,
            vec![
                Event::Token,
                Event::Start {
                    kind: Some(SyntaxKind::Literal),
                    forward_parent: Some(4),
                },
                Event::Token,
                Event::Finish,
                Event::Start {
                    kind: Some(SyntaxKind::ExpressionStatement),
                    forward_parent: None,
                },
                Event::Finish,
            ],
        );
    }

    #[test]
    fn nth_looks_ahead_without_consuming() {
        let parser = parser_over("<?php echo 1;");
        assert_eq!(parser.nth(0), Some(SyntaxKind::OpenTag));
        assert_eq!(parser.nth(1), Some(SyntaxKind::Echo));
        assert_eq!(parser.nth(2), Some(SyntaxKind::IntegerLiteral));
        assert_eq!(parser.nth(4), None);
    }

    #[test]
    fn expect_bumps_or_diagnoses_a_missing_token() {
        let mut parser = parser_over("<?php ;");
        parser.expect(SyntaxKind::OpenTag);
        parser.expect(SyntaxKind::Semicolon);
        assert!(parser.diagnostics.is_empty());
        parser.expect(SyntaxKind::CloseParenthesis);
        assert!(matches!(
            parser.diagnostics.first(),
            Some(ParserDiagnostic {
                kind: ParserDiagnosticKind::Expected(SyntaxKind::CloseParenthesis),
                range,
            }) if range.is_empty()
        ));
    }

    #[test]
    fn the_nesting_guard_refuses_past_the_limit_and_recovers() {
        let mut parser = parser_over("<?php 1");
        let mut entered = 0usize;
        while parser.enter_nesting() {
            entered += 1;
            assert!(entered <= 1_000, "the guard must trip");
        }
        assert!(matches!(
            parser.diagnostics.first(),
            Some(ParserDiagnostic {
                kind: ParserDiagnosticKind::NestingTooDeep,
                ..
            })
        ));
        // Leaving frees capacity again: recovery paths keep parsing.
        parser.leave_nesting();
        assert!(parser.enter_nesting());
    }
}
