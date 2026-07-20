use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
use celerrate_source::FileId;
use celerrate_source::TextRange;

use crate::syntax_kind::SyntaxKind;

/// What went wrong, structurally. Rendering into messages is an upper
/// layer's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexerDiagnosticKind {
    /// A character no lexing rule accepts; the matching token is `Error`.
    UnexpectedCharacter,
    /// `/*` without `*/`; the comment token runs to the end of input.
    UnterminatedBlockComment,
    /// A quoted or backtick string still open at the end of input.
    UnterminatedString,
    /// A heredoc or nowdoc whose closing label never appears.
    UnterminatedHeredoc,
    /// `{$` or `${` without its closing brace.
    UnterminatedInterpolation,
}

/// A lexer diagnostic: a structured kind and the range it points at.
///
/// Diagnostics travel beside the token stream, never instead of it: the
/// stream stays complete and lossless even on degenerate input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexerDiagnostic {
    pub kind: LexerDiagnosticKind,
    pub range: TextRange,
}

/// What the parser expected or could not place, structurally. Rendering
/// into messages is an upper layer's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserDiagnosticKind {
    /// An expression position holds no expression.
    ExpectedExpression,
    /// A statement misses its terminator (`;`, or `?>` / end of input
    /// only where PHP itself allows the omission).
    ExpectedSemicolon,
    /// A specific token is missing; the range is zero-width at the spot
    /// where it belongs.
    Expected(SyntaxKind),
    /// A token no grammar rule accepts; wrapped in an `ErrorNode`.
    UnexpectedToken,
    /// Expressions nest deeper than the parser's recursion budget; the
    /// innermost expression is missing from the tree, the tokens are
    /// preserved through recovery.
    NestingTooDeep,
    /// A non-associative operator chained at the same level, which Zend
    /// rejects (`1 < 2 < 3`, unparenthesized ternary chains, double
    /// `instanceof`). Parsed left-associatively anyway.
    NonAssociativeOperator,
    /// The parser stopped consuming input: an internal grammar loop made
    /// no progress within its step budget. The remaining tokens are
    /// preserved in an `ErrorNode`. Reaching this is a parser bug, kept
    /// survivable by design.
    NoProgress,
    /// `->`, `?->`, or `::` with nothing usable after it.
    ExpectedMemberName,
    /// A control-flow body position holds no statement (`if ($x)` at
    /// end of input, or directly against a closing keyword).
    ExpectedStatement,
    /// A type position holds no type (`function f(): {}`).
    ExpectedType,
    /// A position that requires a declaration holds none (attribute
    /// groups in front of a non-declaration, modifiers with nothing to
    /// modify).
    ExpectedDeclaration,
}

/// A parser diagnostic: a structured kind and the range it points at.
/// Zero-width ranges mark something missing at that offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParserDiagnostic {
    pub kind: ParserDiagnosticKind,
    pub range: TextRange,
}

/// One diagnostic from the syntax layer, wherever it arose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxDiagnosticKind {
    Lexer(LexerDiagnosticKind),
    Parser(ParserDiagnosticKind),
}

/// A syntax diagnostic: lexer and parser findings merged into one
/// stream, in source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxDiagnostic {
    pub kind: SyntaxDiagnosticKind,
    pub range: TextRange,
}

impl LexerDiagnosticKind {
    /// The stable identifier of this kind. Permanent once published.
    pub const fn diagnostic_id(self) -> DiagnosticId {
        match self {
            Self::UnexpectedCharacter => DiagnosticId::new("CEL0002"),
            Self::UnterminatedBlockComment => DiagnosticId::new("CEL0003"),
            Self::UnterminatedString => DiagnosticId::new("CEL0004"),
            Self::UnterminatedHeredoc => DiagnosticId::new("CEL0005"),
            Self::UnterminatedInterpolation => DiagnosticId::new("CEL0006"),
        }
    }
}

impl ParserDiagnosticKind {
    /// The stable identifier of this kind. The `Expected` family shares
    /// one identifier: the identifier names the problem class, not the
    /// missing token. Permanent once published.
    pub const fn diagnostic_id(self) -> DiagnosticId {
        match self {
            Self::ExpectedExpression => DiagnosticId::new("CEL0007"),
            Self::ExpectedSemicolon => DiagnosticId::new("CEL0008"),
            Self::Expected(_) => DiagnosticId::new("CEL0009"),
            Self::UnexpectedToken => DiagnosticId::new("CEL0010"),
            Self::NestingTooDeep => DiagnosticId::new("CEL0011"),
            Self::NonAssociativeOperator => DiagnosticId::new("CEL0012"),
            Self::NoProgress => DiagnosticId::new("CEL0013"),
            Self::ExpectedMemberName => DiagnosticId::new("CEL0014"),
            Self::ExpectedStatement => DiagnosticId::new("CEL0015"),
            Self::ExpectedType => DiagnosticId::new("CEL0016"),
            Self::ExpectedDeclaration => DiagnosticId::new("CEL0017"),
        }
    }
}

