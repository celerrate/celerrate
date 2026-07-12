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
use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, ast, ast::AstNode};

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

/// The declared name, when the first argument is a literal string.
/// Anything dynamic stays out of scope, under the same stance that
/// already excludes `new $class`: a `define($name, ...)` names a constant
/// we cannot know, and guessing would be a false positive in the other
/// direction.
fn defined_name(call: &ast::CallExpression) -> Option<DefinedConstant> {
    let arguments = call.argument_list()?;
    let argument = name_argument(&arguments)?;
    let (name, range) = literal_name(argument.expression()?.syntax())?;
    if name.is_empty() {
        return None;
    }
    Some(DefinedConstant { name, range })
}

/// The string a literal argument spells, and the span of the whole
/// literal.
///
/// One PHP concept, two node kinds. The parser wraps a single-quoted
/// string in a `Literal`, but a double-quoted one is an
/// `InterpolatedString`, because it may interpolate. Reading only the
/// first left every `define("NAME", ...)` unindexed, and an unseen
/// `define()` is a false positive, the one direction the policy forbids.
/// Double-quoted `define()` is at least as common as single-quoted in
/// real PHP.
///
/// A double-quoted string that really does interpolate names a constant
/// that cannot be known, and stays out of scope exactly as `define($name,
/// ...)` does.
fn literal_name(syntax: &SyntaxNode) -> Option<(String, TextRange)> {
    if let Some(literal) = ast::Literal::cast(syntax.clone()) {
        let token = literal.value_token()?;
        if token.kind() != SyntaxKind::SingleQuotedString {
            return None;
        }
        return Some((single_quoted_value(token.text())?, token.text_range()));
    }
    let string = ast::InterpolatedString::cast(syntax.clone())?;
    let fragment = the_only_fragment(&string)?;
    Some((
        double_quoted_value(fragment.text())?,
        string.syntax().text_range(),
    ))
}

