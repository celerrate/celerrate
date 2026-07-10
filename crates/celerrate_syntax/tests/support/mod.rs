//! Shared helpers for the lexer integration tests. Every lexing goes
//! through the lossless assertion: concatenated token lengths must cover
//! the source exactly.
// Slicing by accumulated token lengths is the point of these helpers; a
// bad slice must fail the test loudly.
#![allow(clippy::indexing_slicing, clippy::expect_used)]

use celerrate_syntax::{LexerDiagnostic, SyntaxKind, Token, lex};

pub fn assert_lossless(source: &str, tokens: &[Token]) {
    let total: usize = tokens.iter().map(|token| usize::from(token.length)).sum();
    assert_eq!(
        total,
        source.len(),
        "token lengths must cover the source exactly: {tokens:?}"
    );
    assert!(
        tokens.iter().all(|token| u32::from(token.length) > 0),
        "no token may be empty: {tokens:?}"
    );
}

pub fn lex_verified(source: &str) -> (Vec<Token>, Vec<LexerDiagnostic>) {
    let (tokens, diagnostics) = lex(source);
    assert_lossless(source, &tokens);
    (tokens, diagnostics)
}

#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn kinds(source: &str) -> Vec<SyntaxKind> {
    lex_verified(source)
        .0
        .iter()
        .map(|token| token.kind)
        .collect()
}

#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn texts(source: &str) -> Vec<(SyntaxKind, String)> {
    let (tokens, _diagnostics) = lex_verified(source);
    let mut offset = 0usize;
    tokens
        .iter()
        .map(|token| {
            let end = offset + usize::from(token.length);
            let text = source[offset..end].to_owned();
            offset = end;
            (token.kind, text)
        })
        .collect()
}

#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn parse_verified(source: &str) -> celerrate_syntax::Parse {
    let parse = celerrate_syntax::parse(source);
    assert_eq!(
        parse.tree().text().to_string(),
        source,
        "the tree must be lossless"
    );
    parse
}

/// Renders a parse as an indented tree (`Kind@start..end`, token text
/// quoted) plus a diagnostics footer, asserting losslessness on the way.
#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn render_parse(source: &str) -> String {
    use std::fmt::Write as _;

    let parse = parse_verified(source);
    let mut output = String::new();
    render_element(&mut output, parse.tree().into(), 0);
    if !parse.diagnostics().is_empty() {
        let _ = writeln!(output, "---");
        for diagnostic in parse.diagnostics() {
            let _ = writeln!(
                output,
                "{:?} @ {}..{}",
                diagnostic.kind,
                u32::from(diagnostic.range.start()),
                u32::from(diagnostic.range.end()),
            );
        }
    }
    output
}

#[allow(dead_code)]
fn render_element(output: &mut String, element: celerrate_syntax::SyntaxElement, depth: usize) {
    use std::fmt::Write as _;

    let indent = "  ".repeat(depth);
    match element {
        celerrate_syntax::SyntaxElement::Node(node) => {
            let range = node.text_range();
            let _ = writeln!(
                output,
                "{indent}{:?}@{}..{}",
                node.kind(),
                u32::from(range.start()),
                u32::from(range.end()),
            );
            for child in node.children_with_tokens() {
                render_element(output, child, depth + 1);
            }
        }
        celerrate_syntax::SyntaxElement::Token(token) => {
            let range = token.text_range();
            let _ = writeln!(
                output,
                "{indent}{:?}@{}..{} {:?}",
                token.kind(),
                u32::from(range.start()),
                u32::from(range.end()),
                token.text(),
            );
        }
    }
}

/// Renders the first statement's expression as an indented tree of node
/// kinds and token texts, offsets and trivia omitted: the workhorse
/// assertion of the expression grammar tests. Wraps the fragment as one
/// PHP statement, so the fragment must be a single valid-ish expression.
#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn render_expression(expression_source: &str) -> String {
    let source = format!("<?php {expression_source};");
    let parse = parse_verified(&source);
    let statement = parse.tree().children().next().expect("a first statement");
    let expression = statement
        .children()
        .next()
        .expect("an expression inside the statement");
    let mut output = String::new();
    render_element_without_offsets(&mut output, expression.into(), 0);
    output
}

#[allow(dead_code)]
fn render_element_without_offsets(
    output: &mut String,
    element: celerrate_syntax::SyntaxElement,
    depth: usize,
) {
    use std::fmt::Write as _;

    let indent = "  ".repeat(depth);
    match element {
        celerrate_syntax::SyntaxElement::Node(node) => {
            let _ = writeln!(output, "{indent}{:?}", node.kind());
            for child in node.children_with_tokens() {
                render_element_without_offsets(output, child, depth + 1);
            }
        }
        celerrate_syntax::SyntaxElement::Token(token) => {
            if token.kind().is_trivia() {
                return;
            }
            let _ = writeln!(output, "{indent}{:?} {:?}", token.kind(), token.text());
        }
    }
}

/// Renders the first statement of `<?php {statement_source}` as an
/// indented tree of node kinds and token texts, offsets and trivia
/// omitted: the workhorse assertion of the statement grammar tests.
#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn render_statement(statement_source: &str) -> String {
    let source = format!("<?php {statement_source}");
    let parse = parse_verified(&source);
    let statement = parse.tree().children().next().expect("a first statement");
    let mut output = String::new();
    render_element_without_offsets(&mut output, statement.into(), 0);
    output
}

/// The parser-side diagnostics of one parse, lexer findings filtered
/// out, for structural assertions.
#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn parser_diagnostics(source: &str) -> Vec<celerrate_syntax::ParserDiagnosticKind> {
    parse_verified(source)
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            celerrate_syntax::SyntaxDiagnosticKind::Parser(kind) => Some(kind),
            celerrate_syntax::SyntaxDiagnosticKind::Lexer(_) => None,
        })
        .collect()
}

/// Renders the return type of `<?php function fixture(): {type_source} {}`
/// as an indented tree, offsets and trivia omitted: the workhorse
/// assertion of the type grammar tests.
#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn render_type(type_source: &str) -> String {
    let source = format!("<?php function fixture(): {type_source} {{}}");
    let parse = parse_verified(&source);
    let function = parse
        .tree()
        .children()
        .next()
        .expect("a function declaration");
    let type_node = function
        .children()
        .find(|node| {
            !matches!(
                node.kind(),
                celerrate_syntax::SyntaxKind::ParameterList | celerrate_syntax::SyntaxKind::Block
            )
        })
        .expect("a return type node");
    let mut output = String::new();
    render_element_without_offsets(&mut output, type_node.into(), 0);
    output
}
