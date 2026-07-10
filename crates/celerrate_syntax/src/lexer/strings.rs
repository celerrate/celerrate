use celerrate_source::{TextRange, TextSize};

use crate::diagnostic::LexerDiagnosticKind;
use crate::lexer::{Lexer, Mode};
use crate::syntax_kind::SyntaxKind;

use super::scripting::{is_name_continue, is_name_start};

impl Lexer<'_> {
    /// A whole `'...'` string as one token: no interpolation exists in
    /// single quotes, so there is nothing fine-grained to emit. Only
    /// `\\` and `\'` are escapes; any other backslash is literal. An
    /// unterminated string runs to the end of input, keeps its normal
    /// kind (mid-edit code is the nominal case in an editor), and
    /// reports `UnterminatedString` at the opening quote.
    pub(super) fn lex_single_quoted_string(&mut self) {
        let opening = self.token_start() + self.cursor.pending_length();
        self.cursor.eat('\'');
        loop {
            match self.cursor.bump() {
                Some('\'') => break,
                Some('\\') => {
                    self.cursor.bump();
                }
                Some(_) => {}
                None => {
                    self.diagnose_at(LexerDiagnosticKind::UnterminatedString, opening, 1);
                    break;
                }
            }
        }
        self.emit(SyntaxKind::SingleQuotedString);
    }

    pub(super) fn lex_double_quote_delimiter(&mut self) {
        let opening = self.token_start() + self.cursor.pending_length();
        self.cursor.eat('"');
        self.emit(SyntaxKind::DoubleQuote);
        self.push_mode(Mode::DoubleQuotedString { opening });
    }

    pub(super) fn lex_backtick_delimiter(&mut self) {
        let opening = self.token_start();
        self.cursor.eat('`');
        self.emit(SyntaxKind::Backtick);
        self.push_mode(Mode::Backtick { opening });
    }

    pub(super) fn lex_double_quoted(&mut self) {
        if self.cursor.eat('"') {
            self.emit(SyntaxKind::DoubleQuote);
            self.pop_mode();
            return;
        }
        if self.lex_interpolation() {
            return;
        }
        self.lex_interpolated_fragment(Some('"'));
    }

    pub(super) fn lex_backtick(&mut self) {
        if self.cursor.eat('`') {
            self.emit(SyntaxKind::Backtick);
            self.pop_mode();
            return;
        }
        if self.lex_interpolation() {
            return;
        }
        self.lex_interpolated_fragment(Some('`'));
    }

    /// Handles the three interpolation openers when the cursor sits on
    /// one; returns false when the current character is plain content.
    /// `${` and `{$` push a scripting mode tagged with the opener's
    /// offset so end-of-input can report an unterminated interpolation;
    /// the matching `}` pops it through the ordinary brace rule.
    pub(super) fn lex_interpolation(&mut self) -> bool {
        let rest = self.cursor.rest();
        if rest.starts_with("${") {
            let opening = self.token_start();
            self.cursor.bump_bytes(2);
            self.emit(SyntaxKind::DollarOpenBrace);
            self.push_mode(Mode::Scripting {
                opened_by_interpolation_at: Some(opening),
            });
            return true;
        }
        if self.cursor.peek() == Some('$') && self.cursor.peek_second().is_some_and(is_name_start) {
            self.lex_string_variable();
            return true;
        }
        if rest.starts_with("{$") {
            let opening = self.token_start();
            self.cursor.eat('{');
            self.emit(SyntaxKind::OpenBrace);
            self.push_mode(Mode::Scripting {
                opened_by_interpolation_at: Some(opening),
            });
            return true;
        }
        false
    }

    /// `$name` plus at most one simple suffix, as in Zend's simple
    /// interpolation: `->prop` or `?->prop` (one level only), or a
    /// bracketed offset, which switches to the `VariableOffset` mode.
    fn lex_string_variable(&mut self) {
        self.cursor.eat('$');
        self.cursor.eat_while(is_name_continue);
        self.emit(SyntaxKind::Variable);
        let rest = self.cursor.rest();
        if let Some(after_arrow) = rest.strip_prefix("->") {
            if after_arrow.starts_with(is_name_start) {
                self.cursor.bump_bytes(2);
                self.emit(SyntaxKind::Arrow);
                self.cursor.eat_while(is_name_continue);
                self.emit(SyntaxKind::Identifier);
            }
        } else if let Some(after_arrow) = rest.strip_prefix("?->") {
            if after_arrow.starts_with(is_name_start) {
                self.cursor.bump_bytes(3);
                self.emit(SyntaxKind::NullsafeArrow);
                self.cursor.eat_while(is_name_continue);
                self.emit(SyntaxKind::Identifier);
            }
        } else if rest.starts_with('[') {
            self.cursor.eat('[');
            self.emit(SyntaxKind::OpenBracket);
            self.push_mode(Mode::VariableOffset);
        }
    }

    /// One step inside `$var[...]`: an offset atom, the closing
    /// bracket, or (on anything unrecognized) a bare pop so the
    /// enclosing string mode takes over at this character. The pop
    /// consumes nothing but strictly shrinks the mode stack, so
    /// progress is preserved.
    pub(super) fn lex_variable_offset(&mut self) {
        match self.cursor.peek() {
            Some(']') => {
                self.cursor.eat(']');
                self.emit(SyntaxKind::CloseBracket);
                self.pop_mode();
            }
            Some('-') => {
                self.cursor.eat('-');
                self.emit(SyntaxKind::Minus);
            }
            Some(character) if character.is_ascii_digit() => {
                self.cursor.eat_while(|c| c.is_ascii_digit());
                self.emit(SyntaxKind::IntegerLiteral);
            }
            Some('$') if self.cursor.peek_second().is_some_and(is_name_start) => {
                self.cursor.eat('$');
                self.cursor.eat_while(is_name_continue);
                self.emit(SyntaxKind::Variable);
            }
            Some(character) if is_name_start(character) => {
                self.cursor.eat_while(is_name_continue);
                self.emit(SyntaxKind::Identifier);
            }
            _ => self.pop_mode(),
        }
    }

    /// A literal run: consumes up to (not including) the terminator, an
    /// interpolation opener, or the end of input. `\` escapes the next
    /// character, so `\"`, `\$`, and `\\` stay in the fragment. Always
    /// consumes at least one character: the callers only reach here
    /// after excluding the terminator and the openers at the current
    /// position.
    pub(super) fn lex_interpolated_fragment(&mut self, terminator: Option<char>) {
        while let Some(character) = self.cursor.peek() {
            if Some(character) == terminator {
                break;
            }
            if character == '\\' {
                self.cursor.bump();
                self.cursor.bump();
                continue;
            }
            if character == '$'
                && self
                    .cursor
                    .peek_second()
                    .is_some_and(|next| is_name_start(next) || next == '{')
            {
                break;
            }
            if character == '{' && self.cursor.peek_second() == Some('$') {
                break;
            }
            self.cursor.bump();
        }
        self.emit(SyntaxKind::StringFragment);
    }

    /// Only called when `heredoc_header_at` matched; the redundant parse
    /// keeps the call sites free of unwraps. A consumed `b`/`B` prefix
    /// shifts where the header begins (`cursor.pending_length()` is
    /// nonzero in that case); the start range still covers the whole
    /// token, prefix included.
    pub(super) fn lex_heredoc_start(&mut self) {
        let Some(header) = heredoc_header_at(self.cursor.rest()) else {
            self.lex_unexpected_character();
            return;
        };
        let header_start = self.token_start() + self.cursor.pending_length();
        let start = TextRange::new(self.token_start(), header_start + text_size(header.length));
        let label = TextRange::at(
            header_start + text_size(header.label_start),
            text_size(header.label_length),
        );
        self.cursor.bump_bytes(header.length);
        self.emit(SyntaxKind::HeredocStart);
        if header.is_nowdoc {
            self.push_mode(Mode::Nowdoc { start, label });
        } else {
            self.push_mode(Mode::Heredoc { start, label });
        }
    }

    pub(super) fn lex_heredoc(&mut self, label: TextRange) {
        if self.at_line_start() && self.lex_heredoc_end(label) {
            return;
        }
        if self.lex_interpolation() {
            return;
        }
        self.lex_heredoc_fragment(label, true);
    }

    pub(super) fn lex_nowdoc(&mut self, label: TextRange) {
        if self.at_line_start() && self.lex_heredoc_end(label) {
            return;
        }
        self.lex_heredoc_fragment(label, false);
    }

    /// Emits `HeredocEnd` (indentation plus label, per PHP 7.3 flexible
    /// closing markers) when the closing line starts here.
    fn lex_heredoc_end(&mut self, label: TextRange) -> bool {
        let Some(closer_length) = self.heredoc_closer_length(label) else {
            return false;
        };
        self.cursor.bump_bytes(closer_length);
        self.emit(SyntaxKind::HeredocEnd);
        self.pop_mode();
        true
    }

    /// When the unconsumed input begins a closing-label line (optional
    /// spaces and tabs, the label, then no name character), returns the
    /// byte length of indentation plus label.
    fn heredoc_closer_length(&self, label: TextRange) -> Option<usize> {
        let label_text = self
            .source
            .get(usize::from(label.start())..usize::from(label.end()))?;
        let rest = self.cursor.rest();
        let after_indentation = rest.trim_start_matches([' ', '\t']);
        let indentation = rest.len() - after_indentation.len();
        let after_label = after_indentation.strip_prefix(label_text)?;
        if after_label.starts_with(is_name_continue) {
            return None;
        }
        Some(indentation + label_text.len())
    }

    /// A heredoc or nowdoc literal run: stops before an interpolation
    /// opener (heredoc only) and right after a newline that begins the
    /// closing-label line.
    fn lex_heredoc_fragment(&mut self, label: TextRange, interpolated: bool) {
        while let Some(character) = self.cursor.peek() {
            if interpolated {
                if character == '\\' {
                    self.cursor.bump();
                    let escaped = self.cursor.bump();
                    // A backslash at the end of a line is literal; the
                    // newline it precedes may still start the closer.
                    if escaped == Some('\n') && self.heredoc_closer_length(label).is_some() {
                        break;
                    }
                    continue;
                }
                if character == '$'
                    && self
                        .cursor
                        .peek_second()
                        .is_some_and(|next| is_name_start(next) || next == '{')
                {
                    break;
                }
                if character == '{' && self.cursor.peek_second() == Some('$') {
                    break;
                }
            }
            self.cursor.bump();
            if character == '\n' && self.heredoc_closer_length(label).is_some() {
                break;
            }
        }
        self.emit(SyntaxKind::StringFragment);
    }
}