/// The one literal run of a double-quoted string that interpolates
/// nothing: the two delimiters, and exactly one fragment between them.
///
/// An interpolation is a child *node*, so a string that has any is
/// rejected by the token-only walk. So is an unterminated string, which
/// never closes and therefore never reaches two delimiters, and an empty
/// one, which has no fragment to read.
fn the_only_fragment(string: &ast::InterpolatedString) -> Option<SyntaxToken> {
    let mut fragment = None;
    let mut delimiters = 0_u32;
    for element in string.syntax().children_with_tokens() {
        let token = element.into_token()?;
        match token.kind() {
            SyntaxKind::DoubleQuote => delimiters += 1,
            SyntaxKind::StringFragment if fragment.is_none() => fragment = Some(token),
            _ => return None,
        }
    }
    if delimiters != 2 {
        return None;
    }
    fragment
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

/// The value of the literal run of a double-quoted string, which honours
/// escapes a single-quoted one does not.
///
/// The escapes are PHP's, decoded the way PHP decodes them, and the line
/// is drawn where the name stops being *representable* rather than where
/// it stops being pretty:
///
/// - `\\`, `\"` and `\$` are unescaped. They are the escapes a real
///   constant name meets, and `\\` is the one that matters: it is how a
///   qualified name is written.
/// - `\n`, `\r`, `\t`, `\v`, `\e` and `\f` are unescaped faithfully.
///   Nothing is guessed: a constant whose name holds a newline is indexed
///   under that name, and no identifier can ever reference it, so it is
///   inert rather than wrong.
/// - `\x` with one or two hexadecimal digits, and `\` with one to three
///   octal digits, denote a byte, which is emitted as written.
/// - `\u{...}` denotes a *code point* and emits its UTF-8 encoding.
/// - **Every other backslash sequence is a literal backslash followed by
///   that character**, exactly as PHP reads it, and that is what makes
///   `"Vendor\Product\LIMIT"` index under the name it declares. It covers
///   `\u` not followed by `{` and `\x` not followed by a hexadecimal
///   digit, which are no escapes at all in PHP: `"Acme\utils\VERSION"`
///   names exactly what it looks like.
///
/// `None` is reserved for the one name this model genuinely cannot hold:
/// a byte sequence that is not valid UTF-8. A PHP constant name is a byte
/// string while `DefinedConstant::name` is a `String`, so `"\xff"` names a
/// constant we will not guess at, the same stance `define($name, ...)`
/// takes. Everything else is indexed, because an unseen `define()` is a
/// false positive at every use site, the one direction the policy forbids.
fn double_quoted_value(text: &str) -> Option<String> {
    let mut bytes: Vec<u8> = Vec::with_capacity(text.len());
    let mut rest = text;
    while let Some(character) = rest.chars().next() {
        let after = rest.get(character.len_utf8()..).unwrap_or_default();
        rest = if character == '\\' {
            read_escape(after, &mut bytes)?
        } else {
            push_character(&mut bytes, character);
            after
        };
    }
    String::from_utf8(bytes).ok()
}

/// Reads the escape that opens `rest`, which is what follows a backslash,
/// appends the bytes it denotes, and returns what is left.
///
/// A sequence PHP reads as no escape leaves the backslash behind literally
/// and hands the character back unread, so it is emitted as itself. `None`
/// means the escape denotes a code point no `String` can hold.
///
/// Terminating: the caller has already consumed the backslash, so `rest`
/// is shorter than what it was called with even when nothing here is.
fn read_escape<'a>(rest: &'a str, bytes: &mut Vec<u8>) -> Option<&'a str> {
    let literal = |bytes: &mut Vec<u8>| {
        bytes.push(b'\\');
        Some(rest)
    };
    let Some(character) = rest.chars().next() else {
        // Unreachable: the lexer consumes the escaped character with the
        // backslash, so a fragment never ends on one. Literal, for totality.
        return literal(bytes);
    };
    let after = rest.get(character.len_utf8()..).unwrap_or_default();

    if let Some(byte) = single_character_escape(character) {
        bytes.push(byte);
        return Some(after);
    }
    if character == 'x'
        && let Some((byte, remaining)) = hexadecimal_escape(after)
    {
        bytes.push(byte);
        return Some(remaining);
    }
    if character == 'u' {
        match code_point_escape(after) {
            CodePoint::Scalar(scalar, remaining) => {
                push_character(bytes, scalar);
                return Some(remaining);
            }
            // A code point that is not a Unicode scalar value: no `String`
            // holds it, so the name is out of scope.
            CodePoint::Unrepresentable => return None,
            CodePoint::NoEscape => {}
        }
    }
    if let Some((byte, remaining)) = octal_escape(rest) {
        bytes.push(byte);
        return Some(remaining);
    }
    literal(bytes)
}

/// The escapes that stand for one byte and read no further.
fn single_character_escape(character: char) -> Option<u8> {
    const VERTICAL_TAB: u8 = 0x0b;
    const ESCAPE: u8 = 0x1b;
    const FORM_FEED: u8 = 0x0c;
    match character {
        '\\' => Some(b'\\'),
        '"' => Some(b'"'),
        '$' => Some(b'$'),
        'n' => Some(b'\n'),
        'r' => Some(b'\r'),
        't' => Some(b'\t'),
        'v' => Some(VERTICAL_TAB),
        'e' => Some(ESCAPE),
        'f' => Some(FORM_FEED),
        _ => None,
    }
}

/// `\x` followed by one or two hexadecimal digits, the byte it spells and
/// what is left. `rest` starts after the `x`. `None` when no digit
/// follows, which is no escape at all: PHP reads `"Foo\xml"` literally.
fn hexadecimal_escape(rest: &str) -> Option<(u8, &str)> {
    digits(rest, 16, 2)
        .and_then(|(value, remaining)| u8::try_from(value).ok().map(|byte| (byte, remaining)))
}

/// `\` followed by one to three octal digits. `rest` starts at the first
/// digit. A value above 255 wraps, as PHP wraps it.
fn octal_escape(rest: &str) -> Option<(u8, &str)> {
    const BYTE: u32 = 256;
    digits(rest, 8, 3).and_then(|(value, remaining)| {
        u8::try_from(value % BYTE)
            .ok()
            .map(|byte| (byte, remaining))
    })
}

