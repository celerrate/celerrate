//! Standard-tag extraction over the [`Tag`](crate::Tag) stream
//! produced by the docblock lexer: `@param`, `@return`, `@var`,
//! `@throws` feed [`MemberDocblock`]; `@property` (and its `-read` /
//! `-write` variants) and `@method` feed the virtual-member vocabulary
//! from `celerrate_plugin`. Tag contents parse a maximal type-expression
//! prefix; trailing prose is free text. Loss is per construct, never per
//! annotation: one unparseable tag drops, its siblings survive. Dialect
//! classification and tier-aware slot resolution provide inter-dialect
//! precedence: PHPStan-prefixed over Psalm-prefixed over bare, within a
//! tier first parseable wins; `@throws` accumulates.

use celerrate_plugin::{VirtualMember, VirtualMemberKind, VirtualParameter};

use crate::dialect::{self, TagRole, TagTier};
use crate::{Tag, TypeExpression, parse_type_expression_prefix};

/// The standard tags a single member's docblock contributes:
/// `@param`, `@return`, `@var`, `@throws`, `@template`, `@assert` family,
/// plus the inheritance-position tags (`@extends`, `@implements`,
/// `@use`) and named inline `@var` entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberDocblock {
    pub return_type: Option<TypeExpression>,
    pub value_type: Option<TypeExpression>,
    pub parameters: Vec<(String, TypeExpression)>,
    pub throws: Vec<TypeExpression>,
    pub templates: Vec<TemplateDeclaration>,
    pub assertions: Vec<AssertionDeclaration>,
    /// `@extends`, `@implements`, `@use` and their `@template-*` long
    /// forms, slot-resolved per written head name.
    pub ancestors: Vec<AncestorDeclaration>,
    /// Named inline `@var Type $name` entries, slot-resolved per
    /// variable name. Tag extraction is context-free — it cannot know
    /// whether the docblock sits above a property or above a local
    /// statement — so a named entry ALSO fills `value_type` under the
    /// tier rule, exactly as the unnamed `@var` form does; a property's
    /// declaring site (`declared.rs`) reads `value_type`, while the
    /// later inline-narrowing consumer reads `variables`.
    pub variable_values: Vec<(String, TypeExpression)>,
}

/// One inheritance-position tag (`@extends`, `@implements`, `@use`
/// and their `@template-*` long forms): the written ancestor with its
/// fixed generic arguments, an unresolved type expression until the
/// declaring site qualifies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AncestorDeclaration {
    pub expression: TypeExpression,
}

/// One `@template` declaration: `T`, `T of Bound`, `T as Bound`
/// (the Psalm keyword is a synonym). A `= Default` tail and the
/// variance of `-covariant`/`-contravariant` variants are dropped
/// (recorded debt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateDeclaration {
    pub name: String,
    pub bound: Option<TypeExpression>,
}

/// One assertion tag (`@psalm-assert`, `@phpstan-assert` family):
/// unresolved type expression pending the narrowing consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionDeclaration {
    pub subject: String,
    pub asserted: TypeExpression,
    pub polarity: celerrate_plugin::AssertionPolarity,
    pub negated: bool,
}

