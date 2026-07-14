//! The lowering walk: one pass over a body's syntax turning statements
//! and expressions into the dense, range-free arenas of
//! [`crate::body::BodyIr`] and its range-carrying source-map sibling.
//! Every accessor miss lowers to `Missing`; error-recovery wreckage is
//! tolerated, never a failure.

use celerrate_source::FileId;
use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxNodePtr};

use crate::ast_id::{AstId, AstIdMap};
use crate::body::{
    ArrayEntry, BodyExpression, BodyIr, BodySourceMap, BodyStatement, CallArgument, CatchArm,
    ClassReference, ExpressionId, MatchCase, MemberReference, StatementId,
    StaticVariableDeclaration, StringPart, SwitchArm,
};

/// The kind of a wreckage-tolerant operator token: `Error` when error
/// recovery produced none, keeping the lowering total and deterministic.
fn token_kind_or_error(token: Option<celerrate_syntax::SyntaxToken>) -> SyntaxKind {
    token.map_or(SyntaxKind::Error, |token| token.kind())
}

/// Whether the dereference chain rooted at `expression` contains a
/// `?->` link. Only the four chain kinds are descended; a parenthesized
/// prefix is not part of the chain (PHP semantics: parentheses stop the
/// short-circuit), so it answers `false` here and wraps on its own when
/// its interior is lowered.
fn chain_contains_null_safe(expression: &ast::Expression) -> bool {
    let mut current = expression.clone();
    loop {
        let subject = match &current {
            ast::Expression::MemberAccessExpression(access) => {
                if access
                    .operator_token()
                    .is_some_and(|token| token.kind() == SyntaxKind::NullsafeArrow)
                {
                    return true;
                }
                access.subject()
            }
            ast::Expression::CallExpression(call) => call.callee(),
            ast::Expression::IndexExpression(index) => index.subject(),
            ast::Expression::ScopedAccessExpression(scoped) => scoped.subject(),
            _ => return false,
        };
        match subject {
            Some(subject) => current = subject,
            None => return false,
        }
    }
}

/// Lowers one function or method declaration's body. `None` when the
/// node is not a function or method, or carries no body.
pub(crate) fn lower_body(
    file: FileId,
    map: &AstIdMap,
    declaration: &SyntaxNode,
) -> Option<(BodyIr, BodySourceMap)> {
    let block = match declaration.kind() {
        SyntaxKind::FunctionDeclaration => {
            ast::FunctionDeclaration::cast(declaration.clone())?.block()?
        }
        SyntaxKind::MethodDeclaration => {
            ast::MethodDeclaration::cast(declaration.clone())?.block()?
        }
        _ => return None,
    };
    let mut lowering = Lowering {
        file,
        map,
        ir: BodyIr::default(),
        source_map: BodySourceMap::default(),
    };
    let root = lowering.lower_statements(block.statements());
    lowering.ir.root = root;
    Some((lowering.ir, lowering.source_map))
}

struct Lowering<'a> {
    file: FileId,
    map: &'a AstIdMap,
    ir: BodyIr,
    source_map: BodySourceMap,
}

