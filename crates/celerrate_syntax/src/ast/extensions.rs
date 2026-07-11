//! Hand-written typed-AST accessors: the logic a generator cannot
//! express. Two families. Semi-reserved names: any keyword may stand
//! for the name (`const FOR = 1;`), so a generated Identifier accessor
//! would silently return `None`; these resolve identifier-or-keyword
//! tokens instead, anchored to the token that precedes the name.
//! Position-dependent roles: in `key => value` shapes, the short
//! ternary, and trait adaptations, the role of an expression depends on
//! where it sits relative to an anchor token, not on its position among
//! siblings.

use super::generated::{
    Argument, ArrayElement, ClassDeclaration, ConstantDeclaration, ConstantElement, EnumCase,
    Expression, ForeachStatement, InterfaceDeclaration, MatchArm, MemberName, MethodDeclaration,
    Name, NamedType, Parameter, PropertyDeclaration, PropertyHook, Statement, TernaryExpression,
    TraitAlias, TraitDeclaration, TraitPrecedence, UseClause, YieldExpression,
};
use super::{AstChildren, AstNode, support};
use crate::syntax_kind::SyntaxKind;
use crate::tree::{SyntaxNode, SyntaxToken};

/// A semi-reserved name position accepts an identifier or any keyword.
fn is_name_token(kind: SyntaxKind) -> bool {
    kind == SyntaxKind::Identifier || kind.is_keyword()
}

/// The direct token children of one node, trivia skipped. Tokens
/// inside child nodes are invisible here, which is what anchoring
/// relies on (a `Name`'s identifiers never leak into its parent).
fn tokens_of(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| !token.kind().is_trivia())
}

/// The first identifier-or-keyword token after the anchor token.
fn name_after(node: &SyntaxNode, anchor: SyntaxKind) -> Option<SyntaxToken> {
    let mut seen_anchor = false;
    tokens_of(node).find(|token| {
        if seen_anchor && is_name_token(token.kind()) {
            return true;
        }
        if token.kind() == anchor {
            seen_anchor = true;
        }
        false
    })
}

/// The member-modifier tokens among a node's direct children.
fn modifier_tokens(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> {
    tokens_of(node).filter(|token| {
        matches!(
            token.kind(),
            SyntaxKind::Public
                | SyntaxKind::Protected
                | SyntaxKind::Private
                | SyntaxKind::Static
                | SyntaxKind::Abstract
                | SyntaxKind::Final
                | SyntaxKind::Readonly
                | SyntaxKind::Var
        )
    })
}

/// The first `Expression` child strictly after `anchor`, by range.
fn expression_after(node: &SyntaxNode, anchor: &SyntaxToken) -> Option<Expression> {
    let minimum = anchor.text_range().end();
    support::children::<Expression>(node)
        .find(|expression| expression.syntax().text_range().start() >= minimum)
}

/// The first `Expression` child strictly between two anchors, by range.
fn expression_between(
    node: &SyntaxNode,
    start: &SyntaxToken,
    end: &SyntaxToken,
) -> Option<Expression> {
    let after = start.text_range().end();
    let before = end.text_range().start();
    support::children::<Expression>(node).find(|expression| {
        let range = expression.syntax().text_range();
        after <= range.start() && range.end() <= before
    })
}

/// The trait-adaptation member token, before `boundary` (`insteadof` or
/// `as`). A qualified reference (`A::member`) parses its member as a
/// bare token (`adaptation_member_name`), reachable directly among
/// `node`'s tokens. An unqualified reference has no `::`, so the whole
/// reference went through the shared `name()` parser: a keyword member
/// (`list insteadof B;`) still lands as a bare token, but an identifier
/// member (`hello insteadof B;`) is wrapped in a `Name` node, invisible
/// to a direct-token search. Both readings are tried here, in that
/// order, so the member resolves regardless of which shape produced it.
fn adaptation_member_token(
    node: &SyntaxNode,
    separator: Option<&SyntaxToken>,
    boundary: &SyntaxToken,
) -> Option<SyntaxToken> {
    let limit = boundary.text_range().start();
    if let Some(separator) = separator {
        let after = separator.text_range().end();
        return tokens_of(node).find(|token| {
            is_name_token(token.kind())
                && token.text_range().start() >= after
                && token.text_range().end() <= limit
        });
    }
    if let Some(name) =
        support::children::<Name>(node).find(|name| name.syntax().text_range().end() <= limit)
    {
        return tokens_of(name.syntax())
            .filter(|token| is_name_token(token.kind()))
            .last();
    }
    tokens_of(node).find(|token| is_name_token(token.kind()) && token.text_range().end() <= limit)
}

/// The two shapes of a named type, unified: `Foo\Bar` is a `Name`
/// node, `array` / `callable` / `static` (and permissively any keyword)
/// is a bare token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedTypeName {
    Name(Name),
    Keyword(SyntaxToken),
}