/// Extracts the member slots under the tier rule:
/// PHPStan-prefixed over Psalm-prefixed over bare; within a tier the
/// first parseable tag wins; `@param` resolves per parameter name;
/// `@throws` and `@assert` family accumulate across tiers; `@template`
/// resolves per declared name under the same tier rule.
pub fn extract_member_docblock(tags: &[Tag]) -> MemberDocblock {
    let mut return_slot: Option<(TagTier, TypeExpression)> = None;
    let mut value_slot: Option<(TagTier, TypeExpression)> = None;
    let mut parameters: Vec<(String, TagTier, TypeExpression)> = Vec::new();
    let mut throws = Vec::new();
    let mut templates: Vec<(String, TagTier, TemplateDeclaration)> = Vec::new();
    let mut extracted_assertions = Vec::new();
    let mut ancestors: Vec<(String, TagTier, AncestorDeclaration)> = Vec::new();
    let mut variable_values: Vec<(String, TagTier, TypeExpression)> = Vec::new();
    for tag in tags {
        let Some(classified) = dialect::classify(&tag.name) else {
            continue;
        };
        match classified.role {
            TagRole::Return => offer_value(&mut return_slot, classified.tier, &tag.content),
            TagRole::Var => offer_var(
                &mut value_slot,
                &mut variable_values,
                classified.tier,
                &tag.content,
            ),
            TagRole::Param => offer_parameter(&mut parameters, classified.tier, &tag.content),
            TagRole::Throws => {
                if let Some(expression) = value_type(&tag.content) {
                    throws.push(expression);
                }
            }
            TagRole::Template => offer_template(&mut templates, classified.tier, &tag.content),
            TagRole::Assert(polarity) => {
                if let Some(assertion) = parse_assert_tag(&tag.content, polarity) {
                    extracted_assertions.push(assertion);
                }
            }
            TagRole::Extends | TagRole::Implements | TagRole::UseTrait => {
                offer_ancestor(&mut ancestors, classified.tier, &tag.content)
            }
            TagRole::Property | TagRole::Method | TagRole::Ignored => {}
        }
    }
    MemberDocblock {
        return_type: return_slot.map(|(_, expression)| expression),
        value_type: value_slot.map(|(_, expression)| expression),
        parameters: parameters
            .into_iter()
            .map(|(name, _, expression)| (name, expression))
            .collect(),
        throws,
        templates: templates
            .into_iter()
            .map(|(_, _, declaration)| declaration)
            .collect(),
        assertions: extracted_assertions,
        ancestors: ancestors
            .into_iter()
            .map(|(_, _, declaration)| declaration)
            .collect(),
        variable_values: variable_values
            .into_iter()
            .map(|(name, _, expression)| (name, expression))
            .collect(),
    }
}

/// A stronger tier replaces; the same or a weaker tier keeps the
/// holder (first parseable within a tier). An unparseable candidate
/// never touches the slot.
fn offer_value(slot: &mut Option<(TagTier, TypeExpression)>, tier: TagTier, content: &str) {
    if matches!(slot, Some((existing, _)) if *existing <= tier) {
        return;
    }
    if let Some(expression) = value_type(content) {
        *slot = Some((tier, expression));
    }
}

/// Per-key slots in first-appearance order, so the output stays
/// deterministic without a map: a stronger tier replaces the holder;
/// the same or a weaker tier keeps the first declaration within that
/// tier. Shared by every extraction slot resolved per key —
/// `offer_parameter`, `offer_template`, `offer_named_variable`
/// (per name), `offer_ancestor` (per written head name).
fn offer_keyed<T>(slots: &mut Vec<(String, TagTier, T)>, tier: TagTier, key: String, value: T) {
    match slots.iter_mut().find(|(existing, _, _)| *existing == key) {
        Some((_, existing_tier, existing_value)) => {
            if tier < *existing_tier {
                *existing_tier = tier;
                *existing_value = value;
            }
        }
        None => slots.push((key, tier, value)),
    }
}

fn offer_parameter(
    parameters: &mut Vec<(String, TagTier, TypeExpression)>,
    tier: TagTier,
    content: &str,
) {
    let Some((name, expression)) = parse_param_tag(content) else {
        return;
    };
    offer_keyed(parameters, tier, name, expression);
}

fn offer_template(
    templates: &mut Vec<(String, TagTier, TemplateDeclaration)>,
    tier: TagTier,
    content: &str,
) {
    let Some(declaration) = parse_template_tag(content) else {
        return;
    };
    offer_keyed(templates, tier, declaration.name.clone(), declaration);
}

