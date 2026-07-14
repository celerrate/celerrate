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
    BodyExpression, BodyIr, BodySourceMap, BodyStatement, CatchArm, ExpressionId, StatementId,
    StaticVariableDeclaration, SwitchArm,
};

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
        self.lower_expression_kind(expression)
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
            _ => BodyExpression::Missing,
        };
        self.allocate_expression(lowered, node)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use celerrate_syntax::SyntaxKind;

    use crate::body::{BodyExpression, BodyIr, BodySourceMap, BodyStatement};

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
}
