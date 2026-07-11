//! Loads `php.ungram` into a lowered, emission-ready description. The
//! ungrammar file describes the nominal shape of every node; the
//! constrained dialect accepted here keeps the lowering total: node
//! alternations only in dedicated enum rules, token alternations
//! anywhere, and labels on atoms only.

use std::collections::HashMap;

use ungrammar::{Grammar, Rule};

use super::tokens::resolve_ungrammar_token;
use crate::Result;

/// Nodes whose accessors are hand-written in
/// `celerrate_syntax/src/ast/extensions.rs` because source position
/// alone cannot assign roles (the short ternary, `key => value` forms,
/// trait adaptations). They get structs and `AstNode` impls but no
/// generated accessors, and are exempt from the ambiguity check.
pub const HANDWRITTEN_ACCESSOR_NODES: &[&str] = &[
    "TernaryExpression",
    "ArrayElement",
    "ForeachStatement",
    "MatchArm",
    "YieldExpression",
    "TraitPrecedence",
    "TraitAlias",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarSource {
    pub nodes: Vec<AstNodeSource>,
    pub enums: Vec<AstEnumSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstNodeSource {
    pub name: String,
    pub documentation: Vec<String>,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstEnumSource {
    pub name: String,
    pub documentation: Vec<String>,
    pub variants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub kind: FieldKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Node {
        type_name: String,
        cardinality: Cardinality,
        /// Position among same-type siblings: `children().nth(index)`.
        index: usize,
    },
    Token {
        /// `SyntaxKind` variant names; more than one for operator sets.
        variants: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    Optional,
    Many,
}

pub fn load(text: &str) -> Result<GrammarSource> {
    let grammar: Grammar = text
        .parse()
        .map_err(|error| format!("php.ungram does not parse: {error}"))?;
    let documentation = extract_documentation(text);
    let mut nodes = Vec::new();
    let mut enums = Vec::new();
    for node in grammar.iter() {
        let name = grammar[node].name.clone();
        let rule = &grammar[node].rule;
        let docs = documentation.get(&name).cloned().unwrap_or_default();
        if let Some(variants) = enum_variants(&grammar, rule) {
            enums.push(AstEnumSource {
                name,
                documentation: docs,
                variants,
            });
        } else {
            let fields = if HANDWRITTEN_ACCESSOR_NODES.contains(&name.as_str()) {
                Vec::new()
            } else {
                lower_node_rule(&grammar, &name, rule)?
            };
            nodes.push(AstNodeSource {
                name,
                documentation: docs,
                fields,
            });
        }
    }
    Ok(GrammarSource { nodes, enums })
}

/// An enum rule is a top-level alternation of plain node references.
fn enum_variants(grammar: &Grammar, rule: &Rule) -> Option<Vec<String>> {
    let Rule::Alt(branches) = rule else {
        return None;
    };
    branches
        .iter()
        .map(|branch| match branch {
            Rule::Node(node) => Some(grammar[*node].name.clone()),
            _ => None,
        })
        .collect()
}

/// One field before merging and positional indices. Node names are
/// computed after the merge pass, because pluralization depends on the
/// merged cardinality.
enum RawField {
    Node {
        label: Option<String>,
        type_name: String,
        many: bool,
    },
    Token {
        name: String,
        variants: Vec<String>,
    },
}

fn lower_node_rule(grammar: &Grammar, node_name: &str, rule: &Rule) -> Result<Vec<Field>> {
    let mut raw_fields = Vec::new();
    lower_rule(grammar, node_name, rule, None, false, &mut raw_fields)?;
    assign_indices(node_name, raw_fields)
}

fn lower_rule(
    grammar: &Grammar,
    node_name: &str,
    rule: &Rule,
    label: Option<&str>,
    many: bool,
    accumulator: &mut Vec<RawField>,
) -> Result<()> {
    match rule {
        Rule::Labeled { label: name, rule } => {
            lower_rule(grammar, node_name, rule, Some(name), many, accumulator)
        }
        Rule::Node(node) => {
            let type_name = grammar[*node].name.clone();
            accumulator.push(RawField::Node {
                label: label.map(str::to_owned),
                type_name,
                many,
            });
            Ok(())
        }
        Rule::Token(token) => {
            // Resolve even unlabeled tokens: a typo in a spelling must
            // fail loudly, not silently drop from the shape.
            let variant = resolve_token(grammar, node_name, *token)?;
            if let Some(label) = label {
                accumulator.push(RawField::Token {
                    name: label.to_owned(),
                    variants: vec![variant],
                });
            }
            Ok(())
        }
        Rule::Seq(rules) => {
            if let Some(label) = label {
                return Err(format!(
                    "{node_name}: the label {label} wraps a sequence; labels wrap atoms only"
                )
                .into());
            }
            for rule in rules {
                lower_rule(grammar, node_name, rule, None, many, accumulator)?;
            }
            Ok(())
        }
        Rule::Opt(inner) => lower_rule(grammar, node_name, inner, label, many, accumulator),
        Rule::Rep(inner) => lower_rule(grammar, node_name, inner, label, true, accumulator),
        Rule::Alt(branches) => {
            let all_tokens = branches
                .iter()
                .all(|branch| matches!(branch, Rule::Token(_)));
            if all_tokens {
                let mut variants = Vec::new();
                for branch in branches {
                    if let Rule::Token(token) = branch {
                        variants.push(resolve_token(grammar, node_name, *token)?);
                    }
                }
                if let Some(label) = label {
                    accumulator.push(RawField::Token {
                        name: label.to_owned(),
                        variants,
                    });
                }
                return Ok(());
            }
            if let Some(label) = label {
                return Err(format!(
                    "{node_name}: the label {label} wraps an alternation with node \
                     references; node alternations belong in enum rules"
                )
                .into());
            }
            for branch in branches {
                lower_rule(grammar, node_name, branch, None, many, accumulator)?;
            }
            Ok(())
        }
    }
}

fn resolve_token(grammar: &Grammar, node_name: &str, token: ungrammar::Token) -> Result<String> {
    let spelling = &grammar[token].name;
    resolve_ungrammar_token(spelling)
        .map(str::to_owned)
        .ok_or_else(|| format!("{node_name}: unknown token spelling {spelling}").into())
}

fn node_field_name(label: Option<&str>, type_name: &str, many: bool) -> String {
    match label {
        Some(label) => label.to_owned(),
        None => {
            let base = snake_case(type_name);
            if many {
                // `types`, not `tys`: the reserved-word rename only
                // matters for the singular accessor.
                format!("{base}s")
            } else if base == "type" {
                "ty".to_owned()
            } else {
                base
            }
        }
    }
}

fn snake_case(name: &str) -> String {
    let mut output = String::new();
    for (position, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if position > 0 {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

/// Merges same-key node occurrences (`X (',' X)*` is one list field),
/// assigns `children().nth(index)` positions among same-type node
/// fields, and rejects the shapes position cannot disambiguate.
fn assign_indices(node_name: &str, raw_fields: Vec<RawField>) -> Result<Vec<Field>> {
    // Merge pass: same (label, type) means one conceptual field, but
    // only when at least one occurrence repeats. This is what makes the
    // comma-separated list idiom `(X (',' X)*)?` lower to a single
    // `Many` field while still rejecting a plain, unmarked `X X`
    // sequence: two occurrences that never repeat are a genuine
    // ambiguity (position cannot tell them apart) rather than a list,
    // so they are left unmerged and fall through to the duplicate-name
    // check below.
    let mut merged: Vec<RawField> = Vec::new();
    'raw: for raw_field in raw_fields {
        if let RawField::Node {
            label,
            type_name,
            many,
        } = &raw_field
        {
            for existing in &mut merged {
                if let RawField::Node {
                    label: existing_label,
                    type_name: existing_type,
                    many: existing_many,
                } = existing
                    && existing_label == label
                    && existing_type == type_name
                    && (*existing_many || *many)
                {
                    *existing_many = true;
                    continue 'raw;
                }
            }
        }
        merged.push(raw_field);
    }
    let mut per_type_totals: HashMap<String, (usize, bool)> = HashMap::new();
    for raw_field in &merged {
        if let RawField::Node {
            type_name, many, ..
        } = raw_field
        {
            let entry = per_type_totals
                .entry(type_name.clone())
                .or_insert((0, false));
            entry.0 += 1;
            entry.1 |= *many;
        }
    }
    let mut seen_names: Vec<String> = Vec::new();
    let mut per_type_counters: HashMap<String, usize> = HashMap::new();
    let mut fields = Vec::new();
    for raw_field in merged {
        let field = match raw_field {
            RawField::Node {
                label,
                type_name,
                many,
            } => {
                let (total, any_many) = per_type_totals
                    .get(&type_name)
                    .copied()
                    .unwrap_or((0, false));
                if total > 1 && any_many {
                    return Err(format!(
                        "{node_name}: {type_name} appears both repeated and single; \
                         position cannot assign roles, add the node to \
                         HANDWRITTEN_ACCESSOR_NODES"
                    )
                    .into());
                }
                let name = node_field_name(label.as_deref(), &type_name, many);
                let counter = per_type_counters.entry(type_name.clone()).or_insert(0);
                let index = *counter;
                *counter += 1;
                Field {
                    name,
                    kind: FieldKind::Node {
                        type_name,
                        cardinality: if many {
                            Cardinality::Many
                        } else {
                            Cardinality::Optional
                        },
                        index,
                    },
                }
            }
            RawField::Token { name, variants } => Field {
                name,
                kind: FieldKind::Token { variants },
            },
        };
        if seen_names.contains(&field.name) {
            return Err(format!(
                "{node_name}: two fields want the name {}; label them apart",
                field.name
            )
            .into());
        }
        seen_names.push(field.name.clone());
        fields.push(field);
    }
    Ok(fields)
}

/// Doc lines (`///`) immediately above a rule definition attach to it.
/// Any other non-blank line resets the pending block, so stray comments
/// cannot leak onto the next rule.
fn extract_documentation(text: &str) -> HashMap<String, Vec<String>> {
    let mut documentation = HashMap::new();
    let mut pending: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("///") {
            pending.push(rest.strip_prefix(' ').unwrap_or(rest).to_owned());
            continue;
        }
        if let Some(name) = rule_start_name(trimmed) {
            if !pending.is_empty() {
                documentation.insert(name.to_owned(), std::mem::take(&mut pending));
            }
        } else if !trimmed.is_empty() {
            pending.clear();
        }
    }
    documentation
}

/// `Name =` at the start of a line begins a rule; continuation lines
/// never match (their text before any `=` is not a bare capitalized
/// identifier).
fn rule_start_name(line: &str) -> Option<&str> {
    let (candidate, _rest) = line.split_once('=')?;
    let candidate = candidate.trim();
    let starts_uppercase = candidate
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_uppercase());
    (starts_uppercase
        && candidate
            .chars()
            .all(|character| character.is_ascii_alphanumeric()))
    .then_some(candidate)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::{Cardinality, FieldKind, load};

    const MINI: &str = r#"
/// The root of the mini grammar.
Root = Item*

/// One item, with a labeled token and an operator set.
Item = 'fn' name:'identifier' operator:('+' | '-') Body? Type? value:Expression?

Body = '{' Pair '}'

Pair = first:Item second:Item

Type = 'identifier'

Expression = Root | Item

/// A ternary lookalike: on the override list, so no fields.
TernaryExpression = Item* Body
"#;

    /// Nodes by name: ungrammar interns nodes in first-reference order,
    /// not definition order, so tests never index the node list.
    fn node<'grammar>(
        grammar: &'grammar super::GrammarSource,
        name: &str,
    ) -> &'grammar super::AstNodeSource {
        grammar
            .nodes
            .iter()
            .find(|node| node.name == name)
            .expect("node exists")
    }

    #[test]
    fn enum_rules_become_enums_and_never_nodes() {
        let grammar = load(MINI).expect("mini grammar loads");
        assert_eq!(grammar.enums.len(), 1);
        assert_eq!(grammar.enums[0].name, "Expression");
        assert_eq!(grammar.enums[0].variants, ["Root", "Item"]);
        let mut names: Vec<&str> = grammar
            .nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            ["Body", "Item", "Pair", "Root", "TernaryExpression", "Type"]
        );
    }

    #[test]
    fn documentation_attaches_to_the_rule_that_follows_it() {
        let grammar = load(MINI).expect("mini grammar loads");
        assert_eq!(
            node(&grammar, "Root").documentation,
            ["The root of the mini grammar."]
        );
        assert!(
            node(&grammar, "Body").documentation.is_empty(),
            "Body has no doc"
        );
    }

    #[test]
    fn repetition_pluralizes_and_becomes_many() {
        let grammar = load(MINI).expect("mini grammar loads");
        let root = node(&grammar, "Root");
        assert_eq!(root.fields.len(), 1);
        assert_eq!(root.fields[0].name, "items");
        match &root.fields[0].kind {
            FieldKind::Node {
                type_name,
                cardinality,
                ..
            } => {
                assert_eq!(type_name, "Item");
                assert_eq!(*cardinality, Cardinality::Many);
            }
            FieldKind::Token { .. } => panic!("Item* is a node field"),
        }
    }

    #[test]
    fn labeled_tokens_become_token_fields_and_unlabeled_ones_vanish() {
        let grammar = load(MINI).expect("mini grammar loads");
        let item = node(&grammar, "Item");
        let names: Vec<&str> = item
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect();
        // `'fn'` is unlabeled: shape documentation only. `Type` renames
        // to `ty` (reserved spelling).
        assert_eq!(names, ["name", "operator", "body", "ty", "value"]);
        match &item.fields[1].kind {
            FieldKind::Token { variants } => assert_eq!(variants, &["Plus", "Minus"]),
            FieldKind::Node { .. } => panic!("operator is a token set"),
        }
        match &item.fields[4].kind {
            FieldKind::Node { type_name, .. } => assert_eq!(type_name, "Expression"),
            FieldKind::Token { .. } => panic!("value is a node field"),
        }
    }

    #[test]
    fn same_type_twice_gets_positional_indices() {
        let grammar = load(MINI).expect("mini grammar loads");
        let pair = node(&grammar, "Pair");
        let indices: Vec<usize> = pair
            .fields
            .iter()
            .map(|field| match field.kind {
                FieldKind::Node { index, .. } => index,
                FieldKind::Token { .. } => panic!("node fields only"),
            })
            .collect();
        assert_eq!(indices, [0, 1]);
    }

    #[test]
    fn override_listed_nodes_get_no_fields() {
        // `TernaryExpression` mixes Many and single of related shapes;
        // being on HANDWRITTEN_ACCESSOR_NODES, it lowers to zero fields
        // instead of failing the ambiguity check.
        let grammar = load(MINI).expect("mini grammar loads");
        let ternary = node(&grammar, "TernaryExpression");
        assert!(ternary.fields.is_empty());
    }

    #[test]
    fn comma_separated_lists_merge_into_one_many_field() {
        let source = "Root = (Item (',' Item)*)?\nItem = 'identifier'";
        let grammar = load(source).expect("grammar loads");
        let root = node(&grammar, "Root");
        assert_eq!(root.fields.len(), 1);
        assert_eq!(root.fields[0].name, "items");
        match &root.fields[0].kind {
            FieldKind::Node { cardinality, .. } => {
                assert_eq!(*cardinality, Cardinality::Many);
            }
            FieldKind::Token { .. } => panic!("Item is a node field"),
        }
    }

    #[test]
    fn an_unknown_token_spelling_is_an_error() {
        let error = load("Root = 'no_such_token'").expect_err("must fail");
        assert!(error.to_string().contains("no_such_token"));
    }

    #[test]
    fn a_many_and_single_conflict_off_the_override_list_is_an_error() {
        // The labeled occurrence does not merge with the unlabeled
        // repeated one, so position cannot assign roles.
        let source = "Root = Item* extra:Item\nItem = 'identifier'";
        let error = load(source).expect_err("must fail");
        assert!(error.to_string().contains("Root"));
    }

    #[test]
    fn duplicate_field_names_are_an_error() {
        let source = "Root = Item Item\nItem = 'identifier'";
        // Two unlabeled `Item`s both want the name `item`: the grammar
        // must label them. (Positional indices exist, but distinct
        // accessor names cannot be derived without labels.)
        let error = load(source).expect_err("must fail");
        assert!(error.to_string().contains("item"));
    }
}
