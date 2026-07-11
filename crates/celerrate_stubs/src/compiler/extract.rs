//! Extraction: one stub file's text in, its top-level symbols out.
//! Tolerant end to end: malformed PHP still yields whatever
//! declarations the error-resilient parser recovered.

use celerrate_project::PhpVersion;
use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::symbol::{StubAvailability, StubDeprecation, StubSymbol, StubSymbolKind};

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

/// Availability from the declaration's own metadata: leading doc tags,
/// then attributes. Each field is set once; the first source wins.
fn availability_of(node: &SyntaxNode) -> StubAvailability {
    let mut availability = doc_availability(node);
    apply_attributes(node, &mut availability);
    availability
}

fn doc_availability(node: &SyntaxNode) -> StubAvailability {
    let mut availability = StubAvailability::ALWAYS;
    let Some(comment) = leading_doc_comment(node) else {
        return availability;
    };
    for line in comment.text().lines() {
        let line = line.trim_start_matches(['/', '*', ' ', '\t']).trim_end();
        if let Some(rest) = line.strip_prefix("@since") {
            if availability.introduced.is_none() {
                availability.introduced = parse_version(rest);
            }
        } else if let Some(rest) = line.strip_prefix("@removed") {
            if availability.removed.is_none() {
                availability.removed = parse_version(rest);
            }
        } else if let Some(rest) = line.strip_prefix("@deprecated")
            && availability.deprecated.is_none()
        {
            availability.deprecated = Some(StubDeprecation {
                since: parse_version(rest),
            });
        }
    }
    availability
}

/// The closest `/** ... */` before the node, separated from it only by
/// trivia. The doc comment is not a descendant of the declaration node
/// it decorates (trivia flushes into the parent before the declaration
/// node opens), so the walk starts at the first meaningful token of the
/// subtree and follows the flat token stream backwards regardless of
/// node boundaries.
fn leading_doc_comment(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut token = first_meaningful_token(node)?.prev_token();
    while let Some(current) = token {
        match current.kind() {
            SyntaxKind::DocComment => return Some(current),
            SyntaxKind::Whitespace | SyntaxKind::LineComment | SyntaxKind::BlockComment => {
                token = current.prev_token();
            }
            _ => return None,
        }
    }
    None
}

fn first_meaningful_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut token = node.first_token()?;
    while matches!(
        token.kind(),
        SyntaxKind::Whitespace
            | SyntaxKind::LineComment
            | SyntaxKind::BlockComment
            | SyntaxKind::DocComment
    ) {
        token = token.next_token()?;
    }
    Some(token)
}

fn apply_attributes(node: &SyntaxNode, availability: &mut StubAvailability) {
    for group in node.children().filter_map(ast::AttributeGroup::cast) {
        for attribute in group.attributes() {
            let Some(name) = attribute.name() else {
                continue;
            };
            let name = name_text(&name);
            let name = name.trim_start_matches('\\');
            let simple = name.rsplit('\\').next().unwrap_or(name);
            if simple.eq_ignore_ascii_case("PhpStormStubsElementAvailable") {
                apply_element_available(&attribute, availability);
            } else if simple.eq_ignore_ascii_case("Deprecated") && availability.deprecated.is_none()
            {
                availability.deprecated = Some(StubDeprecation {
                    since: labeled_version(&attribute, "since"),
                });
            }
        }
    }
}

/// `#[PhpStormStubsElementAvailable(from:, to:)]`: labeled or
/// positional (first positional is `from`, second is `to`). `to` is
/// the last version that still has the symbol, so removal is its
/// successor.
fn apply_element_available(attribute: &ast::Attribute, availability: &mut StubAvailability) {
    let Some(argument_list) = attribute.argument_list() else {
        return;
    };
    let mut positional_index = 0usize;
    for argument in argument_list.arguments() {
        let label = argument.label_token().map(|token| token.text().to_owned());
        let version = argument
            .expression()
            .as_ref()
            .and_then(string_literal)
            .as_deref()
            .and_then(parse_version);
        let role = match label.as_deref() {
            Some("from") => Some(0),
            Some("to") => Some(1),
            Some(_) => None,
            None => {
                let role = positional_index;
                positional_index += 1;
                (role < 2).then_some(role)
            }
        };
        match (role, version) {
            (Some(0), Some(version)) if availability.introduced.is_none() => {
                availability.introduced = Some(version);
            }
            (Some(1), Some(version)) if availability.removed.is_none() => {
                availability.removed = Some(successor(version));
            }
            _ => {}
        }
    }
}

fn labeled_version(attribute: &ast::Attribute, label: &str) -> Option<PhpVersion> {
    attribute
        .argument_list()?
        .arguments()
        .find(|argument| {
            argument
                .label_token()
                .is_some_and(|token| token.text() == label)
        })?
        .expression()
        .as_ref()
        .and_then(string_literal)
        .as_deref()
        .and_then(parse_version)
}

/// Parses `8.1`, `8.1.2`, `8.1RC1`, or `8` into a major.minor version;
/// anything unparseable is `None`, never an error.
fn parse_version(text: &str) -> Option<PhpVersion> {
    let mut parts = text.trim().split('.');
    let major = parts.next()?.parse::<u8>().ok()?;
    let minor = parts.next().map_or(0, leading_digits);
    Some(PhpVersion::new(major, minor))
}

fn leading_digits(part: &str) -> u8 {
    let digits: String = part
        .chars()
        .take_while(|character: &char| character.is_ascii_digit())
        .collect();
    digits.parse().unwrap_or(0)
}

