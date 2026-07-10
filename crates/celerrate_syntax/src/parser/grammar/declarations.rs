//! The declaration grammar: `const`, `namespace`, `use` imports,
//! functions, and the class-likes with their members. Every rule takes
//! an already-open [`Marker`], and the [`declaration`] dispatcher owns
//! opening it, so attribute groups (a later task of this plan) can
//! open the node before the dispatch runs.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::expressions::{
    argument_list, error_element, expect_list_separator, expression, name, parameter_list,
};
use super::statements::{block, terminate_statement};
use super::{Marker, Parser};

/// Whether the current token is a semi-reserved name: an identifier or
/// any keyword. Zend accepts any keyword wherever a semi-reserved name
/// is expected (constant, method, case, and adaptation-member names,
/// aliases); per-position reservation is judged upstairs.
fn at_semi_reserved_name(parser: &mut Parser) -> bool {
    parser
        .current()
        .is_some_and(|kind| kind == SyntaxKind::Identifier || kind.is_keyword())
}

/// Bumps a semi-reserved name, or diagnoses one missing.
fn expect_semi_reserved_name(parser: &mut Parser) {
    if at_semi_reserved_name(parser) {
        parser.bump();
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
    }
}

/// Dispatch for declaration statements. The statement dispatcher only
/// routes here on tokens it has already vetted (with lookahead where a
/// keyword is overloaded), so the fallback arm is defensive: it is
/// reachable only behind consumed tokens (attribute groups, from a
/// later task), never at a standstill.
pub(super) fn declaration(parser: &mut Parser) {
    let marker = parser.start();
    super::attributes::attribute_groups(parser);
    match parser.current() {
        Some(SyntaxKind::Function) if at_function_declaration(parser) => {
            function_declaration(parser, marker);
        }
        // `#[...]` behind a closure or an arrow function, not a
        // declaration: only reachable behind attribute groups (a bare
        // `function (...) {}` or `fn (...) => ...` at statement level
        // is routed to `expression_statement` directly, never here).
        // The closure keeps the groups as its own leading children;
        // wrapped in an `ExpressionStatement` so it terminates like any
        // other expression statement, with a trailing `;`.
        Some(SyntaxKind::Function | SyntaxKind::Fn) => {
            attributed_closure_statement(parser, marker);
        }
        Some(SyntaxKind::Static)
            if matches!(parser.nth(1), Some(SyntaxKind::Function | SyntaxKind::Fn)) =>
        {
            attributed_closure_statement(parser, marker);
        }
        Some(SyntaxKind::Const) => constant_declaration(parser, marker),
        Some(SyntaxKind::Namespace) if parser.nth(1) != Some(SyntaxKind::Backslash) => {
            namespace_declaration(parser, marker);
        }
        Some(SyntaxKind::Use) => use_declaration(parser, marker),
        Some(
            SyntaxKind::Class | SyntaxKind::Abstract | SyntaxKind::Final | SyntaxKind::Readonly,
        ) => class_declaration(parser, marker),
        Some(SyntaxKind::Interface) => interface_declaration(parser, marker),
        Some(SyntaxKind::Trait) => trait_declaration(parser, marker),
        Some(SyntaxKind::Enum) if parser.nth(1) == Some(SyntaxKind::Identifier) => {
            enum_declaration(parser, marker);
        }
        // No declaration-shaped token here: either a genuinely
        // unexpected token, or end of input right after attribute
        // groups (`#[A]` with nothing behind it). Either way the
        // groups may already be consumed, so they become wreckage
        // rather than splicing silently into the parent; `diagnose_current`
        // is zero-width after the last token when at end of input.
        _ => {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedDeclaration);
            marker.complete(parser, SyntaxKind::ErrorNode);
        }
    }
}

/// The closure/arrow-function tail of an attribute-led statement:
/// completes `marker` as the closure itself (the groups stay its
/// leading children), then wraps it in an `ExpressionStatement` so the
/// trailing `;` is consumed the same way any other expression
/// statement consumes it.
fn attributed_closure_statement(parser: &mut Parser, marker: Marker) {
    let closure = super::expressions::closure_or_arrow_function(parser, marker);
    let statement = closure.precede(parser);
    terminate_statement(parser);
    statement.complete(parser, SyntaxKind::ExpressionStatement);
}

