//! The `phpdoc-bridge` plugin: translates the inherited PHPDoc
//! convention family (standard PHPDoc in this plan; the PHPStan
//! dialect and Psalm synonyms arrive with plan 4b as internal
//! modules over the same lexer). Depends on `celerrate_plugin` and
//! nothing else in the workspace — enforced by
//! `cargo xtask dependency-shape`. No docblock diagnostics: malformed
//! annotations are silently ignored, per construct.

mod expression;
mod lexer;
mod tags;

pub use expression::{TypeExpression, parse_type_expression_text};
pub use lexer::{Tag, lex_docblock};
pub use tags::{MemberDocblock, extract_member_docblock, extract_virtual_members};