impl LexerDiagnosticKind {
    fn message(self) -> String {
        match self {
            Self::UnexpectedCharacter => "unexpected character".to_owned(),
            Self::UnterminatedBlockComment => "unterminated block comment".to_owned(),
            Self::UnterminatedString => "unterminated string literal".to_owned(),
            Self::UnterminatedHeredoc => "unterminated heredoc".to_owned(),
            Self::UnterminatedInterpolation => "unterminated string interpolation".to_owned(),
        }
    }
}

impl ParserDiagnosticKind {
    fn message(self) -> String {
        match self {
            Self::ExpectedExpression => "expected an expression".to_owned(),
            Self::ExpectedSemicolon => "expected `;`".to_owned(),
            Self::Expected(kind) => format!("expected {}", kind.describe()),
            Self::UnexpectedToken => "unexpected token".to_owned(),
            Self::NestingTooDeep => "the input nests too deeply".to_owned(),
            Self::NonAssociativeOperator => {
                "non-associative operators cannot be chained".to_owned()
            }
            Self::NoProgress => "unable to interpret the input here".to_owned(),
            Self::ExpectedMemberName => "expected a member name".to_owned(),
            Self::ExpectedStatement => "expected a statement".to_owned(),
            Self::ExpectedType => "expected a type".to_owned(),
            Self::ExpectedDeclaration => "expected a declaration".to_owned(),
        }
    }
}

/// Every identifier this crate allocates, for the registry check at the
/// composition root. The `Expected` family shares `CEL0009`: the
/// identifier names the problem class, not the missing token.
pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId] = &[
    DiagnosticId::new("CEL0002"),
    DiagnosticId::new("CEL0003"),
    DiagnosticId::new("CEL0004"),
    DiagnosticId::new("CEL0005"),
    DiagnosticId::new("CEL0006"),
    DiagnosticId::new("CEL0007"),
    DiagnosticId::new("CEL0008"),
    DiagnosticId::new("CEL0009"),
    DiagnosticId::new("CEL0010"),
    DiagnosticId::new("CEL0011"),
    DiagnosticId::new("CEL0012"),
    DiagnosticId::new("CEL0013"),
    DiagnosticId::new("CEL0014"),
    DiagnosticId::new("CEL0015"),
    DiagnosticId::new("CEL0016"),
    DiagnosticId::new("CEL0017"),
];

impl SyntaxDiagnostic {
    /// The stable identifier of this diagnostic's kind.
    pub const fn diagnostic_id(&self) -> DiagnosticId {
        match self.kind {
            SyntaxDiagnosticKind::Lexer(kind) => kind.diagnostic_id(),
            SyntaxDiagnosticKind::Parser(kind) => kind.diagnostic_id(),
        }
    }

    /// The rendered message of this finding.
    pub fn message(&self) -> String {
        match self.kind {
            SyntaxDiagnosticKind::Lexer(kind) => kind.message(),
            SyntaxDiagnosticKind::Parser(kind) => kind.message(),
        }
    }

