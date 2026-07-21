use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_diagnostics::Diagnostic;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{BodyQuery, DeclarationKind, MemberKind};
use celerrate_source::FileId;
use celerrate_stubs::StubIndexInput;

use crate::context::SyntaxContext;
use crate::finding::{Finding, FindingAnchor, FindingSink};
use crate::registry::{RuleImplementation, RuleRegistry};

/// The syntax phase: one query per file, draining the active syntax
/// rules in registration order. Output is sorted by the diagnostic
/// total order, so it is independent of registration order by
/// construction.
#[salsa::tracked(returns(ref))]
pub fn syntax_phase_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic> {
    let Some(registry) = RuleRegistry::try_get(db) else {
        return Vec::new();
    };
    let file_id = file.file_id(db);
    let mut diagnostics = Vec::new();
    for registration in registry.registrations(db) {
        if !registration.active {
            continue;
        }
        let RuleImplementation::Syntax(rule) = &registration.implementation else {
            continue;
        };
        let context = SyntaxContext::new(db, file, configuration);
        let mut sink = FindingSink::new(&registration.metadata);
        rule.check(&context, &mut sink);
        diagnostics.extend(
            sink.into_findings()
                .into_iter()
                .filter_map(|finding| resolved_diagnostic(db, file, file_id, finding)),
        );
    }
    diagnostics.sort();
    diagnostics
}

/// The semantic phase: one query per file, draining the active
/// semantic rules against the sealed context. The inputs are exactly
/// what the context's facade methods read (part 3 reserved this
/// extension).
#[salsa::tracked(returns(ref))]
pub fn semantic_phase_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic> {
    let Some(registry) = RuleRegistry::try_get(db) else {
        return Vec::new();
    };
    let file_id = file.file_id(db);
    let mut diagnostics = Vec::new();
    for registration in registry.registrations(db) {
        if !registration.active {
            continue;
        }
        let RuleImplementation::Semantic(rule) = &registration.implementation else {
            continue;
        };
        let context = celerrate_semantics::semantic_context(db, file, files, stubs, configuration);
        let mut sink = FindingSink::new(&registration.metadata);
        rule.check(&context, &mut sink);
        diagnostics.extend(
            sink.into_findings()
                .into_iter()
                .filter_map(|finding| resolved_diagnostic(db, file, file_id, finding)),
        );
    }
    diagnostics.sort();
    diagnostics
}

/// The typed findings of one body. Tracked per body on purpose: the
/// framework preserves `body_typed_verdicts`' proven tier — editing one
/// body never re-checks its siblings. The `body_ir` guard is the tier's
/// honest content dependency: a body that does not lower has nothing to
/// check, and a body edit invalidates exactly this body's query while
/// an offset-only edit backdates it.
#[salsa::tracked(returns(ref))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn body_phase_findings<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    body: BodyQuery<'db>,
) -> Vec<Finding> {
    if celerrate_semantics::body_ir(db, file, body).is_none() {
        return Vec::new();
    }
    let Some(registry) = RuleRegistry::try_get(db) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    for registration in registry.registrations(db) {
        if !registration.active {
            continue;
        }
        let RuleImplementation::TypedBody(rule) = &registration.implementation else {
            continue;
        };
        let context = celerrate_types::typed_body_context(
            db,
            files,
            stubs,
            configuration,
            file,
            body.ast_id(db),
        );
        let mut sink = FindingSink::new(&registration.metadata);
        rule.check(&context, &mut sink);
        findings.extend(sink.into_findings());
    }
    findings
}

/// The typed phase: aggregates the per-body tier over the file's
/// function and method bodies (the `typed_file_verdicts` enumeration,
/// traits excluded) and reconciles anchors at the tail. Not yet wired
/// into the CLI composition: part 4's typed migration wires it together
/// with the stored-verdict co-production.
#[salsa::tracked(returns(ref))]
pub fn typed_body_phase_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic> {
    let file_id = file.file_id(db);
    let tree = celerrate_semantics::member_tree(db, file);
    let mut diagnostics = Vec::new();
    let function_bodies = tree.functions.iter().map(|function| function.ast_id);
    let method_bodies = tree
        .classes
        .iter()
        .filter(|class| class.kind != DeclarationKind::Trait)
        .flat_map(|class| {
            class
                .members
                .iter()
                .filter(|member| member.kind == MemberKind::Method)
                .map(|member| member.ast_id)
        });
    for ast_id in function_bodies.chain(method_bodies) {
        let body = BodyQuery::new(db, ast_id);
        for finding in body_phase_findings(db, file, files, stubs, configuration, body) {
            if let Some(diagnostic) = resolved_diagnostic(db, file, file_id, finding.clone()) {
                diagnostics.push(diagnostic);
            }
        }
    }
    diagnostics.sort();
    diagnostics
}

