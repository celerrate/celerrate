//! The bridge's `TypeSyntax` implementation: wires tag extraction
//! (`tags::extract_member_docblock`) into a per-docblock template
//! scope (`docblock_scope`, `declare_into`), then lowers every
//! extracted expression through the total lowering table
//! (`lowering::lower`) into the facade's builders.

use celerrate_plugin::{AnnotationSite, ParsedAnnotations, ParsedAssertion, TypeId, TypeSyntax};

use crate::lexer::lex_docblock;
use crate::lowering::{LoweringScope, lower};
use crate::tags::{TemplateDeclaration, extract_member_docblock};

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
        let mut scope = docblock_scope(site, &extracted.templates);
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
        let assertions = extracted
            .assertions
            .iter()
            .map(|assertion| ParsedAssertion {
                subject: assertion.subject.clone(),
                asserted: lower(site, &mut scope, &assertion.asserted),
                polarity: assertion.polarity,
                negated: assertion.negated,
            })
            .collect();
        ParsedAnnotations {
            return_type,
            value_type,
            parameters,
            throws,
            assertions,
        }
    }

    fn parse_type_expression<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        expression: &str,
    ) -> Option<TypeId<'db>> {
        let parsed = crate::expression::parse_type_expression_text(expression)?;
        // A bare payload (a virtual member's type text) has no
        // docblock of its own: the enclosing one, if any, IS its
        // declaring docblock.
        let mut scope = docblock_scope(site, &[]);
        Some(lower(site, &mut scope, &parsed))
    }
}

/// Builds the docblock's name-resolution scope: the enclosing
/// class-like's own `@template` declarations first (when the site
/// carries one), then this docblock's own declarations — sequential,
/// so a bound may reference an earlier template and a same-named own
/// declaration shadows the class one (last declared wins).
fn docblock_scope<'db>(
    site: &AnnotationSite<'db, '_>,
    own_templates: &[TemplateDeclaration],
) -> LoweringScope<'db> {
    let mut scope = LoweringScope::default();
    if let (Some(class_scope), Some(class_docblock)) = (
        site.enclosing_class_scope(),
        site.enclosing_class_docblock(),
    ) {
        let class_templates = extract_member_docblock(&lex_docblock(class_docblock)).templates;
        for declaration in &class_templates {
            declare_into(site, &mut scope, declaration, class_scope);
        }
    }
    let declaring = site.declaring_scope();
    for declaration in own_templates {
        declare_into(site, &mut scope, declaration, declaring);
    }
    scope
}

/// Lowers one `@template` declaration's bound (`mixed` when absent)
/// and declares it into the scope at the given scope key.
fn declare_into<'db>(
    site: &AnnotationSite<'db, '_>,
    scope: &mut LoweringScope<'db>,
    declaration: &TemplateDeclaration,
    scope_key: &str,
) {
    let db = site.database();
    let bound = declaration
        .bound
        .as_ref()
        .map(|expression| lower(site, scope, expression))
        .unwrap_or_else(|| TypeId::mixed(db));
    scope.declare_template(db, scope_key, declaration.name.clone(), bound);
}