/// `@var Type [$name]`: every tag feeds the `value_type` slot under
/// the tier rule (`offer_value`), same as an unnamed `@var` always
/// has; a named tag ADDITIONALLY routes to `variable_values`,
/// slot-resolved per variable name under the same tier rule. Tag
/// extraction cannot see whether its docblock sits above a property or
/// a local statement, so both slots are populated and the consuming
/// site picks (adjudicated: a named `@var` above a property must keep
/// typing the property).
fn offer_var(
    value_slot: &mut Option<(TagTier, TypeExpression)>,
    variable_values: &mut Vec<(String, TagTier, TypeExpression)>,
    tier: TagTier,
    content: &str,
) {
    if let Some((name, expression)) = parse_named_var(content) {
        offer_named_variable(variable_values, tier, name, expression);
    }
    offer_value(value_slot, tier, content);
}

/// `Type $name`: `None` when the tag has no trailing variable (the
/// unnamed `@var` form, which stays as `value_type`).
fn parse_named_var(content: &str) -> Option<(String, TypeExpression)> {
    let (expression, consumed) = parse_type_expression_prefix(content)?;
    let remainder = content.get(consumed..)?;
    let token = remainder.split_whitespace().next()?;
    let name = token.strip_prefix('$')?;
    if name.is_empty() {
        None
    } else {
        Some((name.to_owned(), expression))
    }
}

fn offer_named_variable(
    variable_values: &mut Vec<(String, TagTier, TypeExpression)>,
    tier: TagTier,
    name: String,
    expression: TypeExpression,
) {
    offer_keyed(variable_values, tier, name, expression);
}

/// The written head name is the first identifier token of the parsed
/// expression; a parsed shape with no such identifier (never produced
/// by the ancestor grammar in practice) drops, same as an unparseable
/// tag.
fn offer_ancestor(
    ancestors: &mut Vec<(String, TagTier, AncestorDeclaration)>,
    tier: TagTier,
    content: &str,
) {
    let Some(declaration) = parse_ancestor_tag(content) else {
        return;
    };
    let Some(head) = ancestor_head_name(&declaration.expression) else {
        return;
    };
    offer_keyed(ancestors, tier, head, declaration);
}

/// `Ancestor[<Arguments>]`: a maximal type-expression prefix; trailing
/// prose (rare for this tag family) is free text.
fn parse_ancestor_tag(content: &str) -> Option<AncestorDeclaration> {
    let (expression, _) = parse_type_expression_prefix(content)?;
    Some(AncestorDeclaration { expression })
}

/// The written head name of an ancestor declaration: the base
/// identifier of a plain name or a generic expression.
fn ancestor_head_name(expression: &TypeExpression) -> Option<String> {
    match expression {
        TypeExpression::Name(name) => Some(name.clone()),
        TypeExpression::Generic { base, .. } => Some(base.clone()),
        _ => None,
    }
}

/// `@template T [of|as Bound] [= Default]`.
fn parse_template_tag(content: &str) -> Option<TemplateDeclaration> {
    let trimmed = content.trim_start();
    let name_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let name = trimmed.get(..name_end)?;
    if !is_valid_identifier(name) {
        return None;
    }
    let rest = trimmed.get(name_end..)?.trim_start();
    let bound = match rest.strip_prefix("of").or_else(|| rest.strip_prefix("as")) {
        Some(after_keyword) if after_keyword.starts_with(char::is_whitespace) => {
            let (expression, _) = parse_type_expression_prefix(after_keyword)?;
            Some(expression)
        }
        _ => None,
    };
    Some(TemplateDeclaration {
        name: name.to_owned(),
        bound,
    })
}

/// `[!]Type $subject`: the negation applies to the asserted type; the
/// subject travels verbatim. A `=`-prefixed content is Psalm's
/// exact-assertion divergence: ignored without error.
fn parse_assert_tag(
    content: &str,
    polarity: celerrate_plugin::AssertionPolarity,
) -> Option<AssertionDeclaration> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('=') {
        return None;
    }
    let (negated, rest) = match trimmed.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    let (asserted, consumed) = parse_type_expression_prefix(rest)?;
    let remainder = rest.get(consumed..)?;
    let subject = remainder.split_whitespace().next()?;
    if !subject.starts_with('$') {
        return None;
    }
    Some(AssertionDeclaration {
        subject: subject.to_owned(),
        asserted,
        polarity,
        negated,
    })
}

