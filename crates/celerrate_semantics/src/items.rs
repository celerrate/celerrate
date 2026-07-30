//! The per-file item tree: the `Eq`-comparable, deterministically
//! ordered projection of one file's declarations. It carries no ranges
//! and no offsets — a body, comment, or whitespace edit produces an
//! identical value, salsa backdates it, and nothing downstream re-runs.
//! That equality is the invalidation boundary of the engine.
//!
//! `defines` is the one field that is not an *item*: `define('NAME', ...)`
//! calls, method bodies included, collected as a separate, range-free
//! list in walk order. The item traversal (`item_nodes`) never descends
//! into a member list, so a `define()` called from a method body would
//! stay unindexed, and an unseen `define()` is a false positive, the one
//! direction the policy forbids. Making the traversal itself see into
//! bodies would renumber every later `AstId` when a body gains a
//! `define()`, so `defines` is walked separately and kept outside the
//! `AstId`-numbered item list; a `DefineId` (its file plus its position in
//! this list) is not an `AstId`, and minting an item index for a define
//! would collide with the real ones. The list is still range-free, so a
//! body edit that adds or removes no `define()` still produces an
//! identical `ItemTree`, and salsa still backdates it: only a body edit
//! that actually changes the set of `define()`-declared names changes
//! this value, which is correct, because it changes the project's
//! symbols.

use celerrate_source::FileId;
use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::ast_id::AstId;
use crate::item_nodes::{ItemNode, item_nodes};

/// The stable identity of one `define()` call: the file plus its
/// position in the file's walk order. Not an `AstId`: a `define()` is not
/// an item, and minting an item index for it would collide with the real
/// ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefineId {
    pub file: FileId,
    pub index: u32,
}

/// The kind of a declared symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationKind {
    Class,
    Interface,
    Trait,
    Enum,
    Function,
    Constant,
}

/// One declared symbol: original spelling, enclosing namespace (`""`
/// is global), stable identity, and the unresolved inheritance names
/// exactly as written (a later consumer needs them; they cost one
/// field now).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Declaration {
    pub kind: DeclarationKind,
    pub name: String,
    pub namespace: String,
    pub ast_id: AstId,
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub trait_uses: Vec<String>,
}

/// What one `use` clause imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportKind {
    Class,
    Function,
    Constant,
}

/// One expanded `use` import: group forms are flattened, the target is
/// the written absolute name (leading backslash trimmed), and the
/// alias is the explicit one or the target's last segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UseImport {
    pub kind: ImportKind,
    pub target: String,
    pub alias: String,
    pub namespace: String,
    pub ast_id: AstId,
}

/// The projection of one file's declarations and imports, in tree
/// order, plus the range-free list of `define()`-declared constant
/// names (see the module doc).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemTree {
    pub declarations: Vec<Declaration>,
    pub imports: Vec<UseImport>,
    pub defines: Vec<String>,
}

impl ItemTree {
    /// Projects one file's syntax tree. Positions in the shared
    /// declaration-node traversal are the `AstId` indexes, so this
    /// numbering and [`crate::AstIdMap`]'s agree by construction. The
    /// `define()` walk is separate and does not participate in that
    /// numbering.
    pub fn from_root(file: FileId, root: &SyntaxNode) -> Self {
        let mut tree = ItemTree::default();
        for (position, item) in item_nodes(root).into_iter().enumerate() {
            let Ok(index) = u32::try_from(position) else {
                break;
            };
            lower(&item, AstId { file, index }, &mut tree);
        }
        tree.defines = defines_in(root);
        tree
    }
}

/// The unresolved inheritance names of one class-like declaration.
struct Inheritance {
    extends: Vec<String>,
    implements: Vec<String>,
    trait_uses: Vec<String>,
}

impl Inheritance {
    const NONE: Inheritance = Inheritance {
        extends: Vec::new(),
        implements: Vec::new(),
        trait_uses: Vec::new(),
    };
}