/// `function` declares only when a name follows, with an optional `&`
/// between: `function (` and `function &(` are closure expressions.
pub(super) fn at_function_declaration(parser: &mut Parser) -> bool {
    match parser.nth(1) {
        Some(SyntaxKind::Identifier) => true,
        Some(SyntaxKind::Ampersand) => parser.nth(2) == Some(SyntaxKind::Identifier),
        _ => false,
    }
}

fn function_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `function`
    parser.eat(SyntaxKind::Ampersand); // by-reference return
    parser.expect(SyntaxKind::Identifier);
    parameter_list(parser);
    if parser.eat(SyntaxKind::Colon) {
        super::types::type_expression(parser);
    }
    block(parser);
    marker.complete(parser, SyntaxKind::FunctionDeclaration);
}

/// `const FOO = 1, BAR = 2;`, optionally typed since 8.3
/// (`const int FOO = 1;`). The type is absent exactly when the next
/// token is a name directly followed by `=`. Constant names accept any
/// keyword (semi-reserved: `const FOR = 1;`); which names and types
/// are legal where is semantic. Also the class-constant rule: the
/// member path parses its modifiers into `marker` first.
///
/// Terminates: every iteration bumps a name or breaks.
fn constant_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `const`
    let at_untyped_name =
        at_semi_reserved_name(parser) && parser.nth(1) == Some(SyntaxKind::Equals);
    if !at_untyped_name && parser.current().is_some_and(super::types::starts_type) {
        super::types::type_expression(parser);
    }
    loop {
        if !at_semi_reserved_name(parser) {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
            break;
        }
        let element = parser.start();
        parser.bump();
        parser.expect(SyntaxKind::Equals);
        expression(parser);
        element.complete(parser, SyntaxKind::ConstantElement);
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::ConstantDeclaration);
}

/// `namespace A\B;`, `namespace A\B { ... }`, `namespace { ... }`.
/// Only dispatched when the next token is not `\` (`namespace\Foo` is
/// a name expression). Where namespaces may appear and nest is
/// semantic.
fn namespace_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `namespace`
    if parser.at(SyntaxKind::Identifier) {
        name(parser);
    }
    if parser.at(SyntaxKind::OpenBrace) {
        block(parser);
    } else {
        terminate_statement(parser);
    }
    marker.complete(parser, SyntaxKind::NamespaceDeclaration);
}

/// `use A\B;`, `use A\B as C;`, `use function a\b;`,
/// `use const A\B;`, comma-separated clause lists, and the group form
/// `use A\{B, function c as d};`. Collisions and resolution are
/// semantic. Terminates: every iteration parses a clause that consumed
/// at least one name token, or breaks.
fn use_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `use`
    if matches!(
        parser.current(),
        Some(SyntaxKind::Function | SyntaxKind::Const)
    ) {
        parser.bump(); // the import type, for the whole clause list
    }
    loop {
        if !use_clause(parser) {
            break;
        }
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::UseDeclaration);
}

/// One top-level import clause: a name, then a group (`\{ ... }`) or
/// an optional alias. Returns false without consuming when no name can
/// start here.
fn use_clause(parser: &mut Parser) -> bool {
    if !matches!(
        parser.current(),
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace)
    ) {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
        return false;
    }
    let marker = parser.start();
    name(parser);
    if parser.at(SyntaxKind::Backslash) && parser.nth(1) == Some(SyntaxKind::OpenBrace) {
        use_group(parser);
    } else {
        use_alias(parser);
    }
    marker.complete(parser, SyntaxKind::UseClause);
    true
}

