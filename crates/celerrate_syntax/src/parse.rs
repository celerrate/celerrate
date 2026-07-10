use crate::diagnostic::{SyntaxDiagnostic, SyntaxDiagnosticKind};
use crate::tree::SyntaxNode;
use crate::tree::builder::build_tree;

/// The result of parsing one source file: the lossless syntax tree and
/// every diagnostic, lexer and parser merged, in source order.
#[derive(Debug, Clone)]
pub struct Parse {
    root: rowan::GreenNode,
    diagnostics: Vec<SyntaxDiagnostic>,
}

impl Parse {
    /// The root of the red tree: always a `SourceFile`.
    pub fn tree(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.root.clone())
    }

    pub fn diagnostics(&self) -> &[SyntaxDiagnostic] {
        &self.diagnostics
    }
}

/// Parses decoded PHP source text into a lossless syntax tree plus
/// structured diagnostics. Always terminates, never fails: degenerate
/// input yields `ErrorNode`s and diagnostics, never a crash or a hole
/// in the tree; `parse(source).tree().text() == source`, always.
pub fn parse(source: &str) -> Parse {
    let (tokens, lexer_diagnostics) = crate::lexer::lex(source);
    let (events, parser_diagnostics) = crate::parser::run(&tokens);
    let root = build_tree(source, &tokens, events);
    let mut diagnostics: Vec<SyntaxDiagnostic> = lexer_diagnostics
        .into_iter()
        .map(|diagnostic| SyntaxDiagnostic {
            kind: SyntaxDiagnosticKind::Lexer(diagnostic.kind),
            range: diagnostic.range,
        })
        .chain(
            parser_diagnostics
                .into_iter()
                .map(|diagnostic| SyntaxDiagnostic {
                    kind: SyntaxDiagnosticKind::Parser(diagnostic.kind),
                    range: diagnostic.range,
                }),
        )
        .collect();
    // Stable sort: on equal ranges, lexer diagnostics stay first.
    diagnostics.sort_by_key(|diagnostic| (diagnostic.range.start(), diagnostic.range.end()));
    Parse { root, diagnostics }
}
