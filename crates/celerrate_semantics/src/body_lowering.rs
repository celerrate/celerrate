//! The lowering walk: one pass over a body's syntax turning statements
//! and expressions into the dense, range-free arenas of
//! [`crate::body::BodyIr`] and its range-carrying source-map sibling.
//! Every accessor miss lowers to `Missing`; error-recovery wreckage is
//! tolerated, never a failure.

use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxNodePtr};

use crate::body::{
    BodyExpression, BodyIr, BodySourceMap, BodyStatement, ExpressionId, StatementId,
};

/// Lowers one function or method declaration's body. `None` when the
/// node is not a function or method, or carries no body.
pub(crate) fn lower_body(declaration: &SyntaxNode) -> Option<(BodyIr, BodySourceMap)> {
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
        ir: BodyIr::default(),
        source_map: BodySourceMap::default(),
    };
    let root = lowering.lower_statements(block.statements());
    lowering.ir.root = root;
    Some((lowering.ir, lowering.source_map))
}

struct Lowering {
    ir: BodyIr,
    source_map: BodySourceMap,
}

impl Lowering {
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

    fn lower_statement(&mut self, statement: &ast::Statement) -> Option<StatementId> {
        let lowered = match statement {
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
            _ => BodyStatement::Missing,
        };
        Some(self.allocate_statement(lowered, statement.syntax()))
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
        super::lower_body(&declaration).unwrap()
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
        assert!(super::lower_body(&method).is_none());
    }

    #[test]
    fn a_non_body_declaration_answers_none() {
        let parse = celerrate_syntax::parse("<?php class A { public int $x = 0; }");
        let property = parse
            .tree()
            .descendants()
            .find(|node| node.kind() == SyntaxKind::PropertyDeclaration)
            .unwrap();
        assert!(super::lower_body(&property).is_none());
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
}