impl NamedType {
    pub fn name_or_keyword(&self) -> Option<NamedTypeName> {
        if let Some(name) = self.name() {
            return Some(NamedTypeName::Name(name));
        }
        tokens_of(self.syntax())
            .find(|token| token.kind().is_keyword())
            .map(NamedTypeName::Keyword)
    }
}

impl ConstantElement {
    /// The declared name: semi-reserved, the first name token.
    pub fn name_token(&self) -> Option<SyntaxToken> {
        tokens_of(self.syntax()).find(|token| is_name_token(token.kind()))
    }
}

impl EnumCase {
    /// The case name: semi-reserved, after `case`.
    pub fn name_token(&self) -> Option<SyntaxToken> {
        name_after(self.syntax(), SyntaxKind::Case)
    }
}

impl MemberName {
    /// The plain-token form of the name: an identifier or any keyword
    /// (`$object->list()`). `None` for the variable and `{ ... }` forms.
    pub fn name_token(&self) -> Option<SyntaxToken> {
        tokens_of(self.syntax()).find(|token| is_name_token(token.kind()))
    }
}

impl MethodDeclaration {
    /// The method name: semi-reserved, after `function` (skipping the
    /// by-reference `&`).
    pub fn name_token(&self) -> Option<SyntaxToken> {
        name_after(self.syntax(), SyntaxKind::Function)
    }

    pub fn modifiers(&self) -> impl Iterator<Item = SyntaxToken> {
        modifier_tokens(self.syntax())
    }
}

impl ClassDeclaration {
    /// The declared name: semi-reserved (`class List {}` parses), after
    /// `class`. `None` for anonymous classes.
    pub fn name_token(&self) -> Option<SyntaxToken> {
        name_after(self.syntax(), SyntaxKind::Class)
    }

    pub fn modifiers(&self) -> impl Iterator<Item = SyntaxToken> {
        modifier_tokens(self.syntax())
    }
}

impl InterfaceDeclaration {
    pub fn name_token(&self) -> Option<SyntaxToken> {
        name_after(self.syntax(), SyntaxKind::Interface)
    }
}

impl TraitDeclaration {
    pub fn name_token(&self) -> Option<SyntaxToken> {
        name_after(self.syntax(), SyntaxKind::Trait)
    }
}

impl PropertyDeclaration {
    pub fn modifiers(&self) -> impl Iterator<Item = SyntaxToken> {
        modifier_tokens(self.syntax())
    }
}

impl ConstantDeclaration {
    pub fn modifiers(&self) -> impl Iterator<Item = SyntaxToken> {
        modifier_tokens(self.syntax())
    }
}

impl Parameter {
    /// The promotion modifiers of a constructor-promoted parameter.
    pub fn modifiers(&self) -> impl Iterator<Item = SyntaxToken> {
        modifier_tokens(self.syntax())
    }
}

impl PropertyHook {
    /// The hook name (`get`, `set`; semi-reserved in practice): the
    /// first name token after a leading `final`, which is always read
    /// as the modifier.
    pub fn name_token(&self) -> Option<SyntaxToken> {
        let mut names = tokens_of(self.syntax()).filter(|token| is_name_token(token.kind()));
        let first = names.next()?;
        if first.kind() == SyntaxKind::Final {
            return names.next();
        }
        Some(first)
    }
}

