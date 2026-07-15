//! The `phpdoc-bridge` plugin: translates the inherited PHPDoc
//! convention family (standard PHPDoc in this plan; the PHPStan
//! dialect and Psalm synonyms arrive with plan 4b as internal
//! modules over the same lexer). Depends on `celerrate_plugin` and
//! nothing else in the workspace — enforced by
//! `cargo xtask dependency-shape`. No docblock diagnostics: malformed
//! annotations are silently ignored, per construct.

mod lexer;

pub use lexer::{Tag, lex_docblock};
