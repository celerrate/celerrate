//! The `phpdoc-bridge` plugin: translates the inherited PHPDoc
//! convention family (standard PHPDoc in this plan; the PHPStan
//! dialect and Psalm synonyms arrive with plan 4b as internal
//! modules over the same lexer). Depends on `celerrate_plugin` and
//! nothing else in the workspace — enforced by
//! `cargo xtask dependency-shape`. No docblock diagnostics: malformed
//! annotations are silently ignored, per construct.

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
pub use tags::{MemberDocblock, extract_member_docblock, extract_virtual_members};

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
