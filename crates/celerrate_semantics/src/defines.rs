//! `define()` constants: the constants the item tree cannot see.
//!
//! The item traversal never descends into a member list, so a `define()`
//! called from a method body would stay unindexed, and an unseen
//! `define()` is a false positive, the one direction the policy forbids.
//! Making the `ItemTree` see into bodies would close the hole at the cost
//! of the two invariants part 4 guarantees: a `define()` added inside a
//! body would renumber every later `AstId` in the file, and a body edit
//! could change the tree, so the early cutoff would stop firing.
//!
//! This query walks the whole tree instead, method bodies included, and
//! leaves the `ItemTree` alone. It is an early-cutoff unit in its own
//! right: editing a body that contains no `define()` produces an
//! identical result, which salsa backdates.

use celerrate_db::SourceFile;
use celerrate_source::{FileId, TextRange};
use celerrate_syntax::{SyntaxKind, SyntaxNode, ast, ast::AstNode};

/// The stable identity of one `define()` call: the file plus its
/// position in the file's walk order. Not an `AstId`: a `define()` is not
/// an item, and minting an item index for it would collide with the real
/// ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefineId {
    pub file: FileId,
    pub index: u32,
}

/// One constant introduced by a `define()` call with a literal name.
///
/// The name is taken literally, so unlike `const`, a `define()` inside a
/// namespace block declares a constant in the **global** namespace,
/// unless the literal is itself qualified (`define('Foo\Bar', ...)`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DefinedConstant {
    pub name: String,
    pub range: TextRange,
}

/// Every `define('NAME', ...)` in the file, in tree order, method bodies
/// included.
#[salsa::tracked(returns(ref))]
pub fn defined_constants(db: &dyn salsa::Database, file: SourceFile) -> Vec<DefinedConstant> {
    defines_in(&celerrate_db::parse(db, file).tree())
}

/// The walk itself, database-free so it can be unit-tested directly.
fn defines_in(root: &SyntaxNode) -> Vec<DefinedConstant> {
    let mut defined = Vec::new();
    collect(root, &mut defined);
    defined
}

fn collect(node: &SyntaxNode, defined: &mut Vec<DefinedConstant>) {
    for child in node.children() {
        if let Some(call) = ast::CallExpression::cast(child.clone())
            && is_define_call(&call)
            && let Some(constant) = defined_name(&call)
        {
            defined.push(constant);
        }
        collect(&child, defined);
    }
}

/// `define`, `\define`, `DEFINE`: function names are case-insensitive,
/// and the root-qualified spelling names the same function. A method
/// call, a static call, or a call through a variable is a different
/// function and is not this one.
fn is_define_call(call: &ast::CallExpression) -> bool {
    let Some(callee) = call.callee() else {
        return false;
    };
    let Some(callee) = ast::NameExpression::cast(callee.syntax().clone()) else {
        return false;
    };
    if callee.static_keyword_token().is_some() {
        return false;
    }
    let Some(name) = callee.name() else {
        return false;
    };
    let written = name.text();
    let bare = written.strip_prefix('\\').unwrap_or(written.as_str());
    bare.eq_ignore_ascii_case("define")
}

/// The declared name, when the first argument is a literal single-quoted
/// string. Anything dynamic stays out of scope, under the same stance
/// that already excludes `new $class`: a `define($name, ...)` names a
/// constant we cannot know, and guessing would be a false positive in the
/// other direction.
fn defined_name(call: &ast::CallExpression) -> Option<DefinedConstant> {
    let arguments = call.argument_list()?;
    let argument = name_argument(&arguments)?;
    let literal = ast::Literal::cast(argument.expression()?.syntax().clone())?;
    let token = literal.value_token()?;
    if token.kind() != SyntaxKind::SingleQuotedString {
        return None;
    }
    let name = single_quoted_value(token.text())?;
    if name.is_empty() {
        return None;
    }
    Some(DefinedConstant {
        name,
        range: token.text_range(),
    })
}