impl UseClause {
    /// The import alias: semi-reserved, after `as`.
    pub fn alias_token(&self) -> Option<SyntaxToken> {
        name_after(self.syntax(), SyntaxKind::As)
    }
}

impl Argument {
    /// The named-argument label: the name token directly before the
    /// `:` separator.
    pub fn label_token(&self) -> Option<SyntaxToken> {
        let mut previous: Option<SyntaxToken> = None;
        for token in tokens_of(self.syntax()) {
            if token.kind() == SyntaxKind::Colon {
                return previous.filter(|candidate| is_name_token(candidate.kind()));
            }
            previous = Some(token);
        }
        None
    }
}

impl TernaryExpression {
    pub fn condition(&self) -> Option<Expression> {
        support::child(self.syntax(), 0)
    }

    /// The middle operand; `None` for the short form `?:`.
    pub fn middle(&self) -> Option<Expression> {
        let question = support::token(self.syntax(), &[SyntaxKind::Question])?;
        let colon = support::token(self.syntax(), &[SyntaxKind::Colon])?;
        expression_between(self.syntax(), &question, &colon)
    }

    pub fn third(&self) -> Option<Expression> {
        let colon = support::token(self.syntax(), &[SyntaxKind::Colon])?;
        expression_after(self.syntax(), &colon)
    }
}

impl ArrayElement {
    /// The key: the expression before `=>`; `None` without an arrow.
    pub fn key(&self) -> Option<Expression> {
        let arrow = support::token(self.syntax(), &[SyntaxKind::FatArrow])?;
        support::children::<Expression>(self.syntax())
            .find(|expression| expression.syntax().text_range().end() <= arrow.text_range().start())
    }

    pub fn value(&self) -> Option<Expression> {
        match support::token(self.syntax(), &[SyntaxKind::FatArrow]) {
            Some(arrow) => expression_after(self.syntax(), &arrow),
            None => support::child(self.syntax(), 0),
        }
    }

    pub fn spread_token(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), &[SyntaxKind::Ellipsis])
    }
}

impl YieldExpression {
    /// The `yield from` token, when this is the delegation form.
    pub fn yield_from_token(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), &[SyntaxKind::YieldFrom])
    }

    /// The key: the expression before `=>`; `None` without an arrow.
    pub fn key(&self) -> Option<Expression> {
        let arrow = support::token(self.syntax(), &[SyntaxKind::FatArrow])?;
        support::children::<Expression>(self.syntax())
            .find(|expression| expression.syntax().text_range().end() <= arrow.text_range().start())
    }

    pub fn value(&self) -> Option<Expression> {
        match support::token(self.syntax(), &[SyntaxKind::FatArrow]) {
            Some(arrow) => expression_after(self.syntax(), &arrow),
            None => support::child(self.syntax(), 0),
        }
    }
}

impl ForeachStatement {
    pub fn subject(&self) -> Option<Expression> {
        support::child(self.syntax(), 0)
    }

    /// The key target: between `as` and `=>`; `None` without an arrow.
    pub fn key(&self) -> Option<Expression> {
        let as_keyword = support::token(self.syntax(), &[SyntaxKind::As])?;
        let arrow = support::token(self.syntax(), &[SyntaxKind::FatArrow])?;
        expression_between(self.syntax(), &as_keyword, &arrow)
    }

    /// The value target: after `=>` when present, else after `as`.
    pub fn value(&self) -> Option<Expression> {
        let anchor = support::token(self.syntax(), &[SyntaxKind::FatArrow])
            .or_else(|| support::token(self.syntax(), &[SyntaxKind::As]))?;
        expression_after(self.syntax(), &anchor)
    }

    /// The body: one statement (classic syntax) or the list before
    /// `endforeach` (alternative syntax).
    pub fn statements(&self) -> AstChildren<Statement> {
        support::children(self.syntax())
    }
}

impl MatchArm {
    pub fn is_default(&self) -> bool {
        support::token(self.syntax(), &[SyntaxKind::Default]).is_some()
    }