/// `\{ B, function c as d, }`: the group of a grouped import. Same
/// recovery contract as the expression lists: unexpected tokens are
/// wrapped and consumed; `;`, `?>`, and end of input abort. The shared
/// separator helper tolerates the trailing comma Zend allows here.
/// Terminates: every iteration consumes through `use_group_item` (its
/// dispatch admits only kinds that item always bumps) or through
/// `error_element`.
fn use_group(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `\`
    parser.bump(); // `{`
    while !parser.at(SyntaxKind::CloseBrace) && !parser.at_end() {
        if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
            break;
        }
        if !matches!(
            parser.current(),
            Some(
                SyntaxKind::Function
                    | SyntaxKind::Const
                    | SyntaxKind::Identifier
                    | SyntaxKind::Backslash
                    | SyntaxKind::Namespace
            )
        ) {
            error_element(parser);
            continue;
        }
        use_group_item(parser);
        expect_list_separator(parser, SyntaxKind::CloseBrace);
    }
    parser.expect(SyntaxKind::CloseBrace);
    marker.complete(parser, SyntaxKind::UseGroup);
}

/// One item of a group: an optional per-item `function`/`const` type,
/// the name, an optional alias. Always consumes at least one token:
/// the group loop admitted only its leading kinds.
fn use_group_item(parser: &mut Parser) {
    let marker = parser.start();
    if matches!(
        parser.current(),
        Some(SyntaxKind::Function | SyntaxKind::Const)
    ) {
        parser.bump();
    }
    if matches!(
        parser.current(),
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace)
    ) {
        name(parser);
        use_alias(parser);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
    }
    marker.complete(parser, SyntaxKind::UseClause);
}

/// `as Alias`. Any keyword is accepted as the alias; validity is
/// semantic.
fn use_alias(parser: &mut Parser) {
    if !parser.eat(SyntaxKind::As) {
        return;
    }
    expect_semi_reserved_name(parser);
}

/// `abstract`, `final`, `readonly` before `class`, in any order and
/// multiplicity; validity is semantic.
fn class_modifiers(parser: &mut Parser) {
    while matches!(
        parser.current(),
        Some(SyntaxKind::Abstract | SyntaxKind::Final | SyntaxKind::Readonly)
    ) {
        parser.bump();
    }
}

fn class_declaration(parser: &mut Parser, marker: Marker) {
    class_modifiers(parser);
    if !parser.eat(SyntaxKind::Class) {
        // Modifiers with no `class` behind them (`abstract 1;`): the
        // modifiers become wreckage and the rest parses on its own.
        // Progress holds: this path is only reachable behind at least
        // one consumed token (a modifier, or attribute groups).
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Class));
        marker.complete(parser, SyntaxKind::ErrorNode);
        return;
    }
    class_like_name(parser);
    heritage_clauses(parser);
    member_list(parser);
    marker.complete(parser, SyntaxKind::ClassDeclaration);
}

fn interface_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `interface`
    class_like_name(parser);
    heritage_clauses(parser);
    member_list(parser);
    marker.complete(parser, SyntaxKind::InterfaceDeclaration);
}

fn trait_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `trait`
    class_like_name(parser);
    heritage_clauses(parser);
    member_list(parser);
    marker.complete(parser, SyntaxKind::TraitDeclaration);
}

/// The declaration half of `new class(...) extends ... { ... }`: no
/// name, optional constructor arguments between the keyword and the
/// heritage clauses. Which modifiers an anonymous class allows
/// (`readonly` since 8.3) is semantic.
pub(super) fn anonymous_class(parser: &mut Parser, marker: Marker) {
    class_modifiers(parser);
    parser.expect(SyntaxKind::Class);
    if parser.at(SyntaxKind::OpenParenthesis) {
        argument_list(parser);
    }
    heritage_clauses(parser);
    member_list(parser);
    marker.complete(parser, SyntaxKind::ClassDeclaration);
}

/// The declared name. Zend rejects keywords here; every keyword parses
/// as the name anyway (`class List {}` stays one analyzable
/// declaration) and reservation is judged upstairs: except `extends`
/// and `implements`, which stay heritage clauses so a missing name
/// cannot swallow them.
fn class_like_name(parser: &mut Parser) {
    if at_semi_reserved_name(parser)
        && !matches!(
            parser.current(),
            Some(SyntaxKind::Extends | SyntaxKind::Implements)
        )
    {
        parser.bump();
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
    }
}

/// `extends ...` then `implements ...`, each optional. One shared rule
/// for every class-like: an interface with `implements` or a trait
/// with either clause parses, and the misplacement is semantic.
fn heritage_clauses(parser: &mut Parser) {
    if parser.at(SyntaxKind::Extends) {
        let clause = parser.start();
        parser.bump();
        name_list(parser);
        clause.complete(parser, SyntaxKind::ExtendsClause);
    }
    if parser.at(SyntaxKind::Implements) {
        let clause = parser.start();
        parser.bump();
        name_list(parser);
        clause.complete(parser, SyntaxKind::ImplementsClause);
    }
}

/// Comma-separated qualified names. Arity (single inheritance) is
/// semantic. Terminates: every iteration parses a name (which always
/// bumps) or breaks.
fn name_list(parser: &mut Parser) {
    loop {
        if matches!(
            parser.current(),
            Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace)
        ) {
            name(parser);
        } else {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
            break;
        }
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
}

/// `{ members }`. The loop is position-guarded like `argument_list`:
/// a member rule can refuse without consuming (the nesting guard can
/// veto a type or initializer sub-parse), and the guard then forces an
/// `error_element` bump, so the list always progresses. The guard on
/// `current` (not `at_end`) makes a blown fuse unwind instead of spin.
fn member_list(parser: &mut Parser) {
    let marker = parser.start();
    if parser.eat(SyntaxKind::OpenBrace) {
        while parser.current().is_some() && !parser.at(SyntaxKind::CloseBrace) {
            let position_before_member = parser.position();
            member(parser);
            if parser.position() == position_before_member {
                error_element(parser);
            }
        }
        parser.expect(SyntaxKind::CloseBrace);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenBrace));
    }
    marker.complete(parser, SyntaxKind::MemberList);
}

/// One class-body member. Tasks of this plan keep growing this
/// dispatch; `member_list`'s position guard backstops any
/// zero-consumption refusal path, so the arms may refuse freely.
fn member(parser: &mut Parser) {
    match parser.current() {
        Some(SyntaxKind::Use) => trait_use(parser),
        Some(SyntaxKind::Case) => {
            let marker = parser.start();
            enum_case(parser, marker);
        }
        Some(kind) if starts_member(kind) => {
            let marker = parser.start();
            modified_member(parser, marker);
        }
        Some(SyntaxKind::AttributeOpen) => {
            let marker = parser.start();
            super::attributes::attribute_groups(parser);
            match parser.current() {
                Some(SyntaxKind::Case) => enum_case(parser, marker),
                Some(kind) if starts_member(kind) => modified_member(parser, marker),
                _ => {
                    // Attribute groups with no member behind them: the
                    // groups (consumed, so the list progresses) become
                    // wreckage.
                    parser.diagnose_current(ParserDiagnosticKind::ExpectedDeclaration);
                    marker.complete(parser, SyntaxKind::ErrorNode);
                }
            }
        }
        Some(_) => error_element(parser),
        None => {}
    }
}

/// Whether `kind` can start a modified member (a property, a
/// constant, or a method).
fn starts_member(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Public
            | SyntaxKind::Protected
            | SyntaxKind::Private
            | SyntaxKind::Static
            | SyntaxKind::Abstract
            | SyntaxKind::Final
            | SyntaxKind::Readonly
            | SyntaxKind::Var
            | SyntaxKind::Const
            | SyntaxKind::Variable
            | SyntaxKind::Function
    ) || super::types::starts_type(kind)
}

/// Modifiers, then the member they modify: a constant, a method, or a
/// property (optionally typed). The member's kind is decided by the
/// first token after the modifiers.
fn modified_member(parser: &mut Parser, marker: Marker) {
    member_modifiers(parser);
    match parser.current() {
        Some(SyntaxKind::Const) => constant_declaration(parser, marker),
        Some(SyntaxKind::Function) => method_declaration(parser, marker),
        // A bare `$x;` member is a Zend parse error; parsing it as an
        // unmodified property keeps it one analyzable unit.
        Some(SyntaxKind::Variable) => property_declaration(parser, marker),
        Some(kind) if super::types::starts_type(kind) => {
            types_then_property(parser, marker);
        }
        _ => {
            // Modifiers with nothing to modify (`public;`). At least
            // one modifier token was consumed on this path, so the
            // member list progresses.
            parser.diagnose_current(ParserDiagnosticKind::ExpectedDeclaration);
            marker.complete(parser, SyntaxKind::ErrorNode);
        }
    }
}

/// The typed-property tail. The type parse can be refused by the
/// nesting guard without consuming; `member_list`'s position guard
/// covers that case.
fn types_then_property(parser: &mut Parser, marker: Marker) {
    super::types::type_expression(parser);
    property_declaration(parser, marker);
}

/// `public`, `protected`, `private` (each optionally asymmetric:
/// `private(set)`), `static`, `abstract`, `final`, `readonly`, `var`.
/// Order, repetition, and combination are all judged upstairs; the
/// parser accepts any sequence. Also the full modifier set constructor
/// promotion admits: php-src's `optional_cpp_modifiers` is the same
/// member-modifier grammar, and which modifiers are legal on a
/// parameter is judged at compile time, not here.
pub(super) fn member_modifiers(parser: &mut Parser) {
    loop {
        match parser.current() {
            Some(SyntaxKind::Public | SyntaxKind::Protected | SyntaxKind::Private) => {
                parser.bump();
                asymmetric_visibility_suffix(parser);
            }
            Some(
                SyntaxKind::Static
                | SyntaxKind::Abstract
                | SyntaxKind::Final
                | SyntaxKind::Readonly
                | SyntaxKind::Var,
            ) => parser.bump(),
            _ => break,
        }
    }
}

/// The `(set)` of 8.4's asymmetric visibility, three flat tokens.
/// Zend lexes `private(set)` as one token and thereby forbids interior
/// whitespace; the trivia-free view cannot see adjacency, so spaced
/// forms parse here and adjacency is judged upstairs (the same trade
/// recorded on `name`). Only the exact `( identifier )` shape is
/// taken: anything else belongs to the member that follows.
///
/// The token view carries kinds, not text, so the identifier cannot be
/// required to be `set` here: any `( identifier )` after a visibility
/// reads as the suffix, including the parenthesized single-name type
/// `private (Foo) $x;`, and the identifier's validity is judged
/// upstairs on the flat tokens. No legal program misreads: Zend
/// rejects both the spaced suffix and a one-member parenthesized DNF
/// group (a group requires at least two intersection members), so the
/// collision only decides which diagnostic path invalid code takes.
fn asymmetric_visibility_suffix(parser: &mut Parser) {
    if parser.at(SyntaxKind::OpenParenthesis)
        && parser.nth(1) == Some(SyntaxKind::Identifier)
        && parser.nth(2) == Some(SyntaxKind::CloseParenthesis)
    {
        parser.bump();
        parser.bump();
        parser.bump();
    }
}

/// The declarators of one property: `$a = 1, $b;`, or the hooked form
/// `$name { get; ... }`, which ends at its closing brace with no `;`.
/// The caller consumed the modifiers and the optional type into
/// `marker`. Terminates: every iteration bumps a variable or breaks.
fn property_declaration(parser: &mut Parser, marker: Marker) {
    let mut hooked = false;
    loop {
        if !parser.at(SyntaxKind::Variable) {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Variable));
            break;
        }
        let element = parser.start();
        parser.bump();
        if parser.eat(SyntaxKind::Equals) {
            expression(parser);
        }
        if parser.at(SyntaxKind::OpenBrace) {
            property_hook_list(parser);
            hooked = true;
        }
        element.complete(parser, SyntaxKind::PropertyElement);
        if !parser.eat(SyntaxKind::Comma) {
            break;
        }
    }
    if !hooked {
        terminate_statement(parser);
    }
    marker.complete(parser, SyntaxKind::PropertyDeclaration);
}

/// `{ get; set => ...; &get { ... } }` (8.4 property hooks).
/// Position-guarded like `member_list`, and for the same reason.
pub(super) fn property_hook_list(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `{`
    while parser.current().is_some() && !parser.at(SyntaxKind::CloseBrace) {
        let position_before_hook = parser.position();
        property_hook(parser);
        if parser.position() == position_before_hook {
            error_element(parser);
        }
    }
    parser.expect(SyntaxKind::CloseBrace);
    marker.complete(parser, SyntaxKind::PropertyHookList);
}

/// One hook: `get;` (abstract), `get => expression;`,
/// `set(string $value) { ... }`, `final &get { ... }`. Hook names are
/// plain identifiers; which names exist (`get`, `set`) and which
/// combinations are legal is semantic.
fn property_hook(parser: &mut Parser) {
    let marker = parser.start();
    super::attributes::attribute_groups(parser);
    parser.eat(SyntaxKind::Final); // the one modifier hooks admit today
    parser.eat(SyntaxKind::Ampersand); // by-reference `get`
    if at_semi_reserved_name(parser) {
        parser.bump();
        if parser.at(SyntaxKind::OpenParenthesis) {
            parameter_list(parser); // `set(string $value)`
        }
        if parser.eat(SyntaxKind::FatArrow) {
            expression(parser);
            terminate_statement(parser);
        } else if parser.at(SyntaxKind::OpenBrace) {
            block(parser);
        } else {
            terminate_statement(parser); // the abstract form `get;`
        }
    } else {
        // Nothing hook-shaped. Tokens may already be consumed
        // (`final`, `&`), so the node completes partially; a
        // zero-consumption trip is swept by the list's position
        // guard.
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
    }
    marker.complete(parser, SyntaxKind::PropertyHook);
}

/// `function name(parameters): type` ending in a block or, for the
/// abstract and interface forms, `;`. Method names are semi-reserved:
/// any keyword parses (`public function list() {}`). The caller
/// consumed the modifiers into `marker`; whether a body is required
/// is semantic.
fn method_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `function`
    parser.eat(SyntaxKind::Ampersand); // by-reference return
    expect_semi_reserved_name(parser);
    parameter_list(parser);
    if parser.eat(SyntaxKind::Colon) {
        super::types::type_expression(parser);
    }
    if parser.at(SyntaxKind::OpenBrace) {
        block(parser);
    } else {
        parser.expect(SyntaxKind::Semicolon);
    }
    marker.complete(parser, SyntaxKind::MethodDeclaration);
}

/// `use A, B;` or `use A, B { adaptations }` inside a class body.
/// Distinct from the import `use` (statement level) and the closure
/// `use` (expression level); the contexts are disjoint.
fn trait_use(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `use`
    name_list(parser);
    if parser.at(SyntaxKind::OpenBrace) {
        trait_adaptation_list(parser);
    } else {
        terminate_statement(parser);
    }
    marker.complete(parser, SyntaxKind::TraitUseClause);
}

/// `{ A::b insteadof C; b as protected c; }`. Position-guarded like
/// `member_list`, and for the same reason.
fn trait_adaptation_list(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `{`
    while parser.current().is_some() && !parser.at(SyntaxKind::CloseBrace) {
        let position_before_adaptation = parser.position();
        trait_adaptation(parser);
        if parser.position() == position_before_adaptation {
            error_element(parser);
        }
    }
    parser.expect(SyntaxKind::CloseBrace);
    marker.complete(parser, SyntaxKind::TraitAdaptationList);
}

/// One adaptation. The method reference is `A\B::member` or a bare
/// `member` (semi-reserved: keywords parse). `insteadof` makes it a
/// precedence; otherwise `as` with an optional visibility and an
/// optional new name makes it an alias. Whether the reference resolves
/// is semantic.
fn trait_adaptation(parser: &mut Parser) {
    let marker = parser.start();
    match parser.current() {
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace) => {
            name(parser);
            if parser.eat(SyntaxKind::ColonColon) {
                adaptation_member_name(parser);
            }
        }
        Some(kind) if kind.is_keyword() => parser.bump(),
        _ => {
            marker.abandon(parser);
            error_element(parser);
            return;
        }
    }
    if parser.eat(SyntaxKind::InsteadOf) {
        name_list(parser);
        terminate_statement(parser);
        marker.complete(parser, SyntaxKind::TraitPrecedence);
    } else {
        parser.expect(SyntaxKind::As);
        let has_visibility = matches!(
            parser.current(),
            Some(SyntaxKind::Public | SyntaxKind::Protected | SyntaxKind::Private)
        );
        if has_visibility {
            parser.bump();
        }
        if at_semi_reserved_name(parser) {
            parser.bump();
        } else if !has_visibility {
            // `A::b as;`: Zend requires a visibility or a name after
            // `as`; neither is here.
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Identifier));
        }
        terminate_statement(parser);
        marker.complete(parser, SyntaxKind::TraitAlias);
    }
}

/// The member half of `A::member` in an adaptation: an identifier or
/// any keyword (semi-reserved).
fn adaptation_member_name(parser: &mut Parser) {
    expect_semi_reserved_name(parser);
}

/// `enum Name: BackingType implements A { ... }`. `enum` is not
/// reserved: both dispatch sites verified a name follows, so
/// `enum(...)` stays a call. Backing types other than `int` and
/// `string` parse; arity is semantic.
fn enum_declaration(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `enum`
    parser.expect(SyntaxKind::Identifier);
    if parser.eat(SyntaxKind::Colon) {
        super::types::type_expression(parser);
    }
    heritage_clauses(parser);
    member_list(parser);
    marker.complete(parser, SyntaxKind::EnumDeclaration);
}

/// `case Name;` or `case Name = expression;`. Case names are
/// semi-reserved; whether a case belongs here (enums only) and whether
/// the value is required (backed enums) are semantic.
fn enum_case(parser: &mut Parser, marker: Marker) {
    parser.bump(); // `case`
    expect_semi_reserved_name(parser);
    if parser.eat(SyntaxKind::Equals) {
        expression(parser);
    }
    terminate_statement(parser);
    marker.complete(parser, SyntaxKind::EnumCase);
}