    /// Projects this syntax diagnostic into the shared model. Every
    /// syntax finding is an error: the file does not parse as written.
    pub fn to_diagnostic(&self, file: FileId) -> Diagnostic {
        Diagnostic::spanned(
            self.diagnostic_id(),
            Severity::Error,
            file,
            self.range,
            self.message(),
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_diagnostics::Severity;
    use celerrate_source::{FileId, TextRange, TextSize};

    use super::*;

    #[test]
    fn lexer_kinds_map_to_their_stable_identifiers() {
        let expected = [
            (LexerDiagnosticKind::UnexpectedCharacter, "CEL0002"),
            (LexerDiagnosticKind::UnterminatedBlockComment, "CEL0003"),
            (LexerDiagnosticKind::UnterminatedString, "CEL0004"),
            (LexerDiagnosticKind::UnterminatedHeredoc, "CEL0005"),
            (LexerDiagnosticKind::UnterminatedInterpolation, "CEL0006"),
        ];
        for (kind, identifier) in expected {
            assert_eq!(kind.diagnostic_id().as_str(), identifier);
        }
    }

    #[test]
    fn parser_kinds_map_to_their_stable_identifiers() {
        use crate::syntax_kind::SyntaxKind;
        let expected = [
            (ParserDiagnosticKind::ExpectedExpression, "CEL0007"),
            (ParserDiagnosticKind::ExpectedSemicolon, "CEL0008"),
            (
                ParserDiagnosticKind::Expected(SyntaxKind::Semicolon),
                "CEL0009",
            ),
            (ParserDiagnosticKind::UnexpectedToken, "CEL0010"),
            (ParserDiagnosticKind::NestingTooDeep, "CEL0011"),
            (ParserDiagnosticKind::NonAssociativeOperator, "CEL0012"),
            (ParserDiagnosticKind::NoProgress, "CEL0013"),
            (ParserDiagnosticKind::ExpectedMemberName, "CEL0014"),
            (ParserDiagnosticKind::ExpectedStatement, "CEL0015"),
            (ParserDiagnosticKind::ExpectedType, "CEL0016"),
            (ParserDiagnosticKind::ExpectedDeclaration, "CEL0017"),
        ];
        for (kind, identifier) in expected {
            assert_eq!(kind.diagnostic_id().as_str(), identifier);
        }
    }

    #[test]
    fn projection_carries_identifier_severity_file_and_range() {
        let syntax_diagnostic = SyntaxDiagnostic {
            kind: SyntaxDiagnosticKind::Lexer(LexerDiagnosticKind::UnterminatedString),
            range: TextRange::new(TextSize::from(3), TextSize::from(8)),
        };
        let diagnostic = syntax_diagnostic.to_diagnostic(FileId::new(7));
        assert_eq!(diagnostic.id.as_str(), "CEL0004");
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(
            diagnostic.span(),
            Some((FileId::new(7), syntax_diagnostic.range))
        );
    }

    #[test]
    fn parses_compare_equal_to_themselves() {
        let parse = crate::parse("<?php echo 1;");
        assert_eq!(parse.clone(), parse);
    }

    #[test]
    fn a_missing_semicolon_projects_its_message() {
        let parse = crate::parse("<?php $x = 1");
        let messages: Vec<String> = parse
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.to_diagnostic(FileId::new(0)).message)
            .collect();
        assert!(
            messages.contains(&"expected `;`".to_owned()),
            "messages: {messages:?}",
        );
    }

    #[test]
    fn a_missing_token_reads_as_php_not_as_rust() {
        assert_eq!(
            ParserDiagnosticKind::Expected(SyntaxKind::OpenBrace).message(),
            "expected `{`",
        );
        assert_eq!(
            ParserDiagnosticKind::Expected(SyntaxKind::Identifier).message(),
            "expected a name",
        );
        assert_eq!(
            ParserDiagnosticKind::Expected(SyntaxKind::Variable).message(),
            "expected a variable",
        );
        assert_eq!(
            ParserDiagnosticKind::Expected(SyntaxKind::Catch).message(),
            "expected `catch`",
        );
    }

    #[test]
    fn every_kind_has_a_non_empty_message() {
        let lexer_kinds = [
            LexerDiagnosticKind::UnexpectedCharacter,
            LexerDiagnosticKind::UnterminatedBlockComment,
            LexerDiagnosticKind::UnterminatedString,
            LexerDiagnosticKind::UnterminatedHeredoc,
            LexerDiagnosticKind::UnterminatedInterpolation,
        ];
        for kind in lexer_kinds {
            let diagnostic = SyntaxDiagnostic {
                kind: SyntaxDiagnosticKind::Lexer(kind),
                range: TextRange::new(TextSize::from(0), TextSize::from(0)),
            };
            assert!(!diagnostic.to_diagnostic(FileId::new(0)).message.is_empty());
        }

        let parser_kinds = [
            ParserDiagnosticKind::ExpectedExpression,
            ParserDiagnosticKind::ExpectedSemicolon,
            ParserDiagnosticKind::Expected(SyntaxKind::Semicolon),
            ParserDiagnosticKind::UnexpectedToken,
            ParserDiagnosticKind::NestingTooDeep,
            ParserDiagnosticKind::NonAssociativeOperator,
            ParserDiagnosticKind::NoProgress,
            ParserDiagnosticKind::ExpectedMemberName,
            ParserDiagnosticKind::ExpectedStatement,
            ParserDiagnosticKind::ExpectedType,
            ParserDiagnosticKind::ExpectedDeclaration,
        ];
        for kind in parser_kinds {
            let diagnostic = SyntaxDiagnostic {
                kind: SyntaxDiagnosticKind::Parser(kind),
                range: TextRange::new(TextSize::from(0), TextSize::from(0)),
            };
            assert!(!diagnostic.to_diagnostic(FileId::new(0)).message.is_empty());
        }
    }

    #[test]
    fn the_allocation_list_is_exactly_what_the_kinds_use() {
        let mut used: Vec<DiagnosticId> = Vec::new();
        for kind in [
            LexerDiagnosticKind::UnexpectedCharacter,
            LexerDiagnosticKind::UnterminatedBlockComment,
            LexerDiagnosticKind::UnterminatedString,
            LexerDiagnosticKind::UnterminatedHeredoc,
            LexerDiagnosticKind::UnterminatedInterpolation,
        ] {
            used.push(kind.diagnostic_id());
        }
        for kind in [
            ParserDiagnosticKind::ExpectedExpression,
            ParserDiagnosticKind::ExpectedSemicolon,
            ParserDiagnosticKind::Expected(SyntaxKind::Semicolon),
            ParserDiagnosticKind::UnexpectedToken,
            ParserDiagnosticKind::NestingTooDeep,
            ParserDiagnosticKind::NonAssociativeOperator,
            ParserDiagnosticKind::NoProgress,
            ParserDiagnosticKind::ExpectedMemberName,
            ParserDiagnosticKind::ExpectedStatement,
            ParserDiagnosticKind::ExpectedType,
            ParserDiagnosticKind::ExpectedDeclaration,
        ] {
            used.push(kind.diagnostic_id());
        }
        used.sort();
        used.dedup();
        assert_eq!(used, ALLOCATED_IDENTIFIERS.to_vec());
    }
}