/// A parsed `<<<LABEL` header: `<<<`, optional spaces and tabs, the
/// label (bare, double-quoted for a heredoc, single-quoted for a
/// nowdoc), and the line's newline.
pub(super) struct HeredocHeader {
    /// Total header length in bytes, trailing newline included.
    pub(super) length: usize,
    /// The bare label's position, relative to the header start.
    pub(super) label_start: usize,
    pub(super) label_length: usize,
    pub(super) is_nowdoc: bool,
}

/// Parses a heredoc or nowdoc header at the start of `rest`, or returns
/// `None` so `<<<` falls back to shift operators.
pub(super) fn heredoc_header_at(rest: &str) -> Option<HeredocHeader> {
    let after_arrows = rest.strip_prefix("<<<")?;
    let after_spaces = after_arrows.trim_start_matches([' ', '\t']);
    let spaces = after_arrows.len() - after_spaces.len();
    let quote = after_spaces
        .chars()
        .next()
        .filter(|character| matches!(character, '"' | '\''));
    let after_quote = match quote {
        Some(_) => after_spaces.get(1..)?,
        None => after_spaces,
    };
    if !after_quote.starts_with(is_name_start) {
        return None;
    }
    let label_length: usize = after_quote
        .chars()
        .take_while(|character| is_name_continue(*character))
        .map(char::len_utf8)
        .sum();
    let after_label = after_quote.get(label_length..)?;
    let after_closing_quote = match quote {
        Some(quote) => after_label.strip_prefix(quote)?,
        None => after_label,
    };
    let newline_length = if after_closing_quote.starts_with("\r\n") {
        2
    } else if after_closing_quote.starts_with('\n') {
        1
    } else {
        return None;
    };
    let quote_length = usize::from(quote.is_some());
    let label_start = 3 + spaces + quote_length;
    Some(HeredocHeader {
        length: label_start + label_length + quote_length + newline_length,
        label_start,
        label_length,
        is_nowdoc: quote == Some('\''),
    })
}

/// Saturating usize-to-TextSize conversion; inputs are within the 4 GiB
/// cap (`SourceText` guarantees it).
fn text_size(length: usize) -> TextSize {
    u32::try_from(length)
        .map(TextSize::from)
        .unwrap_or_else(|_| TextSize::from(u32::MAX))
}
