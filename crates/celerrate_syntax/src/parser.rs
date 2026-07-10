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
    // Defense in depth, behind every per-loop guard the grammar already
    // carries: unreachable on a legitimate parse, since the grammar
    // consumes every token on every corpus input.
    parser.recover_unconsumed_tail();
    (parser.events, parser.diagnostics)
}

struct Parser {
    source: TokenSource,
    position: usize,
    events: Vec<Event>,
    diagnostics: Vec<ParserDiagnostic>,
    nesting_depth: u32,
    /// Steps observed since the last consumed token; see
    /// `MAXIMUM_STEPS_WITHOUT_PROGRESS`.
    steps_without_progress: u32,
    /// Latched once the step budget is exceeded: `current` and `nth`
    /// report `None` for the rest of the parse, regardless of what the
    /// token stream actually holds.
    fuse_blown: bool,
}

impl Parser {
    fn new(source: TokenSource) -> Self {
        Self {
            source,
            position: 0,
            events: Vec::new(),
            diagnostics: Vec::new(),
            nesting_depth: 0,
            steps_without_progress: 0,
            fuse_blown: false,
        }
    }

    /// Far beyond any legitimate lookahead or diagnose-without-consuming
    /// work between two token consumptions (nesting is already capped at
    /// 128); a counter past this budget means a grammar loop is stuck.
    const MAXIMUM_STEPS_WITHOUT_PROGRESS: u32 = 4_096;

    /// Counts one step without progress; blows the fuse once the budget
    /// is exceeded. Cheap once blown: the counter stops moving, so this
    /// never overflows no matter how long the stuck loop keeps spinning.
    fn observe(&mut self) {
        if self.fuse_blown {
            return;
        }
        self.steps_without_progress += 1;
        if self.steps_without_progress > Self::MAXIMUM_STEPS_WITHOUT_PROGRESS {
            self.fuse_blown = true;
        }
    }

    fn current(&mut self) -> Option<SyntaxKind> {
        self.observe();
        if self.fuse_blown {
            return None;
        }
        self.source.kind(self.position)
    }

    fn at(&mut self, kind: SyntaxKind) -> bool {
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
        self.steps_without_progress = 0;
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
        self.push_diagnostic(ParserDiagnostic { kind, range });
    }

    /// Marks something missing: zero-width at the previous token's end.
    fn diagnose_missing(&mut self, kind: ParserDiagnosticKind) {
        self.push_diagnostic(ParserDiagnostic {
            kind,
            range: TextRange::empty(self.previous_end()),
        });
    }

    /// Appends a diagnostic unless it repeats the previous one exactly.
    /// Recovery that unwinds through many levels at one spot (an
    /// exhausted nesting budget above all) re-diagnoses the same missing
    /// token at the same offset once per level; one report suffices.
    fn push_diagnostic(&mut self, diagnostic: ParserDiagnostic) {
        if self.diagnostics.last() == Some(&diagnostic) {
            return;
        }
        self.diagnostics.push(diagnostic);
    }

    /// Bounds recursive descent: degenerate nesting (`((((...`,
    /// `$$$$...`) must stay a diagnostic, never a stack overflow. 128
    /// levels is far beyond real code and well inside default stacks.
    const MAXIMUM_NESTING_DEPTH: u32 = 128;

    fn nth(&mut self, offset: usize) -> Option<SyntaxKind> {
        self.observe();
        if self.fuse_blown {
            return None;
        }
        self.source.kind(self.position + offset)
    }