/// The written names of one clause, in source order.
fn names_of(names: Option<ast::AstChildren<ast::Name>>) -> Vec<String> {
    names
        .into_iter()
        .flatten()
        .map(|name| name.text())
        .collect()
}

/// The trait names a class-like uses, read from its member list. The
/// traversal never descends into member lists; this projection field
/// is the one place the tree looks inside one, because inheritance
/// names include trait `use`.
fn trait_use_names(member_list: Option<ast::MemberList>) -> Vec<String> {
    member_list
        .into_iter()
        .flat_map(|list| list.member_declarations())
        .filter_map(|member| match member {
            ast::MemberDeclaration::TraitUseClause(clause) => Some(clause),
            _ => None,
        })
        .flat_map(|clause| clause.names())
        .map(|name| name.text())
        .collect()
}

/// The unresolved inheritance names of one class-like declaration. The
/// four generated class-like types share accessor names but no trait;
/// this macro reads them uniformly.
macro_rules! inheritance_of {
    ($declaration:expr) => {
        Inheritance {
            extends: names_of($declaration.extends_clause().map(|clause| clause.names())),
            implements: names_of(
                $declaration
                    .implements_clause()
                    .map(|clause| clause.names()),
            ),
            trait_uses: trait_use_names($declaration.member_list()),
        }
    };
}

fn lower(item: &ItemNode, ast_id: AstId, tree: &mut ItemTree) {
    // Members consume numbering but never project a top-level
    // declaration; the member projection (`MemberTree`) owns them.
    if item.owner.is_some() {
        return;
    }
    match item.node.kind() {
        SyntaxKind::ClassDeclaration => {
            if let Some(declaration) = ast::ClassDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Class,
                    declaration.name_token(),
                    inheritance_of!(declaration),
                );
            }
        }
        SyntaxKind::InterfaceDeclaration => {
            if let Some(declaration) = ast::InterfaceDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Interface,
                    declaration.name_token(),
                    inheritance_of!(declaration),
                );
            }
        }
        SyntaxKind::TraitDeclaration => {
            if let Some(declaration) = ast::TraitDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Trait,
                    declaration.name_token(),
                    inheritance_of!(declaration),
                );
            }
        }
        SyntaxKind::EnumDeclaration => {
            if let Some(declaration) = ast::EnumDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Enum,
                    declaration.name_token(),
                    inheritance_of!(declaration),
                );
            }
        }
        SyntaxKind::FunctionDeclaration => {
            if let Some(declaration) = ast::FunctionDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Function,
                    declaration.name_token(),
                    Inheritance::NONE,
                );
            }
        }
        SyntaxKind::ConstantDeclaration => {
            if let Some(declaration) = ast::ConstantDeclaration::cast(item.node.clone()) {
                for element in declaration.constant_elements() {
                    push_declaration(
                        tree,
                        item,
                        ast_id,
                        DeclarationKind::Constant,
                        element.name_token(),
                        Inheritance::NONE,
                    );
                }
            }
        }
        SyntaxKind::UseDeclaration => {
            if let Some(declaration) = ast::UseDeclaration::cast(item.node.clone()) {
                let inherited =
                    import_kind_of(declaration.import_type_token()).unwrap_or(ImportKind::Class);
                for clause in declaration.use_clauses() {
                    expand_use_clause(&clause, inherited, "", item, ast_id, tree);
                }
            }
        }
        // namespace declarations carry no projection of their own.
        _ => {}
    }
}

fn push_declaration(
    tree: &mut ItemTree,
    item: &ItemNode,
    ast_id: AstId,
    kind: DeclarationKind,
    name_token: Option<SyntaxToken>,
    inheritance: Inheritance,
) {
    let Some(name_token) = name_token else { return };
    tree.declarations.push(Declaration {
        kind,
        name: name_token.text().to_owned(),
        namespace: item.namespace.clone(),
        ast_id,
        extends: inheritance.extends,
        implements: inheritance.implements,
        trait_uses: inheritance.trait_uses,
    });
}