    /// The conditions before the arrow; empty for a `default` arm.
    pub fn conditions(&self) -> impl Iterator<Item = Expression> {
        let arrow_start = support::token(self.syntax(), &[SyntaxKind::FatArrow])
            .map(|token| token.text_range().start());
        support::children::<Expression>(self.syntax()).take_while(move |expression| {
            arrow_start.is_none_or(|start| expression.syntax().text_range().end() <= start)
        })
    }

    /// The body: the expression after the arrow.
    pub fn body(&self) -> Option<Expression> {
        let arrow = support::token(self.syntax(), &[SyntaxKind::FatArrow])?;
        expression_after(self.syntax(), &arrow)
    }
}

impl TraitPrecedence {
    /// The `Name` before `insteadof`: the qualified class half of
    /// `A::member`, or the bare member name itself when the reference
    /// is unqualified (`hello insteadof B;`).
    pub fn reference_name(&self) -> Option<Name> {
        let insteadof = support::token(self.syntax(), &[SyntaxKind::InsteadOf])?;
        support::children::<Name>(self.syntax())
            .find(|name| name.syntax().text_range().end() <= insteadof.text_range().start())
    }

    /// The member token after `::`; also the unqualified reference
    /// form (`hello insteadof B;`, `list insteadof B;`), whether the
    /// parser kept it as a bare token or wrapped it in a `Name`.
    pub fn member_token(&self) -> Option<SyntaxToken> {
        let insteadof = support::token(self.syntax(), &[SyntaxKind::InsteadOf])?;
        let separator = support::token(self.syntax(), &[SyntaxKind::ColonColon]);
        adaptation_member_token(self.syntax(), separator.as_ref(), &insteadof)
    }

    /// The excluded names after `insteadof`.
    pub fn excluded_names(&self) -> impl Iterator<Item = Name> {
        let minimum = support::token(self.syntax(), &[SyntaxKind::InsteadOf])
            .map(|token| token.text_range().end());
        support::children::<Name>(self.syntax()).filter(move |name| {
            minimum.is_some_and(|start| name.syntax().text_range().start() >= start)
        })
    }
}

impl TraitAlias {
    /// The `Name` before `as`: the qualified class half, or the bare
    /// member name itself when the reference is unqualified.
    pub fn reference_name(&self) -> Option<Name> {
        let as_keyword = support::token(self.syntax(), &[SyntaxKind::As])?;
        support::children::<Name>(self.syntax())
            .find(|name| name.syntax().text_range().end() <= as_keyword.text_range().start())
    }

    /// The member token before `as` (after `::` when present); also the
    /// unqualified reference form, whether the parser kept it as a bare
    /// token or wrapped it in a `Name`.
    pub fn member_token(&self) -> Option<SyntaxToken> {
        let as_keyword = support::token(self.syntax(), &[SyntaxKind::As])?;
        let separator = support::token(self.syntax(), &[SyntaxKind::ColonColon]);
        adaptation_member_token(self.syntax(), separator.as_ref(), &as_keyword)
    }

    /// The visibility after `as` (`hello as protected h2;`).
    pub fn visibility_token(&self) -> Option<SyntaxToken> {
        let as_keyword = support::token(self.syntax(), &[SyntaxKind::As])?;
        let minimum = as_keyword.text_range().end();
        tokens_of(self.syntax()).find(|token| {
            token.text_range().start() >= minimum
                && matches!(
                    token.kind(),
                    SyntaxKind::Public | SyntaxKind::Protected | SyntaxKind::Private
                )
        })
    }

    /// The new name: the first name token after the visibility when
    /// one is present, else after `as`.
    pub fn alias_token(&self) -> Option<SyntaxToken> {
        let anchor = self
            .visibility_token()
            .or_else(|| support::token(self.syntax(), &[SyntaxKind::As]))?;
        let minimum = anchor.text_range().end();
        tokens_of(self.syntax())
            .find(|token| token.text_range().start() >= minimum && is_name_token(token.kind()))
    }
}
