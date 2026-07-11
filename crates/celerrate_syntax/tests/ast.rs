#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

mod support;

use celerrate_syntax::SyntaxKind;
use celerrate_syntax::ast::{AstNode, Expression, MemberDeclaration, SourceFile, Statement, Type};

#[test]
fn typed_navigation_reaches_a_method_through_a_class() {
    let parse = support::parse_verified(
        "<?php class Foo extends Bar { public function baz(int $a): void {} }",
    );
    let source_file = SourceFile::cast(parse.tree()).expect("the root casts");
    let class_declaration = source_file
        .statements()
        .find_map(|statement| match statement {
            Statement::ClassDeclaration(class_declaration) => Some(class_declaration),
            _ => None,
        })
        .expect("a class declaration");
    let extends_clause = class_declaration
        .extends_clause()
        .expect("an extends clause");
    assert_eq!(extends_clause.names().count(), 1);
    let member_list = class_declaration.member_list().expect("a member list");
    let method = member_list
        .member_declarations()
        .find_map(|member| match member {
            MemberDeclaration::MethodDeclaration(method) => Some(method),
            _ => None,
        })
        .expect("a method");
    let parameter = method
        .parameter_list()
        .expect("a parameter list")
        .parameters()
        .next()
        .expect("one parameter");
    assert_eq!(
        parameter.name_token().expect("the parameter name").text(),
        "$a"
    );
    assert!(matches!(parameter.ty(), Some(Type::NamedType(_))));
    assert!(matches!(method.return_type(), Some(Type::NamedType(_))));
}

#[test]
fn binary_operands_are_positional() {
    let parse = support::parse_verified("<?php 1 + 2;");
    let source_file = SourceFile::cast(parse.tree()).expect("the root casts");
    let statement = source_file.statements().next().expect("one statement");
    let Statement::ExpressionStatement(expression_statement) = statement else {
        panic!("an expression statement");
    };
    let Some(Expression::BinaryExpression(binary)) = expression_statement.expression() else {
        panic!("a binary expression");
    };
    assert_eq!(
        binary.operator_token().expect("the operator").kind(),
        SyntaxKind::Plus
    );
    assert_eq!(
        binary
            .lhs()
            .expect("the left operand")
            .syntax()
            .text()
            .to_string(),
        "1"
    );
    assert_eq!(
        binary
            .rhs()
            .expect("the right operand")
            .syntax()
            .text()
            .to_string(),
        "2"
    );
}