/// The value of up to `most` leading digits in `radix`, and what is left.
/// `None` when there is not even one, so the caller can read the sequence
/// as the literal text it is.
fn digits(rest: &str, radix: u32, most: usize) -> Option<(u32, &str)> {
    let mut value = 0_u32;
    let mut read = 0_usize;
    let mut remaining = rest;
    while read < most {
        let Some(digit) = remaining
            .chars()
            .next()
            .and_then(|character| character.to_digit(radix))
        else {
            break;
        };
        // Bounded by `most`: three octal digits reach 511, two hexadecimal
        // ones 255. Neither overflows a `u32`.
        value = value.saturating_mul(radix).saturating_add(digit);
        remaining = remaining.get(1..).unwrap_or_default();
        read = read.saturating_add(1);
    }
    (read > 0).then_some((value, remaining))
}

/// What a `\u` turned out to be.
enum CodePoint<'a> {
    /// A well-formed `\u{...}` naming a Unicode scalar value, and what is
    /// left after it.
    Scalar(char, &'a str),
    /// A well-formed `\u{...}` naming a code point that is no scalar value
    /// (a surrogate, or one past the last), which no `String` can hold.
    Unrepresentable,
    /// No escape: a `\u` PHP reads literally, because nothing shaped like
    /// `{...}` follows it. PHP rejects an unclosed `\u{` outright, so
    /// reading it literally indexes a name PHP would never accept, which is
    /// inert, rather than dropping a `define()` we could have seen.
    NoEscape,
}

/// `\u{...}`, whose body is a code point in hexadecimal. `rest` starts
/// after the `u`.
fn code_point_escape(rest: &str) -> CodePoint<'_> {
    let Some(body) = rest.strip_prefix('{') else {
        return CodePoint::NoEscape;
    };
    let Some(end) = body.find('}') else {
        return CodePoint::NoEscape;
    };
    let (Some(written), Some(remaining)) = (body.get(..end), body.get(end.saturating_add(1)..))
    else {
        return CodePoint::NoEscape;
    };
    let Ok(code_point) = u32::from_str_radix(written, 16) else {
        // Empty, or not hexadecimal, or wider than a `u32`: no escape PHP
        // would accept, and nothing to decode.
        return CodePoint::NoEscape;
    };
    match char::from_u32(code_point) {
        Some(scalar) => CodePoint::Scalar(scalar, remaining),
        None => CodePoint::Unrepresentable,
    }
}

