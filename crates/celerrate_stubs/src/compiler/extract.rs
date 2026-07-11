//! Extraction: one stub file's text in, its top-level symbols out.
//! Tolerant end to end: malformed PHP still yields whatever
//! declarations the error-resilient parser recovered.

use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::symbol::{StubAvailability, StubSymbol, StubSymbolKind};

/// The result of extracting one stub file. `had_parse_errors` lets the
/// compiler count warnings without ever failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extraction {
    pub symbols: Vec<StubSymbol>,
    pub had_parse_errors: bool,
}

/// Extracts every top-level symbol of one stub file.
pub fn extract(text: &str) -> Extraction {
    let parse = celerrate_syntax::parse(text);
    let mut symbols = Vec::new();
    if let Some(file) = ast::SourceFile::cast(parse.tree()) {
        collect(file.statements(), "", &mut symbols);
    }
    Extraction {
        symbols,
        had_parse_errors: !parse.diagnostics().is_empty(),
    }
}

fn collect(
    statements: ast::AstChildren<ast::Statement>,
    initial_namespace: &str,
    symbols: &mut Vec<StubSymbol>,
) {
    // Statement-form `namespace Foo;` switches the prefix for the
    // statements that follow it, so the namespace is walk state.
    let mut namespace = initial_namespace.to_owned();
    for statement in statements {
        match statement {
            ast::Statement::NamespaceDeclaration(declaration) => {
                let name = declaration
                    .name()
                    .map(|name| name_text(&name))
                    .unwrap_or_default();
                match declaration.block() {
                    Some(block) => collect(block.statements(), &name, symbols),
                    None => namespace = name,
                }
            }
            ast::Statement::ClassDeclaration(declaration) => push_named(
                symbols,
                &namespace,
                StubSymbolKind::Class,
                declaration.name_token(),
                declaration.syntax(),
            ),
            ast::Statement::InterfaceDeclaration(declaration) => push_named(
                symbols,
                &namespace,
                StubSymbolKind::Interface,
                declaration.name_token(),
                declaration.syntax(),
            ),
            ast::Statement::TraitDeclaration(declaration) => push_named(
                symbols,
                &namespace,
                StubSymbolKind::Trait,
                declaration.name_token(),
                declaration.syntax(),
            ),
            ast::Statement::EnumDeclaration(declaration) => push_named(
                symbols,
                &namespace,
                StubSymbolKind::Enum,
                declaration.name_token(),
                declaration.syntax(),
            ),
            ast::Statement::FunctionDeclaration(declaration) => push_named(
                symbols,
                &namespace,
                StubSymbolKind::Function,
                declaration.name_token(),
                declaration.syntax(),
            ),
            ast::Statement::ConstantDeclaration(declaration) => {
                let availability = availability_of(declaration.syntax());
                for element in declaration.constant_elements() {
                    if let Some(name_token) = element.name_token() {
                        symbols.push(StubSymbol {
                            name: qualify(&namespace, name_token.text()),
                            kind: StubSymbolKind::Constant,
                            availability,
                        });
                    }
                }
            }
            ast::Statement::ExpressionStatement(statement) => {
                if let Some(symbol) = define_constant(&statement) {
                    symbols.push(symbol);
                }
            }
            _ => {}
        }
    }
}

fn push_named(
    symbols: &mut Vec<StubSymbol>,
    namespace: &str,
    kind: StubSymbolKind,
    name_token: Option<SyntaxToken>,
    node: &SyntaxNode,
) {
    let Some(name_token) = name_token else { return };
    symbols.push(StubSymbol {
        name: qualify(namespace, name_token.text()),
        kind,
        availability: availability_of(node),
    });
}

