#![allow(clippy::expect_used)]

mod support;

use celerrate_syntax::ParserDiagnosticKind;
use celerrate_syntax::SyntaxKind;
use support::{parser_diagnostics, render_member};

#[test]
fn a_typed_property_lists_its_declarators() {
    insta::assert_snapshot!(render_member("public int $balance = 0, $pending;"), @r#"
    PropertyDeclaration
      Public "public"
      NamedType
        Name
          Identifier "int"
      PropertyElement
        Variable "$balance"
        Equals "="
        Literal
          IntegerLiteral "0"
      Comma ","
      PropertyElement
        Variable "$pending"
      Semicolon ";"
    "#);
}

#[test]
fn a_var_property_parses() {
    insta::assert_snapshot!(render_member("var $legacy;"), @r#"
    PropertyDeclaration
      Var "var"
      PropertyElement
        Variable "$legacy"
      Semicolon ";"
    "#);
}

#[test]
fn asymmetric_visibility_stays_flat_modifier_tokens() {
    insta::assert_snapshot!(render_member("public private(set) string $name;"), @r#"
    PropertyDeclaration
      Public "public"
      Private "private"
      OpenParenthesis "("
      Identifier "set"
      CloseParenthesis ")"
      NamedType
        Name
          Identifier "string"
      PropertyElement
        Variable "$name"
      Semicolon ";"
    "#);
}

#[test]
fn a_parenthesized_single_name_after_a_visibility_reads_as_the_suffix() {
    // The recorded trade-off of `asymmetric_visibility_suffix`: the
    // kinds-only lookahead cannot require the identifier to be `set`,
    // so a parenthesized single-name type reads as the suffix. Both
    // readings are invalid Zend (a spaced suffix, and a one-member
    // parenthesized DNF group), so no legal program misreads.
    insta::assert_snapshot!(render_member("private (Foo) $x;"), @r#"
    PropertyDeclaration
      Private "private"
      OpenParenthesis "("
      Identifier "Foo"
      CloseParenthesis ")"
      PropertyElement
        Variable "$x"
      Semicolon ";"
    "#);
}

#[test]
fn a_parenthesized_intersection_after_a_visibility_stays_a_type() {
    insta::assert_snapshot!(render_member("private (Foo&Bar) $x;"), @r#"
    PropertyDeclaration
      Private "private"
      ParenthesizedType
        OpenParenthesis "("
        IntersectionType
          NamedType
            Name
              Identifier "Foo"
          Ampersand "&"
          NamedType
            Name
              Identifier "Bar"
        CloseParenthesis ")"
      PropertyElement
        Variable "$x"
      Semicolon ";"
    "#);
}

#[test]
fn a_nullable_static_property_parses() {
    assert_eq!(
        parser_diagnostics("<?php class A { protected static ?self $instance = null; }"),
        vec![]
    );
}

#[test]
fn a_class_constant_carries_modifiers_and_a_type() {
    insta::assert_snapshot!(render_member("final protected const int LIMIT = 10;"), @r#"
    ConstantDeclaration
      Final "final"
      Protected "protected"
      Const "const"
      NamedType
        Name
          Identifier "int"
      ConstantElement
        Identifier "LIMIT"
        Equals "="
        Literal
          IntegerLiteral "10"
      Semicolon ";"
    "#);
}

#[test]
fn a_semi_reserved_class_constant_name_parses() {
    assert_eq!(
        parser_diagnostics("<?php class A { const FOR = 'semi-reserved'; }"),
        vec![]
    );
}

#[test]
fn dangling_modifiers_become_wreckage_and_the_list_recovers() {
    // `public` wraps into an ErrorNode (ExpectedDeclaration); the `;`
    // it dangled on is then swept by the member list (UnexpectedToken);
    // the constant after it parses clean.
    assert_eq!(
        parser_diagnostics("<?php class A { public; const OK = 1; }"),
        vec![
            ParserDiagnosticKind::ExpectedDeclaration,
            ParserDiagnosticKind::UnexpectedToken,
        ]
    );
}

#[test]
fn a_property_without_a_variable_is_diagnosed() {
    let diagnostics = parser_diagnostics("<?php class A { public int = 5; }");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::Expected(SyntaxKind::Variable)),
        "got {diagnostics:?}"
    );
}

#[test]
fn a_method_with_modifiers_a_return_type_and_a_body() {
    insta::assert_snapshot!(render_member("public function list(): array { return []; }"), @r#"
    MethodDeclaration
      Public "public"
      Function "function"
      List "list"
      ParameterList
        OpenParenthesis "("
        CloseParenthesis ")"
      Colon ":"
      NamedType
        Array "array"
      Block
        OpenBrace "{"
        ReturnStatement
          Return "return"
          ArrayExpression
            OpenBracket "["
            CloseBracket "]"
          Semicolon ";"
        CloseBrace "}"
    "#);
}

#[test]
fn an_abstract_method_ends_at_its_semicolon() {
    insta::assert_snapshot!(render_member("abstract protected function close(): void;"), @r#"
    MethodDeclaration
      Abstract "abstract"
      Protected "protected"
      Function "function"
      Identifier "close"
      ParameterList
        OpenParenthesis "("
        CloseParenthesis ")"
      Colon ":"
      NamedType
        Name
          Identifier "void"
      Semicolon ";"
    "#);
}

#[test]
fn a_by_reference_method_parses() {
    assert_eq!(
        parser_diagnostics(
            "<?php class A { public function &reference(): int { return $this->x; } }"
        ),
        vec![]
    );
}

#[test]
fn interface_method_signatures_parse() {
    assert_eq!(
        parser_diagnostics("<?php interface Shape { public function area(): float; }"),
        vec![]
    );
}

#[test]
fn a_simple_trait_use_ends_at_its_semicolon() {
    insta::assert_snapshot!(render_member("use Greets, Counts;"), @r#"
    TraitUseClause
      Use "use"
      Name
        Identifier "Greets"
      Comma ","
      Name
        Identifier "Counts"
      Semicolon ";"
    "#);
}

#[test]
fn trait_adaptations_parse_precedences_and_aliases() {
    insta::assert_snapshot!(render_member("use Greets, Counts { Greets::hello insteadof Counts; Counts::hello as protected countedHello; }"), @r#"
    TraitUseClause
      Use "use"
      Name
        Identifier "Greets"
      Comma ","
      Name
        Identifier "Counts"
      TraitAdaptationList
        OpenBrace "{"
        TraitPrecedence
          Name
            Identifier "Greets"
          ColonColon "::"
          Identifier "hello"
          InsteadOf "insteadof"
          Name
            Identifier "Counts"
          Semicolon ";"
        TraitAlias
          Name
            Identifier "Counts"
          ColonColon "::"
          Identifier "hello"
          As "as"
          Protected "protected"
          Identifier "countedHello"
          Semicolon ";"
        CloseBrace "}"
    "#);
}

#[test]
fn a_bare_alias_with_a_keyword_member_name_parses() {
    // `list as unreserved;`: a bare semi-reserved member name, no
    // class qualifier.
    assert_eq!(
        parser_diagnostics("<?php class A { use B { list as unreserved; } }"),
        vec![]
    );
}

#[test]
fn a_visibility_only_alias_parses() {
    assert_eq!(
        parser_diagnostics("<?php class A { use B { hello as protected; } }"),
        vec![]
    );
}

#[test]
fn junk_inside_an_adaptation_list_is_swept() {
    let diagnostics = parser_diagnostics("<?php class A { use B { 42; hello as h; } }");
    assert!(
        diagnostics.contains(&ParserDiagnosticKind::UnexpectedToken),
        "got {diagnostics:?}"
    );
}
