//! Standard-tag extraction over the [`Tag`](crate::Tag) stream
//! produced by the docblock lexer: `@param`, `@return`, `@var`,
//! `@throws` feed [`MemberDocblock`]; `@property` (and its `-read` /
//! `-write` variants) and `@method` feed the virtual-member vocabulary
//! from `celerrate_plugin`. Loss is per construct, never per
//! annotation: one unparseable tag drops, its siblings survive.

use std::collections::HashSet;

use celerrate_plugin::{VirtualMember, VirtualMemberKind, VirtualParameter};

use crate::{Tag, TypeExpression, parse_type_expression_text};

/// The standard tags a single member's docblock contributes:
/// `@param`, `@return`, `@var`, `@throws`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberDocblock {
    pub return_type: Option<TypeExpression>,
    pub value_type: Option<TypeExpression>,
    pub parameters: Vec<(String, TypeExpression)>,
    pub throws: Vec<TypeExpression>,
}

/// Extracts `@param`/`@return`/`@var`/`@throws` from `tags`. Malformed
/// tags are dropped individually; well-formed siblings survive.
pub fn extract_member_docblock(tags: &[Tag]) -> MemberDocblock {
    let mut extracted = MemberDocblock::default();
    let mut return_claimed = false;
    let mut value_claimed = false;
    let mut seen_parameters: HashSet<String> = HashSet::new();
    for tag in tags {
        match tag.name.as_str() {
            "param" => {
                if let Some(parameter) = parse_param_tag(&tag.content, &mut seen_parameters) {
                    extracted.parameters.push(parameter);
                }
            }
            "return" => {
                if !return_claimed {
                    return_claimed = true;
                    extracted.return_type = first_token_type(&tag.content);
                }
            }
            "var" => {
                if !value_claimed {
                    value_claimed = true;
                    extracted.value_type = first_token_type(&tag.content);
                }
            }
            "throws" => {
                if let Some(type_expression) = first_token_type(&tag.content) {
                    extracted.throws.push(type_expression);
                }
            }
            _ => {}
        }
    }
    extracted
}

/// Extracts the virtual members declared by `@property` (and its
/// `-read` / `-write` variants) and `@method` tags.
pub fn extract_virtual_members(tags: &[Tag]) -> Vec<VirtualMember> {
    let mut members = Vec::new();
    for tag in tags {
        match tag.name.as_str() {
            "property" | "property-read" | "property-write" => {
                if let Some(member) = parse_property_tag(&tag.content) {
                    members.push(member);
                }
            }
            "method" => {
                if let Some(member) = parse_method_tag(&tag.content) {
                    members.push(member);
                }
            }
            _ => {}
        }
    }
    members
}

fn first_token_type(content: &str) -> Option<TypeExpression> {
    let first = content.split_whitespace().next()?;
    parse_type_expression_text(first)
}

/// `@param [type] $name ...prose` (or `&$name` / `...$name` when the
/// type is omitted, in which case there is nothing to contribute: the
/// produced tuple has no slot for an untyped parameter). The first
/// tag for a given parameter name wins; later duplicates are dropped.
fn parse_param_tag(content: &str, seen: &mut HashSet<String>) -> Option<(String, TypeExpression)> {
    let mut tokens = content.split_whitespace();
    let first = tokens.next()?;
    if first.starts_with("...$") || first.starts_with("&$") || first.starts_with('$') {
        return None;
    }
    let type_expression = parse_type_expression_text(first)?;
    let variable_token = tokens.next()?;
    let name = strip_variable_sigils(variable_token)?;
    if seen.contains(&name) {
        return None;
    }
    seen.insert(name.clone());
    Some((name, type_expression))
}