    /// The token cursor, exposed so a delimited-list loop (`argument_list`
    /// and the shared list helpers it anchors) can prove it advances every
    /// iteration: the nesting guard can refuse a sub-expression without
    /// consuming a token, and a list loop that only trusts its element
    /// rule to consume would spin forever on that refusal.
    fn position(&self) -> usize {
        self.position
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

    /// The fuse only silences `current`/`nth`; the underlying position
    /// and token source stay valid behind it. Only `recover_unconsumed_tail`
    /// may reach past the fuse this way, to find where the true
    /// remainder of the stream begins once the grammar has unwound.
    fn raw_range(&self, position: usize) -> Option<TextRange> {
        self.source.range(position)
    }

    /// The lossless backstop, run once the grammar has returned and its
    /// event stream is otherwise finalized. On every legitimate parse
    /// the grammar already consumed every token, so this is a no-op;
    /// it exists only for the case a grammar loop got stuck, the fuse
    /// blew, and `current`/`nth` silenced the true remainder of the
    /// stream from the grammar. Splices a single `ErrorNode` over that
    /// remainder just before the outermost node's closing event, so the
    /// tree stays single-rooted, and pushes exactly one `NoProgress`
    /// diagnostic: losing those tokens instead would violate the
    /// lossless invariant that a parser bug must never break.
    fn recover_unconsumed_tail(&mut self) {
        if self.at_end() {
            return;
        }
        let start = self
            .raw_range(self.position)
            .map(TextRange::start)
            .unwrap_or_else(|| self.previous_end());
        // Reopen the outermost node's tail: pop its closing event, splice
        // the recovery node in as its last child, then restore the
        // closing event so the tree keeps exactly one root.
        let outer_close = self.events.pop();
        debug_assert!(
            matches!(outer_close, Some(Event::Finish)),
            "the backstop reopens the outermost node, so the final event must be its Finish"
        );
        let marker = self.start();
        while !self.at_end() {
            self.bump();
        }
        marker.complete(self, SyntaxKind::ErrorNode);
        if let Some(event) = outer_close {
            self.events.push(event);
        }
        self.push_diagnostic(ParserDiagnostic {
            kind: ParserDiagnosticKind::NoProgress,
            range: TextRange::new(start, self.previous_end()),
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
    use crate::tree::SyntaxNode;

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
        let mut parser = parser_over("<?php echo 1;");
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

    #[test]
    fn the_fuse_blows_after_the_step_budget_and_the_backstop_recovers_losslessly() {
        let source = "<?php echo 1;";
        let (tokens, _lexer_diagnostics) = crate::lexer::lex(source);
        let mut parser = Parser::new(token_source::TokenSource::new(&tokens));
        // A stuck loop: observe `current` without ever bumping.
        for _ in 0..=Parser::MAXIMUM_STEPS_WITHOUT_PROGRESS {
            parser.current();
        }
        assert_eq!(parser.current(), None, "the fuse must have blown");
        assert!(!parser.at_end(), "real tokens must still remain unconsumed");
        // The grammar sees end of input and unwinds normally, exactly as
        // `run` drives it.
        grammar::source_file(&mut parser);
        parser.recover_unconsumed_tail();
        let tree = SyntaxNode::new_root(crate::tree::builder::build_tree(
            source,
            &tokens,
            parser.events,
        ));
        assert_eq!(
            tree.text().to_string(),
            source,
            "the tree must stay lossless even after the fuse blows"
        );
        let no_progress_count = parser
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == ParserDiagnosticKind::NoProgress)
            .count();
        assert_eq!(no_progress_count, 1, "exactly one NoProgress diagnostic");
        assert!(
            tree.descendants()
                .any(|node| node.kind() == SyntaxKind::ErrorNode),
            "the recovered remainder must sit under an ErrorNode"
        );
    }

    #[test]
    fn a_legitimate_parse_never_trips_the_fuse() {
        let source = "<?php echo 1 + 2 * (3 - $x) ?? f(name: 1, ...$rest), $f(1)(2), $a instanceof Foo\\Bar, ${'a' . 'b'};";
        let (tokens, _lexer_diagnostics) = crate::lexer::lex(source);
        let (_events, diagnostics) = run(&tokens);
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == ParserDiagnosticKind::NoProgress),
            "a legitimate parse must never trip the fuse"
        );
    }
}