/// The import kind named by a `function` / `const` token, when present.
fn import_kind_of(token: Option<SyntaxToken>) -> Option<ImportKind> {
    match token?.kind() {
        SyntaxKind::Function => Some(ImportKind::Function),
        SyntaxKind::Const => Some(ImportKind::Constant),
        _ => None,
    }
}

/// Expands one `use` clause: a plain clause becomes one import, a
/// group form recurses with the accumulated prefix. Wreckage without a
/// usable target expands to nothing.
fn expand_use_clause(
    clause: &ast::UseClause,
    inherited: ImportKind,
    prefix: &str,
    item: &ItemNode,
    ast_id: AstId,
    tree: &mut ItemTree,
) {
    let kind = import_kind_of(clause.import_type_token()).unwrap_or(inherited);
    let written = clause.name().map(|name| name.text()).unwrap_or_default();
    let target = join_qualified(prefix, written.trim_start_matches('\\'));
    if let Some(group) = clause.use_group() {
        for inner in group.use_clauses() {
            expand_use_clause(&inner, kind, &target, item, ast_id, tree);
        }
        return;
    }
    if target.is_empty() {
        return;
    }
    let alias = clause
        .alias_token()
        .map(|token| token.text().to_owned())
        .unwrap_or_else(|| last_segment(&target).to_owned());
    tree.imports.push(UseImport {
        kind,
        target,
        alias,
        namespace: item.namespace.clone(),
        ast_id,
    });
}

fn join_qualified(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else if name.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}\\{name}")
    }
}

fn last_segment(target: &str) -> &str {
    target.rsplit('\\').next().unwrap_or(target)
}

/// Every `define('NAME', ...)` in the file, in tree order, method bodies
/// included. Database-free so it can be unit-tested directly, and shared
/// by [`ItemTree::from_root`].
fn defines_in(root: &SyntaxNode) -> Vec<String> {
    let mut defined = Vec::new();
    collect_defines(root, &mut defined);
    defined
}

