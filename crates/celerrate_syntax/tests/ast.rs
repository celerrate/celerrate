#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

mod support;

use celerrate_syntax::SyntaxKind;
use celerrate_syntax::ast::{
    ArrayElement, AstNode, ClassDeclaration, ConstantDeclaration, EnumCase, Expression,
    ForeachStatement, MatchArm, MatchExpression, MemberAccessExpression, MemberDeclaration,
    MethodDeclaration, NamedType, NamedTypeName, SourceFile, Statement, TernaryExpression,
    TraitAlias, TraitPrecedence, TraitUseClause, Type, UseClause, UseDeclaration, YieldExpression,
};

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

/// The first typed descendant of the parse, found by walking every
/// node: the extension tests reach deep shapes without spelling the
/// whole path down.
fn first_descendant<N: AstNode>(parse: &celerrate_syntax::Parse) -> N {
    parse
        .tree()
        .descendants()
        .find_map(N::cast)
        .expect("the shape under test exists")
}

#[test]
fn semi_reserved_names_resolve_through_the_extensions() {
    let parse =
        support::parse_verified("<?php class List { const FOR = 1; public function match() {} }");
    let class_declaration: ClassDeclaration = first_descendant(&parse);
    assert_eq!(
        class_declaration.name_token().expect("a class name").text(),
        "List"
    );
    let constant: ConstantDeclaration = first_descendant(&parse);
    let element = constant.constant_elements().next().expect("one element");
    assert_eq!(element.name_token().expect("a constant name").text(), "FOR");
    let method: MethodDeclaration = first_descendant(&parse);
    assert_eq!(method.name_token().expect("a method name").text(), "match");

    let parse = support::parse_verified("<?php $object->list();");
    let access: MemberAccessExpression = first_descendant(&parse);
    let member_name = access.member_name().expect("a member name");
    assert_eq!(
        member_name.name_token().expect("a name token").text(),
        "list"
    );

    let parse = support::parse_verified("<?php enum Suit { case Default; }");
    let case: EnumCase = first_descendant(&parse);
    assert_eq!(case.name_token().expect("a case name").text(), "Default");
}

#[test]
fn a_named_type_is_one_concept_in_two_shapes() {
    let parse = support::parse_verified("<?php function f(): Foo\\Bar {}");
    let named: NamedType = first_descendant(&parse);
    assert!(matches!(
        named.name_or_keyword(),
        Some(NamedTypeName::Name(_))
    ));

    let parse = support::parse_verified("<?php function f(): array {}");
    let named: NamedType = first_descendant(&parse);
    let Some(NamedTypeName::Keyword(keyword)) = named.name_or_keyword() else {
        panic!("a bare keyword type");
    };
    assert_eq!(keyword.text(), "array");
}

