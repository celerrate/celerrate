use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_diagnostics::Diagnostic;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::BodyQuery;
use celerrate_source::{FileId, TextSize};
use celerrate_stubs::StubIndexInput;

use crate::context::{DirectiveOutcome, ReportingContext, SyntaxContext};
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
/// function and method bodies (`celerrate_types::checked_body_ast_ids`,
/// the single enumeration `celerrate_types::typed_file_verdicts` reads
/// too, traits excluded) and reconciles anchors at the tail. Wired into the
/// CLI composition: `crates/celerrate_cli/src/analysis.rs`'s
/// `typed_portion` calls this query as the typed families' serving
/// path. It and `celerrate_types::typed_file_verdicts` both read
/// `body_typed_verdicts`, the same memoized per-body walk, so the
/// diagnostics served here and the stored verdict's revalidation
/// records are co-produced from one walk, never a second.
#[salsa::tracked(returns(ref))]
pub fn typed_body_phase_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic> {
    let file_id = file.file_id(db);
    let mut diagnostics = Vec::new();
    for ast_id in celerrate_types::checked_body_ast_ids(db, file) {
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

/// The reporting phase: runs the registered `Reporting` rules from
/// per-directive match outcomes - never from the tree, so the warm
/// path serves the same records parse-free (design section 4). A plain
/// function, not a salsa query: its input is composed by the
/// orchestration layer, which is also why the output is recomputed on
/// both paths rather than persisted. Deterministic by construction (a
/// pure function of the registry and the outcomes).
///
/// The one additional, non-iterated suppression pass: (a) rules emit
/// findings, every directive finding naming its subject directive;
/// (b) one pass drops every finding some directive OTHER than its own
/// subject admits (self-cloaking is forbidden, decision 10) and marks
/// every admitting directive used; (c) CEL0042 findings whose subject
/// became used in (b) are dropped. Uses recorded in (b) never re-open
/// (b), and drops in (c) never un-use anything: no fixpoint.
///
/// Findings become `Diagnostic::spanned` here rather than through
/// `resolved_diagnostic`: the reporting phase has no `SourceFile` and
/// must not parse, and its anchors are concrete ranges already. It is
/// the one phase that bypasses the shared reconciliation tail, by
/// design.
pub fn reporting_phase_diagnostics(
    db: &dyn salsa::Database,
    file_id: FileId,
    text_end: TextSize,
    outcomes: &[DirectiveOutcome],
) -> Vec<Diagnostic> {
    let Some(registry) = RuleRegistry::try_get(db) else {
        return Vec::new();
    };
    let inactive: std::collections::BTreeSet<_> = registry
        .registrations(db)
        .iter()
        .filter(|registration| !registration.active)
        .flat_map(|registration| {
            registration
                .metadata
                .identifiers
                .iter()
                .map(|identifier| identifier.id)
        })
        .collect();
    let context = ReportingContext::new(outcomes, &inactive);
    let mut findings = Vec::new();
    for registration in registry.registrations(db) {
        if !registration.active {
            continue;
        }
        let RuleImplementation::Reporting(rule) = &registration.implementation else {
            continue;
        };
        let mut sink = FindingSink::new(&registration.metadata);
        rule.check(&context, &mut sink);
        findings.extend(sink.into_findings());
    }

    let mut used: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let mut kept = Vec::new();
    for finding in findings {
        // Reporting findings anchor at directive ranges by
        // construction: `report_directive` is the only affordance the
        // reporting rules use, and the context has no tree, so a
        // symbolic anchor could not resolve here anyway. A non-range
        // anchor is a future rule's authoring error: dropped, and the
        // invariant is stated here so the drop is a documented
        // contract, not an accident.
        let FindingAnchor::Range(range) = finding.anchor else {
            continue;
        };
        let mut suppressed = false;
        for (index, outcome) in outcomes.iter().enumerate() {
            // A directive never admits a finding that reports on
            // itself: self-cloaking is forbidden (decision 10);
            // cross-suppression between distinct directives stays
            // legal.
            if u32::try_from(index).is_ok_and(|index| finding.subject == Some(index)) {
                continue;
            }
            if outcome
                .directive
                .admits(finding.identifier, range.start(), text_end)
            {
                suppressed = true;
                if let Ok(index) = u32::try_from(index) {
                    used.insert(index);
                }
            }
        }
        if !suppressed {
            kept.push((finding, range));
        }
    }

    let mut diagnostics: Vec<Diagnostic> = kept
        .into_iter()
        .filter(|(finding, _)| {
            !finding
                .subject
                .is_some_and(|subject| used.contains(&subject))
                || finding.identifier != crate::rules::unused_suppression::UNUSED_SUPPRESSION
        })
        .map(|(finding, range)| {
            Diagnostic::spanned(
                finding.identifier,
                finding.severity,
                file_id,
                range,
                finding.message,
            )
        })
        .collect();
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
    use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
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
            subject: None,
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
            subject: None,
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
            subject: None,
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
            subject: None,
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

    // ---- Reporting phase ----

    use crate::context::DirectiveOutcome;
    use crate::phases::reporting_phase_diagnostics;
    use crate::rules::{unknown_suppression_identifier, unused_suppression};
    use celerrate_semantics::{DirectiveOrigin, ResolvedDirective, SuppressionFilter};

    struct NullSyntaxRule;

    impl SyntaxRule for NullSyntaxRule {
        fn check(&self, _context: &SyntaxContext<'_>, _sink: &mut FindingSink<'_>) {}
    }

    fn directive(
        anchor: (u32, u32),
        scope: (u32, u32),
        filter: SuppressionFilter,
        identifiers: &[&str],
        origin: DirectiveOrigin,
    ) -> ResolvedDirective {
        ResolvedDirective {
            anchor: TextRange::new(TextSize::from(anchor.0), TextSize::from(anchor.1)),
            scope: TextRange::new(TextSize::from(scope.0), TextSize::from(scope.1)),
            filter,
            identifiers: identifiers.iter().map(|s| (*s).to_owned()).collect(),
            origin,
        }
    }

    fn native_unused(anchor: (u32, u32), identifiers: &[&str]) -> DirectiveOutcome {
        let mut codes: Vec<_> = identifiers
            .iter()
            .filter_map(|written| celerrate_diagnostics::find_identifier(written))
            .collect();
        // Mirror production `filter_of`: `ResolvedDirective::admits`
        // binary-searches `Only`, so the fixture must satisfy the same
        // sorted-and-deduplicated invariant the consumer assumes.
        codes.sort();
        codes.dedup();
        DirectiveOutcome {
            directive: directive(
                anchor,
                (anchor.0, anchor.1),
                SuppressionFilter::Only(codes),
                identifiers,
                DirectiveOrigin::Native,
            ),
            matched: false,
        }
    }

    /// A database with the full core rule set registered and active.
    fn reporting_setup() -> TestDatabase {
        let db = TestDatabase::default();
        let identity = PluginIdentity {
            name: "celerrate-core".to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        };
        let registrations = crate::rules::core_rules()
            .into_iter()
            .map(|(metadata, implementation)| RuleRegistration {
                identity: identity.clone(),
                active: metadata.tier == Tier::Default,
                metadata,
                implementation,
            })
            .collect();
        let _ = RuleRegistry::builder(registrations)
            .durability(salsa::Durability::HIGH)
            .new(&db);
        db
    }

    fn report(db: &TestDatabase, outcomes: &[DirectiveOutcome]) -> Vec<Diagnostic> {
        reporting_phase_diagnostics(db, FileId::new(0), TextSize::from(1000), outcomes)
    }

    #[test]
    fn an_unknown_native_identifier_is_reported_and_a_known_one_is_not() {
        let db = reporting_setup();
        let diagnostics = report(&db, &[native_unused((10, 40), &["CEL0030", "CEL9999"])]);
        let unknown: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.id == unknown_suppression_identifier::UNKNOWN_SUPPRESSION_IDENTIFIER
            })
            .collect();
        assert_eq!(unknown.len(), 1);
        assert!(
            unknown[0].message.contains("CEL9999"),
            "{}",
            unknown[0].message
        );
        assert_eq!(unknown[0].severity, Severity::Warning);
    }

    #[test]
    fn a_foreign_directive_is_never_reported() {
        let db = reporting_setup();
        let outcome = DirectiveOutcome {
            directive: directive(
                (10, 40),
                (0, 41),
                SuppressionFilter::All,
                &["some.unknownIdentifier"],
                DirectiveOrigin::Foreign,
            ),
            matched: false,
        };
        assert!(report(&db, &[outcome]).is_empty());
    }

    #[test]
    fn a_bare_foreign_directive_is_never_reported() {
        // A bare foreign directive (for example `// @phpstan-ignore-next-line`)
        // resolves to `SuppressionFilter::All` with an EMPTY identifier
        // list, and `all()` over an empty list is vacuously true. The
        // origin half of CEL0042's guard is what keeps this directive
        // out of evaluability at all; without it, this directive would
        // be treated as evaluable and reported unused.
        let db = reporting_setup();
        let outcome = DirectiveOutcome {
            directive: directive(
                (10, 40),
                (0, 41),
                SuppressionFilter::All,
                &[],
                DirectiveOrigin::Foreign,
            ),
            matched: false,
        };
        assert!(report(&db, &[outcome]).is_empty());
    }

    #[test]
    fn an_unused_native_directive_is_reported_and_a_used_one_is_not() {
        let db = reporting_setup();
        let unused = native_unused((10, 40), &["CEL0030"]);
        let mut used = native_unused((50, 80), &["CEL0031"]);
        used.matched = true;
        let diagnostics = report(&db, &[unused, used]);
        let unused_reports: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.id == unused_suppression::UNUSED_SUPPRESSION)
            .collect();
        assert_eq!(unused_reports.len(), 1);
        let (_, range) = unused_reports[0].span().unwrap();
        assert_eq!(
            range,
            TextRange::new(TextSize::from(10), TextSize::from(40))
        );
    }

    #[test]
    fn a_bare_native_directive_is_reported_unused() {
        let db = reporting_setup();
        let diagnostics = report(&db, &[native_unused((10, 40), &[])]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, unused_suppression::UNUSED_SUPPRESSION);
    }

    #[test]
    fn an_unknown_identifier_makes_the_directive_not_evaluable_for_unused() {
        // CEL0041 already reports the typo; CEL0042 must not stack a
        // second warning on the same mistake (decision 11).
        let db = reporting_setup();
        let diagnostics = report(&db, &[native_unused((10, 40), &["CEL9999"])]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].id,
            unknown_suppression_identifier::UNKNOWN_SUPPRESSION_IDENTIFIER,
        );
    }

    #[test]
    fn an_identifier_of_an_inactive_rule_exempts_the_directive() {
        // Register one INACTIVE rule claiming CEL0034 alongside the
        // two reporting rules: the nursery-demotion storm guard.
        let db = TestDatabase::default();
        let identity = PluginIdentity {
            name: "celerrate-core".to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        };
        let mut registrations: Vec<RuleRegistration> = crate::rules::core_rules()
            .into_iter()
            .filter(|(metadata, _)| {
                metadata.name == "unknown-suppression-identifier"
                    || metadata.name == "unused-suppression"
            })
            .map(|(metadata, implementation)| RuleRegistration {
                identity: identity.clone(),
                active: true,
                metadata,
                implementation,
            })
            .collect();
        registrations.push(RuleRegistration {
            identity,
            active: false,
            metadata: RuleMetadata {
                name: "demoted-rule".to_owned(),
                group: RuleGroup::Correctness,
                identifiers: vec![RuleIdentifier {
                    id: DiagnosticId::new("CEL0034"),
                    severity: Severity::Error,
                }],
                tier: Tier::Nursery,
            },
            implementation: RuleImplementation::Syntax(std::sync::Arc::new(NullSyntaxRule)),
        });
        let _ = RuleRegistry::builder(registrations)
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let diagnostics = report(&db, &[native_unused((10, 40), &["CEL0034"])]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn a_resilience_identifier_is_never_treated_as_inactive() {
        // CEL0002 ("unexpected character") is emitted directly by
        // `celerrate_syntax`'s lexer; no `RuleRegistration` anywhere -
        // active or inactive - claims it in its `identifiers` list, so
        // it can never enter the inactive set the reporting phase
        // builds from claimed identifiers alone. A directive naming it
        // must stay evaluable and be reported unused when it matched
        // nothing.
        let db = reporting_setup();
        let diagnostics = report(&db, &[native_unused((10, 40), &["CEL0002"])]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].id, unused_suppression::UNUSED_SUPPRESSION);
    }

    #[test]
    fn a_directive_cannot_suppress_its_own_reports() {
        // A trailing directive whose scope covers its own anchor and
        // whose filter admits CEL0042 must not cloak its own unused
        // warning: self-admission is forbidden (decision 10), so the
        // warning survives.
        let db = reporting_setup();
        let outcome = DirectiveOutcome {
            directive: directive(
                (10, 40),
                (0, 50),
                SuppressionFilter::Only(vec![
                    celerrate_diagnostics::find_identifier("CEL0042").unwrap(),
                ]),
                &["CEL0042"],
                DirectiveOrigin::Native,
            ),
            matched: false,
        };
        let diagnostics = report(&db, &[outcome]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].id, unused_suppression::UNUSED_SUPPRESSION);
    }

    #[test]
    fn a_suppressed_directive_diagnostic_counts_as_use_in_one_pass() {
        // Directive A (index 0) is unused: its CEL0042 would fire at
        // its anchor. Directive B admits CEL0042 on a scope covering
        // A's anchor: A's warning is dropped, B counts as used, and
        // B's own CEL0042 is dropped by the subject rule in step (c) -
        // never by self-admission, which decision 10 forbids - one
        // pass, no iteration.
        let db = reporting_setup();
        let a = native_unused((10, 40), &["CEL0030"]);
        let b = native_unused((50, 90), &["CEL0042"]);
        // B's scope must cover A's anchor start.
        let b = DirectiveOutcome {
            directive: ResolvedDirective {
                scope: TextRange::new(TextSize::from(0), TextSize::from(100)),
                ..b.directive
            },
            matched: false,
        };
        assert!(report(&db, &[a, b]).is_empty());
    }

    #[test]
    fn dropping_a_suppressed_unused_report_does_not_iterate() {
        // C admits nothing anywhere; B suppresses A's CEL0042. C stays
        // unused and IS reported: uses recorded in the pass do not
        // re-open the pass.
        let db = reporting_setup();
        let a = native_unused((10, 40), &["CEL0030"]);
        let b = DirectiveOutcome {
            directive: ResolvedDirective {
                scope: TextRange::new(TextSize::from(0), TextSize::from(45)),
                ..native_unused((50, 90), &["CEL0042"]).directive
            },
            matched: false,
        };
        let c = native_unused((200, 240), &["CEL0031"]);
        let diagnostics = report(&db, &[a, b, c]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        let (_, range) = diagnostics[0].span().unwrap();
        assert_eq!(range.start(), TextSize::from(200));
    }

    #[test]
    fn mutual_cross_suppression_drops_both_findings_in_one_pass() {
        // Two distinct directives, A (index 0, anchor 10..40) and B
        // (index 1, anchor 50..90), both unused native directives
        // naming CEL0042, each with a scope (0..100) wide enough to
        // cover the other's anchor.
        //
        // Pass (a): both are unused and evaluable, so the reporting
        // rule emits one CEL0042 finding per directive: A's finding
        // (subject 0, at A's anchor) and B's finding (subject 1, at
        // B's anchor).
        // Pass (b): for A's finding, the admission loop skips A itself
        // (self-cloaking is forbidden, decision 10) and checks B; B's
        // scope covers A's anchor and B's filter admits CEL0042, so
        // A's finding is dropped and B is marked used. Symmetrically,
        // B's finding is checked only against A (B is skipped as its
        // own subject); A's scope covers B's anchor and admits
        // CEL0042, so B's finding is dropped and A is marked used.
        // Cross-suppression between distinct directives is legal
        // (decision 10 forbids only self-admission), so both drops
        // happen within this single pass.
        // Pass (c): would drop a surviving CEL0042 finding whose
        // subject became used, but both findings were already dropped
        // in pass (b); there is nothing left to drop.
        //
        // Net result: the diagnostic set is empty.
        let db = reporting_setup();
        let a = DirectiveOutcome {
            directive: ResolvedDirective {
                scope: TextRange::new(TextSize::from(0), TextSize::from(100)),
                ..native_unused((10, 40), &["CEL0042"]).directive
            },
            matched: false,
        };
        let b = DirectiveOutcome {
            directive: ResolvedDirective {
                scope: TextRange::new(TextSize::from(0), TextSize::from(100)),
                ..native_unused((50, 90), &["CEL0042"]).directive
            },
            matched: false,
        };
        let diagnostics = report(&db, &[a, b]);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}