/// The reconciliation tail every phase shares: anchors resolve to
/// concrete ranges here, where tree access is legitimate. An anchor
/// that no longer resolves (or that names another file) drops its
/// finding, never a panic.
pub(crate) fn resolved_diagnostic(
    db: &dyn salsa::Database,
    file: SourceFile,
    file_id: FileId,
    finding: Finding,
) -> Option<Diagnostic> {
    let range = match finding.anchor {
        FindingAnchor::Range(range) => range,
        FindingAnchor::Declaration(ast_id) => {
            if ast_id.file != file_id {
                return None;
            }
            let map = celerrate_semantics::ast_id_map(db, file);
            let pointer = map.pointer(ast_id.index)?;
            let root = celerrate_db::parse(db, file).tree();
            pointer.try_to_node(&root)?.text_range()
        }
        FindingAnchor::Expression { body, expression } => {
            if body.file != file_id {
                return None;
            }
            let query = BodyQuery::new(db, body);
            let map = celerrate_semantics::body_source_map(db, file, query).as_ref()?;
            map.expression_pointer(expression)?.text_range()
        }
    };
    Some(Diagnostic::spanned(
        finding.identifier,
        finding.severity,
        file_id,
        range,
        finding.message,
    ))
}

#[cfg(test)]
mod tests {
    //! `unwrap`/`expect`/indexing are fine here: failing loudly is
    //! what a test should do.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use std::sync::Arc;

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_diagnostics::{DiagnosticId, Severity};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_semantics::{AstId, ExpressionId, PluginIdentity, SemanticContext};
    use celerrate_source::{FileId, TextRange, TextSize};
    use celerrate_stubs::{StubIndex, StubIndexInput};
    use celerrate_types::TypedBodyContext;

    use super::{
        resolved_diagnostic, semantic_phase_diagnostics, syntax_phase_diagnostics,
        typed_body_phase_diagnostics,
    };
    use crate::context::SyntaxContext;
    use crate::finding::{Finding, FindingAnchor, FindingSink};
    use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
    use crate::registry::{RuleImplementation, RuleRegistration, RuleRegistry};
    use crate::traits::{SemanticRule, SyntaxRule, TypedBodyRule};

    /// The salsa inputs every phase query reads: the one-file analyzed
    /// set and the empty stub surface come along so the semantic phase
    /// can be driven from the same setup as the syntax one.
    fn test_setup(
        source: &str,
    ) -> (
        TestDatabase,
        SourceFile,
        AnalyzedFileSet,
        StubIndexInput,
        ProjectConfiguration,
    ) {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
        let files = AnalyzedFileSet::new(&db, vec![file]);
        let stubs = StubIndexInput::builder(StubIndex::default())
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        (db, file, files, stubs, configuration)
    }

    fn register(db: &TestDatabase, registrations: Vec<RuleRegistration>) {
        let _ = RuleRegistry::builder(registrations)
            .durability(salsa::Durability::HIGH)
            .new(db);
    }

    struct EmitAt(TextRange);

    impl SyntaxRule for EmitAt {
        fn check(&self, _context: &SyntaxContext<'_>, sink: &mut FindingSink<'_>) {
            sink.report(
                DiagnosticId::new("CEL9998"),
                FindingAnchor::Range(self.0),
                "fake finding".to_owned(),
            );
        }
    }

    fn fake_registration(active: bool) -> RuleRegistration {
        registration_at(
            "emit-at",
            active,
            TextRange::new(TextSize::from(6), TextSize::from(10)),
        )
    }