/// Strips, in order, a leading `&` (by reference), a leading `...`
/// (variadic), then the mandatory leading `$`. `None` when the
/// remainder is empty or the `$` is missing.
fn strip_variable_sigils(token: &str) -> Option<String> {
    let token = token.strip_prefix('&').unwrap_or(token);
    let token = token.strip_prefix("...").unwrap_or(token);
    let name = token.strip_prefix('$')?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

/// `@property[-read|-write] [type] $name`: a single `$name` token
/// means untyped (the member still exists). `type_text` stores the
/// raw token verbatim: unresolved text is the virtual-symbol
/// contract, so it is not run through the expression parser here.
fn parse_property_tag(content: &str) -> Option<VirtualMember> {
    let tokens: Vec<&str> = content.split_whitespace().collect();
    let (type_text, name_token) = match tokens.as_slice() {
        [] => return None,
        [name_token] => (None, *name_token),
        [type_token, name_token, ..] => (Some((*type_token).to_owned()), *name_token),
    };
    let name = name_token.strip_prefix('$')?;
    if name.is_empty() {
        return None;
    }
    Some(VirtualMember {
        kind: VirtualMemberKind::Property,
        name: name.to_owned(),
        is_static: false,
        type_text,
        parameters: Vec::new(),
    })
}

/// `@method [static] [type] name(parameters)`. No nested parentheses
/// in 4a: a nested `(` or a missing `)` skips the tag. The name must
/// be a valid identifier.
fn parse_method_tag(content: &str) -> Option<VirtualMember> {
    let (before, after_open) = content.split_once('(')?;
    let mut before_tokens: Vec<&str> = before.split_whitespace().collect();
    let name_token = before_tokens.pop()?;
    if !is_valid_identifier(name_token) {
        return None;
    }
    let is_static = before_tokens.contains(&"static");
    let mut leftover = before_tokens.into_iter().filter(|token| *token != "static");
    let type_text = match (leftover.next(), leftover.next()) {
        (None, None) => None,
        (Some(token), None) => Some(token.to_owned()),
        _ => return None,
    };

    let mut parameter_segment = String::new();
    let mut closed = false;
    for character in after_open.chars() {
        if character == '(' {
            return None;
        }
        if character == ')' {
            closed = true;
            break;
        }
        parameter_segment.push(character);
    }
    if !closed {
        return None;
    }

    Some(VirtualMember {
        kind: VirtualMemberKind::Method,
        name: name_token.to_owned(),
        is_static,
        type_text,
        parameters: parse_method_parameters(&parameter_segment),
    })
}

fn is_valid_identifier(token: &str) -> bool {
    let mut characters = token.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn parse_method_parameters(segment: &str) -> Vec<VirtualParameter> {
    if segment.trim().is_empty() {
        return Vec::new();
    }
    segment
        .split(',')
        .filter_map(parse_method_parameter)
        .collect()
}

/// `[type] $name [= default]`; a `=` anywhere in the chunk marks
/// `optional`, a `...$` prefix on the name token marks `variadic`.
fn parse_method_parameter(chunk: &str) -> Option<VirtualParameter> {
    let optional = chunk.contains('=');
    let tokens: Vec<&str> = chunk.split_whitespace().collect();
    let name_index = tokens
        .iter()
        .position(|token| token.starts_with("...$") || token.starts_with('$'))?;
    let name_token = *tokens.get(name_index)?;
    let variadic = name_token.starts_with("...$");
    let name = if variadic {
        name_token.strip_prefix("...$")?
    } else {
        name_token.strip_prefix('$')?
    };
    if name.is_empty() {
        return None;
    }
    let type_text = if name_index == 0 {
        None
    } else {
        tokens.get(name_index - 1).map(|token| (*token).to_owned())
    };
    Some(VirtualParameter {
        name: name.to_owned(),
        type_text,
        optional,
        variadic,
    })
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::lex_docblock;

    #[test]
    fn a_member_docblock_extracts_all_standard_tags() {
        let tags = lex_docblock(
            "/**\n * @param int $id\n * @param ?string $name optional prose\n * @return bool\n * @throws \\RuntimeException\n */",
        );
        let extracted = extract_member_docblock(&tags);
        assert_eq!(extracted.parameters.len(), 2);
        assert_eq!(extracted.parameters[0].0, "id");
        assert_eq!(extracted.parameters[1].0, "name");
        assert!(extracted.return_type.is_some());
        assert_eq!(extracted.throws.len(), 1);
        assert_eq!(extracted.value_type, None);
    }

    #[test]
    fn var_reads_the_value_type_and_first_tag_wins_on_duplicates() {
        let tags = lex_docblock("/** @var int */");
        assert!(extract_member_docblock(&tags).value_type.is_some());
        let tags = lex_docblock("/**\n * @return int\n * @return string\n */");
        assert_eq!(
            extract_member_docblock(&tags).return_type,
            Some(TypeExpression::Name("int".to_owned())),
        );
    }

    #[test]
    fn malformed_tags_are_ignored_per_construct() {
        // The unparseable @param drops; the good one survives; the
        // by-reference and variadic sigils are tolerated.
        let tags = lex_docblock(
            "/**\n * @param array<int> $broken\n * @param int $good\n * @param string &$reference\n * @param int ...$rest\n * @param $untyped\n */",
        );
        let extracted = extract_member_docblock(&tags);
        let names: Vec<&str> = extracted
            .parameters
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, vec!["good", "reference", "rest"]);
    }

    #[test]
    fn property_tags_declare_virtual_properties() {
        let tags = lex_docblock(
            "/**\n * @property string $title\n * @property-read int $id\n * @property-write ?string $slug\n * @property $untyped\n */",
        );
        let members = extract_virtual_members(&tags);
        assert_eq!(members.len(), 4);
        assert!(
            members
                .iter()
                .all(|member| member.kind == VirtualMemberKind::Property)
        );
        assert_eq!(members[0].name, "title");
        assert_eq!(members[0].type_text.as_deref(), Some("string"));
        assert_eq!(members[3].name, "untyped");
        assert_eq!(members[3].type_text, None);
    }

    #[test]
    fn method_tags_declare_virtual_methods() {
        let tags = lex_docblock(
            "/**\n * @method static User find(int $id, ?string $name = null)\n * @method void clear()\n * @method broken(\n */",
        );
        let members = extract_virtual_members(&tags);
        assert_eq!(members.len(), 2);
        let find = &members[0];
        assert_eq!(find.name, "find");
        assert!(find.is_static);
        assert_eq!(find.type_text.as_deref(), Some("User"));
        assert_eq!(find.parameters.len(), 2);
        assert_eq!(find.parameters[0].name, "id");
        assert!(!find.parameters[0].optional);
        assert_eq!(find.parameters[1].name, "name");
        assert!(find.parameters[1].optional);
        let clear = &members[1];
        assert_eq!(clear.name, "clear");
        assert!(!clear.is_static);
        assert_eq!(clear.type_text.as_deref(), Some("void"));
        assert!(clear.parameters.is_empty());
    }
}
