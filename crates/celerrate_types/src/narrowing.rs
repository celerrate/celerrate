//! Narrowing subjects (design section 6: locals, and property fetches
//! on a stable base — `$this->prop`, `self::$prop`) and, from Task 4
//! on, the pure leaf transformations the condition forms reduce to.

use celerrate_semantics::{BodyExpression, BodyIr, ExpressionId, MemberReference};

/// One narrowable subject. `Ord` because the environment is a
/// `BTreeMap`: deterministic iteration is a determinism invariant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NarrowingSubject {
    Local {
        name: String,
    },
    /// `$this->name` — never through `?->` (a null-safe fetch is not
    /// a stable base).
    ThisProperty {
        name: String,
    },
    /// `self::$name` / `static::$name` on the defining class.
    StaticProperty {
        name: String,
    },
}

/// The narrowing subject of one expression, seeing through
/// `Assignment` to its target so an assign-and-test condition
/// (`if (($x = f()) instanceof Foo)`) narrows the assigned subject.
pub(crate) fn subject_of(ir: &BodyIr, expression: ExpressionId) -> Option<NarrowingSubject> {
    match ir.expression(expression)? {
        BodyExpression::Variable { name } if name != "this" => {
            Some(NarrowingSubject::Local { name: name.clone() })
        }
        BodyExpression::Assignment { target, .. } => subject_of(ir, *target),
        BodyExpression::MemberAccess {
            receiver,
            member: MemberReference::Named { name },
            null_safe: false,
        } => match ir.expression(*receiver)? {
            BodyExpression::Variable {
                name: receiver_name,
            } if receiver_name == "this" => {
                Some(NarrowingSubject::ThisProperty { name: name.clone() })
            }
            _ => None,
        },
        BodyExpression::ScopedAccess {
            subject,
            member: MemberReference::Variable { name },
        } => match ir.expression(*subject)? {
            BodyExpression::NamedReference { text } => {
                let folded = text.to_ascii_lowercase();
                (folded == "self" || folded == "static")
                    .then(|| NarrowingSubject::StaticProperty { name: name.clone() })
            }
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_semantics::{AstId, BodyQuery, BodyStatement, body_ir};
    use celerrate_source::FileId;

    use super::{NarrowingSubject, subject_of};

    /// Lowers one function body and answers the IR plus the first
    /// top-level expression-statement's expression.
    fn first_expression(
        source: &str,
    ) -> (
        celerrate_semantics::BodyIr,
        celerrate_semantics::ExpressionId,
    ) {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
        let body = BodyQuery::new(
            &db,
            AstId {
                file: FileId::new(0),
                index: 0,
            },
        );
        let ir = body_ir(&db, file, body).as_ref().unwrap().clone();
        let Some(BodyStatement::Expression { expression }) =
            ir.root.first().and_then(|&id| ir.statement(id)).cloned()
        else {
            panic!("expected an expression statement");
        };
        (ir, expression)
    }

    #[test]
    fn subjects_extract_from_their_stable_shapes() {
        let (ir, expression) = first_expression("<?php function f() { $x; }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::Local {
                name: "x".to_owned()
            }),
        );

        let (ir, expression) = first_expression("<?php function f() { $this->prop; }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::ThisProperty {
                name: "prop".to_owned()
            }),
        );

        let (ir, expression) = first_expression("<?php function f() { self::$prop; }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::StaticProperty {
                name: "prop".to_owned()
            }),
        );

        // Assignment sees through to its target.
        let (ir, expression) = first_expression("<?php function f() { $x = 1; }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::Local {
                name: "x".to_owned()
            }),
        );

        // `$this` itself, `?->` fetches, and computed members are not
        // stable bases.
        let (ir, expression) = first_expression("<?php function f() { $this; }");
        assert_eq!(subject_of(&ir, expression), None);
        let (ir, expression) = first_expression("<?php function f() { $a?->prop; }");
        assert_eq!(subject_of(&ir, expression), None);
    }
}