fn collect_defines(node: &SyntaxNode, defined: &mut Vec<String>) {
    for child in node.children() {
        if let Some(call) = ast::CallExpression::cast(child.clone())
            && is_define_call(&call)
            && let Some(name) = defined_name(&call)
        {
            defined.push(name);
        }
        collect_defines(&child, defined);
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
fn defined_name(call: &ast::CallExpression) -> Option<String> {
    let arguments = call.argument_list()?;
    let argument = name_argument(&arguments)?;
    let name = literal_name(argument.expression()?.syntax())?;
    if name.is_empty() {
        return None;
    }
    Some(name)
}

/// The string a literal argument spells.
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
fn literal_name(syntax: &SyntaxNode) -> Option<String> {
    if let Some(literal) = ast::Literal::cast(syntax.clone()) {
        let token = literal.value_token()?;
        if token.kind() != SyntaxKind::SingleQuotedString {
            return None;
        }
        return single_quoted_value(token.text());
    }
    let string = ast::InterpolatedString::cast(syntax.clone())?;
    let fragment = the_only_fragment(&string)?;
    double_quoted_value(fragment.text())
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
/// string while a define's name is a `String`, so `"\xff"` names a
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
    #![allow(clippy::unwrap_used)]

    use celerrate_source::FileId;

    use super::{DeclarationKind, ItemTree};
    use crate::ast_id::AstId;

    fn tree_of(source: &str) -> ItemTree {
        ItemTree::from_root(FileId::new(0), &celerrate_syntax::parse(source).tree())
    }

    fn declared(source: &str) -> Vec<(DeclarationKind, String, String)> {
        tree_of(source)
            .declarations
            .iter()
            .map(|declaration| {
                (
                    declaration.kind,
                    declaration.name.clone(),
                    declaration.namespace.clone(),
                )
            })
            .collect()
    }

    #[test]
    fn every_declaration_kind_is_projected() {
        assert_eq!(
            declared(
                "<?php\n\
                 class Service {}\n\
                 interface Contract {}\n\
                 trait Helper {}\n\
                 enum Suit {}\n\
                 function greet() {}\n\
                 const LIMIT = 1;\n",
            ),
            vec![
                (DeclarationKind::Class, "Service".to_owned(), String::new()),
                (
                    DeclarationKind::Interface,
                    "Contract".to_owned(),
                    String::new()
                ),
                (DeclarationKind::Trait, "Helper".to_owned(), String::new()),
                (DeclarationKind::Enum, "Suit".to_owned(), String::new()),
                (DeclarationKind::Function, "greet".to_owned(), String::new()),
                (DeclarationKind::Constant, "LIMIT".to_owned(), String::new()),
            ],
        );
    }

    #[test]
    fn a_statement_form_namespace_scopes_everything_after_it() {
        assert_eq!(
            declared(
                "<?php\n\
                 namespace First;\n\
                 function one() {}\n\
                 namespace Second;\n\
                 function two() {}\n",
            ),
            vec![
                (
                    DeclarationKind::Function,
                    "one".to_owned(),
                    "First".to_owned()
                ),
                (
                    DeclarationKind::Function,
                    "two".to_owned(),
                    "Second".to_owned()
                ),
            ],
        );
    }

    #[test]
    fn brace_form_namespaces_scope_their_block_only() {
        assert_eq!(
            declared(
                "<?php\n\
                 namespace Ds { class Vector {} }\n\
                 namespace { function outside() {} }\n",
            ),
            vec![
                (DeclarationKind::Class, "Vector".to_owned(), "Ds".to_owned()),
                (
                    DeclarationKind::Function,
                    "outside".to_owned(),
                    String::new()
                ),
            ],
        );
    }

    #[test]
    fn guarded_and_nested_declarations_are_projected() {
        assert_eq!(
            declared(
                "<?php\n\
                 namespace App;\n\
                 if (!function_exists('greet')) { function greet() {} }\n\
                 function outer() { function inner() {} }\n",
            ),
            vec![
                (
                    DeclarationKind::Function,
                    "greet".to_owned(),
                    "App".to_owned()
                ),
                (
                    DeclarationKind::Function,
                    "outer".to_owned(),
                    "App".to_owned()
                ),
                (
                    DeclarationKind::Function,
                    "inner".to_owned(),
                    "App".to_owned()
                ),
            ],
        );
    }

    #[test]
    fn members_are_not_projected() {
        assert_eq!(
            declared(
                "<?php class A { const B = 1; public $property; public function method() {} }",
            ),
            vec![(DeclarationKind::Class, "A".to_owned(), String::new())],
        );
    }

    #[test]
    fn anonymous_classes_are_not_projected() {
        assert_eq!(
            declared("<?php $instance = new class {}; class Named {}"),
            vec![(DeclarationKind::Class, "Named".to_owned(), String::new())],
        );
    }

    #[test]
    fn member_nodes_consume_numbering_but_project_nothing() {
        // Numbering: class = 0, const member = 1, method = 2, so the
        // constant after the class is item 3 — and the projection still
        // carries exactly the class and the top-level constant.
        let tree = tree_of("<?php class A { const B = 1; function m() {} } const C = 1;");
        let kinds_and_ids: Vec<(DeclarationKind, u32)> = tree
            .declarations
            .iter()
            .map(|declaration| (declaration.kind, declaration.ast_id.index))
            .collect();
        assert_eq!(
            kinds_and_ids,
            vec![(DeclarationKind::Class, 0), (DeclarationKind::Constant, 3)],
        );
    }

    #[test]
    fn a_grouped_constant_declaration_projects_one_entry_per_element() {
        let tree = tree_of("<?php const A = 1, B = 2;");
        let names: Vec<&str> = tree
            .declarations
            .iter()
            .map(|declaration| declaration.name.as_str())
            .collect();
        assert_eq!(names, vec!["A", "B"]);
        // Both elements share the declaration node's identity.
        assert_eq!(
            tree.declarations.first().unwrap().ast_id,
            tree.declarations.last().unwrap().ast_id,
        );
    }

    #[test]
    fn ast_ids_are_the_tree_order_positions_of_the_declaration_nodes() {
        // Numbering: namespace = 0, use = 1, class = 2.
        let tree = ItemTree::from_root(
            FileId::new(7),
            &celerrate_syntax::parse("<?php namespace N; use A; class B {}").tree(),
        );
        assert_eq!(
            tree.declarations
                .first()
                .map(|declaration| declaration.ast_id),
            Some(AstId {
                file: FileId::new(7),
                index: 2,
            }),
        );
    }

    #[test]
    fn original_spelling_is_preserved() {
        // Case folding is the index's concern, never the tree's.
        assert_eq!(
            declared("<?php class MiXeDcAsE {}"),
            vec![(
                DeclarationKind::Class,
                "MiXeDcAsE".to_owned(),
                String::new()
            )],
        );
    }

    #[test]
    fn malformed_input_projects_what_the_parser_recovered() {
        assert_eq!(
            declared("<?php class Broken { function ok() {}"),
            vec![(DeclarationKind::Class, "Broken".to_owned(), String::new())],
        );
    }

    #[test]
    fn a_body_edit_produces_an_identical_item_tree() {
        // The early-cutoff property, at the value level: no ranges, no
        // offsets, so bodies, comments, and whitespace never show up.
        let before = tree_of("<?php function greet() { return 1; } class After {}");
        let body_edit = tree_of("<?php function greet() { return 2; } class After {}");
        let comment_edit =
            tree_of("<?php // note\nfunction greet() { return 1; }   class After {}");
        assert_eq!(before, body_edit);
        assert_eq!(before, comment_edit);
    }

    #[test]
    fn empty_and_html_only_files_project_nothing() {
        assert_eq!(tree_of("").declarations, Vec::new());
        assert_eq!(tree_of("plain text, no PHP").declarations, Vec::new());
    }

    fn only_declaration(source: &str) -> super::Declaration {
        let tree = tree_of(source);
        assert_eq!(tree.declarations.len(), 1, "expected one declaration");
        tree.declarations.into_iter().next().unwrap()
    }

    #[test]
    fn a_class_carries_its_unresolved_inheritance_names() {
        let class = only_declaration(
            "<?php namespace App;\n\
             class Service extends \\Core\\Base implements Contract, \\Psr\\Log\\LoggerAwareInterface {\n\
                 use Concerns\\Loggable;\n\
                 use \\Shared\\Serializable;\n\
             }\n",
        );
        assert_eq!(class.extends, vec!["\\Core\\Base".to_owned()]);
        assert_eq!(
            class.implements,
            vec![
                "Contract".to_owned(),
                "\\Psr\\Log\\LoggerAwareInterface".to_owned(),
            ],
        );
        assert_eq!(
            class.trait_uses,
            vec![
                "Concerns\\Loggable".to_owned(),
                "\\Shared\\Serializable".to_owned(),
            ],
        );
    }

    #[test]
    fn an_interface_extends_many_parents() {
        let interface = only_declaration("<?php interface Both extends First, Second\\Third {}");
        assert_eq!(
            interface.extends,
            vec!["First".to_owned(), "Second\\Third".to_owned()],
        );
        assert_eq!(interface.implements, Vec::<String>::new());
    }

    #[test]
    fn an_enum_carries_its_implements_names() {
        let declaration =
            only_declaration("<?php enum Suit: string implements HasColor { use Colored; }");
        assert_eq!(declaration.implements, vec!["HasColor".to_owned()]);
        assert_eq!(declaration.trait_uses, vec!["Colored".to_owned()]);
    }

    #[test]
    fn a_grouped_trait_use_lists_every_name() {
        let class = only_declaration("<?php class Mixed { use A, B\\C; }");
        assert_eq!(class.trait_uses, vec!["A".to_owned(), "B\\C".to_owned()],);
    }

    #[test]
    fn functions_and_constants_carry_no_inheritance() {
        let function = only_declaration("<?php function greet() {}");
        assert_eq!(function.extends, Vec::<String>::new());
        assert_eq!(function.implements, Vec::<String>::new());
        assert_eq!(function.trait_uses, Vec::<String>::new());
    }

    use super::{ImportKind, UseImport};

    fn imports_of(source: &str) -> Vec<UseImport> {
        tree_of(source).imports
    }

    fn targets_and_aliases(source: &str) -> Vec<(ImportKind, String, String)> {
        imports_of(source)
            .into_iter()
            .map(|import| (import.kind, import.target, import.alias))
            .collect()
    }

    #[test]
    fn a_simple_use_imports_a_class_with_its_last_segment_as_alias() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\Bar;"),
            vec![(ImportKind::Class, "Foo\\Bar".to_owned(), "Bar".to_owned())],
        );
    }

    #[test]
    fn a_leading_backslash_is_trimmed_from_the_target() {
        // Use targets are always absolute; the written backslash adds
        // nothing.
        assert_eq!(
            targets_and_aliases("<?php use \\Foo\\Bar;"),
            vec![(ImportKind::Class, "Foo\\Bar".to_owned(), "Bar".to_owned())],
        );
    }

    #[test]
    fn an_explicit_alias_wins() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\Bar as Baz;"),
            vec![(ImportKind::Class, "Foo\\Bar".to_owned(), "Baz".to_owned())],
        );
    }

    #[test]
    fn function_and_const_declarations_set_the_import_kind() {
        assert_eq!(
            targets_and_aliases("<?php use function Foo\\greet; use const Foo\\LIMIT;"),
            vec![
                (
                    ImportKind::Function,
                    "Foo\\greet".to_owned(),
                    "greet".to_owned()
                ),
                (
                    ImportKind::Constant,
                    "Foo\\LIMIT".to_owned(),
                    "LIMIT".to_owned()
                ),
            ],
        );
    }

    #[test]
    fn a_group_expands_with_the_shared_prefix() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\Bar\\{Baz, Qux\\Deep as D};"),
            vec![
                (
                    ImportKind::Class,
                    "Foo\\Bar\\Baz".to_owned(),
                    "Baz".to_owned()
                ),
                (
                    ImportKind::Class,
                    "Foo\\Bar\\Qux\\Deep".to_owned(),
                    "D".to_owned()
                ),
            ],
        );
    }

    #[test]
    fn a_mixed_group_overrides_the_kind_per_clause() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\{function greet, const LIMIT, Service};",),
            vec![
                (
                    ImportKind::Function,
                    "Foo\\greet".to_owned(),
                    "greet".to_owned()
                ),
                (
                    ImportKind::Constant,
                    "Foo\\LIMIT".to_owned(),
                    "LIMIT".to_owned()
                ),
                (
                    ImportKind::Class,
                    "Foo\\Service".to_owned(),
                    "Service".to_owned()
                ),
            ],
        );
    }

    #[test]
    fn comma_separated_clauses_each_import() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\A, Foo\\B;"),
            vec![
                (ImportKind::Class, "Foo\\A".to_owned(), "A".to_owned()),
                (ImportKind::Class, "Foo\\B".to_owned(), "B".to_owned()),
            ],
        );
    }

    #[test]
    fn imports_carry_their_enclosing_namespace_and_identity() {
        let tree = tree_of("<?php namespace App; use Lib\\Helper;");
        let import = tree.imports.first().unwrap();
        assert_eq!(import.namespace, "App");
        // Numbering: namespace = 0, use declaration = 1.
        assert_eq!(import.ast_id.index, 1);
    }

    // `define()` constants: the names the item traversal cannot see,
    // collected separately and range-free (module doc). Retargeted from
    // the query that used to run this walk directly (former defines.rs).

    fn defines(source: &str) -> Vec<String> {
        tree_of(source).defines
    }

    #[test]
    fn a_top_level_define_is_seen() {
        assert_eq!(defines("<?php define('APP_ROOT', __DIR__);"), ["APP_ROOT"]);
    }

    #[test]
    fn a_define_in_a_method_body_is_seen() {
        // The case the item traversal cannot see, and the reason this
        // walk exists separately from it.
        let source = "<?php class Bootstrap {
            public static function boot(): void {
                define('APP_ROOT', __DIR__);
            }
        }";
        assert_eq!(defines(source), ["APP_ROOT"]);
    }

    #[test]
    fn a_body_edit_that_adds_a_define_changes_the_item_tree() {
        // Unlike declarations, `defines` is not range-free with respect to
        // *presence*: adding a `define()` changes the project's symbols,
        // so the tree must change too, and a define-free body edit must
        // still leave it equal (see the next test).
        let before = tree_of("<?php function boot() { return 1; }");
        let after = tree_of("<?php function boot() { define('APP_ROOT', 1); return 1; }");
        assert_ne!(before, after);
        assert_eq!(after.defines, vec!["APP_ROOT".to_owned()]);
    }

    #[test]
    fn a_define_free_body_edit_produces_an_identical_tree() {
        let before = tree_of("<?php function boot() { define('APP_ROOT', 1); return 1; }");
        let after = tree_of("<?php function boot() { define('APP_ROOT', 1); return 2; }");
        assert_eq!(before, after);
    }

    #[test]
    fn the_callee_is_matched_case_insensitively_in_both_spellings() {
        assert_eq!(defines("<?php \\define('A', 1);"), ["A"]);
        assert_eq!(defines("<?php DEFINE('B', 1);"), ["B"]);
        assert_eq!(defines("<?php \\DeFiNe('C', 1);"), ["C"]);
    }

    #[test]
    fn a_dynamic_name_is_out_of_scope() {
        // The same stance that already excludes `new $class`.
        assert!(defines("<?php define($name, 1);").is_empty());
        assert!(defines("<?php define(\"A$suffix\", 1);").is_empty());
        assert!(defines("<?php define(NAME_OF, 1);").is_empty());
    }

    #[test]
    fn a_name_is_taken_literally_quotes_prefix_and_escapes_removed() {
        assert_eq!(defines("<?php define(b'A', 1);"), ["A"]);
        assert_eq!(defines(r"<?php define('Foo\\Bar', 1);"), [r"Foo\Bar"]);
        assert_eq!(defines(r"<?php define('It\'s', 1);"), ["It's"]);
    }

    #[test]
    fn a_double_quoted_name_with_nothing_interpolated_is_a_literal_name() {
        // The parser builds a `Literal` only for a single-quoted string; a
        // double-quoted one is an `InterpolatedString`. Demanding the
        // first missed `define("APP_ROOT", 1)` entirely, and an unseen
        // `define()` is a false positive, the one direction the policy
        // forbids.
        assert_eq!(defines(r#"<?php define("APP_ROOT", 1);"#), ["APP_ROOT"]);
        assert_eq!(defines(r#"<?php define(b"A", 1);"#), ["A"]);
        assert_eq!(
            defines(r#"<?php define(constant_name: "A", value: 1);"#),
            ["A"],
        );
    }

    #[test]
    fn a_double_quoted_name_honours_the_escapes_a_single_quoted_one_does_not() {
        // `\\`, `\"` and `\$` are escapes in double quotes. Every other
        // backslash PHP does not read as an escape stays literal, which is
        // what makes a qualified name work.
        assert_eq!(defines(r#"<?php define("Foo\\Bar", 1);"#), [r"Foo\Bar"]);
        assert_eq!(
            defines(r#"<?php define("Vendor\Product\LIMIT", 1);"#),
            [r"Vendor\Product\LIMIT"],
        );
        assert_eq!(defines(r#"<?php define("A\"B", 1);"#), [r#"A"B"#]);
        assert_eq!(defines(r#"<?php define("A\$B", 1);"#), ["A$B"]);
    }

    #[test]
    fn a_backslash_that_starts_no_escape_stays_literal_even_before_x_or_u() {
        // `\u` is an escape only before `{`, and `\x` only before a
        // hexadecimal digit. Everywhere else PHP reads both literally, and
        // a lowercase namespace segment, unusual as it is, is legal. Both
        // names below are knowable and representable, so refusing them
        // would be a false positive at every use site.
        assert_eq!(
            defines(r#"<?php define("Acme\utils\VERSION", 1);"#),
            [r"Acme\utils\VERSION"],
        );
        assert_eq!(
            defines(r#"<?php define("Foo\xml\NS", 1);"#),
            [r"Foo\xml\NS"],
        );
    }

    #[test]
    fn a_byte_or_code_point_escape_is_decoded_the_way_php_decodes_it() {
        // `\u{...}` denotes a code point and emits its UTF-8 encoding, so it
        // is always representable. `\x41` and `\101` denote a byte, and an
        // ASCII one is representable too.
        assert_eq!(defines(r#"<?php define("\u{41}PP", 1);"#), ["APP"]);
        assert_eq!(defines(r#"<?php define("\u{e9}TAT", 1);"#), ["éTAT"]);
        assert_eq!(defines(r#"<?php define("\x41PP", 1);"#), ["APP"]);
        assert_eq!(defines(r#"<?php define("\101PP", 1);"#), ["APP"]);
        // Two byte escapes that spell one valid UTF-8 character.
        assert_eq!(defines(r#"<?php define("\xc3\xa9TAT", 1);"#), ["éTAT"]);
    }

    #[test]
    fn a_name_that_is_not_valid_utf8_is_out_of_scope() {
        // The only case left: a PHP constant name is a byte string, and this
        // model holds a UTF-8 `String`. A byte sequence no `String` can hold
        // is not guessed at, the same stance a dynamic `define` takes.
        assert!(defines(r#"<?php define("\xffPP", 1);"#).is_empty());
        assert!(defines(r#"<?php define("\377PP", 1);"#).is_empty());
        // A lone surrogate is a code point, but not a Unicode scalar value.
        assert!(defines(r#"<?php define("\u{d800}PP", 1);"#).is_empty());
    }

    #[test]
    fn a_truncated_escape_terminates_and_stays_literal() {
        // No user input may crash the tool. PHP rejects an unclosed `\u{`
        // outright, so whatever we index for it is inert either way; the
        // backslash stays literal, which is the direction that never drops
        // a real `define()`.
        assert_eq!(defines(r#"<?php define("\u{41", 1);"#), [r"\u{41"]);
        assert_eq!(defines(r#"<?php define("A\u{", 1);"#), [r"A\u{"]);
        // A fragment never ends on a backslash: the lexer takes the escaped
        // character with it, and here that character is the closing quote,
        // which leaves the string unterminated and the name unknown.
        assert!(defines(r#"<?php define("A\", 1);"#).is_empty());
        // The unescaper is total regardless, on any input the lexer could
        // never hand it.
        assert_eq!(super::double_quoted_value("A\\").as_deref(), Some("A\\"));
        assert_eq!(super::double_quoted_value("\\x").as_deref(), Some("\\x"));
    }

    #[test]
    fn an_empty_or_unterminated_double_quoted_name_is_no_name() {
        assert!(defines(r#"<?php define("", 1);"#).is_empty());
        assert!(defines(r#"<?php define("APP_ROOT, 1);"#).is_empty());
    }

    #[test]
    fn a_named_argument_is_read_by_its_label() {
        assert_eq!(
            defines("<?php define(constant_name: 'A', value: 1);"),
            ["A"]
        );
        assert_eq!(
            defines("<?php define(value: 1, constant_name: 'B');"),
            ["B"]
        );
    }

    #[test]
    fn a_method_named_define_is_not_a_define() {
        assert!(defines("<?php $container->define('A', 1);").is_empty());
        assert!(defines("<?php Registry::define('A', 1);").is_empty());
    }
}
