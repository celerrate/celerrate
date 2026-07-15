//! Standard-tag extraction over the [`Tag`](crate::Tag) stream
//! produced by the docblock lexer: `@param`, `@return`, `@var`,
//! `@throws` feed [`MemberDocblock`]; `@property` (and its `-read` /
//! `-write` variants) and `@method` feed the virtual-member vocabulary
//! from `celerrate_plugin`. Tag contents parse a maximal type-expression
//! prefix; trailing prose is free text. Loss is per construct, never per
//! annotation: one unparseable tag drops, its siblings survive.

use std::collections::HashSet;

use celerrate_plugin::{VirtualMember, VirtualMemberKind, VirtualParameter};

use crate::{Tag, TypeExpression, parse_type_expression_prefix};

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
/// tags are dropped individually; well-formed siblings survive: an
/// unparseable `@return`/`@var` consumes nothing, so the slot takes
/// the first parseable tag.
pub fn extract_member_docblock(tags: &[Tag]) -> MemberDocblock {
    let mut extracted = MemberDocblock::default();
    let mut seen_parameters: HashSet<String> = HashSet::new();
    for tag in tags {
        match tag.name.as_str() {
            "param" => {
                if let Some(parameter) = parse_param_tag(&tag.content, &mut seen_parameters) {
                    extracted.parameters.push(parameter);
                }
            }
            "return" => {
                if extracted.return_type.is_none() {
                    extracted.return_type = value_type(&tag.content);
                }
            }
            "var" => {
                if extracted.value_type.is_none() {
                    extracted.value_type = value_type(&tag.content);
                }
            }
            "throws" => {
                if let Some(type_expression) = value_type(&tag.content) {
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

/// The tag's value slot: a maximal type-expression prefix; trailing
/// prose is free text.
fn value_type(content: &str) -> Option<TypeExpression> {
    let (expression, _) = parse_type_expression_prefix(content)?;
    Some(expression)
}

/// `@param type $name ...prose` (or `&$name` / `...$name` when the
/// type is omitted, in which case there is nothing to contribute).
/// The first tag for a given parameter name wins; later duplicates
/// are dropped.
fn parse_param_tag(content: &str, seen: &mut HashSet<String>) -> Option<(String, TypeExpression)> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('$') || trimmed.starts_with("...$") || trimmed.starts_with("&$") {
        return None;
    }
    let (type_expression, consumed) = parse_type_expression_prefix(content)?;
    let remainder = content.get(consumed..)?;
    let variable_token = remainder.split_whitespace().next()?;
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

/// `@property[-read|-write] [type] $name`: a leading `$name` means
/// untyped (the member still exists). `type_text` stores the consumed
/// prefix verbatim: unresolved text is the virtual-symbol contract.
fn parse_property_tag(content: &str) -> Option<VirtualMember> {
    let first_word = content.split_whitespace().next()?;
    if let Some(name) = first_word.strip_prefix('$') {
        if name.is_empty() {
            return None;
        }
        return Some(VirtualMember {
            kind: VirtualMemberKind::Property,
            name: name.to_owned(),
            is_static: false,
            type_text: None,
            parameters: Vec::new(),
        });
    }
    let (_, consumed) = parse_type_expression_prefix(content)?;
    let type_text = content.get(..consumed)?.trim().to_owned();
    let remainder = content.get(consumed..)?;
    let name = remainder.split_whitespace().next()?.strip_prefix('$')?;
    if name.is_empty() {
        return None;
    }
    Some(VirtualMember {
        kind: VirtualMemberKind::Property,
        name: name.to_owned(),
        is_static: false,
        type_text: Some(type_text),
        parameters: Vec::new(),
    })
}

/// `@method [static] [type] name(parameters)`. The return type is a
/// dialect prefix taken verbatim; when the prefix turns out to be the
/// method name itself (the next character is `(`), the method is
/// untyped. The parameter segment ends at the matching parenthesis,
/// so callable parameters nest.
fn parse_method_tag(content: &str) -> Option<VirtualMember> {
    let trimmed = content.trim_start();
    let (is_static, after_static) = match trimmed.strip_prefix("static") {
        Some(rest) if rest.starts_with(char::is_whitespace) => (true, rest.trim_start()),
        _ => (false, trimmed),
    };
    let (type_text, rest) = match parse_type_expression_prefix(after_static) {
        Some((_, consumed)) => {
            let text = after_static.get(..consumed)?.trim();
            let after_type = after_static.get(consumed..)?.trim_start();
            if after_type.starts_with('(') {
                (None, after_static)
            } else {
                (Some(text.to_owned()), after_type)
            }
        }
        None => (None, after_static),
    };
    let open = rest.find('(')?;
    let name = rest.get(..open)?.trim();
    if !is_valid_identifier(name) {
        return None;
    }
    let after_open = rest.get(open + 1..)?;
    let (parameter_segment, _) = split_at_matching_parenthesis(after_open)?;
    Some(VirtualMember {
        kind: VirtualMemberKind::Method,
        name: name.to_owned(),
        is_static,
        type_text,
        parameters: parse_method_parameters(parameter_segment),
    })
}

/// Splits `text` at the parenthesis matching an already-consumed `(`:
/// the segment before it, and the remainder after it. `None` when
/// unbalanced.
fn split_at_matching_parenthesis(text: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    for (offset, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return Some((text.get(..offset)?, text.get(offset + 1..)?));
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

fn parse_method_parameters(segment: &str) -> Vec<VirtualParameter> {
    split_top_level_commas(segment)
        .into_iter()
        .filter_map(parse_method_parameter)
        .collect()
}

/// Top-level comma split, depth-aware across `()<>{}[]`, so callable
/// signatures, generics, shapes, and array defaults ride inside one
/// parameter chunk.
fn split_top_level_commas(segment: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut depth = 0i64;
    let mut start = 0usize;
    for (offset, character) in segment.char_indices() {
        match character {
            '(' | '<' | '{' | '[' => depth += 1,
            ')' | '>' | '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                if let Some(chunk) = segment.get(start..offset) {
                    chunks.push(chunk);
                }
                start = offset + 1;
            }
            _ => {}
        }
    }
    if let Some(chunk) = segment.get(start..) {
        chunks.push(chunk);
    }
    chunks.retain(|chunk| !chunk.trim().is_empty());
    chunks
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

/// `[type] $name [= default]`: the type is a dialect prefix taken
/// verbatim; a `=` after the name marks `optional`, a `...$` prefix
/// marks `variadic`. A by-reference `&$name` drops, as in 4a.
fn parse_method_parameter(chunk: &str) -> Option<VirtualParameter> {
    let trimmed = chunk.trim();
    let (type_text, rest) = if trimmed.starts_with('$') || trimmed.starts_with("...$") {
        (None, trimmed)
    } else {
        let (_, consumed) = parse_type_expression_prefix(trimmed)?;
        let text = trimmed.get(..consumed)?.trim();
        let rest = trimmed.get(consumed..)?.trim_start();
        (Some(text.to_owned()), rest)
    };
    let optional = rest.contains('=');
    let name_token = rest.split_whitespace().next()?;
    let variadic = name_token.starts_with("...$");
    let name = if variadic {
        name_token.strip_prefix("...$")?
    } else {
        name_token.strip_prefix('$')?
    };
    // A space-less default (`$x=5`) rides on the name token: the name
    // stops at the first `=`.
    let name = name.split_once('=').map_or(name, |(head, _)| head).trim();
    if name.is_empty() {
        return None;
    }
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
    fn the_value_slot_takes_the_first_parseable_tag() {
        // An unparseable first @return must not suppress a later
        // parseable one: loss is per construct, never cross construct.
        let tags = lex_docblock("/**\n * @return array{\n * @return string\n */");
        assert_eq!(
            extract_member_docblock(&tags).return_type,
            Some(TypeExpression::Name("string".to_owned())),
        );
    }

    #[test]
    fn method_parameter_names_stop_at_the_default() {
        // A space-less default (`$x=5`) must not pollute the name.
        let tags = lex_docblock("/** @method void go(int $x=5, string $y = 'a') */");
        let members = extract_virtual_members(&tags);
        assert_eq!(members.len(), 1);
        let parameters = &members[0].parameters;
        assert_eq!(parameters.len(), 2);
        assert_eq!(parameters[0].name, "x");
        assert!(parameters[0].optional);
        assert_eq!(parameters[1].name, "y");
        assert!(parameters[1].optional);
    }

    #[test]
    fn malformed_tags_are_ignored_per_construct() {
        // The unparseable @param drops; the good one survives; the
        // by-reference and variadic sigils are tolerated.
        let tags = lex_docblock(
            "/**\n * @param array{ $broken\n * @param int $good\n * @param string &$reference\n * @param int ...$rest\n * @param $untyped\n */",
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
    fn dialect_types_with_spaces_extract() {
        let tags = lex_docblock(
            "/**\n * @param array{id: int, name?: string} $subject\n * @return array<int, string> the rows\n * @var int<1, max>\n * @throws \\RuntimeException\n */",
        );
        let extracted = extract_member_docblock(&tags);
        assert_eq!(extracted.parameters.len(), 1);
        assert_eq!(extracted.parameters[0].0, "subject");
        assert!(matches!(
            extracted.parameters[0].1,
            TypeExpression::Shape { .. }
        ));
        assert!(matches!(
            extracted.return_type,
            Some(TypeExpression::Generic { .. })
        ));
        assert!(matches!(
            extracted.value_type,
            Some(TypeExpression::Generic { .. })
        ));
        assert_eq!(extracted.throws.len(), 1);
    }

    #[test]
    fn method_tags_carry_dialect_types_and_nested_parentheses() {
        let tags = lex_docblock(
            "/** @method static Collection<User> map(callable(User): string $mapper, array{limit?: int} $options = []) */",
        );
        let members = extract_virtual_members(&tags);
        assert_eq!(members.len(), 1);
        let map = &members[0];
        assert!(map.is_static);
        assert_eq!(map.type_text.as_deref(), Some("Collection<User>"));
        assert_eq!(map.parameters.len(), 2);
        assert_eq!(map.parameters[0].name, "mapper");
        assert_eq!(
            map.parameters[0].type_text.as_deref(),
            Some("callable(User): string"),
        );
        assert_eq!(map.parameters[1].name, "options");
        assert!(map.parameters[1].optional);
        assert_eq!(
            map.parameters[1].type_text.as_deref(),
            Some("array{limit?: int}"),
        );
    }

    #[test]
    fn property_tags_carry_dialect_types_verbatim() {
        let tags = lex_docblock("/** @property array{id: int} $row */");
        let members = extract_virtual_members(&tags);
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "row");
        assert_eq!(members[0].type_text.as_deref(), Some("array{id: int}"));
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

    #[test]
    fn a_method_tag_can_return_the_static_type() {
        // Only a LEADING "static" token is the staticness modifier; a
        // second "static" token is the return type, spelled the same
        // as the modifier.
        let tags = lex_docblock("/** @method static static create() */");
        let members = extract_virtual_members(&tags);
        assert_eq!(members.len(), 1);
        let create = &members[0];
        assert_eq!(create.name, "create");
        assert!(create.is_static);
        assert_eq!(create.type_text.as_deref(), Some("static"));
    }
}