/// A `define('NAME', ...)` statement with a literal string name: a
/// global constant declaration, whatever the current namespace.
/// Dynamic names are out of scope, like every dynamic reference.
fn define_constant(statement: &ast::ExpressionStatement) -> Option<StubSymbol> {
    let ast::Expression::CallExpression(call) = statement.expression()? else {
        return None;
    };
    let ast::Expression::NameExpression(callee) = call.callee()? else {
        return None;
    };
    // Function names are case-insensitive in PHP.
    let callee_name = name_text(&callee.name()?);
    if !callee_name
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("define")
    {
        return None;
    }
    let first_argument = call.argument_list()?.arguments().next()?;
    let name = string_literal(&first_argument.expression()?)?;
    Some(StubSymbol {
        name: name.trim_start_matches('\\').to_owned(),
        kind: StubSymbolKind::Constant,
        availability: availability_of(statement.syntax()),
    })
}

/// The text of a `Name` node with any interior trivia stripped.
fn name_text(name: &ast::Name) -> String {
    name.syntax()
        .text()
        .to_string()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

/// The content of a simple string literal naming a `define()` constant:
/// a single-quoted `Literal` or a double-quoted `InterpolatedString`
/// with no interpolation. Anything else (interpolation, heredoc,
/// concatenation) is `None`.
fn string_literal(expression: &ast::Expression) -> Option<String> {
    match expression {
        ast::Expression::Literal(literal) => single_quoted_literal(literal),
        ast::Expression::InterpolatedString(interpolated) => double_quoted_literal(interpolated),
        _ => None,
    }
}

/// The lexer keeps a single-quoted string as one token, quotes
/// included and escapes unprocessed; `\\` and `\'` are the only
/// escapes single quotes recognize, and stub constant names never use
/// them, so the raw content between the quotes is the name.
fn single_quoted_literal(literal: &ast::Literal) -> Option<String> {
    let token = literal.value_token()?;
    if token.kind() != SyntaxKind::SingleQuotedString {
        return None;
    }
    let text = token.text();
    let unquoted = text.strip_prefix('\'')?.strip_suffix('\'')?;
    Some(unquoted.to_owned())
}

/// A double-quoted string with no interpolation: the parser still
/// produces an `InterpolatedString` node (double quotes always do),
/// but with no `StringInterpolation` children its content is exactly
/// the plain text carried by its (at most one) `StringFragment` token.
fn double_quoted_literal(interpolated: &ast::InterpolatedString) -> Option<String> {
    if interpolated.string_interpolations().next().is_some() {
        return None;
    }
    let mut fragments = interpolated
        .syntax()
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind() == SyntaxKind::StringFragment)
        .map(|token| token.text().to_owned());
    let text = fragments.next().unwrap_or_default();
    if fragments.next().is_some() {
        return None;
    }
    Some(text)
}

fn qualify(namespace: &str, name: &str) -> String {
    let name = name.trim_start_matches('\\');
    if namespace.is_empty() {
        name.to_owned()
    } else {
        format!("{namespace}\\{name}")
    }
}

/// Availability metadata arrives with the next task; until then every
/// symbol is unconstrained.
fn availability_of(_node: &SyntaxNode) -> StubAvailability {
    StubAvailability::ALWAYS
}

#[cfg(test)]
mod tests {
    use super::{Extraction, extract};
    use crate::symbol::StubSymbolKind;

    fn names_and_kinds(extraction: &Extraction) -> Vec<(String, StubSymbolKind)> {
        extraction
            .symbols
            .iter()
            .map(|symbol| (symbol.name.clone(), symbol.kind))
            .collect()
    }

    #[test]
    fn every_top_level_declaration_kind_is_extracted() {
        let extraction = extract(
            "<?php\n\
             class Exception {}\n\
             interface Traversable {}\n\
             trait Helper {}\n\
             enum Suit {}\n\
             function strlen(string $string): int {}\n\
             const PHP_EOL = \"\\n\";\n",
        );
        assert_eq!(
            names_and_kinds(&extraction),
            vec![
                ("Exception".to_owned(), StubSymbolKind::Class),
                ("Traversable".to_owned(), StubSymbolKind::Interface),
                ("Helper".to_owned(), StubSymbolKind::Trait),
                ("Suit".to_owned(), StubSymbolKind::Enum),
                ("strlen".to_owned(), StubSymbolKind::Function),
                ("PHP_EOL".to_owned(), StubSymbolKind::Constant),
            ],
        );
        assert!(!extraction.had_parse_errors);
    }

