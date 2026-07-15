//! The bridge as a type-syntax implementation: standard PHPDoc over
//! the 4a expression grammar, lowered through the facade's builders.

use celerrate_plugin::{AnnotationSite, ParsedAnnotations, TypeId, TypeSyntax};

use crate::expression::TypeExpression;
use crate::lexer::lex_docblock;
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
        ParsedAnnotations {
            return_type: extracted
                .return_type
                .as_ref()
                .map(|expression| lower(site, expression)),
            value_type: extracted
                .value_type
                .as_ref()
                .map(|expression| lower(site, expression)),
            parameters: extracted
                .parameters
                .iter()
                .map(|(name, expression)| (name.clone(), lower(site, expression)))
                .collect(),
            throws: extracted
                .throws
                .iter()
                .map(|expression| lower(site, expression))
                .collect(),
        }
    }

    fn parse_type_expression<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        expression: &str,
    ) -> Option<TypeId<'db>> {
        crate::expression::parse_type_expression_text(expression).map(|parsed| lower(site, &parsed))
    }
}

/// Lowers a parsed expression through the facade's builders. Keywords
/// go through the shared native table; everything else qualifies at
/// the declaring site and becomes a class type.
fn lower<'db>(site: &AnnotationSite<'db, '_>, expression: &TypeExpression) -> TypeId<'db> {
    let db = site.database();
    match expression {
        TypeExpression::Name(name) => site
            .keyword_type(name)
            .unwrap_or_else(|| TypeId::class(db, &site.qualify_class_name(name), Vec::new())),
        TypeExpression::Nullable(inner) => {
            TypeId::union(db, [lower(site, inner), TypeId::null(db)])
        }
        TypeExpression::Union(parts) => TypeId::union(
            db,
            parts
                .iter()
                .map(|part| lower(site, part))
                .collect::<Vec<_>>(),
        ),
        TypeExpression::Intersection(parts) => TypeId::intersection(
            db,
            parts
                .iter()
                .map(|part| lower(site, part))
                .collect::<Vec<_>>(),
        ),
        TypeExpression::ArrayOf(element) => {
            let key = TypeId::union(db, [TypeId::int(db), TypeId::string(db)]);
            TypeId::array(db, key, lower(site, element))
        }
        // Task 3: Parsed but not lowered yet. Task 6 will implement lowering.
        TypeExpression::IntLiteral(_) => TypeId::int(db),
        TypeExpression::FloatLiteral(_) => TypeId::float(db),
        TypeExpression::StringLiteral(_) => TypeId::string(db),
        TypeExpression::Generic { base, arguments } => {
            let lowered_arguments = arguments.iter().map(|arg| lower(site, arg)).collect();
            site.keyword_type(base).unwrap_or_else(|| {
                TypeId::class(db, &site.qualify_class_name(base), lowered_arguments)
            })
        }
        // Task 4: Parsed but not lowered yet. Task 6 will implement lowering.
        TypeExpression::Shape { .. } => TypeId::mixed(db),
    }
}