#[test]
fn position_dependent_roles_resolve_against_token_anchors() {
    // The short ternary has no middle.
    let parse = support::parse_verified("<?php $a ? $b : $c;");
    let ternary: TernaryExpression = first_descendant(&parse);
    assert_eq!(ternary.middle().expect("a middle").syntax().text(), "$b");
    assert_eq!(ternary.third().expect("a third").syntax().text(), "$c");
    let parse = support::parse_verified("<?php $a ?: $c;");
    let ternary: TernaryExpression = first_descendant(&parse);
    assert!(ternary.middle().is_none(), "the short form has no middle");
    assert_eq!(ternary.third().expect("a third").syntax().text(), "$c");

    // `[value]` versus `[key => value]`.
    let parse = support::parse_verified("<?php [1, 'k' => 2];");
    let elements: Vec<ArrayElement> = parse
        .tree()
        .descendants()
        .filter_map(ArrayElement::cast)
        .collect();
    assert!(elements[0].key().is_none());
    assert_eq!(elements[0].value().expect("a value").syntax().text(), "1");
    assert_eq!(elements[1].key().expect("a key").syntax().text(), "'k'");
    assert_eq!(elements[1].value().expect("a value").syntax().text(), "2");

    // foreach with and without a key target.
    let parse = support::parse_verified("<?php foreach ($all as $k => $v) {}");
    let foreach: ForeachStatement = first_descendant(&parse);
    assert_eq!(
        foreach.subject().expect("a subject").syntax().text(),
        "$all"
    );
    assert_eq!(foreach.key().expect("a key").syntax().text(), "$k");
    assert_eq!(foreach.value().expect("a value").syntax().text(), "$v");
    let parse = support::parse_verified("<?php foreach ($all as $v) {}");
    let foreach: ForeachStatement = first_descendant(&parse);
    assert!(foreach.key().is_none());
    assert_eq!(foreach.value().expect("a value").syntax().text(), "$v");

    // Match arms: conditions before the arrow, the body after it.
    let parse = support::parse_verified("<?php match ($x) { 1, 2 => 'a', default => 'b' };");
    let expression: MatchExpression = first_descendant(&parse);
    let arms: Vec<MatchArm> = expression.match_arms().collect();
    assert_eq!(arms[0].conditions().count(), 2);
    assert!(!arms[0].is_default());
    assert_eq!(arms[0].body().expect("a body").syntax().text(), "'a'");
    assert!(arms[1].is_default());
    assert_eq!(arms[1].conditions().count(), 0);
    assert_eq!(arms[1].body().expect("a body").syntax().text(), "'b'");

    // yield in its three shapes.
    let parse = support::parse_verified("<?php function g() { yield $k => $v; }");
    let yielded: YieldExpression = first_descendant(&parse);
    assert_eq!(yielded.key().expect("a key").syntax().text(), "$k");
    assert_eq!(yielded.value().expect("a value").syntax().text(), "$v");
    let parse = support::parse_verified("<?php function g() { yield $v; }");
    let yielded: YieldExpression = first_descendant(&parse);
    assert!(yielded.key().is_none());
    assert_eq!(yielded.value().expect("a value").syntax().text(), "$v");
    let parse = support::parse_verified("<?php function g() { yield from $inner; }");
    let yielded: YieldExpression = first_descendant(&parse);
    assert!(yielded.yield_from_token().is_some());
    assert_eq!(yielded.value().expect("a value").syntax().text(), "$inner");
}

#[test]
fn trait_adaptations_and_import_aliases_resolve() {
    let parse = support::parse_verified(
        "<?php class C { use A, B { A::hello insteadof B; hello as protected h2; } }",
    );
    let trait_use: TraitUseClause = first_descendant(&parse);
    assert_eq!(trait_use.names().count(), 2);
    let precedence: TraitPrecedence = first_descendant(&parse);
    assert_eq!(
        precedence
            .reference_name()
            .expect("a reference")
            .syntax()
            .text(),
        "A"
    );
    assert_eq!(precedence.member_token().expect("a member").text(), "hello");
    assert_eq!(precedence.excluded_names().count(), 1);
    let alias: TraitAlias = first_descendant(&parse);
    assert_eq!(alias.member_token().expect("a member").text(), "hello");
    assert_eq!(
        alias.visibility_token().expect("a visibility").text(),
        "protected"
    );
    assert_eq!(alias.alias_token().expect("an alias").text(), "h2");

    let parse = support::parse_verified("<?php use Foo\\Bar as Baz;");
    let use_declaration: UseDeclaration = first_descendant(&parse);
    let clause: UseClause = use_declaration.use_clauses().next().expect("one clause");
    assert_eq!(clause.alias_token().expect("an alias").text(), "Baz");
}

#[test]
fn labels_hooks_and_modifiers_resolve() {
    let parse = support::parse_verified("<?php f(name: 1);");
    let argument = first_descendant::<celerrate_syntax::ast::Argument>(&parse);
    assert_eq!(argument.label_token().expect("a label").text(), "name");

    let parse =
        support::parse_verified("<?php class C { public int $x { get => 1; final set($v) {} } }");
    let hooks: Vec<celerrate_syntax::ast::PropertyHook> = parse
        .tree()
        .descendants()
        .filter_map(celerrate_syntax::ast::PropertyHook::cast)
        .collect();
    assert_eq!(hooks[0].name_token().expect("a hook name").text(), "get");
    assert_eq!(hooks[1].name_token().expect("a hook name").text(), "set");

    let parse = support::parse_verified("<?php class C { public static function f() {} }");
    let method: MethodDeclaration = first_descendant(&parse);
    let modifiers: Vec<String> = method
        .modifiers()
        .map(|token| token.text().to_owned())
        .collect();
    assert_eq!(modifiers, ["public", "static"]);
}