    #[test]
    fn a_statement_form_namespace_qualifies_everything_after_it() {
        let extraction = extract(
            "<?php\n\
             namespace Random;\n\
             class Randomizer {}\n\
             const SEED = 1;\n",
        );
        assert_eq!(
            names_and_kinds(&extraction),
            vec![
                ("Random\\Randomizer".to_owned(), StubSymbolKind::Class),
                ("Random\\SEED".to_owned(), StubSymbolKind::Constant),
            ],
        );
    }

    #[test]
    fn brace_form_namespaces_scope_their_block_only() {
        let extraction = extract(
            "<?php\n\
             namespace Ds { class Vector {} }\n\
             namespace { function outside() {} }\n",
        );
        assert_eq!(
            names_and_kinds(&extraction),
            vec![
                ("Ds\\Vector".to_owned(), StubSymbolKind::Class),
                ("outside".to_owned(), StubSymbolKind::Function),
            ],
        );
    }

    #[test]
    fn sequential_statement_form_namespaces_switch_the_prefix() {
        let extraction = extract(
            "<?php\n\
             namespace First;\n\
             function one() {}\n\
             namespace Second;\n\
             function two() {}\n",
        );
        assert_eq!(
            names_and_kinds(&extraction),
            vec![
                ("First\\one".to_owned(), StubSymbolKind::Function),
                ("Second\\two".to_owned(), StubSymbolKind::Function),
            ],
        );
    }

    #[test]
    fn a_grouped_constant_declaration_yields_one_symbol_per_element() {
        let extraction = extract("<?php const A = 1, B = 2;");
        assert_eq!(
            names_and_kinds(&extraction),
            vec![
                ("A".to_owned(), StubSymbolKind::Constant),
                ("B".to_owned(), StubSymbolKind::Constant),
            ],
        );
    }

    #[test]
    fn define_calls_with_a_literal_name_declare_global_constants() {
        let extraction = extract(
            "<?php\n\
             namespace Ignored;\n\
             define('E_ALL', 32767);\n\
             define(\"E_STRICT\", 2048);\n\
             define($dynamic, 1);\n\
             define(E_ALL, 1);\n",
        );
        // define() names the constant absolutely, whatever the current
        // namespace; dynamic names are skipped.
        assert_eq!(
            names_and_kinds(&extraction),
            vec![
                ("E_ALL".to_owned(), StubSymbolKind::Constant),
                ("E_STRICT".to_owned(), StubSymbolKind::Constant),
            ],
        );
    }

    #[test]
    fn nested_and_conditional_declarations_are_not_top_level() {
        let extraction = extract(
            "<?php\n\
             class Outer { public function method(): void {} }\n\
             if (true) { function guarded() {} }\n",
        );
        assert_eq!(
            names_and_kinds(&extraction),
            vec![("Outer".to_owned(), StubSymbolKind::Class)],
        );
    }

    #[test]
    fn malformed_input_extracts_what_the_parser_recovered_and_reports_errors() {
        let extraction = extract("<?php class Broken { function ok() {}");
        assert!(extraction.had_parse_errors);
        assert_eq!(
            names_and_kinds(&extraction),
            vec![("Broken".to_owned(), StubSymbolKind::Class)],
        );
    }

    #[test]
    fn empty_and_html_only_files_extract_nothing() {
        assert!(extract("").symbols.is_empty());
        assert!(extract("plain text, no PHP").symbols.is_empty());
    }
}