/// Virtual members under the same tier rule, resolved per
/// `(kind, name)`; the first declaration wins within a tier.
pub fn extract_virtual_members(tags: &[Tag]) -> Vec<VirtualMember> {
    let mut members: Vec<(TagTier, VirtualMember)> = Vec::new();
    for tag in tags {
        let Some(classified) = dialect::classify(&tag.name) else {
            continue;
        };
        let parsed = match classified.role {
            TagRole::Property => parse_property_tag(&tag.content),
            TagRole::Method => parse_method_tag(&tag.content),
            _ => None,
        };
        let Some(member) = parsed else {
            continue;
        };
        match members
            .iter_mut()
            .find(|(_, existing)| existing.kind == member.kind && existing.name == member.name)
        {
            Some((existing_tier, existing)) => {
                if classified.tier < *existing_tier {
                    *existing_tier = classified.tier;
                    *existing = member;
                }
            }
            None => members.push((classified.tier, member)),
        }
    }
    members.into_iter().map(|(_, member)| member).collect()
}

/// The tag's value slot: a maximal type-expression prefix; trailing
/// prose is free text.
fn value_type(content: &str) -> Option<TypeExpression> {
    let (expression, _) = parse_type_expression_prefix(content)?;
    Some(expression)
}

/// `@param type $name ...prose` (or `&$name` / `...$name` when the
/// type is omitted, in which case there is nothing to contribute).
fn parse_param_tag(content: &str) -> Option<(String, TypeExpression)> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('$') || trimmed.starts_with("...$") || trimmed.starts_with("&$") {
        return None;
    }
    let (type_expression, consumed) = parse_type_expression_prefix(content)?;
    let remainder = content.get(consumed..)?;
    let variable_token = remainder.split_whitespace().next()?;
    let name = strip_variable_sigils(variable_token)?;
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
/// untyped (the member still exists), UNLESS that leading word is
/// exactly `$this` — `$this` is a valid type (`TypeExpression::This`),
/// not the property's own variable, so it falls through to the typed
/// path below. `type_text` stores the consumed prefix verbatim:
/// unresolved text is the virtual-symbol contract.
fn parse_property_tag(content: &str) -> Option<VirtualMember> {
    let first_word = content.split_whitespace().next()?;
    if first_word != "$this"
        && let Some(name) = first_word.strip_prefix('$')
    {
        if name.is_empty() {
            return None;
        }
        return Some(VirtualMember::new(
            VirtualMemberKind::Property,
            name.to_owned(),
        ));
    }
    let (_, consumed) = parse_type_expression_prefix(content)?;
    let type_text = content.get(..consumed)?.trim().to_owned();
    let remainder = content.get(consumed..)?;
    let name = remainder.split_whitespace().next()?.strip_prefix('$')?;
    if name.is_empty() {
        return None;
    }
    let mut member = VirtualMember::new(VirtualMemberKind::Property, name.to_owned());
    member.type_text = Some(type_text);
    Some(member)
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
    let mut member = VirtualMember::new(VirtualMemberKind::Method, name.to_owned());
    member.is_static = is_static;
    member.type_text = type_text;
    member.parameters = parse_method_parameters(parameter_segment);
    Some(member)
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
/// parameter chunk. A `>` that is part of `=>` or `->` (an array
/// default's arrow, e.g. `['a' => 1]`) is not a closing angle
/// bracket: it must not decrement `depth`, or a comma inside the
/// default would wrongly read as top-level and split the parameter.
fn split_top_level_commas(segment: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut depth = 0i64;
    let mut start = 0usize;
    let mut previous_character: Option<char> = None;
    for (offset, character) in segment.char_indices() {
        match character {
            '(' | '<' | '{' | '[' => depth += 1,
            '>' if matches!(previous_character, Some('=' | '-')) => {}
            ')' | '>' | '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                if let Some(chunk) = segment.get(start..offset) {
                    chunks.push(chunk);
                }
                start = offset + 1;
            }
            _ => {}
        }
        previous_character = Some(character);
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
    let mut parameter = VirtualParameter::new(name.to_owned());
    parameter.type_text = type_text;
    parameter.optional = optional;
    parameter.variadic = variadic;
    Some(parameter)
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::panic)]
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

    #[test]
    fn tool_prefixed_tags_win_over_bare_regardless_of_order() {
        let tags = lex_docblock(
            "/**\n * @return string\n * @psalm-return bool\n * @phpstan-return int\n */",
        );
        assert_eq!(
            extract_member_docblock(&tags).return_type,
            Some(TypeExpression::Name("int".to_owned())),
        );
        // Without a PHPStan-prefixed tag, the Psalm synonym beats bare.
        let tags = lex_docblock("/**\n * @psalm-return bool\n * @return string\n */");
        assert_eq!(
            extract_member_docblock(&tags).return_type,
            Some(TypeExpression::Name("bool".to_owned())),
        );
    }

    #[test]
    fn an_unparseable_prefixed_tag_never_clears_a_parseable_bare_one() {
        let tags = lex_docblock("/**\n * @phpstan-return array{\n * @return string\n */");
        assert_eq!(
            extract_member_docblock(&tags).return_type,
            Some(TypeExpression::Name("string".to_owned())),
        );
    }

    #[test]
    fn param_precedence_resolves_per_parameter_name() {
        let tags = lex_docblock(
            "/**\n * @param string $a\n * @param string $b\n * @phpstan-param int $a\n */",
        );
        let extracted = extract_member_docblock(&tags);
        assert_eq!(extracted.parameters.len(), 2);
        assert_eq!(
            extracted.parameters[0],
            ("a".to_owned(), TypeExpression::Name("int".to_owned())),
        );
        assert_eq!(
            extracted.parameters[1],
            ("b".to_owned(), TypeExpression::Name("string".to_owned())),
        );
    }

    #[test]
    fn psalm_synonyms_and_virtual_member_prefixes_extract() {
        let tags = lex_docblock("/** @psalm-var non-empty-string */");
        assert!(extract_member_docblock(&tags).value_type.is_some());
        let tags = lex_docblock("/** @psalm-property string $title */");
        let members = extract_virtual_members(&tags);
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "title");
    }

    #[test]
    fn the_ignored_divergent_bucket_contributes_nothing_and_disturbs_nothing() {
        // The enumerated bucket: parsed, ignored
        // without error, siblings survive.
        let tags = lex_docblock(
            "/**\n * @psalm-pure\n * @psalm-mutation-free\n * @psalm-taint-sink html $output\n * @psalm-taint-source input\n * @psalm-if-this-is Foo\n * @phpstan-pure\n * @return int\n */",
        );
        let extracted = extract_member_docblock(&tags);
        assert_eq!(
            extracted.return_type,
            Some(TypeExpression::Name("int".to_owned())),
        );
        assert!(extracted.parameters.is_empty());
        assert!(extracted.throws.is_empty());
    }

    #[test]
    fn arrow_defaults_do_not_break_the_parameter_split() {
        let tags = lex_docblock("/** @method void go(array $x = ['a' => 1], int $y) */");
        let members = extract_virtual_members(&tags);
        assert_eq!(members.len(), 1);
        let names: Vec<&str> = members[0]
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        assert_eq!(names, vec!["x", "y"]);
    }

    #[test]
    fn a_this_typed_property_keeps_this_as_its_type() {
        let tags = lex_docblock("/** @property $this $owner */");
        let members = extract_virtual_members(&tags);
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "owner");
        assert_eq!(members[0].type_text.as_deref(), Some("$this"));
    }

    #[test]
    fn extends_implements_and_use_extract_with_their_long_forms() {
        let tags = lex_docblock(
            "/**\n * @extends Base<User>\n * @implements Countable<int>\n * @template-use Loggable<string>\n */",
        );
        let extracted = extract_member_docblock(&tags);
        assert_eq!(extracted.ancestors.len(), 3);
        let heads: Vec<&str> = extracted
            .ancestors
            .iter()
            .map(|ancestor| match &ancestor.expression {
                TypeExpression::Generic { base, .. } => base.as_str(),
                other => panic!("expected a generic ancestor, got {other:?}"),
            })
            .collect();
        assert_eq!(heads, vec!["Base", "Countable", "Loggable"]);
        assert!(extracted.ancestors.iter().all(|ancestor| matches!(
            &ancestor.expression,
            TypeExpression::Generic { arguments, .. } if arguments.len() == 1
        )));
    }

    #[test]
    fn a_tool_prefixed_ancestor_tag_wins_its_bare_sibling() {
        let tags =
            lex_docblock("/**\n * @extends Base<User>\n * @phpstan-extends Base<Admin>\n */");
        let extracted = extract_member_docblock(&tags);
        assert_eq!(
            extracted.ancestors.len(),
            1,
            "PHPStan tier wins per head name"
        );
        match &extracted.ancestors[0].expression {
            TypeExpression::Generic { base, arguments } => {
                assert_eq!(base, "Base");
                assert_eq!(arguments, &vec![TypeExpression::Name("Admin".to_owned())]);
            }
            other => panic!("expected a generic ancestor, got {other:?}"),
        }
    }

    #[test]
    fn a_named_inline_var_fills_both_variable_values_and_value_type() {
        // Settled: tag extraction cannot
        // know whether its docblock sits above a property or a local
        // statement, so a named `@var Type $name` must fill BOTH slots
        // — `variable_values` for the later inline-narrowing consumer,
        // and `value_type` so a property's declaring site
        // (`declared.rs`) keeps its declared type instead of falling
        // back to `mixed`.
        let named = extract_member_docblock(&lex_docblock("/** @var Collection<User> $items */"));
        assert_eq!(named.variable_values.len(), 1);
        assert_eq!(named.variable_values[0].0, "items");
        assert!(
            named.value_type.is_some(),
            "the named form also fills the value_type slot"
        );

        let unnamed = extract_member_docblock(&lex_docblock("/** @var Collection<User> */"));
        assert!(unnamed.value_type.is_some());
        assert!(unnamed.variable_values.is_empty());
    }

    #[test]
    fn an_unparseable_ancestor_drops_and_its_siblings_survive() {
        let tags = lex_docblock("/**\n * @extends <<<broken\n * @implements Countable<int>\n */");
        let extracted = extract_member_docblock(&tags);
        assert_eq!(extracted.ancestors.len(), 1, "loss is per construct");
    }

    #[test]
    fn assertion_tags_extract_with_polarity_and_negation() {
        use celerrate_plugin::AssertionPolarity;
        let tags = lex_docblock(
            "/**\n * @psalm-assert string $value\n * @phpstan-assert-if-true !null $user\n * @psalm-assert =string $exact\n */",
        );
        let extracted = extract_member_docblock(&tags);
        assert_eq!(extracted.assertions.len(), 2);
        let first = &extracted.assertions[0];
        assert_eq!(first.subject, "$value");
        assert_eq!(first.polarity, AssertionPolarity::Always);
        assert!(!first.negated);
        let second = &extracted.assertions[1];
        assert_eq!(second.subject, "$user");
        assert_eq!(second.polarity, AssertionPolarity::IfTrue);
        assert!(second.negated);
        // The `=`-prefixed exact form is the divergent bucket: ignored.
        assert!(!extracted.assertions.iter().any(|a| a.subject == "$exact"));
    }
}