fn push_character(bytes: &mut Vec<u8>, character: char) {
    let mut buffer = [0_u8; 4];
    bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_syntax::parse;

    use super::{DefinedConstant, defines_in, double_quoted_value};

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
    fn a_double_quoted_name_with_nothing_interpolated_is_a_literal_name() {
        // The parser builds a `Literal` only for a single-quoted string; a
        // double-quoted one is an `InterpolatedString`. Demanding the
        // first missed `define("APP_ROOT", 1)` entirely, and an unseen
        // `define()` is a false positive, the one direction the policy
        // forbids.
        assert_eq!(names(r#"<?php define("APP_ROOT", 1);"#), ["APP_ROOT"]);
        assert_eq!(names(r#"<?php define(b"A", 1);"#), ["A"]);
        assert_eq!(
            names(r#"<?php define(constant_name: "A", value: 1);"#),
            ["A"],
        );
    }

    #[test]
    fn a_double_quoted_name_honours_the_escapes_a_single_quoted_one_does_not() {
        // `\\`, `\"` and `\$` are escapes in double quotes. Every other
        // backslash PHP does not read as an escape stays literal, which is
        // what makes a qualified name work.
        assert_eq!(names(r#"<?php define("Foo\\Bar", 1);"#), [r"Foo\Bar"]);
        assert_eq!(
            names(r#"<?php define("Vendor\Product\LIMIT", 1);"#),
            [r"Vendor\Product\LIMIT"],
        );
        assert_eq!(names(r#"<?php define("A\"B", 1);"#), [r#"A"B"#]);
        assert_eq!(names(r#"<?php define("A\$B", 1);"#), ["A$B"]);
    }

    #[test]
    fn a_backslash_that_starts_no_escape_stays_literal_even_before_x_or_u() {
        // `\u` is an escape only before `{`, and `\x` only before a
        // hexadecimal digit. Everywhere else PHP reads both literally, and
        // a lowercase namespace segment, unusual as it is, is legal. Both
        // names below are knowable and representable, so refusing them
        // would be a false positive at every use site.
        assert_eq!(
            names(r#"<?php define("Acme\utils\VERSION", 1);"#),
            [r"Acme\utils\VERSION"],
        );
        assert_eq!(names(r#"<?php define("Foo\xml\NS", 1);"#), [r"Foo\xml\NS"]);
    }

    #[test]
    fn a_byte_or_code_point_escape_is_decoded_the_way_php_decodes_it() {
        // `\u{...}` denotes a code point and emits its UTF-8 encoding, so it
        // is always representable. `\x41` and `\101` denote a byte, and an
        // ASCII one is representable too.
        assert_eq!(names(r#"<?php define("\u{41}PP", 1);"#), ["APP"]);
        assert_eq!(names(r#"<?php define("\u{e9}TAT", 1);"#), ["éTAT"]);
        assert_eq!(names(r#"<?php define("\x41PP", 1);"#), ["APP"]);
        assert_eq!(names(r#"<?php define("\101PP", 1);"#), ["APP"]);
        // Two byte escapes that spell one valid UTF-8 character.
        assert_eq!(names(r#"<?php define("\xc3\xa9TAT", 1);"#), ["éTAT"]);
    }

    #[test]
    fn a_name_that_is_not_valid_utf8_is_out_of_scope() {
        // The only case left: a PHP constant name is a byte string, and this
        // model holds a UTF-8 `String`. A byte sequence no `String` can hold
        // is not guessed at, the same stance a dynamic `define` takes.
        assert!(names(r#"<?php define("\xffPP", 1);"#).is_empty());
        assert!(names(r#"<?php define("\377PP", 1);"#).is_empty());
        // A lone surrogate is a code point, but not a Unicode scalar value.
        assert!(names(r#"<?php define("\u{d800}PP", 1);"#).is_empty());
    }

    #[test]
    fn a_truncated_escape_terminates_and_stays_literal() {
        // No user input may crash the tool. PHP rejects an unclosed `\u{`
        // outright, so whatever we index for it is inert either way; the
        // backslash stays literal, which is the direction that never drops
        // a real `define()`.
        assert_eq!(names(r#"<?php define("\u{41", 1);"#), [r"\u{41"]);
        assert_eq!(names(r#"<?php define("A\u{", 1);"#), [r"A\u{"]);
        // A fragment never ends on a backslash: the lexer takes the escaped
        // character with it, and here that character is the closing quote,
        // which leaves the string unterminated and the name unknown.
        assert!(names(r#"<?php define("A\", 1);"#).is_empty());
        // The unescaper is total regardless, on any input the lexer could
        // never hand it.
        assert_eq!(double_quoted_value("A\\").as_deref(), Some("A\\"));
        assert_eq!(double_quoted_value("\\x").as_deref(), Some("\\x"));
    }

    #[test]
    fn an_empty_or_unterminated_double_quoted_name_is_no_name() {
        assert!(names(r#"<?php define("", 1);"#).is_empty());
        assert!(names(r#"<?php define("APP_ROOT, 1);"#).is_empty());
    }

    #[test]
    fn the_span_of_a_double_quoted_name_covers_the_whole_literal() {
        let text = r#"<?php define("APP_ROOT", 1);"#;
        let defined = defines_in(&parse(text).tree());
        let range = defined[0].range;
        assert_eq!(&text[range], r#""APP_ROOT""#);
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