/// The first version after `version` on PHP's actual release line:
/// minors increment, except the two historical jumps (5.6 → 7.0,
/// PHP 6 never shipped, and 7.4 → 8.0).
fn successor(version: PhpVersion) -> PhpVersion {
    match (version.major, version.minor) {
        (5, 6) => PhpVersion::new(7, 0),
        (7, 4) => PhpVersion::new(8, 0),
        (major, minor) => PhpVersion::new(major, minor.saturating_add(1)),
    }
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

    use celerrate_project::PhpVersion;

    use crate::symbol::{StubAvailability, StubDeprecation};

    fn only_availability(source: &str) -> StubAvailability {
        let extraction = extract(source);
        assert_eq!(
            extraction.symbols.len(),
            1,
            "expected one symbol in {source}"
        );
        extraction
            .symbols
            .first()
            .map(|symbol| symbol.availability)
            .unwrap_or(StubAvailability::ALWAYS)
    }

    #[test]
    fn doc_tags_set_the_availability_window() {
        let availability = only_availability(
            "<?php\n\
             /**\n\
              * Frobnicates.\n\
              * @since 8.1\n\
              * @deprecated 8.3\n\
              */\n\
             function frobnicate() {}\n",
        );
        assert_eq!(
            availability,
            StubAvailability {
                introduced: Some(PhpVersion::new(8, 1)),
                removed: None,
                deprecated: Some(StubDeprecation {
                    since: Some(PhpVersion::new(8, 3)),
                }),
            },
        );
    }

    #[test]
    fn a_removed_tag_sets_the_removal_version() {
        let availability =
            only_availability("<?php\n/** @removed 8.0 */\nfunction create_function() {}\n");
        assert_eq!(availability.removed, Some(PhpVersion::new(8, 0)));
    }

    #[test]
    fn a_deprecation_without_a_version_is_recorded_as_unversioned() {
        let availability =
            only_availability("<?php\n/** @deprecated */\nfunction old_thing() {}\n");
        assert_eq!(
            availability.deprecated,
            Some(StubDeprecation { since: None })
        );
    }

    #[test]
    fn patch_components_and_suffixes_are_truncated() {
        let availability =
            only_availability("<?php\n/** @since 5.3.0 */\nfunction with_patch() {}\n");
        assert_eq!(availability.introduced, Some(PhpVersion::new(5, 3)));
    }

    #[test]
    fn unparseable_versions_are_ignored_not_fatal() {
        let availability = only_availability("<?php\n/** @since forever */\nfunction murky() {}\n");
        assert_eq!(availability, StubAvailability::ALWAYS);
    }

    #[test]
    fn a_doc_comment_only_binds_to_the_declaration_that_follows_it() {
        let extraction = extract(
            "<?php\n\
             /** @since 8.1 */\n\
             function first() {}\n\
             function second() {}\n",
        );
        let introduced: Vec<Option<PhpVersion>> = extraction
            .symbols
            .iter()
            .map(|symbol| symbol.availability.introduced)
            .collect();
        assert_eq!(introduced, vec![Some(PhpVersion::new(8, 1)), None]);
    }

    #[test]
    fn the_availability_attribute_sets_the_window_with_labels() {
        let availability = only_availability(
            "<?php\n\
             #[PhpStormStubsElementAvailable(from: '8.2')]\n\
             function fresh() {}\n",
        );
        assert_eq!(availability.introduced, Some(PhpVersion::new(8, 2)));
    }

    #[test]
    fn the_availability_attribute_accepts_positional_arguments() {
        let availability = only_availability(
            "<?php\n\
             #[PhpStormStubsElementAvailable('7.0', '7.4')]\n\
             function spanned() {}\n",
        );
        assert_eq!(availability.introduced, Some(PhpVersion::new(7, 0)));
        // `to: 7.4` means present up to 7.4: gone in the successor, 8.0.
        assert_eq!(availability.removed, Some(PhpVersion::new(8, 0)));
    }

    #[test]
    fn the_to_bound_uses_the_real_php_release_line() {
        let availability = only_availability(
            "<?php\n\
             #[PhpStormStubsElementAvailable(from: '8.0', to: '8.1')]\n\
             function narrow() {}\n",
        );
        assert_eq!(availability.removed, Some(PhpVersion::new(8, 2)));
    }

    #[test]
    fn the_deprecated_attribute_matches_by_last_segment_and_reads_since() {
        let availability = only_availability(
            "<?php\n\
             #[\\JetBrains\\PhpStorm\\Deprecated(reason: 'use something else', since: '8.1')]\n\
             function dated() {}\n",
        );
        assert_eq!(
            availability.deprecated,
            Some(StubDeprecation {
                since: Some(PhpVersion::new(8, 1)),
            }),
        );
    }

    #[test]
    fn a_doc_comment_reaches_its_declaration_across_the_attributes() {
        let availability = only_availability(
            "<?php\n\
             /** @since 8.1 */\n\
             #[PhpStormStubsElementAvailable(from: '8.2')]\n\
             function both() {}\n",
        );
        // Each field is set once, first source wins: the doc tag came first.
        assert_eq!(availability.introduced, Some(PhpVersion::new(8, 1)));
    }

    #[test]
    fn a_define_call_carries_its_leading_doc_metadata() {
        let availability = only_availability("<?php\n/** @since 8.4 */\ndefine('BRAND_NEW', 1);\n");
        assert_eq!(availability.introduced, Some(PhpVersion::new(8, 4)));
    }

    #[test]
    fn line_comments_between_doc_and_declaration_do_not_break_the_binding() {
        let availability = only_availability(
            "<?php\n\
             /** @since 8.1 */\n\
             // implementation note\n\
             function commented() {}\n",
        );
        assert_eq!(availability.introduced, Some(PhpVersion::new(8, 1)));
    }
}