    /// A named `EmitAt` registration at an arbitrary range, so the
    /// sorted-output test can register two distinct rules.
    fn registration_at(name: &str, active: bool, range: TextRange) -> RuleRegistration {
        RuleRegistration {
            identity: PluginIdentity {
                name: "test-plugin".to_owned(),
                version: "0.0.0".to_owned(),
                configuration: String::new(),
            },
            active,
            metadata: RuleMetadata {
                name: name.to_owned(),
                group: RuleGroup::Correctness,
                identifiers: vec![RuleIdentifier {
                    id: DiagnosticId::new("CEL9998"),
                    severity: Severity::Error,
                }],
                tier: Tier::Default,
            },
            implementation: RuleImplementation::Syntax(Arc::new(EmitAt(range))),
        }
    }

    #[test]
    fn an_unset_registry_is_the_empty_path() {
        let (db, file, _files, _stubs, configuration) = test_setup("<?php echo 1;");
        assert!(syntax_phase_diagnostics(&db, file, configuration).is_empty());
    }

    #[test]
    fn an_active_syntax_rule_reports_through_the_phase() {
        let (db, file, _files, _stubs, configuration) = test_setup("<?php echo 1;");
        register(&db, vec![fake_registration(true)]);
        let diagnostics = syntax_phase_diagnostics(&db, file, configuration);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, DiagnosticId::new("CEL9998"));
        assert_eq!(diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn an_inactive_rule_is_skipped() {
        let (db, file, _files, _stubs, configuration) = test_setup("<?php echo 1;");
        register(&db, vec![fake_registration(false)]);
        assert!(syntax_phase_diagnostics(&db, file, configuration).is_empty());
    }

    #[test]
    fn the_output_is_sorted_by_the_diagnostic_total_order() {
        // Two rules registered in reverse positional order; the phase
        // sorts, so the result is position-ordered regardless.
        let (db, file, _files, _stubs, configuration) = test_setup("<?php echo 1;");
        let later = registration_at(
            "later-rule",
            true,
            TextRange::new(TextSize::from(9), TextSize::from(13)),
        );
        let earlier = registration_at(
            "earlier-rule",
            true,
            TextRange::new(TextSize::from(0), TextSize::from(4)),
        );
        register(&db, vec![later, earlier]);
        let diagnostics = syntax_phase_diagnostics(&db, file, configuration);
        assert_eq!(diagnostics.len(), 2);
        let (_, first_range) = diagnostics[0].span().unwrap();
        let (_, second_range) = diagnostics[1].span().unwrap();
        assert!(first_range.start() < second_range.start());
        assert_eq!(
            first_range,
            TextRange::new(TextSize::from(0), TextSize::from(4))
        );
        assert_eq!(
            second_range,
            TextRange::new(TextSize::from(9), TextSize::from(13))
        );
    }

    #[test]
    fn a_declaration_anchor_resolves_through_the_ast_id_map() {
        let (db, file, _files, _stubs, _configuration) =
            test_setup("<?php function demo() { echo 1; }");
        // Index 0 is the file's first declaration in tree order.
        let ast_id = AstId {
            file: FileId::new(0),
            index: 0,
        };
        let finding = Finding {
            identifier: DiagnosticId::new("CEL9998"),
            severity: Severity::Error,
            anchor: FindingAnchor::Declaration(ast_id),
            message: "anchored to a declaration".to_owned(),
        };
        let diagnostic = resolved_diagnostic(&db, file, FileId::new(0), finding);
        assert!(diagnostic.is_some());
    }

    #[test]
    fn a_declaration_anchor_of_another_file_is_dropped() {
        let (db, file, _files, _stubs, _configuration) =
            test_setup("<?php function demo() { echo 1; }");
        let ast_id = AstId {
            file: FileId::new(1),
            index: 0,
        };
        let finding = Finding {
            identifier: DiagnosticId::new("CEL9998"),
            severity: Severity::Error,
            anchor: FindingAnchor::Declaration(ast_id),
            message: "anchored to another file".to_owned(),
        };
        let diagnostic = resolved_diagnostic(&db, file, FileId::new(0), finding);
        assert!(diagnostic.is_none());
    }

    #[test]
    fn an_expression_anchor_resolves_through_the_body_source_map() {
        // A body with one expression; `ExpressionId::from_index(0)` resolves.
        let (db, file, _files, _stubs, _configuration) =
            test_setup("<?php function f() { return 1; }");
        let body = AstId {
            file: FileId::new(0),
            index: 0,
        };
        let expression = ExpressionId::from_index(0).unwrap();
        let finding = Finding {
            identifier: DiagnosticId::new("CEL9998"),
            severity: Severity::Error,
            anchor: FindingAnchor::Expression { body, expression },
            message: "anchored to an expression".to_owned(),
        };
        let diagnostic = resolved_diagnostic(&db, file, FileId::new(0), finding);
        assert!(diagnostic.is_some());
    }

    #[test]
    fn a_dangling_anchor_is_dropped_never_a_panic() {
        // `AstId { index: u32::MAX }` on a real file -> None.
        let (db, file, _files, _stubs, _configuration) =
            test_setup("<?php function demo() { echo 1; }");
        let ast_id = AstId {
            file: FileId::new(0),
            index: u32::MAX,
        };
        let finding = Finding {
            identifier: DiagnosticId::new("CEL9998"),
            severity: Severity::Error,
            anchor: FindingAnchor::Declaration(ast_id),
            message: "dangling".to_owned(),
        };
        let diagnostic = resolved_diagnostic(&db, file, FileId::new(0), finding);
        assert!(diagnostic.is_none());
    }

    struct EmitPerFile;

    impl SemanticRule for EmitPerFile {
        fn check(&self, _context: &SemanticContext<'_>, sink: &mut FindingSink<'_>) {
            sink.report(
                DiagnosticId::new("CEL9997"),
                FindingAnchor::Range(TextRange::new(TextSize::from(0), TextSize::from(5))),
                "per file".to_owned(),
            );
        }
    }

    fn semantic_registration() -> RuleRegistration {
        RuleRegistration {
            identity: PluginIdentity {
                name: "test-plugin".to_owned(),
                version: "0.0.0".to_owned(),
                configuration: String::new(),
            },
            active: true,
            metadata: RuleMetadata {
                name: "emit-per-file".to_owned(),
                group: RuleGroup::Correctness,
                identifiers: vec![RuleIdentifier {
                    id: DiagnosticId::new("CEL9997"),
                    severity: Severity::Error,
                }],
                tier: Tier::Default,
            },
            implementation: RuleImplementation::Semantic(Arc::new(EmitPerFile)),
        }
    }

    #[test]
    fn a_semantic_rule_reports_once_per_file() {
        let (db, file, files, stubs, configuration) = test_setup("<?php echo 1;");
        register(&db, vec![semantic_registration()]);
        assert_eq!(
            semantic_phase_diagnostics(&db, file, files, stubs, configuration).len(),
            1
        );
    }

    struct MarkEveryBody;

    impl TypedBodyRule for MarkEveryBody {
        fn check(&self, context: &TypedBodyContext<'_>, sink: &mut FindingSink<'_>) {
            sink.report(
                DiagnosticId::new("CEL9996"),
                FindingAnchor::Declaration(context.body()),
                "marked body".to_owned(),
            );
        }
    }

    fn typed_registration() -> RuleRegistration {
        RuleRegistration {
            identity: PluginIdentity {
                name: "test-plugin".to_owned(),
                version: "0.0.0".to_owned(),
                configuration: String::new(),
            },
            active: true,
            metadata: RuleMetadata {
                name: "mark-every-body".to_owned(),
                group: RuleGroup::Correctness,
                identifiers: vec![RuleIdentifier {
                    id: DiagnosticId::new("CEL9996"),
                    severity: Severity::Error,
                }],
                tier: Tier::Default,
            },
            implementation: RuleImplementation::TypedBody(Arc::new(MarkEveryBody)),
        }
    }

    #[test]
    fn a_typed_body_rule_runs_once_per_function_and_method_body() {
        let source = "<?php\nfunction first() { echo 1; }\nclass Demo { public function second(): void { echo 2; } }\n";
        let (db, file, files, stubs, configuration) = test_setup(source);
        register(&db, vec![typed_registration()]);
        let diagnostics = typed_body_phase_diagnostics(&db, file, files, stubs, configuration);
        assert_eq!(
            diagnostics.len(),
            2,
            "one finding per body, both reconciled"
        );
    }

    #[test]
    fn a_trait_method_body_is_not_enumerated() {
        // Mirrors `typed_file_verdicts`' trait filter.
        let source = "<?php\ntrait Helper { public function inside(): void { echo 1; } }\n";
        let (db, file, files, stubs, configuration) = test_setup(source);
        register(&db, vec![typed_registration()]);
        assert!(typed_body_phase_diagnostics(&db, file, files, stubs, configuration).is_empty());
    }
}
