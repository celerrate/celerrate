use celerrate_source::{TextRange, TextSize};

use crate::cursor::Cursor;
use crate::diagnostic::{LexerDiagnostic, LexerDiagnosticKind};
use crate::syntax_kind::SyntaxKind;
use crate::token::Token;

mod inline_html;
mod scripting;
mod strings;

/// Lexes decoded PHP source text into a lossless token stream plus
/// structured diagnostics. Always terminates, never fails: degenerate
/// input yields `Error` tokens and diagnostics, never a crash or a hole
/// in the stream.
pub fn lex(source: &str) -> (Vec<Token>, Vec<LexerDiagnostic>) {
    let mut lexer = Lexer::new(source);
    lexer.run();
    (lexer.tokens, lexer.diagnostics)
}

/// The lexer's current context. `Copy`: label and opening positions are
/// ranges into the source, not owned strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Outside PHP tags; the initial mode.
    InlineHtml,
    /// Inside PHP code. `opened_by_interpolation_at` is `Some` when this
    /// entry was pushed by `{$` or `${` inside a string, so an
    /// unterminated interpolation can be reported at end of input.
    Scripting {
        opened_by_interpolation_at: Option<TextSize>,
    },
    /// Inside `"..."`; `opening` locates the opening quote.
    DoubleQuotedString { opening: TextSize },
    /// Inside `` `...` ``.
    Backtick { opening: TextSize },
    /// Inside a heredoc body; `start` is the `<<<LABEL` token's range and
    /// `label` the range of the bare label text within it. Not
    /// constructed until Task 11.
    #[allow(dead_code)]
    Heredoc { start: TextRange, label: TextRange },
    /// Inside a nowdoc body (no interpolation). Not constructed until
    /// Task 11.
    #[allow(dead_code)]
    Nowdoc { start: TextRange, label: TextRange },
    /// Inside the `[...]` offset of a simple string interpolation.
    VariableOffset,
}

const BASE_SCRIPTING: Mode = Mode::Scripting {
    opened_by_interpolation_at: None,
};

struct Lexer<'source> {
    // Only read by `at_line_start`, unused until the heredoc task.
    #[allow(dead_code)]
    source: &'source str,
    cursor: Cursor<'source>,
    modes: Vec<Mode>,
    /// Absolute offset of the current token's start.
    offset: TextSize,
    tokens: Vec<Token>,
    diagnostics: Vec<LexerDiagnostic>,
}

impl<'source> Lexer<'source> {
    fn new(source: &'source str) -> Self {
        Self {
            source,
            cursor: Cursor::new(source),
            modes: vec![Mode::InlineHtml],
            offset: TextSize::from(0),
            tokens: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn run(&mut self) {
        while !self.cursor.is_at_end() {
            match self.current_mode() {
                Mode::InlineHtml => self.lex_inline_html(),
                Mode::Scripting { .. } => self.lex_scripting(),
                Mode::DoubleQuotedString { .. } => self.lex_double_quoted(),
                Mode::Backtick { .. } => self.lex_backtick(),
                // Heredoc bodies arrive in Task 11.
                Mode::Heredoc { .. } | Mode::Nowdoc { .. } => self.lex_unexpected_character(),
                Mode::VariableOffset => self.lex_variable_offset(),
            }
        }
        self.flush_open_modes();
    }

    /// Reports every construction still open at end of input. Base
    /// scripting and brace-pushed scripting are normal (a PHP file needs
    /// no `?>` and unbalanced braces are the parser's business).
    fn flush_open_modes(&mut self) {
        for mode in self.modes.clone() {
            match mode {
                Mode::InlineHtml | Mode::VariableOffset => {}
                Mode::Scripting {
                    opened_by_interpolation_at,
                } => {
                    if let Some(opening) = opened_by_interpolation_at {
                        self.diagnose_at(
                            LexerDiagnosticKind::UnterminatedInterpolation,
                            opening,
                            1,
                        );
                    }
                }
                Mode::DoubleQuotedString { opening } | Mode::Backtick { opening } => {
                    self.diagnose_at(LexerDiagnosticKind::UnterminatedString, opening, 1);
                }
                Mode::Heredoc { start, .. } | Mode::Nowdoc { start, .. } => {
                    self.diagnose(LexerDiagnosticKind::UnterminatedHeredoc, start);
                }
            }
        }
    }

    // Shared machinery for the mode modules.

    /// Absolute offset where the token being built starts.
    fn token_start(&self) -> TextSize {
        self.offset
    }

    /// Finishes the pending text as one token of the given kind. Never
    /// called with nothing consumed; a defensive guard skips empty
    /// tokens so the stream can never contain one.
    fn emit(&mut self, kind: SyntaxKind) {
        let length = self.cursor.take_length();
        if length == TextSize::from(0) {
            return;
        }
        self.tokens.push(Token::new(kind, length));
        self.offset += length;
    }

    fn diagnose(&mut self, kind: LexerDiagnosticKind, range: TextRange) {
        self.diagnostics.push(LexerDiagnostic { kind, range });
    }

    fn diagnose_at(&mut self, kind: LexerDiagnosticKind, start: TextSize, length: u32) {
        self.diagnose(kind, TextRange::at(start, TextSize::from(length)));
    }

    /// Fallback for any character no rule accepts: a one-character
    /// `Error` token, an `UnexpectedCharacter` diagnostic, and lexing
    /// continues at the next character. This is also the guaranteed
    /// progress argument: the fallback always consumes.
    fn lex_unexpected_character(&mut self) {
        let start = self.token_start();
        if let Some(character) = self.cursor.bump() {
            let length = u32::try_from(character.len_utf8()).unwrap_or(4);
            self.diagnose_at(LexerDiagnosticKind::UnexpectedCharacter, start, length);
        }
        self.emit(SyntaxKind::Error);
    }

    // Mode-stack discipline: tags replace the top (`set_mode`), braces
    // and strings push and pop. `pop_mode` keeps the stack non-empty.
    //
    // `push_mode`, `pop_mode`, `can_pop_mode`, and `at_line_start` are not
    // called yet: braces, string interpolation, and heredocs arrive in
    // later tasks. They ship now with the rest of the shared machinery so
    // those tasks only add call sites, never reshape this section.

    fn current_mode(&self) -> Mode {
        self.modes.last().copied().unwrap_or(Mode::InlineHtml)
    }

    fn set_mode(&mut self, mode: Mode) {
        if let Some(top) = self.modes.last_mut() {
            *top = mode;
        }
    }

    fn push_mode(&mut self, mode: Mode) {
        self.modes.push(mode);
    }

    fn pop_mode(&mut self) {
        if self.modes.len() > 1 {
            self.modes.pop();
        }
    }

    /// Whether the mode stack has room to pop (used by `}` handling).
    fn can_pop_mode(&self) -> bool {
        self.modes.len() > 1
    }

    /// True at offset zero or right after a line feed; heredoc closing
    /// labels are only recognized at a line start.
    #[allow(dead_code)]
    fn at_line_start(&self) -> bool {
        let consumed = self
            .source
            .get(..usize::from(self.offset) + self.cursor.pending_text().len())
            .unwrap_or_default();
        consumed.is_empty() || consumed.ends_with('\n')
    }
}
