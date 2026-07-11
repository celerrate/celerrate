#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use xtask::codegen::grammar::{Cardinality, FieldKind, load};

fn source() -> xtask::codegen::grammar::GrammarSource {
    let text = xtask::codegen::php_ungram_source().expect("php.ungram is readable");
    load(&text).expect("php.ungram loads and lowers")
}

fn node<'grammar>(
    grammar: &'grammar xtask::codegen::grammar::GrammarSource,
    name: &str,
) -> &'grammar xtask::codegen::grammar::AstNodeSource {
    grammar
        .nodes
        .iter()
        .find(|node| node.name == name)
        .expect("node exists")
}

#[test]
fn the_grammar_covers_every_node_kind() {
    let grammar = source();
    assert_eq!(grammar.nodes.len(), 105, "every node kind except ErrorNode");
    assert_eq!(grammar.enums.len(), 6);
    let enum_names: Vec<&str> = grammar
        .enums
        .iter()
        .map(|enumeration| enumeration.name.as_str())
        .collect();
    assert_eq!(
        enum_names,
        [
            "Expression",
            "Statement",
            "Type",
            "MemberDeclaration",
            "StringInterpolation",
            "TraitAdaptation"
        ]
    );
}

#[test]
fn spot_checks_on_lowered_shapes() {
    let grammar = source();

    let class_declaration = node(&grammar, "ClassDeclaration");
    let names: Vec<&str> = class_declaration
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "attribute_groups",
            "argument_list",
            "extends_clause",
            "implements_clause",
            "member_list"
        ]
    );

    let binary = node(&grammar, "BinaryExpression");
    assert_eq!(binary.fields[0].name, "lhs");
    assert_eq!(binary.fields[2].name, "rhs");
    match (&binary.fields[0].kind, &binary.fields[2].kind) {
        (FieldKind::Node { index: 0, .. }, FieldKind::Node { index: 1, .. }) => {}
        _ => panic!("lhs and rhs are positional Expression fields"),
    }

    let block = node(&grammar, "Block");
    assert_eq!(block.fields[0].name, "statements");
    match &block.fields[0].kind {
        FieldKind::Node { cardinality, .. } => {
            assert_eq!(*cardinality, Cardinality::Many);
        }
        FieldKind::Token { .. } => panic!("statements is a node field"),
    }

    // Override-listed nodes lower to zero fields.
    for name in ["TernaryExpression", "ForeachStatement", "MatchArm"] {
        assert!(node(&grammar, name).fields.is_empty(), "{name} overrides");
    }

    // Semi-reserved positions carry no generated name accessor.
    assert!(
        node(&grammar, "ConstantElement")
            .fields
            .iter()
            .all(|field| field.name != "name"),
        "ConstantElement names are hand-written"
    );

    let expression = grammar
        .enums
        .iter()
        .find(|enumeration| enumeration.name == "Expression")
        .expect("Expression enum");
    assert_eq!(expression.variants.len(), 33);
    let statement = grammar
        .enums
        .iter()
        .find(|enumeration| enumeration.name == "Statement")
        .expect("Statement enum");
    assert_eq!(statement.variants.len(), 28);
}