impl Lowering<'_> {
    fn allocate_expression(
        &mut self,
        expression: BodyExpression,
        node: &SyntaxNode,
    ) -> ExpressionId {
        let Some(id) = ExpressionId::from_index(self.ir.expressions.len()) else {
            return ExpressionId::OVERFLOW;
        };
        self.ir.expressions.push(expression);
        self.source_map.expressions.push(SyntaxNodePtr::new(node));
        id
    }

    fn allocate_statement(&mut self, statement: BodyStatement, node: &SyntaxNode) -> StatementId {
        let Some(id) = StatementId::from_index(self.ir.statements.len()) else {
            return StatementId::OVERFLOW;
        };
        self.ir.statements.push(statement);
        self.source_map.statements.push(SyntaxNodePtr::new(node));
        id
    }

    fn lower_statements(
        &mut self,
        statements: ast::AstChildren<ast::Statement>,
    ) -> Vec<StatementId> {
        statements
            .filter_map(|statement| self.lower_statement(&statement))
            .collect()
    }

    /// A branch position (an `if` arm, a loop body): one statement in
    /// the classic syntax, a list in the alternative syntax. A single
    /// `Block` dissolves into its statements, so brace style is
    /// formatting, not code.
    fn lower_branch(&mut self, statements: Vec<ast::Statement>) -> Vec<StatementId> {
        if statements.len() == 1
            && let Some(ast::Statement::Block(block)) = statements.first()
        {
            return self.lower_statements(block.statements());
        }
        statements
            .iter()
            .filter_map(|statement| self.lower_statement(statement))
            .collect()
    }

    /// The bare expressions of an argument list where the list is
    /// grouping syntax rather than a call (`unset`, `isset`, ...).
    fn lower_argument_expressions(&mut self, list: Option<ast::ArgumentList>) -> Vec<ExpressionId> {
        let arguments: Vec<ast::Argument> = list
            .into_iter()
            .flat_map(|list| list.arguments().collect::<Vec<_>>())
            .collect();
        arguments
            .iter()
            .map(|argument| {
                self.lower_expression_or_missing(argument.expression(), argument.syntax())
            })
            .collect()
    }

    fn lower_for_list(&mut self, list: Option<ast::ForExpressionList>) -> Vec<ExpressionId> {
        let expressions: Vec<ast::Expression> = list
            .into_iter()
            .flat_map(|list| list.expressions().collect::<Vec<_>>())
            .collect();
        expressions
            .iter()
            .map(|expression| self.lower_expression(expression))
            .collect()
    }

    fn lower_block_statements(&mut self, block: Option<ast::Block>) -> Vec<StatementId> {
        match block {
            Some(block) => self.lower_statements(block.statements()),
            None => Vec::new(),
        }
    }

    fn lower_statement(&mut self, statement: &ast::Statement) -> Option<StatementId> {
        let lowered = match statement {
            ast::Statement::Block(block) => BodyStatement::Block {
                statements: self.lower_statements(block.statements()),
            },
            ast::Statement::EmptyStatement(_) => return None,
            ast::Statement::EchoStatement(echo) => {
                let expressions: Vec<ast::Expression> = echo.expressions().collect();
                BodyStatement::Echo {
                    values: expressions
                        .iter()
                        .map(|expression| self.lower_expression(expression))
                        .collect(),
                }
            }
            ast::Statement::BreakStatement(break_statement) => BodyStatement::Break {
                level: break_statement
                    .expression()
                    .map(|expression| self.lower_expression(&expression)),
            },
            ast::Statement::ContinueStatement(continue_statement) => BodyStatement::Continue {
                level: continue_statement
                    .expression()
                    .map(|expression| self.lower_expression(&expression)),
            },
            ast::Statement::GlobalStatement(global) => {
                let expressions: Vec<ast::Expression> = global.expressions().collect();
                BodyStatement::Global {
                    targets: expressions
                        .iter()
                        .map(|expression| self.lower_expression(expression))
                        .collect(),
                }
            }
            ast::Statement::StaticStatement(static_statement) => {
                let declarations: Vec<ast::StaticVariable> =
                    static_statement.static_variables().collect();
                let mut variables = Vec::new();
                for declaration in &declarations {
                    let Some(name) = declaration.name_token() else {
                        continue;
                    };
                    variables.push(StaticVariableDeclaration {
                        name: name.text().trim_start_matches('$').to_owned(),
                        initializer: declaration
                            .expression()
                            .map(|expression| self.lower_expression(&expression)),
                    });
                }
                BodyStatement::StaticVariables { variables }
            }
            ast::Statement::UnsetStatement(unset) => BodyStatement::Unset {
                targets: self.lower_argument_expressions(unset.argument_list()),
            },
            ast::Statement::GotoStatement(goto) => BodyStatement::Goto {
                label: goto.label_token().map(|token| token.text().to_owned()),
            },
            ast::Statement::LabelStatement(label) => BodyStatement::Label {
                name: label.name_token().map(|token| token.text().to_owned()),
            },
            ast::Statement::IfStatement(if_statement) => self.lower_if(if_statement),
            ast::Statement::WhileStatement(while_statement) => BodyStatement::While {
                condition: self.lower_expression_or_missing(
                    while_statement.condition(),
                    while_statement.syntax(),
                ),
                body: self.lower_branch(while_statement.statements().collect()),
            },
            ast::Statement::DoWhileStatement(do_while) => BodyStatement::DoWhile {
                body: self.lower_branch(do_while.body().into_iter().collect()),
                condition: self
                    .lower_expression_or_missing(do_while.condition(), do_while.syntax()),
            },
            ast::Statement::ForStatement(for_statement) => BodyStatement::For {
                initializers: self.lower_for_list(for_statement.initializers()),
                conditions: self.lower_for_list(for_statement.condition()),
                updates: self.lower_for_list(for_statement.updates()),
                body: self.lower_branch(for_statement.statements().collect()),
            },
            ast::Statement::ForeachStatement(foreach) => {
                let by_reference = foreach
                    .syntax()
                    .children_with_tokens()
                    .filter_map(|element| element.into_token())
                    .any(|token| token.kind() == SyntaxKind::Ampersand);
                BodyStatement::Foreach {
                    subject: self.lower_expression_or_missing(foreach.subject(), foreach.syntax()),
                    key: foreach.key().map(|key| self.lower_expression(&key)),
                    value: self.lower_expression_or_missing(foreach.value(), foreach.syntax()),
                    by_reference,
                    body: self.lower_branch(foreach.statements().collect()),
                }
            }
            ast::Statement::SwitchStatement(switch) => {
                let switch_cases: Vec<ast::SwitchCase> = switch.switch_cases().collect();
                let mut cases = Vec::new();
                for case in &switch_cases {
                    cases.push(SwitchArm {
                        condition: case
                            .condition()
                            .map(|condition| self.lower_expression(&condition)),
                        statements: self.lower_statements(case.statements()),
                    });
                }
                BodyStatement::Switch {
                    subject: self.lower_expression_or_missing(switch.condition(), switch.syntax()),
                    cases,
                }
            }
            ast::Statement::TryStatement(try_statement) => {
                let clauses: Vec<ast::CatchClause> = try_statement.catch_clauses().collect();
                let mut catches = Vec::new();
                for clause in &clauses {
                    catches.push(CatchArm {
                        types: clause.names().map(|name| name.text()).collect(),
                        variable: clause
                            .variable_reference()
                            .and_then(|variable| variable.name_token())
                            .map(|token| token.text().trim_start_matches('$').to_owned()),
                        statements: self.lower_block_statements(clause.block()),
                    });
                }
                BodyStatement::Try {
                    body: self.lower_block_statements(try_statement.block()),
                    catches,
                    finally: try_statement
                        .finally_clause()
                        .map(|clause| self.lower_block_statements(clause.block())),
                }
            }
            ast::Statement::DeclareStatement(declare) => BodyStatement::Declare {
                statements: self.lower_statements(declare.statements()),
            },
            ast::Statement::FunctionDeclaration(_)
            | ast::Statement::ConstantDeclaration(_)
            | ast::Statement::NamespaceDeclaration(_)
            | ast::Statement::UseDeclaration(_)
            | ast::Statement::ClassDeclaration(_)
            | ast::Statement::InterfaceDeclaration(_)
            | ast::Statement::TraitDeclaration(_)
            | ast::Statement::EnumDeclaration(_) => match self.map.index_of(statement.syntax()) {
                Some(index) => BodyStatement::Declaration {
                    declaration: AstId {
                        file: self.file,
                        index,
                    },
                },
                None => BodyStatement::Missing,
            },
            ast::Statement::ExpressionStatement(expression_statement) => {
                BodyStatement::Expression {
                    expression: self.lower_expression_or_missing(
                        expression_statement.expression(),
                        expression_statement.syntax(),
                    ),
                }
            }
            ast::Statement::ReturnStatement(return_statement) => BodyStatement::Return {
                value: return_statement
                    .expression()
                    .map(|expression| self.lower_expression(&expression)),
            },
        };
        Some(self.allocate_statement(lowered, statement.syntax()))
    }

    fn lower_if(&mut self, if_statement: &ast::IfStatement) -> BodyStatement {
        let condition =
            self.lower_expression_or_missing(if_statement.condition(), if_statement.syntax());
        let then_branch = self.lower_branch(if_statement.statements().collect());
        let else_ifs: Vec<ast::ElseIfClause> = if_statement.else_if_clauses().collect();
        let else_branch = self.lower_else(&else_ifs, if_statement.else_clause().as_ref());
        BodyStatement::If {
            condition,
            then_branch,
            else_branch,
        }
    }

    /// `elseif` is sugar for `else { if ... }`: each clause nests one
    /// synthetic `If` (anchored on the clause node), the `else` clause
    /// innermost.
    fn lower_else(
        &mut self,
        else_ifs: &[ast::ElseIfClause],
        else_clause: Option<&ast::ElseClause>,
    ) -> Vec<StatementId> {
        let Some((first, rest)) = else_ifs.split_first() else {
            return match else_clause {
                Some(clause) => self.lower_branch(clause.statements().collect()),
                None => Vec::new(),
            };
        };
        let condition = self.lower_expression_or_missing(first.condition(), first.syntax());
        let then_branch = self.lower_branch(first.statements().collect());
        let else_branch = self.lower_else(rest, else_clause);
        let nested = self.allocate_statement(
            BodyStatement::If {
                condition,
                then_branch,
                else_branch,
            },
            first.syntax(),
        );
        vec![nested]
    }

    fn lower_expression(&mut self, expression: &ast::Expression) -> ExpressionId {
        let id = self.lower_chain_link(expression);
        if chain_contains_null_safe(expression) {
            return self.allocate_expression(
                BodyExpression::NullSafeChain { chain: id },
                expression.syntax(),
            );
        }
        id
    }

    fn lower_chain_link(&mut self, expression: &ast::Expression) -> ExpressionId {
        match expression {
            ast::Expression::MemberAccessExpression(access) => {
                let receiver = self.lower_link_or_missing(access.subject(), access.syntax());
                let null_safe = access
                    .operator_token()
                    .is_some_and(|token| token.kind() == SyntaxKind::NullsafeArrow);
                let member = self.lower_member_reference(access.member_name());
                self.allocate_expression(
                    BodyExpression::MemberAccess {
                        receiver,
                        member,
                        null_safe,
                    },
                    access.syntax(),
                )
            }
            ast::Expression::ScopedAccessExpression(scoped) => {
                let subject = self.lower_link_or_missing(scoped.subject(), scoped.syntax());
                let member = self.lower_member_reference(scoped.member_name());
                self.allocate_expression(
                    BodyExpression::ScopedAccess { subject, member },
                    scoped.syntax(),
                )
            }
            ast::Expression::CallExpression(call) => {
                let callee = self.lower_link_or_missing(call.callee(), call.syntax());
                let is_first_class = call.argument_list().is_some_and(|list| {
                    list.ellipsis_token().is_some() && list.arguments().next().is_none()
                });
                let lowered = if is_first_class {
                    BodyExpression::CallableReference { callee }
                } else {
                    BodyExpression::Call {
                        callee,
                        arguments: self.lower_call_arguments(call.argument_list()),
                    }
                };
                self.allocate_expression(lowered, call.syntax())
            }
            ast::Expression::IndexExpression(index) => {
                let subject = self.lower_link_or_missing(index.subject(), index.syntax());
                let lowered = BodyExpression::Index {
                    subject,
                    index: index
                        .index()
                        .map(|expression| self.lower_expression(&expression)),
                };
                self.allocate_expression(lowered, index.syntax())
            }
            other => self.lower_expression_kind(other),
        }
    }

    fn lower_link_or_missing(
        &mut self,
        expression: Option<ast::Expression>,
        parent: &SyntaxNode,
    ) -> ExpressionId {
        match expression {
            Some(expression) => self.lower_chain_link(&expression),
            None => self.allocate_expression(BodyExpression::Missing, parent),
        }
    }

    fn lower_expression_or_missing(
        &mut self,
        expression: Option<ast::Expression>,
        parent: &SyntaxNode,
    ) -> ExpressionId {
        match expression {
            Some(expression) => self.lower_expression(&expression),
            None => self.allocate_expression(BodyExpression::Missing, parent),
        }
    }

    fn lower_member_reference(&mut self, member: Option<ast::MemberName>) -> MemberReference {
        let Some(member) = member else {
            return MemberReference::Missing;
        };
        if let Some(token) = member.name_token() {
            return MemberReference::Named {
                name: token.text().to_owned(),
            };
        }
        let braced = member
            .syntax()
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .any(|token| token.kind() == SyntaxKind::OpenBrace);
        match member.expression() {
            Some(ast::Expression::VariableReference(variable)) if !braced => {
                match variable.name_token() {
                    Some(token) => MemberReference::Variable {
                        name: token.text().trim_start_matches('$').to_owned(),
                    },
                    None => MemberReference::Missing,
                }
            }
            Some(expression) => MemberReference::Computed {
                expression: self.lower_expression(&expression),
            },
            None => MemberReference::Missing,
        }
    }

    fn lower_call_arguments(&mut self, list: Option<ast::ArgumentList>) -> Vec<CallArgument> {
        let arguments: Vec<ast::Argument> = list
            .into_iter()
            .flat_map(|list| list.arguments().collect::<Vec<_>>())
            .collect();
        let mut lowered = Vec::new();
        for argument in &arguments {
            lowered.push(CallArgument {
                label: argument.label_token().map(|token| token.text().to_owned()),
                spread: argument.spread_token().is_some(),
                value: self.lower_expression_or_missing(argument.expression(), argument.syntax()),
            });
        }
        lowered
    }

    fn lower_expression_kind(&mut self, expression: &ast::Expression) -> ExpressionId {
        let node = expression.syntax();
        let lowered = match expression {
            ast::Expression::Literal(literal) => match literal.value_token() {
                Some(token) => BodyExpression::Literal {
                    text: token.text().to_owned(),
                },
                None => BodyExpression::Missing,
            },
            ast::Expression::VariableReference(variable) => match variable.name_token() {
                Some(token) => BodyExpression::Variable {
                    name: token.text().trim_start_matches('$').to_owned(),
                },
                None => BodyExpression::Missing,
            },
            ast::Expression::NameExpression(name) => {
                let text = name.name().map(|name| name.text()).or_else(|| {
                    name.static_keyword_token()
                        .map(|token| token.text().to_owned())
                });
                match text {
                    Some(text) => BodyExpression::NamedReference { text },
                    None => BodyExpression::Missing,
                }
            }
            ast::Expression::DynamicVariableExpression(dynamic) => {
                BodyExpression::DynamicVariable {
                    target: self
                        .lower_expression_or_missing(dynamic.expression(), dynamic.syntax()),
                }
            }
            ast::Expression::ParenthesizedExpression(parenthesized) => {
                // Transparent: `($x)` and `$x` are one IR. The null-safe
                // chain boundary this creates is handled in Task 4.
                return self.lower_expression_or_missing(
                    parenthesized.expression(),
                    parenthesized.syntax(),
                );
            }
            ast::Expression::PrefixExpression(prefix) => BodyExpression::Unary {
                operator: token_kind_or_error(prefix.operator_token()),
                operand: self.lower_expression_or_missing(prefix.operand(), prefix.syntax()),
            },
            ast::Expression::PostfixExpression(postfix) => BodyExpression::Postfix {
                operator: token_kind_or_error(postfix.operator_token()),
                operand: self.lower_expression_or_missing(postfix.operand(), postfix.syntax()),
            },
            ast::Expression::BinaryExpression(binary) => BodyExpression::Binary {
                operator: token_kind_or_error(binary.operator_token()),
                lhs: self.lower_expression_or_missing(binary.lhs(), binary.syntax()),
                rhs: self.lower_expression_or_missing(binary.rhs(), binary.syntax()),
            },
            ast::Expression::AssignmentExpression(assignment) => BodyExpression::Assignment {
                operator: token_kind_or_error(assignment.operator_token()),
                by_reference: assignment.by_reference_token().is_some(),
                target: self.lower_expression_or_missing(assignment.target(), assignment.syntax()),
                value: self.lower_expression_or_missing(assignment.value(), assignment.syntax()),
            },
            ast::Expression::CastExpression(cast) => BodyExpression::Cast {
                operator: token_kind_or_error(cast.operator_token()),
                operand: self.lower_expression_or_missing(cast.operand(), cast.syntax()),
            },
            ast::Expression::TernaryExpression(ternary) => BodyExpression::Ternary {
                condition: self.lower_expression_or_missing(ternary.condition(), ternary.syntax()),
                middle: ternary
                    .middle()
                    .map(|middle| self.lower_expression(&middle)),
                alternative: self.lower_expression_or_missing(ternary.third(), ternary.syntax()),
            },
            ast::Expression::ArrayExpression(array) => BodyExpression::Array {
                entries: self.lower_array_entries(array.syntax()),
            },
            ast::Expression::ListExpression(list) => BodyExpression::Array {
                entries: self.lower_array_entries(list.syntax()),
            },
            ast::Expression::InterpolatedString(string) => BodyExpression::InterpolatedString {
                parts: self.lower_string_parts(string.syntax()),
            },
            ast::Expression::HeredocExpression(heredoc) => BodyExpression::InterpolatedString {
                parts: self.lower_string_parts(heredoc.syntax()),
            },
            ast::Expression::ShellExecExpression(shell) => BodyExpression::ShellExec {
                parts: self.lower_string_parts(shell.syntax()),
            },
            ast::Expression::IssetExpression(isset) => BodyExpression::Isset {
                targets: self.lower_argument_expressions(isset.argument_list()),
            },
            ast::Expression::EmptyExpression(empty) => BodyExpression::Empty {
                target: self.lower_first_argument_or_missing(empty.argument_list(), empty.syntax()),
            },
            ast::Expression::EvalExpression(eval) => BodyExpression::Eval {
                argument: self.lower_first_argument_or_missing(eval.argument_list(), eval.syntax()),
            },
            ast::Expression::ExitExpression(exit) => BodyExpression::Exit {
                argument: exit
                    .argument_list()
                    .and_then(|list| list.arguments().next())
                    .and_then(|argument| argument.expression())
                    .map(|expression| self.lower_expression(&expression)),
            },
            ast::Expression::PrintExpression(print) => BodyExpression::Print {
                operand: self.lower_expression_or_missing(print.operand(), print.syntax()),
            },
            ast::Expression::CloneExpression(clone) => {
                let operand = match clone.operand() {
                    Some(operand) => self.lower_expression(&operand),
                    None => {
                        self.lower_first_argument_or_missing(clone.argument_list(), clone.syntax())
                    }
                };
                BodyExpression::Clone { operand }
            }
            ast::Expression::ThrowExpression(throw) => BodyExpression::Throw {
                operand: self.lower_expression_or_missing(throw.operand(), throw.syntax()),
            },
            ast::Expression::YieldExpression(yield_expression) => BodyExpression::Yield {
                key: yield_expression
                    .key()
                    .map(|key| self.lower_expression(&key)),
                value: yield_expression
                    .value()
                    .map(|value| self.lower_expression(&value)),
                delegated: yield_expression.yield_from_token().is_some(),
            },
            ast::Expression::IncludeExpression(include) => BodyExpression::Include {
                operator: token_kind_or_error(include.operator_token()),
                operand: self.lower_expression_or_missing(include.operand(), include.syntax()),
            },
            ast::Expression::MatchExpression(match_expression) => {
                let match_arms: Vec<ast::MatchArm> = match_expression.match_arms().collect();
                let mut arms = Vec::new();
                for arm in &match_arms {
                    let conditions: Vec<ast::Expression> = arm.conditions().collect();
                    arms.push(MatchCase {
                        conditions: conditions
                            .iter()
                            .map(|condition| self.lower_expression(condition))
                            .collect(),
                        is_default: arm.is_default(),
                        body: self.lower_expression_or_missing(arm.body(), arm.syntax()),
                    });
                }
                BodyExpression::Match {
                    subject: self.lower_expression_or_missing(
                        match_expression.subject(),
                        match_expression.syntax(),
                    ),
                    arms,
                }
            }
            ast::Expression::NewExpression(new) => {
                let (class, arguments) = if let Some(declaration) = new.class_declaration() {
                    let class = match self.map.index_of(declaration.syntax()) {
                        Some(index) => ClassReference::Anonymous {
                            declaration: AstId {
                                file: self.file,
                                index,
                            },
                        },
                        None => ClassReference::Missing,
                    };
                    // An anonymous class carries its own constructor
                    // arguments inside the declaration node.
                    (
                        class,
                        self.lower_call_arguments(declaration.argument_list()),
                    )
                } else {
                    let class = if new.static_keyword_token().is_some() {
                        ClassReference::StaticKeyword
                    } else if let Some(name) = new.name() {
                        ClassReference::Named { name: name.text() }
                    } else if let Some(expression) = new.expression() {
                        ClassReference::Dynamic {
                            expression: self.lower_expression(&expression),
                        }
                    } else {
                        ClassReference::Missing
                    };
                    (class, self.lower_call_arguments(new.argument_list()))
                };
                BodyExpression::New { class, arguments }
            }
            _ => BodyExpression::Missing,
        };
        self.allocate_expression(lowered, node)
    }

    fn lower_first_argument_or_missing(
        &mut self,
        list: Option<ast::ArgumentList>,
        parent: &SyntaxNode,
    ) -> ExpressionId {
        let expression = list
            .and_then(|list| list.arguments().next())
            .and_then(|argument| argument.expression());
        self.lower_expression_or_missing(expression, parent)
    }

    /// Array and list entries in written order, empty destructuring
    /// slots kept as holes: a comma with no element since the previous
    /// separator marks one (`[, $second]`), a trailing comma does not.
    fn lower_array_entries(&mut self, node: &SyntaxNode) -> Vec<ArrayEntry> {
        let mut entries = Vec::new();
        let mut element_since_separator = false;
        for element in node.children_with_tokens() {
            match element {
                celerrate_syntax::SyntaxElement::Node(child) => {
                    let Some(array_element) = ast::ArrayElement::cast(child) else {
                        continue;
                    };
                    let by_reference = array_element
                        .syntax()
                        .children_with_tokens()
                        .filter_map(|element| element.into_token())
                        .any(|token| token.kind() == SyntaxKind::Ampersand);
                    entries.push(ArrayEntry::Element {
                        key: array_element.key().map(|key| self.lower_expression(&key)),
                        value: self.lower_expression_or_missing(
                            array_element.value(),
                            array_element.syntax(),
                        ),
                        spread: array_element.spread_token().is_some(),
                        by_reference,
                    });
                    element_since_separator = true;
                }
                celerrate_syntax::SyntaxElement::Token(token)
                    if token.kind() == SyntaxKind::Comma =>
                {
                    if !element_since_separator {
                        entries.push(ArrayEntry::Hole);
                    }
                    element_since_separator = false;
                }
                _ => {}
            }
        }
        entries
    }

    fn lower_string_parts(&mut self, node: &SyntaxNode) -> Vec<StringPart> {
        let mut parts = Vec::new();
        for element in node.children_with_tokens() {
            match element {
                celerrate_syntax::SyntaxElement::Token(token)
                    if token.kind() == SyntaxKind::StringFragment =>
                {
                    parts.push(StringPart::Fragment {
                        text: token.text().to_owned(),
                    });
                }
                celerrate_syntax::SyntaxElement::Node(child) => {
                    let Some(interpolation) = ast::StringInterpolation::cast(child) else {
                        continue;
                    };
                    parts.push(match &interpolation {
                        ast::StringInterpolation::SimpleInterpolation(simple) => {
                            StringPart::Simple {
                                text: simple.syntax().text().to_string(),
                            }
                        }
                        ast::StringInterpolation::BraceInterpolation(brace) => {
                            StringPart::Interpolation {
                                expression: self.lower_expression_or_missing(
                                    brace.expression(),
                                    brace.syntax(),
                                ),
                            }
                        }
                        ast::StringInterpolation::DollarBraceInterpolation(brace) => {
                            StringPart::Interpolation {
                                expression: self.lower_expression_or_missing(
                                    brace.expression(),
                                    brace.syntax(),
                                ),
                            }
                        }
                    });
                }
                _ => {}
            }
        }
        parts
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use celerrate_syntax::SyntaxKind;

    use crate::body::{
        ArrayEntry, BodyExpression, BodyIr, BodySourceMap, BodyStatement, ClassReference,
        MemberReference, StringPart,
    };

    /// Lowers the first function or method of `source`.
    fn lowered(source: &str) -> (BodyIr, BodySourceMap) {
        let parse = celerrate_syntax::parse(source);
        let root = parse.tree();
        let declaration = root
            .descendants()
            .find(|node| {
                matches!(
                    node.kind(),
                    SyntaxKind::FunctionDeclaration | SyntaxKind::MethodDeclaration,
                )
            })
            .unwrap();
        let map = crate::ast_id::AstIdMap::from_root(&root);
        super::lower_body(celerrate_source::FileId::new(0), &map, &declaration).unwrap()
    }

    fn body(source: &str) -> BodyIr {
        lowered(source).0
    }

    fn root_statement(ir: &BodyIr, position: usize) -> &BodyStatement {
        ir.statement(ir.root[position]).unwrap()
    }

    fn root_expression(ir: &BodyIr, position: usize) -> &BodyExpression {
        let BodyStatement::Expression { expression } = root_statement(ir, position) else {
            panic!("expected an expression statement");
        };
        ir.expression(*expression).unwrap()
    }

    #[test]
    fn a_return_of_a_literal_lowers() {
        let ir = body("<?php function f() { return 1; }");
        assert_eq!(ir.root.len(), 1);
        let BodyStatement::Return { value: Some(value) } = root_statement(&ir, 0) else {
            panic!("expected a return with a value");
        };
        assert_eq!(
            ir.expression(*value),
            Some(&BodyExpression::Literal {
                text: "1".to_owned()
            }),
        );
    }

    #[test]
    fn a_bare_return_has_no_value() {
        let ir = body("<?php function f() { return; }");
        assert_eq!(
            root_statement(&ir, 0),
            &BodyStatement::Return { value: None },
        );
    }

    #[test]
    fn variables_lose_their_sigil_and_names_keep_their_spelling() {
        let ir = body("<?php function f() { $count; PHP_EOL; }");
        assert_eq!(
            root_expression(&ir, 0),
            &BodyExpression::Variable {
                name: "count".to_owned()
            },
        );
        assert_eq!(
            root_expression(&ir, 1),
            &BodyExpression::NamedReference {
                text: "PHP_EOL".to_owned()
            },
        );
    }

    #[test]
    fn a_method_body_lowers_and_a_bodyless_method_answers_none() {
        let ir = body("<?php class A { public function m() { return 1; } }");
        assert_eq!(ir.root.len(), 1);

        let parse = celerrate_syntax::parse("<?php interface I { public function m(); }");
        let method = parse
            .tree()
            .descendants()
            .find(|node| node.kind() == SyntaxKind::MethodDeclaration)
            .unwrap();
        let map = crate::ast_id::AstIdMap::from_root(&parse.tree());
        assert!(super::lower_body(celerrate_source::FileId::new(0), &map, &method).is_none());
    }

    #[test]
    fn a_non_body_declaration_answers_none() {
        let parse = celerrate_syntax::parse("<?php class A { public int $x = 0; }");
        let property = parse
            .tree()
            .descendants()
            .find(|node| node.kind() == SyntaxKind::PropertyDeclaration)
            .unwrap();
        let map = crate::ast_id::AstIdMap::from_root(&parse.tree());
        assert!(super::lower_body(celerrate_source::FileId::new(0), &map, &property).is_none());
    }

    #[test]
    fn formatting_only_edits_produce_an_identical_value() {
        // The load-bearing claim of the whole plan, pinned from the
        // first task: no text offset ever enters the IR.
        let compact = body("<?php function f() { return 1; }");
        let spread_out = body("<?php function f()   {\n\n    return    1;\n}");
        assert_eq!(compact, spread_out);

        let different = body("<?php function f() { return 2; }");
        assert_ne!(compact, different);
    }

    #[test]
    fn identifiers_are_dense_and_reconcile_through_the_source_map() {
        let source = "<?php function f() { $x; return $x; }";
        let (ir, map) = lowered(source);
        assert_eq!(ir.expressions.len(), 2);
        assert_eq!(ir.statements.len(), 2);

        let parse = celerrate_syntax::parse(source);
        for (index, _) in ir.expressions.iter().enumerate() {
            let id = crate::body::ExpressionId::from_index(index).unwrap();
            let pointer = map.expression_pointer(id).unwrap();
            assert!(pointer.try_to_node(&parse.tree()).is_some());
        }
        for (index, _) in ir.statements.iter().enumerate() {
            let id = crate::body::StatementId::from_index(index).unwrap();
            let pointer = map.statement_pointer(id).unwrap();
            assert!(pointer.try_to_node(&parse.tree()).is_some());
        }
    }

    #[test]
    fn truncated_input_lowers_without_failure() {
        // The never-fail contract: error recovery's wreckage lowers
        // (to Missing forms) rather than failing the walk, and the
        // arenas stay parallel to their source map. Task 7's
        // adversarial batch pins arena integrity broadly.
        let (ir, map) = lowered("<?php function f() { $x = ");
        assert_eq!(ir.statements.len(), map.statements.len());
        assert_eq!(ir.expressions.len(), map.expressions.len());
    }

    #[test]
    fn the_three_if_syntaxes_produce_one_identical_body() {
        // The dissolution rule: a single-Block branch dissolves into
        // its statements, so brace style is formatting, not code.
        let plain = body("<?php function f() { if ($c) return 1; }");
        let braced = body("<?php function f() { if ($c) { return 1; } }");
        let alternative = body("<?php function f() { if ($c): return 1; endif; }");
        assert_eq!(plain, braced);
        assert_eq!(braced, alternative);
    }

    #[test]
    fn elseif_chains_nest_as_else_if() {
        let ir = body(
            "<?php function f() { if ($a) { return 1; } elseif ($b) { return 2; } else { return 3; } }",
        );
        let BodyStatement::If { else_branch, .. } = root_statement(&ir, 0) else {
            panic!("expected an if");
        };
        assert_eq!(else_branch.len(), 1);
        let BodyStatement::If {
            then_branch,
            else_branch: innermost,
            ..
        } = ir.statement(else_branch[0]).unwrap()
        else {
            panic!("expected the nested elseif as an if");
        };
        assert_eq!(then_branch.len(), 1);
        assert_eq!(innermost.len(), 1);
        assert!(matches!(
            ir.statement(innermost[0]).unwrap(),
            BodyStatement::Return { value: Some(_) },
        ));
    }

    #[test]
    fn loops_lower_with_their_shapes() {
        let ir = body(
            "<?php function f() { while ($a) { $x; } do { $y; } while ($b); for ($i = 0; $i < 3; $i++) { $z; } }",
        );
        assert!(
            matches!(root_statement(&ir, 0), BodyStatement::While { body, .. } if body.len() == 1)
        );
        assert!(
            matches!(root_statement(&ir, 1), BodyStatement::DoWhile { body, .. } if body.len() == 1)
        );
        let BodyStatement::For {
            initializers,
            conditions,
            updates,
            body: for_body,
        } = root_statement(&ir, 2)
        else {
            panic!("expected a for");
        };
        assert_eq!(
            (
                initializers.len(),
                conditions.len(),
                updates.len(),
                for_body.len()
            ),
            (1, 1, 1, 1),
        );
    }

    #[test]
    fn foreach_lowers_key_value_and_by_reference() {
        let ir = body("<?php function f() { foreach ($items as $key => &$value) { $value; } }");
        let BodyStatement::Foreach {
            key,
            by_reference,
            body: foreach_body,
            ..
        } = root_statement(&ir, 0)
        else {
            panic!("expected a foreach");
        };
        assert!(key.is_some());
        assert!(*by_reference);
        assert_eq!(foreach_body.len(), 1);

        let ir = body("<?php function f() { foreach ($items as $value) {} }");
        let BodyStatement::Foreach {
            key, by_reference, ..
        } = root_statement(&ir, 0)
        else {
            panic!("expected a foreach");
        };
        assert!(key.is_none());
        assert!(!*by_reference);
    }

    #[test]
    fn switch_lowers_cases_and_default() {
        let ir =
            body("<?php function f() { switch ($x) { case 1: return 1; default: return 2; } }");
        let BodyStatement::Switch { cases, .. } = root_statement(&ir, 0) else {
            panic!("expected a switch");
        };
        assert_eq!(cases.len(), 2);
        assert!(cases[0].condition.is_some());
        assert!(cases[1].condition.is_none());
        assert_eq!(cases[1].statements.len(), 1);
    }

    #[test]
    fn try_catch_finally_lowers_types_and_variables() {
        let ir = body(
            "<?php function f() { try { $a; } catch (FooError | BarError $e) { $b; } catch (BazError) { $c; } finally { $d; } }",
        );
        let BodyStatement::Try {
            body: try_body,
            catches,
            finally,
        } = root_statement(&ir, 0)
        else {
            panic!("expected a try");
        };
        assert_eq!(try_body.len(), 1);
        assert_eq!(catches.len(), 2);
        assert_eq!(
            catches[0].types,
            vec!["FooError".to_owned(), "BarError".to_owned()]
        );
        assert_eq!(catches[0].variable.as_deref(), Some("e"));
        assert_eq!(catches[1].variable, None);
        assert_eq!(finally.as_ref().map(Vec::len), Some(1));
    }

    #[test]
    fn the_simple_statement_forms_lower() {
        let ir = body(
            "<?php function f() { echo 1, 2; unset($a, $b); global $g; static $s = 1; break 2; continue; goto end; end: ; declare(ticks=1) { $t; } }",
        );
        assert!(
            matches!(root_statement(&ir, 0), BodyStatement::Echo { values } if values.len() == 2)
        );
        assert!(
            matches!(root_statement(&ir, 1), BodyStatement::Unset { targets } if targets.len() == 2)
        );
        assert!(
            matches!(root_statement(&ir, 2), BodyStatement::Global { targets } if targets.len() == 1)
        );
        let BodyStatement::StaticVariables { variables } = root_statement(&ir, 3) else {
            panic!("expected static variables");
        };
        assert_eq!(variables.len(), 1);
        assert_eq!(variables[0].name, "s");
        assert!(variables[0].initializer.is_some());
        assert!(matches!(
            root_statement(&ir, 4),
            BodyStatement::Break { level: Some(_) }
        ));
        assert!(matches!(
            root_statement(&ir, 5),
            BodyStatement::Continue { level: None }
        ));
        assert!(
            matches!(root_statement(&ir, 6), BodyStatement::Goto { label: Some(label) } if label == "end")
        );
        assert!(
            matches!(root_statement(&ir, 7), BodyStatement::Label { name: Some(name) } if name == "end")
        );
        assert!(
            matches!(root_statement(&ir, 8), BodyStatement::Declare { statements } if statements.len() == 1)
        );
    }

    #[test]
    fn empty_statements_vanish_and_a_standalone_block_stays() {
        let ir = body("<?php function f() { ; $x; ; }");
        assert_eq!(ir.root.len(), 1);

        let ir = body("<?php function f() { { $x; } }");
        assert!(
            matches!(root_statement(&ir, 0), BodyStatement::Block { statements } if statements.len() == 1)
        );
    }

    #[test]
    fn a_nested_declaration_lowers_to_its_identity() {
        // Numbering: outer function = 0, nested function = 1 (the
        // traversal descends into bodies; the 1a contract).
        let ir = body("<?php function f() { function nested() {} $x; }");
        let BodyStatement::Declaration { declaration } = root_statement(&ir, 0) else {
            panic!("expected a declaration statement");
        };
        assert_eq!(declaration.index, 1);
        assert_eq!(ir.root.len(), 2);
    }

    #[test]
    fn operators_are_distinguished_by_their_token_kind() {
        let ir = body("<?php function f() { $a + $b; $a . $b; $a instanceof Foo; $a ?? $b; }");
        assert!(matches!(
            root_expression(&ir, 0),
            BodyExpression::Binary {
                operator: SyntaxKind::Plus,
                ..
            }
        ));
        assert!(matches!(
            root_expression(&ir, 1),
            BodyExpression::Binary {
                operator: SyntaxKind::Dot,
                ..
            }
        ));
        assert!(matches!(
            root_expression(&ir, 2),
            BodyExpression::Binary {
                operator: SyntaxKind::InstanceOf,
                ..
            }
        ));
        assert!(matches!(
            root_expression(&ir, 3),
            BodyExpression::Binary {
                operator: SyntaxKind::QuestionQuestion,
                ..
            }
        ));
    }

    #[test]
    fn prefix_postfix_cast_and_assignment_lower() {
        let ir = body("<?php function f() { !$a; $a++; (int) $a; $a ??= $b; $a = &$b; }");
        assert!(matches!(
            root_expression(&ir, 0),
            BodyExpression::Unary {
                operator: SyntaxKind::Bang,
                ..
            }
        ));
        assert!(matches!(
            root_expression(&ir, 1),
            BodyExpression::Postfix {
                operator: SyntaxKind::PlusPlus,
                ..
            }
        ));
        assert!(matches!(
            root_expression(&ir, 2),
            BodyExpression::Cast {
                operator: SyntaxKind::IntCast,
                ..
            }
        ));
        assert!(matches!(
            root_expression(&ir, 3),
            BodyExpression::Assignment {
                operator: SyntaxKind::QuestionQuestionEquals,
                by_reference: false,
                ..
            },
        ));
        assert!(matches!(
            root_expression(&ir, 4),
            BodyExpression::Assignment {
                operator: SyntaxKind::Equals,
                by_reference: true,
                ..
            },
        ));
    }

    #[test]
    fn the_short_ternary_has_no_middle() {
        let ir = body("<?php function f() { $a ? $b : $c; $a ?: $c; }");
        assert!(matches!(
            root_expression(&ir, 0),
            BodyExpression::Ternary {
                middle: Some(_),
                ..
            }
        ));
        assert!(matches!(
            root_expression(&ir, 1),
            BodyExpression::Ternary { middle: None, .. }
        ));
    }

    #[test]
    fn parentheses_are_transparent() {
        assert_eq!(
            body("<?php function f() { ($x); }"),
            body("<?php function f() { $x; }"),
        );
    }

    #[test]
    fn arrays_lower_keys_spreads_and_destructuring_holes() {
        let ir = body("<?php function f() { [1 => $a, ...$rest, &$b]; }");
        let BodyExpression::Array { entries } = root_expression(&ir, 0) else {
            panic!("expected an array");
        };
        assert_eq!(entries.len(), 3);
        assert!(matches!(
            &entries[0],
            ArrayEntry::Element {
                key: Some(_),
                spread: false,
                ..
            }
        ));
        assert!(matches!(
            &entries[1],
            ArrayEntry::Element { spread: true, .. }
        ));
        assert!(matches!(
            &entries[2],
            ArrayEntry::Element {
                by_reference: true,
                ..
            }
        ));

        let ir = body("<?php function f() { [, $second] = $pair; }");
        let BodyExpression::Assignment { target, .. } = root_expression(&ir, 0) else {
            panic!("expected an assignment");
        };
        let BodyExpression::Array { entries } = ir.expression(*target).unwrap() else {
            panic!("expected an array target");
        };
        assert!(matches!(&entries[0], ArrayEntry::Hole));
        assert!(matches!(&entries[1], ArrayEntry::Element { .. }));

        // A trailing comma is not a hole.
        let ir = body("<?php function f() { [$a, $b,]; }");
        let BodyExpression::Array { entries } = root_expression(&ir, 0) else {
            panic!("expected an array");
        };
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn list_destructuring_lowers_exactly_like_the_bracket_form() {
        assert_eq!(
            body("<?php function f() { list($a, $b) = $pair; }"),
            body("<?php function f() { [$a, $b] = $pair; }"),
        );
    }

    #[test]
    fn interpolated_strings_lower_fragments_and_interpolations() {
        let ir = body("<?php function f() { \"a {$x} b\"; }");
        let BodyExpression::InterpolatedString { parts } = root_expression(&ir, 0) else {
            panic!("expected an interpolated string");
        };
        assert_eq!(parts.len(), 3);
        assert!(matches!(&parts[0], StringPart::Fragment { text } if text == "a "));
        assert!(matches!(&parts[1], StringPart::Interpolation { .. }));
        assert!(matches!(&parts[2], StringPart::Fragment { text } if text == " b"));
    }

    #[test]
    fn simple_interpolations_carry_their_written_text() {
        // The simple syntax reaches one property or index deep; the IR
        // records it as written and defers the semantics.
        let ir = body("<?php function f() { \"$user->name\"; }");
        let BodyExpression::InterpolatedString { parts } = root_expression(&ir, 0) else {
            panic!("expected an interpolated string");
        };
        assert!(matches!(&parts[0], StringPart::Simple { text } if text == "$user->name"));
    }

    #[test]
    fn heredocs_lower_as_interpolated_strings_with_the_label_erased() {
        let first = body("<?php function f() { <<<EOT\nhello $x\nEOT; }");
        let second = body("<?php function f() { <<<OTHER\nhello $x\nOTHER; }");
        // The label is formatting: two heredocs differing only in it
        // produce one identical IR.
        assert_eq!(first, second);
        let BodyExpression::InterpolatedString { .. } = root_expression(&first, 0) else {
            panic!("expected an interpolated string");
        };
    }

    #[test]
    fn the_grouping_forms_lower() {
        let ir = body(
            "<?php function f() { isset($a, $b); empty($c); print $d; clone $e; throw $f; require 'x.php'; `ls $dir`; }",
        );
        assert!(
            matches!(root_expression(&ir, 0), BodyExpression::Isset { targets } if targets.len() == 2)
        );
        assert!(matches!(
            root_expression(&ir, 1),
            BodyExpression::Empty { .. }
        ));
        assert!(matches!(
            root_expression(&ir, 2),
            BodyExpression::Print { .. }
        ));
        assert!(matches!(
            root_expression(&ir, 3),
            BodyExpression::Clone { .. }
        ));
        assert!(matches!(
            root_expression(&ir, 4),
            BodyExpression::Throw { .. }
        ));
        assert!(matches!(
            root_expression(&ir, 5),
            BodyExpression::Include {
                operator: SyntaxKind::Require,
                ..
            }
        ));
        assert!(matches!(
            root_expression(&ir, 6),
            BodyExpression::ShellExec { .. }
        ));
    }

    #[test]
    fn exit_and_yield_encode_valid_absence() {
        let ir = body(
            "<?php function f() { exit; exit(1); yield; yield $v; yield $k => $v; yield from $g; }",
        );
        assert!(matches!(
            root_expression(&ir, 0),
            BodyExpression::Exit { argument: None }
        ));
        assert!(matches!(
            root_expression(&ir, 1),
            BodyExpression::Exit { argument: Some(_) }
        ));
        assert!(matches!(
            root_expression(&ir, 2),
            BodyExpression::Yield {
                key: None,
                value: None,
                delegated: false
            }
        ));
        assert!(matches!(
            root_expression(&ir, 3),
            BodyExpression::Yield {
                key: None,
                value: Some(_),
                delegated: false
            }
        ));
        assert!(matches!(
            root_expression(&ir, 4),
            BodyExpression::Yield {
                key: Some(_),
                value: Some(_),
                delegated: false
            }
        ));
        assert!(matches!(
            root_expression(&ir, 5),
            BodyExpression::Yield {
                delegated: true,
                ..
            }
        ));
    }

    #[test]
    fn match_lowers_arms_and_default() {
        let ir = body("<?php function f() { match ($x) { 1, 2 => 'low', default => 'other' }; }");
        let BodyExpression::Match { arms, .. } = root_expression(&ir, 0) else {
            panic!("expected a match");
        };
        assert_eq!(arms.len(), 2);
        assert_eq!(arms[0].conditions.len(), 2);
        assert!(!arms[0].is_default);
        assert!(arms[1].is_default);
        assert!(arms[1].conditions.is_empty());
    }

    #[test]
    fn dynamic_variables_lower_their_target() {
        let ir = body("<?php function f() { $$name; }");
        let BodyExpression::DynamicVariable { target } = root_expression(&ir, 0) else {
            panic!("expected a dynamic variable");
        };
        assert!(matches!(
            ir.expression(*target).unwrap(),
            BodyExpression::Variable { .. }
        ));
    }

    fn null_safe_chain_count(ir: &BodyIr) -> usize {
        ir.expressions
            .iter()
            .filter(|expression| matches!(expression, BodyExpression::NullSafeChain { .. }))
            .count()
    }

    #[test]
    fn a_method_call_on_this_lowers_as_call_over_member_access() {
        let ir = body("<?php function f() { $this->run(1, label: 2, ...$rest); }");
        let BodyExpression::Call { callee, arguments } = root_expression(&ir, 0) else {
            panic!("expected a call");
        };
        let BodyExpression::MemberAccess {
            receiver,
            member,
            null_safe,
        } = ir.expression(*callee).unwrap()
        else {
            panic!("expected a member access callee");
        };
        assert!(!*null_safe);
        assert_eq!(
            member,
            &MemberReference::Named {
                name: "run".to_owned()
            }
        );
        assert_eq!(
            ir.expression(*receiver),
            Some(&BodyExpression::Variable {
                name: "this".to_owned()
            }),
        );
        assert_eq!(arguments.len(), 3);
        assert_eq!(arguments[0].label, None);
        assert_eq!(arguments[1].label.as_deref(), Some("label"));
        assert!(arguments[2].spread);
    }

    #[test]
    fn member_references_distinguish_named_variable_and_computed() {
        let ir =
            body("<?php function f() { $a->name; $a->$dynamic; $a->{$computed}; Foo::$property; }");
        let expect_member = |position: usize| match root_expression(&ir, position) {
            BodyExpression::MemberAccess { member, .. } => member.clone(),
            BodyExpression::ScopedAccess { member, .. } => member.clone(),
            other => panic!("expected an access, got {other:?}"),
        };
        assert_eq!(
            expect_member(0),
            MemberReference::Named {
                name: "name".to_owned()
            }
        );
        assert_eq!(
            expect_member(1),
            MemberReference::Variable {
                name: "dynamic".to_owned()
            }
        );
        assert!(matches!(expect_member(2), MemberReference::Computed { .. }));
        assert_eq!(
            expect_member(3),
            MemberReference::Variable {
                name: "property".to_owned()
            }
        );
    }

    #[test]
    fn scoped_access_and_class_constants_lower() {
        let ir = body("<?php function f() { Foo::bar(); static::create(); Foo::class; }");
        let BodyExpression::Call { callee, .. } = root_expression(&ir, 0) else {
            panic!("expected a call");
        };
        assert!(matches!(
            ir.expression(*callee).unwrap(),
            BodyExpression::ScopedAccess { .. }
        ));
        let BodyExpression::Call { callee, .. } = root_expression(&ir, 1) else {
            panic!("expected a call");
        };
        let BodyExpression::ScopedAccess { subject, .. } = ir.expression(*callee).unwrap() else {
            panic!("expected a scoped access");
        };
        assert_eq!(
            ir.expression(*subject),
            Some(&BodyExpression::NamedReference {
                text: "static".to_owned()
            }),
        );
        assert!(matches!(
            root_expression(&ir, 2),
            BodyExpression::ScopedAccess { member: MemberReference::Named { name }, .. } if name == "class",
        ));
    }

    #[test]
    fn indexes_lower_and_the_push_form_has_no_index() {
        let ir = body("<?php function f() { $a[0]; $a[] = 1; }");
        assert!(matches!(
            root_expression(&ir, 0),
            BodyExpression::Index { index: Some(_), .. }
        ));
        let BodyExpression::Assignment { target, .. } = root_expression(&ir, 1) else {
            panic!("expected an assignment");
        };
        assert!(matches!(
            ir.expression(*target).unwrap(),
            BodyExpression::Index { index: None, .. },
        ));
    }

    #[test]
    fn new_lowers_all_class_reference_shapes() {
        let ir = body(
            "<?php function f() { new Foo(1); new self; new static; new $factory; new class(2) {}; }",
        );
        let class_of = |position: usize| match root_expression(&ir, position) {
            BodyExpression::New { class, .. } => class.clone(),
            other => panic!("expected new, got {other:?}"),
        };
        assert_eq!(
            class_of(0),
            ClassReference::Named {
                name: "Foo".to_owned()
            }
        );
        assert_eq!(
            class_of(1),
            ClassReference::Named {
                name: "self".to_owned()
            }
        );
        assert_eq!(class_of(2), ClassReference::StaticKeyword);
        assert!(matches!(class_of(3), ClassReference::Dynamic { .. }));
        // The anonymous class is the second numbered declaration of the
        // file (function = 0, anonymous class = 1), and its constructor
        // arguments travel on the New expression.
        let BodyExpression::New {
            class: ClassReference::Anonymous { declaration },
            arguments,
        } = root_expression(&ir, 4)
        else {
            panic!("expected an anonymous new");
        };
        assert_eq!(declaration.index, 1);
        assert_eq!(arguments.len(), 1);
    }

    #[test]
    fn a_first_class_callable_lowers_to_a_callable_reference() {
        let ir =
            body("<?php function f() { strlen(...); $obj->m(...); Foo::bar(...); foo(...$args); }");
        assert!(matches!(
            root_expression(&ir, 0),
            BodyExpression::CallableReference { .. }
        ));
        assert!(matches!(
            root_expression(&ir, 1),
            BodyExpression::CallableReference { .. }
        ));
        assert!(matches!(
            root_expression(&ir, 2),
            BodyExpression::CallableReference { .. }
        ));
        // Spreading an argument is a call, not a callable reference.
        assert!(matches!(
            root_expression(&ir, 3),
            BodyExpression::Call { arguments, .. } if arguments.len() == 1 && arguments[0].spread,
        ));
    }

    #[test]
    fn a_null_safe_chain_gets_exactly_one_wrapper_at_its_top() {
        let ir = body("<?php function f() { $a?->b->c()['d']; }");
        assert_eq!(null_safe_chain_count(&ir), 1);
        // The wrapper is the outermost expression of the statement.
        let BodyStatement::Expression { expression } = root_statement(&ir, 0) else {
            panic!("expected an expression statement");
        };
        let BodyExpression::NullSafeChain { chain } = ir.expression(*expression).unwrap() else {
            panic!("expected the wrapper at the top");
        };
        // Inside, the null-safe link carries its flag.
        let mut current = *chain;
        let null_safe_flag = loop {
            match ir.expression(current).unwrap() {
                BodyExpression::Index { subject, .. } => current = *subject,
                BodyExpression::Call { callee, .. } => current = *callee,
                BodyExpression::MemberAccess {
                    receiver,
                    null_safe,
                    ..
                } => {
                    if *null_safe {
                        break true;
                    }
                    current = *receiver;
                }
                _ => break false,
            }
        };
        assert!(null_safe_flag);
    }

    #[test]
    fn a_plain_chain_gets_no_wrapper() {
        let ir = body("<?php function f() { $a->b->c(); }");
        assert_eq!(null_safe_chain_count(&ir), 0);
    }

    #[test]
    fn parentheses_bound_the_short_circuit() {
        // PHP semantics: `($a?->b)->c` does not short-circuit `->c`;
        // the wrapper closes at the parenthesis, and the outer access
        // sees a possibly-null receiver (exactly what the nullability
        // family must report).
        let ir = body("<?php function f() { ($a?->b)->c; }");
        assert_eq!(null_safe_chain_count(&ir), 1);
        let BodyStatement::Expression { expression } = root_statement(&ir, 0) else {
            panic!("expected an expression statement");
        };
        let BodyExpression::MemberAccess {
            receiver,
            null_safe,
            ..
        } = ir.expression(*expression).unwrap()
        else {
            panic!("expected the outer access at the top");
        };
        assert!(!*null_safe);
        assert!(matches!(
            ir.expression(*receiver).unwrap(),
            BodyExpression::NullSafeChain { .. },
        ));
    }

    #[test]
    fn independent_chains_wrap_independently() {
        let ir = body("<?php function f() { $a?->b[$c?->d]; }");
        assert_eq!(null_safe_chain_count(&ir), 2);
    }
}
