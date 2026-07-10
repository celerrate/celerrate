use crate::diagnostic::LexerDiagnosticKind;
use crate::lexer::Lexer;
use crate::syntax_kind::SyntaxKind;

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
}
