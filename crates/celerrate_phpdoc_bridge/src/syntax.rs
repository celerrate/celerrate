//! The bridge as a type-syntax implementation: standard PHPDoc over
//! the 4a expression grammar, lowered through the facade's builders.

use celerrate_plugin::{AnnotationSite, ParsedAnnotations, TypeId, TypeSyntax};

use crate::lexer::lex_docblock;
use crate::lowering::{LoweringScope, lower};
use crate::tags::extract_member_docblock;

/// The `phpdoc-bridge` plugin. Stateless by design: guest
/// statelessness is the WASM sketch's first acceptance case, and the
/// native tier honors it by construction.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhpdocBridge;

impl PhpdocBridge {
    pub fn new() -> Self {
        Self
    }
}

impl TypeSyntax for PhpdocBridge {
    fn can_parse(&self, _docblock: &str) -> bool {
        // The bridge owns the inherited notation and registers first
        // (decision 8): it claims every docblock it is offered.
        true
    }

    fn parse_docblock<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        docblock: &str,
    ) -> ParsedAnnotations<'db> {
        let tags = lex_docblock(docblock);
        let extracted = extract_member_docblock(&tags);
        let mut scope = LoweringScope::default();
        let return_type = extracted
            .return_type
            .as_ref()
            .map(|expression| lower(site, &mut scope, expression));
        let value_type = extracted
            .value_type
            .as_ref()
            .map(|expression| lower(site, &mut scope, expression));
        let parameters = extracted
            .parameters
            .iter()
            .map(|(name, expression)| (name.clone(), lower(site, &mut scope, expression)))
            .collect();
        let throws = extracted
            .throws
            .iter()
            .map(|expression| lower(site, &mut scope, expression))
            .collect();
        ParsedAnnotations {
            return_type,
            value_type,
            parameters,
            throws,
        }
    }

    fn parse_type_expression<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        expression: &str,
    ) -> Option<TypeId<'db>> {
        let parsed = crate::expression::parse_type_expression_text(expression)?;
        let mut scope = LoweringScope::default();
        Some(lower(site, &mut scope, &parsed))
    }
}
