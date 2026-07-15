//! The `phpdoc-bridge` plugin: translates the inherited PHPDoc
//! convention family — standard PHPDoc plus the PHPStan dialect, with
//! Psalm synonyms — as one plugin built on one docblock lexer and two
//! semantic dialect modules, `dialect::phpstan` and `dialect::psalm`.
//! The tag conflict table is `dialect`'s rustdoc; the total lowering
//! table is `lowering`'s rustdoc; the pinned-reference coverage
//! statement lives in `tests/phpstan_corpus/verdicts.txt` (repository
//! documentation is the interim publication home until plan 9c).
//! Depends on `celerrate_plugin` and nothing else in the workspace —
//! enforced by `cargo xtask dependency-shape`. No docblock
//! diagnostics: malformed annotations are silently ignored, per
//! construct.

mod dialect;
mod expression;
mod lexer;
mod lowering;
mod syntax;
mod tags;
mod virtual_members;

pub use expression::{
    CallableParameterExpression, ConditionalSubject, ShapeFieldExpression, ShapeKeyExpression,
    TypeExpression, UnsealedTail, parse_type_expression_prefix, parse_type_expression_text,
};
pub use lexer::{Tag, lex_docblock};
pub use syntax::PhpdocBridge;
pub use tags::{
    AssertionDeclaration, MemberDocblock, TemplateDeclaration, extract_member_docblock,
    extract_virtual_members,
};

/// What the composition root registers.
pub fn descriptor() -> celerrate_plugin::PluginDescriptor {
    celerrate_plugin::PluginDescriptor {
        identity: celerrate_plugin::PluginIdentity {
            name: "phpdoc-bridge".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            configuration: String::new(),
        },
        api_version: celerrate_plugin::PLUGIN_API_VERSION,
    }
}