/// The argument holding the name: the one labeled `constant_name` when
/// the call uses named arguments, the first positional one otherwise.
fn name_argument(arguments: &ast::ArgumentList) -> Option<ast::Argument> {
    let all: Vec<ast::Argument> = arguments.arguments().collect();
    let labeled = all.iter().find(|argument| {
        argument
            .label_token()
            .is_some_and(|label| label.text() == "constant_name")
    });
    if let Some(argument) = labeled {
        return Some(argument.clone());
    }
    let first = all.first()?;
    if first.label_token().is_some() || first.spread_token().is_some() {
        return None;
    }
    Some(first.clone())
}

/// The value of a single-quoted string token: the `b`/`B` prefix and the
/// quotes stripped, `\\` and `\'` unescaped, every other backslash
/// literal. `None` for an unterminated string, which the lexer still
/// hands us as this kind.
fn single_quoted_value(text: &str) -> Option<String> {
    let body = text
        .strip_prefix('b')
        .or_else(|| text.strip_prefix('B'))
        .unwrap_or(text);
    let body = body.strip_prefix('\'')?;
    let body = body.strip_suffix('\'')?;
    let mut value = String::with_capacity(body.len());
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            value.push(character);
            continue;
        }
        match characters.next() {
            Some(escaped @ ('\\' | '\'')) => value.push(escaped),
            Some(other) => {
                value.push('\\');
                value.push(other);
            }
            None => value.push('\\'),
        }
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_syntax::parse;

    use super::{DefinedConstant, defines_in};

    fn names(source: &str) -> Vec<String> {
        defines_in(&parse(source).tree())
            .into_iter()
            .map(|defined: DefinedConstant| defined.name)
            .collect()
    }

    #[test]
    fn a_top_level_define_is_seen() {
        assert_eq!(names("<?php define('APP_ROOT', __DIR__);"), ["APP_ROOT"]);
    }

    #[test]
    fn a_define_in_a_method_body_is_seen() {
        // The case the item tree cannot see, and the reason this query exists.
        let source = "<?php class Bootstrap {
            public static function boot(): void {
                define('APP_ROOT', __DIR__);
            }
        }";
        assert_eq!(names(source), ["APP_ROOT"]);
    }

    #[test]
    fn the_callee_is_matched_case_insensitively_in_both_spellings() {
        assert_eq!(names("<?php \\define('A', 1);"), ["A"]);
        assert_eq!(names("<?php DEFINE('B', 1);"), ["B"]);
        assert_eq!(names("<?php \\DeFiNe('C', 1);"), ["C"]);
    }

    #[test]
    fn a_dynamic_name_is_out_of_scope() {
        // The same stance that already excludes `new $class`.
        assert!(names("<?php define($name, 1);").is_empty());
        assert!(names("<?php define(\"A$suffix\", 1);").is_empty());
        assert!(names("<?php define(NAME_OF, 1);").is_empty());
    }

    #[test]
    fn a_name_is_taken_literally_quotes_prefix_and_escapes_removed() {
        assert_eq!(names("<?php define(b'A', 1);"), ["A"]);
        assert_eq!(names(r"<?php define('Foo\\Bar', 1);"), [r"Foo\Bar"]);
        assert_eq!(names(r"<?php define('It\'s', 1);"), ["It's"]);
    }

    #[test]
    fn a_named_argument_is_read_by_its_label() {
        assert_eq!(names("<?php define(constant_name: 'A', value: 1);"), ["A"]);
        assert_eq!(names("<?php define(value: 1, constant_name: 'B');"), ["B"]);
    }

    #[test]
    fn a_method_named_define_is_not_a_define() {
        assert!(names("<?php $container->define('A', 1);").is_empty());
        assert!(names("<?php Registry::define('A', 1);").is_empty());
    }

    #[test]
    fn the_span_points_at_the_name_literal() {
        let defined = defines_in(&parse("<?php define('APP_ROOT', 1);").tree());
        let text = "<?php define('APP_ROOT', 1);";
        let range = defined[0].range;
        assert_eq!(&text[range], "'APP_ROOT'");
    }
}
