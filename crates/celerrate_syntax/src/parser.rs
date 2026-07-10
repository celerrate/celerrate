//! The hand-written recursive-descent parser. Event-based: it reads a
//! trivia-free view of the token stream and records [`Event`]s; the
//! tree builder replays them. The parser never fails and never touches
//! the tree.

// TEMPORARY until Task 5's `parse()` consumes the parser: keeps the
// intermediate commit clean under `-D warnings`. Task 5 removes it.
#![allow(dead_code)]

mod event;
mod token_source;

pub(crate) use event::Event;

use celerrate_source::{TextRange, TextSize};

use crate::diagnostic::{ParserDiagnostic, ParserDiagnosticKind};
use crate::syntax_kind::SyntaxKind;
use token_source::TokenSource;

struct Parser {
    source: TokenSource,
    position: usize,
    events: Vec<Event>,
    diagnostics: Vec<ParserDiagnostic>,
}

impl Parser {
    fn new(source: TokenSource) -> Self {
        Self {
            source,
            position: 0,
            events: Vec::new(),
            diagnostics: Vec::new(),
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
        CompletedMarker
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

/// A finished node. Grows `precede` (the forward-parent producer) with
/// the expressions plan.
struct CompletedMarker;

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
}
