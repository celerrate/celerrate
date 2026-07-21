# Diagnostics and Fixes Part 3: The Framework Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the `celerrate_rules` crate — the four phase traits (`Reporting` included), the fifth extension-point registry, the sealed contexts, and the per-phase salsa queries with the per-body typed tier — and prove it by migrating the smallest check family, `syntax-version-gating` (CEL0024), into it with zero output change.

**Architecture:** `celerrate_rules` sits above `celerrate_types` and below `celerrate_plugin` (design section 2). Rules are registered into a `#[salsa::input(singleton)]` registry on the exact template of the existing four registries; core rules register at the composition root under a reserved core identity that never enters the plugin-set digest. Rules consume **outcomes** through sealed contexts (the `InvocationSite`/`TypeContext` model from issue #61: private database handle, delegating public methods, no salsa vocabulary in any rule-facing signature) and emit findings into a sink whose severities come from rule metadata. Phase queries drain the active rules per file (per body in the typed tier) and reconcile anchors to ranges at their tail. The gated-construct **walk stays below** in `celerrate_semantics` as an outcome query; only diagnostic construction moves up (design section 2: "What moves is diagnostic construction, not the walks").

**Tech Stack:** Rust (edition 2024, workspace toolchain), salsa 0.27 tracked queries and singleton inputs, `trybuild` compile-fail seal proofs, the existing corpus/seeded-defect/invalidation-scope/warm-cold harnesses.

## Global Constraints

Copied from the project's non-negotiable rules; every task implicitly includes them.

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Production code returns `Result` / `Option`. Test modules may locally `#[allow]` these lints with a one-line justification comment (existing convention).
- TDD: failing test first, minimal implementation, refactor. No production code without a test that demanded it.
- Strict layering: `celerrate_rules` depends on `celerrate_source`, `celerrate_diagnostics`, `celerrate_db`, `celerrate_project`, `celerrate_semantics`, `celerrate_types`, and `salsa` — nothing higher. `celerrate_plugin` re-exports nominally, never a whole crate, never salsa.
- Determinism: phase queries iterate registrations in registration order and sort their output by the `Diagnostic` total order. No wall clock, no randomness, no environment reads inside queries.
- Error resilience: a finding whose anchor no longer resolves is dropped, never a panic.
- Everything in English, full words, no abbreviated names (`registration`, not `reg`).
- Commits: gitmoji + Conventional Commits (e.g. `✨ feat(rules): ...`), authored with the repository-configured identity (never override it).
- Local gates before every commit: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`. Run `cargo deny check` when a manifest changes.
- Corpus gates where a task says so: `cargo xtask fetch-corpus` once, then `cargo xtask corpus` (byte-identical snapshot) and `cargo xtask mixed-rate` (baseline unchanged).

## Existing surface this plan consumes (verified against the code)

- `celerrate_diagnostics`: `Diagnostic` (`::spanned(id, severity, file, range, message)`, public `labels`/`notes`/`suggestions` vecs, total `Ord`, derived `Eq`), `DiagnosticId` (`const fn new(&'static str)`, `.as_str()`), `Severity::{Warning, Error}`, `ExplainPage`, `REGISTRY: &[RegisteredDiagnostic]` with `RegisteredDiagnostic { id, family, owner, explain }` and the in-crate gapless test hard-coding 40 entries (unchanged by this plan: no new identifiers are allocated here).
- CEL0024 today: `crates/celerrate_semantics/src/syntax_gating.rs` — `SYNTAX_NOT_AVAILABLE`, the `gated_uses` walk (seven constructs), and the tracked query `syntax_version_diagnostics(db, file, configuration)`. Merged by `semantic_diagnostics` (`crates/celerrate_semantics/src/queries.rs:70-88`), which the CLI's `persistable_diagnostics` (`crates/celerrate_cli/src/analysis.rs:174-193`) consumes ahead of `retain_unsuppressed` and persistence. Message format: `` `{label}` requires PHP {required}, but the project's minimum PHP version is {minimum} ``, severity `Error`.
- `celerrate_semantics` public exports used here: `AstId { file: FileId, index: u32 }`, `AstIdMap` (`.pointer(index) -> Option<pointer>`, `pointer.try_to_node(&root) -> Option<SyntaxNode>`), `ast_id_map(db, file) -> AstIdMap` (plain function over the parse), `BodyQuery` (`#[salsa::interned]`, `::new(db, ast_id)`, `.ast_id(db)`), `body_ir(db, file, body) -> &Option<BodyIr>`, `body_source_map(db, file, body) -> &Option<BodySourceMap>` (`.expression_pointer(id) -> Option<pointer>`, `pointer.text_range()`), `ExpressionId` (`::from_index(usize) -> Option<Self>`), `member_tree(db, file) -> &MemberTree` (`.functions`, `.classes`, `Declaration.kind`, `Member.kind`, `.ast_id`), `DeclarationKind`, `MemberKind`, `PluginIdentity { name, version, configuration }`, `suppressed_ranges`, `reference_diagnostics`.
- The registry template (all four existing registries): a `Send + Sync` trait; a registration record `{ identity: PluginIdentity, implementation: Arc<dyn Trait> }` with a hand-written `Debug` printing only the identity via `.finish_non_exhaustive()`; a `#[salsa::input(singleton)]` registry holding `#[returns(ref)] pub registrations: Vec<...>`; set once at the composition root with `.durability(salsa::Durability::HIGH).new(database)`; consumers read `Registry::try_get(db)` — `None` is the no-plugin path (`suppressed_ranges` is the verbatim idiom).
- The sealing precedent (issue #61): `InvocationSite`/`TypeContext` in `celerrate_types` — private `db` field, `pub(crate) fn new`, a public `testing_*` construction seam (harmless because it demands a `&dyn salsa::Database`, which the facade never provides), and trybuild compile-fail cases in `crates/celerrate_plugin/tests/seal/` with pinned `.stderr` files.
- Composition root: `crates/celerrate_cli/src/plugins.rs` (`register_plugins`, `admission`, `RegisteredPlugins { admitted, excluded }`, `plugin_set_digest` — hashing only the admitted identities and excluded names), called from `Session::start` (`crates/celerrate_cli/src/session.rs`) after the singleton inputs and before the cache load. `AnalysisDatabase` in `crates/celerrate_cli/src/database.rs`.
- The identifier ledger: each producer crate exports `pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId]`; `crates/celerrate_cli/tests/registry.rs` derives producers by scanning dependency sources for that literal, and asserts `producers()` equals the derivation, allocation uniqueness, registry equality, and that `REGISTRY.owner` names the actual allocator.
- Test idioms: `celerrate_db::testing::TestDatabase` (`.take_executed()` yields the salsa execution log), `SourceFile::new(&db, FileId::new(n), bytes)`, `AnalyzedFileSet::new(&db, vec![...])`, `ProjectConfiguration::builder(PhpVersionRange::new(...)).durability(salsa::Durability::MEDIUM).new(&db)`, `salsa::Setter` for `file.set_bytes(&mut db).to(...)`, and the per-crate `executions_of(log, query)` counter (duplicated per crate by design; copy it, do not share it).
- Harnesses this plan must keep green: `crates/celerrate_cli/tests/seeded_defects.rs` (extend with CEL0024), `crates/celerrate_cli/tests/cache_equivalence.rs` (warm equals cold, unchanged), `crates/celerrate_semantics/tests/invalidation_scope.rs` and `crates/celerrate_types/tests/invalidation_scope.rs` (pins move or gain assertions, never weaken), `xtask corpus` and `xtask mixed-rate`.

## Design decisions this plan locks in (with their design-spec anchors)

1. **Context surfaces grow at the pace of shipped rules** (the `TypeContext` YAGNI criterion, quoted in its rustdoc). `SyntaxContext` exposes exactly what `syntax-version-gating` consumes (`php_version_range`, `gated_syntax_uses`); the line index and any generic tree interrogation arrive with their first client (the style group). `SemanticContext` and `TypedBodyContext` exist, are sealed, and expose one plain-data method each; part 4's per-family plans enumerate their real facade methods (design section 2 makes that enumeration part-4 work, not part-3 work).
2. **The walk stays below.** `gated_uses` becomes the public outcome query `celerrate_semantics::gated_syntax_uses(db, file)`, version-range independent (a configuration change re-filters without re-walking — a strict invalidation narrowing, never a broadening). The rule consumes outcomes and constructs diagnostics (design section 2).
3. **The `Reporting` phase is declared, not executed.** Its trait and sealed context exist so the registry model and the ownership gate see the phase (design section 4), but its execution point arrives in part 5 with its input (the per-directive match records). It is not re-exported by the facade.
4. **The typed phase query is built and pinned but not CLI-wired.** Its per-body tier, reconciliation, and invalidation pins are proven here with a test rule; wiring it into the CLI composition is part 4's typed-family migration, where the stored-verdict co-production is handled. The syntax and semantic phase queries are CLI-wired now (the semantic one is empty until part 4 and contributes nothing).
5. **The finding sink is minimal.** Findings carry identifier, anchor, and message; severity is resolved from rule metadata (design section 4: severity is metadata, not a rule's choice). Notes, labels, and the symbolic suggestion vocabulary join the sink with their first emitting family (part 4), exactly as `celerrate_edit` grew only its shipped operations.
6. **No new diagnostic identifiers.** CEL0041/CEL0042 are part 5. The registry stays at 40; only CEL0024's owner string changes.
7. **The emission-side source scan** ("no check outside the framework") is closure-gate machinery: it cannot be enabled while unmigrated families legitimately construct diagnostics in domain crates, so it belongs to the closure part, not here.
8. **Construction seams are database-gated.** Cross-crate context constructors (`semantic_context`, `typed_body_context`) are public but take `&dyn salsa::Database`, which the facade never re-exports and plugin crates cannot name (the `dependency_shape` check forbids a direct salsa dependency) — the same reason `testing_type_context` is safely public today.

## Out of scope (later parts of the design)

Identifier-aware suppression, the correspondence table, the native directive, CEL0041/CEL0042 (part 5); the autofix engine and did-you-mean (part 6); the renderer and its cargo feature on `celerrate_rules` (part 7 — the crate is created without features here); explain pages and their harness (part 8); the semantic and typed family migrations (part 4); plugin-registered rules (deferred by design section 8).

## File structure

```
crates/celerrate_rules/
  Cargo.toml                      new crate manifest
  src/lib.rs                      module map and re-exports
  src/metadata.rs                 RuleMetadata, RuleIdentifier, RuleGroup, Tier
  src/finding.rs                  FindingAnchor, Finding, FindingSink
  src/context.rs                  SyntaxContext (owned here by design), ReportingContext
  src/traits.rs                   SyntaxRule, SemanticRule, TypedBodyRule, ReportingRule
  src/registry.rs                 RuleImplementation, RuleRegistration, RuleRegistry,
                                  RuleConflict, validate_rules, CORE_IDENTITY_NAME
  src/phases.rs                   syntax/semantic/typed phase queries, anchor resolution
  src/rules/mod.rs                core_rules()
  src/rules/syntax_version_gating.rs
  tests/invalidation_scope.rs     the phase-query pins

crates/celerrate_semantics/
  src/syntax_gating.rs            refactored: GatedSyntaxUse + gated_syntax_uses walk query;
                                  syntax_version_diagnostics deleted at the swap
  src/rule_context.rs             new: SemanticContext + semantic_context seam
  src/queries.rs                  semantic_diagnostics drops the gating merge at the swap
  src/reference_checks.rs         ALLOCATED_IDENTIFIERS shrinks at the ownership move
  src/lib.rs                      re-export updates

crates/celerrate_types/
  src/rule_context.rs             new: TypedBodyContext + typed_body_context seam
  src/lib.rs                      re-export update

crates/celerrate_diagnostics/
  src/registry.rs                 CEL0024 owner -> "celerrate_rules"

crates/celerrate_cli/
  Cargo.toml                      + celerrate_rules
  src/plugins.rs                  register_core_rules, core-identity admission guard
  src/session.rs                  register_core_rules call
  src/analysis.rs                 persistable_diagnostics wires the syntax + semantic phases
  tests/registry.rs               producers() gains celerrate_rules
  tests/seeded_defects.rs         + CEL0024 fixture

crates/celerrate_plugin/
  Cargo.toml                      + celerrate_rules
  src/lib.rs                      rule-authoring re-exports
  tests/seal/                     new compile-fail cases

.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md   new rule-phase section
CHANGELOG.md
```

---

### Task 1: Extract the gated-construct walk into an outcome query

**Files:**
- Modify: `crates/celerrate_semantics/src/syntax_gating.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs:82`
- Test: in-module tests of `syntax_gating.rs`, plus `crates/celerrate_semantics/tests/invalidation_scope.rs:554-584`

**Interfaces:**
- Consumes: the existing `gated_uses(root) -> Vec<GatedUse>` walk and `syntax_version_diagnostics` query.
- Produces: `pub struct GatedSyntaxUse { pub label: &'static str, pub required: PhpVersion, pub range: TextRange }` and `#[salsa::tracked(returns(ref))] pub fn gated_syntax_uses(db: &dyn salsa::Database, file: SourceFile) -> Vec<GatedSyntaxUse>`, both re-exported from `celerrate_semantics`. `syntax_version_diagnostics` keeps its exact signature and output (Task 7 deletes it).

- [ ] **Step 1: Write the failing tests**

In the `tests` module of `syntax_gating.rs`, add a walk-level test (the walk is now an outcome, version-independent, so every gated use appears regardless of the range):

```rust
#[test]
fn the_walk_reports_every_gated_use_regardless_of_the_version_range() {
    let db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php readonly class Point { public const int X = 1; }".to_vec(),
    );
    let uses = gated_syntax_uses(&db, file);
    let labels: Vec<&str> = uses.iter().map(|gated| gated.label).collect();
    assert_eq!(labels, vec!["readonly class", "typed constant"]);
    assert_eq!(uses[0].required, PhpVersion::new(8, 2));
}
```

And in `crates/celerrate_semantics/tests/invalidation_scope.rs`, extend the existing pin `a_version_range_change_re_runs_the_gating_queries` with one assertion after the existing ones (the narrowing this refactor buys):

```rust
    assert_eq!(
        executions_of(&log, "gated_syntax_uses"),
        0,
        "the walk is version-independent; a range change only re-filters: {log:?}",
    );
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_semantics syntax_gating gating_queries`
Expected: FAIL — `gated_syntax_uses` not found.

- [ ] **Step 3: Implement the refactor**

In `syntax_gating.rs`: replace the private `GatedUse` with the public outcome type, wrap the walk in a tracked query, and make `syntax_version_diagnostics` a filter over it. The walk body (`gated_uses`, renamed `collect_gated_uses`) is unchanged except for the type rename.

```rust
/// One use of a version-gated construct, in tree order: the outcome
/// the syntax-version-gating rule consumes. The walk stays below; the
/// rule turns outcomes into diagnostics (design section 2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GatedSyntaxUse {
    pub label: &'static str,
    pub required: PhpVersion,
    pub range: TextRange,
}

/// Every gated-construct use in the file, in tree order.
/// Version-range independent on purpose: a configuration change
/// re-filters without re-walking.
#[salsa::tracked(returns(ref))]
pub fn gated_syntax_uses(db: &dyn salsa::Database, file: SourceFile) -> Vec<GatedSyntaxUse> {
    let root = celerrate_db::parse(db, file).tree();
    collect_gated_uses(&root)
}

/// The per-file syntax gating diagnostics.
#[salsa::tracked(returns(ref))]
pub fn syntax_version_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic> {
    let minimum = configuration.php_version_range(db).minimum;
    let file_id = file.file_id(db);
    let mut diagnostics: Vec<Diagnostic> = gated_syntax_uses(db, file)
        .iter()
        .filter(|gated| gated.required > minimum)
        .map(|gated| {
            Diagnostic::spanned(
                SYNTAX_NOT_AVAILABLE,
                Severity::Error,
                file_id,
                gated.range,
                format!(
                    "`{}` requires PHP {}, but the project's minimum PHP version is {minimum}",
                    gated.label, gated.required,
                ),
            )
        })
        .collect();
    diagnostics.sort();
    diagnostics
}
```

`collect_gated_uses(root: &SyntaxNode) -> Vec<GatedSyntaxUse>` is the existing `gated_uses` body with `GatedUse` replaced by `GatedSyntaxUse` in the eight construct arms. Update `lib.rs:82`:

```rust
pub use syntax_gating::{
    GatedSyntaxUse, SYNTAX_NOT_AVAILABLE, gated_syntax_uses, syntax_version_diagnostics,
};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_semantics`
Expected: PASS, including every pre-existing gating unit test (behavior preserved: same messages, same ranges, same order).

- [ ] **Step 5: Run the local gates and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`

```bash
git add crates/celerrate_semantics
git commit -m "♻️ refactor(semantics): extract the gated-construct walk into an outcome query"
```

---

### Task 2: The `celerrate_rules` crate scaffold — metadata and the finding sink

**Files:**
- Create: `crates/celerrate_rules/Cargo.toml`, `crates/celerrate_rules/src/lib.rs`, `crates/celerrate_rules/src/metadata.rs`, `crates/celerrate_rules/src/finding.rs`

**Interfaces:**
- Produces: `RuleGroup::Correctness`, `Tier::{Default, Nursery}`, `RuleIdentifier { id: DiagnosticId, severity: Severity }`, `RuleMetadata { name: String, group, identifiers: Vec<RuleIdentifier>, tier }` with `severity_of(&self, DiagnosticId) -> Option<Severity>`; `FindingAnchor::{Range(TextRange), Declaration(AstId), Expression { body: AstId, expression: ExpressionId }}`; `FindingSink` with `pub fn report(&mut self, identifier, anchor, message)` and crate-internal `new(&RuleMetadata)` / `into_findings() -> Vec<Finding>`; `pub(crate) struct Finding { identifier, severity, anchor, message }`.
- Note: metadata is **owned data, not `&'static`** — design section 8 pins that shape so plugin rules can travel registration data later.

- [ ] **Step 1: Create the manifest and module map**

`crates/celerrate_rules/Cargo.toml`:

```toml
[package]
name = "celerrate_rules"
description = "The rule framework: phase traits, the rule registry, and the core rules"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
salsa = { workspace = true }
celerrate_db = { path = "../celerrate_db" }
celerrate_diagnostics = { path = "../celerrate_diagnostics" }
celerrate_project = { path = "../celerrate_project" }
celerrate_semantics = { path = "../celerrate_semantics" }
celerrate_source = { path = "../celerrate_source" }
celerrate_types = { path = "../celerrate_types" }

[lints]
workspace = true
```

`src/lib.rs` (grows over the next tasks; start with):

```rust
//! The rule framework: rules are coherent families with declared
//! identifiers and metadata, registered into the fifth extension-point
//! registry, executed by per-phase queries against sealed contexts,
//! and reporting through a sink whose severities come from metadata.

mod finding;
mod metadata;

pub use finding::{FindingAnchor, FindingSink};
pub use metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
```

- [ ] **Step 2: Write the failing tests**

In `src/metadata.rs` and `src/finding.rs` test modules (written first, alongside empty type stubs so the crate compiles enough to fail on assertions, or written directly with the types — the red step is the first `cargo test -p celerrate_rules` run):

```rust
// metadata.rs tests
#[test]
fn severity_is_looked_up_per_identifier() {
    let metadata = test_metadata();
    assert_eq!(
        metadata.severity_of(DiagnosticId::new("CEL9998")),
        Some(Severity::Error),
    );
    assert_eq!(metadata.severity_of(DiagnosticId::new("CEL9999")), None);
}
```

```rust
// finding.rs tests
#[test]
fn a_declared_identifier_is_accepted_with_its_metadata_severity() {
    let metadata = test_metadata();
    let mut sink = FindingSink::new(&metadata);
    sink.report(
        DiagnosticId::new("CEL9998"),
        FindingAnchor::Range(TextRange::new(TextSize::from(0), TextSize::from(1))),
        "finding".to_owned(),
    );
    let findings = sink.into_findings();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::Error);
}

#[test]
fn an_undeclared_identifier_is_dropped_never_a_panic() {
    let metadata = test_metadata();
    let mut sink = FindingSink::new(&metadata);
    sink.report(
        DiagnosticId::new("CEL9999"),
        FindingAnchor::Range(TextRange::new(TextSize::from(0), TextSize::from(1))),
        "undeclared".to_owned(),
    );
    assert!(sink.into_findings().is_empty());
}
```

The shared `test_metadata()` helper builds a `RuleMetadata` named `"test-rule"`, group `Correctness`, tier `Default`, declaring `CEL9998` at `Severity::Error`.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p celerrate_rules`
Expected: FAIL to compile (types missing) — that is the red step for a new crate.

- [ ] **Step 4: Implement**

`src/metadata.rs`:

```rust
use celerrate_diagnostics::{DiagnosticId, Severity};

/// The rule groups of the parent design. Only `correctness` exists
/// until the style group arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleGroup {
    Correctness,
}

/// Whether a rule joins the default-enabled set. Demotion under the
/// anti-false-positive policy is a one-line change of this field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Default,
    Nursery,
}

/// One identifier a rule may emit, with its default severity
/// (families already mix `Error` and `Warning`, so severity is
/// per identifier, not per rule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleIdentifier {
    pub id: DiagnosticId,
    pub severity: Severity,
}

/// A rule's declarative unit: a coherent family, not a single
/// identifier. Owned data, not `&'static` — plugin-registered rules
/// will travel their metadata as registration data (design section 8
/// pins that shape now).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleMetadata {
    /// Stable kebab-case name, e.g. `syntax-version-gating`.
    pub name: String,
    pub group: RuleGroup,
    /// The closed list of identifiers the rule may emit.
    pub identifiers: Vec<RuleIdentifier>,
    pub tier: Tier,
}

impl RuleMetadata {
    /// The default severity of `id`, `None` when the rule never
    /// declared it (the sink drops such an emission).
    pub fn severity_of(&self, id: DiagnosticId) -> Option<Severity> {
        self.identifiers
            .iter()
            .find(|identifier| identifier.id == id)
            .map(|identifier| identifier.severity)
    }
}
```

`src/finding.rs`:

```rust
use celerrate_diagnostics::{DiagnosticId, Severity};
use celerrate_semantics::{AstId, ExpressionId};
use celerrate_source::TextRange;

use crate::metadata::RuleMetadata;

/// Where a finding lands. Range-late phases anchor by identity and
/// reconcile at the phase query's tail; a phase that honestly has a
/// same-file range (the syntax phase) uses it directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingAnchor {
    /// A concrete range in the checked file.
    Range(TextRange),
    /// A declaration, resolved through the `AstIdMap` at the tail.
    Declaration(AstId),
    /// An expression in a body arena, resolved through the body
    /// source map at the tail (the `TypedVerdict` pattern generalized).
    Expression { body: AstId, expression: ExpressionId },
}

/// One accepted finding, severity already resolved from metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub identifier: DiagnosticId,
    pub severity: Severity,
    pub anchor: FindingAnchor,
    pub message: String,
}

/// The sink one rule reports into. Severity comes from the rule's own
/// metadata — a rule cannot choose a severity its declaration did not
/// fix. An identifier outside the declared list is dropped, never a
/// panic (pinned by test); the registry's declaration checks make a
/// core rule doing so a bug caught in CI.
pub struct FindingSink<'rule> {
    metadata: &'rule RuleMetadata,
    findings: Vec<Finding>,
}

impl<'rule> FindingSink<'rule> {
    pub(crate) fn new(metadata: &'rule RuleMetadata) -> Self {
        Self {
            metadata,
            findings: Vec::new(),
        }
    }

    pub fn report(&mut self, identifier: DiagnosticId, anchor: FindingAnchor, message: String) {
        let Some(severity) = self.metadata.severity_of(identifier) else {
            return;
        };
        self.findings.push(Finding {
            identifier,
            severity,
            anchor,
            message,
        });
    }

    pub(crate) fn into_findings(self) -> Vec<Finding> {
        self.findings
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p celerrate_rules`
Expected: PASS.

- [ ] **Step 6: Run the local gates and commit**

Run the gates (a manifest changed: include `cargo deny check`).

```bash
git add crates/celerrate_rules Cargo.lock
git commit -m "✨ feat(rules): scaffold the rule crate with metadata and the finding sink"
```

---

### Task 3: The four phase traits and their sealed contexts

**Files:**
- Create: `crates/celerrate_rules/src/context.rs`, `crates/celerrate_rules/src/traits.rs`
- Create: `crates/celerrate_semantics/src/rule_context.rs`; modify `crates/celerrate_semantics/src/lib.rs`
- Create: `crates/celerrate_types/src/rule_context.rs`; modify `crates/celerrate_types/src/lib.rs`
- Modify: `crates/celerrate_rules/src/lib.rs`

**Interfaces:**
- Consumes: `gated_syntax_uses` (Task 1), `ProjectConfiguration::php_version_range(db)`, `SourceFile::file_id(db)`.
- Produces:
  - `celerrate_rules::SyntaxContext<'db>` — `pub(crate) fn new(db, file, configuration)`, `pub fn php_version_range(&self) -> PhpVersionRange`, `pub fn gated_syntax_uses(&self) -> &'db [GatedSyntaxUse]`, plus `pub fn testing_syntax_context(db, file, configuration)`.
  - `celerrate_rules::ReportingContext<'db>` — declared, no surface (part 5).
  - `celerrate_semantics::SemanticContext<'db>` — `pub fn file(&self) -> FileId`; seam `pub fn semantic_context(db, file) -> SemanticContext`.
  - `celerrate_types::TypedBodyContext<'db>` — `pub fn body(&self) -> AstId`; seam `pub fn typed_body_context(db, body: AstId) -> TypedBodyContext`.
  - `celerrate_rules::{SyntaxRule, SemanticRule, TypedBodyRule, ReportingRule}` — each `Send + Sync` with `fn check(&self, context: &...Context<'_>, sink: &mut FindingSink<'_>)`.

- [ ] **Step 1: Write the failing tests**

In `crates/celerrate_rules/src/context.rs` tests (the sealing model's positive proof, mirroring `dynamic_type_provider.rs:308-324`):

```rust
#[test]
fn the_syntax_context_exposes_outcomes_and_never_the_database() {
    let db = TestDatabase::default();
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php readonly class Point {}".to_vec(),
    );
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    let context = testing_syntax_context(&db, file, configuration);
    assert_eq!(context.php_version_range().minimum, PhpVersion::new(8, 1));
    assert_eq!(context.gated_syntax_uses().len(), 1);
    assert_eq!(context.gated_syntax_uses()[0].label, "readonly class");
}
```

In `crates/celerrate_semantics/src/rule_context.rs` and `crates/celerrate_types/src/rule_context.rs` tests, the analogous one-method checks (`semantic_context(...).file() == FileId::new(0)`; `typed_body_context(&db, ast_id).body() == ast_id`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p celerrate_rules -p celerrate_semantics -p celerrate_types rule_context syntax_context`
Expected: FAIL to compile — the modules do not exist.

- [ ] **Step 3: Implement the contexts**

`crates/celerrate_rules/src/context.rs`:

```rust
use celerrate_db::SourceFile;
use celerrate_project::{PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::GatedSyntaxUse;

/// The syntax-phase context. Owned by `celerrate_rules` — its contents
/// span `celerrate_db` and `celerrate_project` with no single domain
/// owner (design section 4's stated exception). Sealed on the
/// `InvocationSite` model: the database is private, methods delegate,
/// and no salsa vocabulary appears in any rule-facing signature. The
/// surface is exactly what the shipped syntax rules consume (the
/// `TypeContext` YAGNI criterion); the line index and any generic tree
/// interrogation arrive with their first client.
pub struct SyntaxContext<'db> {
    db: &'db dyn salsa::Database,
    file: SourceFile,
    configuration: ProjectConfiguration,
}

impl<'db> SyntaxContext<'db> {
    pub(crate) fn new(
        db: &'db dyn salsa::Database,
        file: SourceFile,
        configuration: ProjectConfiguration,
    ) -> Self {
        Self {
            db,
            file,
            configuration,
        }
    }

    /// The project's supported PHP version range.
    pub fn php_version_range(&self) -> PhpVersionRange {
        self.configuration.php_version_range(self.db)
    }

    /// Every version-gated construct use in the file, in tree order.
    pub fn gated_syntax_uses(&self) -> &'db [GatedSyntaxUse] {
        celerrate_semantics::gated_syntax_uses(self.db, self.file)
    }
}

/// Test-only construction seam, same contract as
/// `testing_type_context`: harmless because it demands a database
/// handle, which the facade never provides.
pub fn testing_syntax_context<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    configuration: ProjectConfiguration,
) -> SyntaxContext<'db> {
    SyntaxContext::new(db, file, configuration)
}

/// The `Reporting` phase context. Part 5 gives it its real surface
/// (directives and their per-directive match outcomes); it exists now
/// so the phase trait and the registry see the phase (design section
/// 4). Core-only: never re-exported by the facade.
pub struct ReportingContext<'db> {
    _database: &'db dyn salsa::Database,
}
```

`crates/celerrate_semantics/src/rule_context.rs` (add `mod rule_context;` and `pub use rule_context::{SemanticContext, semantic_context};` to `lib.rs`):

```rust
use celerrate_db::SourceFile;
use celerrate_source::FileId;

/// The semantic-phase context, owned by this crate (design section 4).
/// Sealed: private database, delegating methods, no salsa vocabulary
/// rule-side. Part 4's family migrations enumerate its real facade
/// methods (resolution outcomes, symbol index); until then it carries
/// only plain file identity.
pub struct SemanticContext<'db> {
    db: &'db dyn salsa::Database,
    file: SourceFile,
}

impl SemanticContext<'_> {
    /// The checked file's identity.
    pub fn file(&self) -> FileId {
        self.file.file_id(self.db)
    }
}

/// Engine construction seam. Public but database-gated: the facade
/// never re-exports salsa nor hands out a database, so a plugin can
/// neither name nor supply the argument (the `testing_type_context`
/// precedent).
pub fn semantic_context<'db>(db: &'db dyn salsa::Database, file: SourceFile) -> SemanticContext<'db> {
    SemanticContext { db, file }
}
```

`crates/celerrate_types/src/rule_context.rs` (add `mod rule_context;` and `pub use rule_context::{TypedBodyContext, typed_body_context};` to `lib.rs`):

```rust
use celerrate_semantics::AstId;

/// The typed-body-phase context, owned by this crate (design section
/// 4). One context per checked body — the per-body tracked tier's
/// unit. Part 4's family migrations enumerate its real facade methods
/// (body IR interrogation, inferred types, membership and
/// assignability questions), each recording its consulted classes
/// structurally; until then it carries only the body's identity.
pub struct TypedBodyContext<'db> {
    _database: &'db dyn salsa::Database,
    body: AstId,
}

impl TypedBodyContext<'_> {
    /// The identity of the body under check.
    pub fn body(&self) -> AstId {
        self.body
    }
}

/// Engine construction seam, database-gated like `semantic_context`.
pub fn typed_body_context<'db>(db: &'db dyn salsa::Database, body: AstId) -> TypedBodyContext<'db> {
    TypedBodyContext {
        _database: db,
        body,
    }
}
```

`crates/celerrate_rules/src/traits.rs`:

```rust
use celerrate_semantics::SemanticContext;
use celerrate_types::TypedBodyContext;

use crate::context::{ReportingContext, SyntaxContext};
use crate::finding::FindingSink;

/// A rule of the syntax phase: syntax outcomes and the PHP version
/// range, no name resolution, no types.
pub trait SyntaxRule: Send + Sync {
    fn check(&self, context: &SyntaxContext<'_>, sink: &mut FindingSink<'_>);
}

/// A rule of the semantic phase: reference resolution outcomes and the
/// symbol index (surface arrives with part 4's migrated families).
pub trait SemanticRule: Send + Sync {
    fn check(&self, context: &SemanticContext<'_>, sink: &mut FindingSink<'_>);
}

/// A rule of the typed-body phase, executed once per body under the
/// per-body tracked tier.
pub trait TypedBodyRule: Send + Sync {
    fn check(&self, context: &TypedBodyContext<'_>, sink: &mut FindingSink<'_>);
}

/// A rule of the reporting phase: directives and their match outcomes.
/// Core-only in this sub-project (design section 4): declared so the
/// registry model and the ownership gate see the phase; its execution
/// point and context surface arrive in part 5, and the facade does not
/// re-export it.
pub trait ReportingRule: Send + Sync {
    fn check(&self, context: &ReportingContext<'_>, sink: &mut FindingSink<'_>);
}
```

Extend `crates/celerrate_rules/src/lib.rs`:

```rust
mod context;
mod traits;

pub use context::{ReportingContext, SyntaxContext, testing_syntax_context};
pub use traits::{ReportingRule, SemanticRule, SyntaxRule, TypedBodyRule};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p celerrate_rules -p celerrate_semantics -p celerrate_types`
Expected: PASS.

- [ ] **Step 5: Run the local gates and commit**

```bash
git add crates/celerrate_rules crates/celerrate_semantics crates/celerrate_types
git commit -m "✨ feat(rules): declare the four phase traits and their sealed contexts"
```

---

### Task 4: The rule registry — the fifth extension-point registry

**Files:**
- Create: `crates/celerrate_rules/src/registry.rs`
- Modify: `crates/celerrate_rules/src/lib.rs`

**Interfaces:**
- Consumes: `PluginIdentity` (from `celerrate_semantics`), the four traits (Task 3), `RuleMetadata` (Task 2).
- Produces: `CORE_IDENTITY_NAME: &str = "celerrate-core"`; `RuleImplementation::{Syntax, Semantic, TypedBody, Reporting}(Arc<dyn ...>)`; `RuleRegistration { identity: PluginIdentity, active: bool, metadata: RuleMetadata, implementation: RuleImplementation }` (hand-written `Debug` printing identity and rule name only); `#[salsa::input(singleton)] RuleRegistry { registrations: Vec<RuleRegistration> }`; `RuleConflict` and `pub fn validate_rules(&[RuleRegistration]) -> Result<(), RuleConflict>`.
- The `active` flag is **computed at the composition root** (`tier == Tier::Default` today); sub-project 5's configuration changes only that computation, and the reserved cache-header field absorbs it (design sections 3 and 4).

- [ ] **Step 1: Write the failing tests** (in `registry.rs` tests module)

```rust
#[test]
fn a_duplicate_identifier_claim_is_a_conflict_naming_both_rules() {
    let first = test_registration("first-rule", "CEL9998");
    let second = test_registration("second-rule", "CEL9998");
    assert_eq!(
        validate_rules(&[first, second]),
        Err(RuleConflict::DuplicateIdentifier {
            id: DiagnosticId::new("CEL9998"),
            first: "first-rule".to_owned(),
            second: "second-rule".to_owned(),
        }),
    );
}

#[test]
fn a_duplicate_rule_name_is_a_conflict() {
    let first = test_registration("same-name", "CEL9997");
    let second = test_registration("same-name", "CEL9998");
    assert_eq!(
        validate_rules(&[first, second]),
        Err(RuleConflict::DuplicateName {
            name: "same-name".to_owned(),
        }),
    );
}

#[test]
fn an_empty_identifier_list_is_a_conflict() {
    let mut registration = test_registration("no-identifiers", "CEL9998");
    registration.metadata.identifiers.clear();
    assert_eq!(
        validate_rules(std::slice::from_ref(&registration)),
        Err(RuleConflict::EmptyIdentifierList {
            name: "no-identifiers".to_owned(),
        }),
    );
}

#[test]
fn a_conflict_free_set_validates() {
    let first = test_registration("first-rule", "CEL9997");
    let second = test_registration("second-rule", "CEL9998");
    assert_eq!(validate_rules(&[first, second]), Ok(()));
}

#[test]
fn debug_prints_identity_and_name_never_the_implementation() {
    let registration = test_registration("printable", "CEL9998");
    let printed = format!("{registration:?}");
    assert!(printed.contains("printable"));
    assert!(!printed.contains("implementation"));
}
```

`test_registration(name, id)` builds a `RuleRegistration` with a throwaway identity, `active: true`, metadata declaring `id` at `Severity::Error`, and `RuleImplementation::Syntax(Arc::new(NullSyntaxRule))` where `NullSyntaxRule`'s `check` does nothing.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p celerrate_rules registry`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use celerrate_diagnostics::DiagnosticId;
use celerrate_semantics::PluginIdentity;

use crate::metadata::RuleMetadata;
use crate::traits::{ReportingRule, SemanticRule, SyntaxRule, TypedBodyRule};

/// The reserved registration identity of core rules. Core
/// registrations never enter the admitted plugin set, so they never
/// key the plugin-set digest — binary identity already keys the cache
/// for core behavior (design section 2). The composition root refuses
/// a plugin descriptor carrying this name.
pub const CORE_IDENTITY_NAME: &str = "celerrate-core";

/// A rule's phase-typed implementation.
#[derive(Clone)]
pub enum RuleImplementation {
    Syntax(Arc<dyn SyntaxRule>),
    Semantic(Arc<dyn SemanticRule>),
    TypedBody(Arc<dyn TypedBodyRule>),
    Reporting(Arc<dyn ReportingRule>),
}

impl RuleImplementation {
    fn phase_name(&self) -> &'static str {
        match self {
            Self::Syntax(_) => "syntax",
            Self::Semantic(_) => "semantic",
            Self::TypedBody(_) => "typed-body",
            Self::Reporting(_) => "reporting",
        }
    }
}

impl std::fmt::Debug for RuleImplementation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuleImplementation")
            .field("phase", &self.phase_name())
            .finish_non_exhaustive()
    }
}

/// One registered rule: who registered it, whether it is in the active
/// set, its declarative unit, and its implementation. `active` is
/// computed at the composition root (`Default`-tier rules are active,
/// `Nursery` rules are not); sub-project 5's configuration adjusts
/// that computation and nothing else.
#[derive(Clone)]
pub struct RuleRegistration {
    pub identity: PluginIdentity,
    pub active: bool,
    pub metadata: RuleMetadata,
    pub implementation: RuleImplementation,
}

impl std::fmt::Debug for RuleRegistration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuleRegistration")
            .field("identity", &self.identity)
            .field("active", &self.active)
            .field("name", &self.metadata.name)
            .finish_non_exhaustive()
    }
}

/// The fifth extension-point registry, on the template of the existing
/// four: set once at the composition root with HIGH durability;
/// consumers read `try_get` — unset is the empty path.
#[salsa::input(singleton)]
pub struct RuleRegistry {
    #[returns(ref)]
    pub registrations: Vec<RuleRegistration>,
}

/// Why a rule set does not validate. A core-versus-core conflict is a
/// bug and fails the composition-root test in CI, never a runtime
/// degradation (design section 4); plugin conflicts reuse the
/// whole-plugin exclusion model when plugin rules become registrable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleConflict {
    DuplicateName { name: String },
    DuplicateIdentifier {
        id: DiagnosticId,
        first: String,
        second: String,
    },
    EmptyIdentifierList { name: String },
}

/// Checks the registry invariants: unique rule names, every identifier
/// claimed by exactly one rule, no rule with an empty claim list.
pub fn validate_rules(registrations: &[RuleRegistration]) -> Result<(), RuleConflict> {
    let mut names: BTreeSet<&str> = BTreeSet::new();
    let mut identifiers: BTreeMap<DiagnosticId, &str> = BTreeMap::new();
    for registration in registrations {
        let rule_name = registration.metadata.name.as_str();
        if !names.insert(rule_name) {
            return Err(RuleConflict::DuplicateName {
                name: rule_name.to_owned(),
            });
        }
        if registration.metadata.identifiers.is_empty() {
            return Err(RuleConflict::EmptyIdentifierList {
                name: rule_name.to_owned(),
            });
        }
        for identifier in &registration.metadata.identifiers {
            if let Some(first) = identifiers.insert(identifier.id, rule_name) {
                return Err(RuleConflict::DuplicateIdentifier {
                    id: identifier.id,
                    first: first.to_owned(),
                    second: rule_name.to_owned(),
                });
            }
        }
    }
    Ok(())
}
```

Extend `lib.rs`:

```rust
mod registry;

pub use registry::{
    CORE_IDENTITY_NAME, RuleConflict, RuleImplementation, RuleRegistration, RuleRegistry,
    validate_rules,
};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p celerrate_rules registry`
Expected: PASS.

- [ ] **Step 5: Run the local gates and commit**

```bash
git add crates/celerrate_rules
git commit -m "✨ feat(rules): add the rule registry, the fifth extension-point registry"
```

---

### Task 5: The syntax phase query and anchor resolution

**Files:**
- Create: `crates/celerrate_rules/src/phases.rs`
- Modify: `crates/celerrate_rules/src/lib.rs`

**Interfaces:**
- Consumes: `RuleRegistry::try_get` (Task 4), `SyntaxContext::new` (Task 3), `FindingSink` (Task 2), `celerrate_semantics::{ast_id_map, body_source_map, BodyQuery}`, `celerrate_db::parse`.
- Produces: `#[salsa::tracked(returns(ref))] pub fn syntax_phase_diagnostics(db, file: SourceFile, configuration: ProjectConfiguration) -> Vec<Diagnostic>`; crate-internal `resolved_diagnostic(db, file, file_id, finding) -> Option<Diagnostic>` handling all three anchors (shared by every phase query).

- [ ] **Step 1: Write the failing tests** (in `phases.rs` tests module)

Fixture helper for this module's tests:

```rust
fn test_setup(source: &str) -> (TestDatabase, SourceFile, ProjectConfiguration) {
    let db = TestDatabase::default();
    let file = SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    (db, file, configuration)
}

fn register(db: &TestDatabase, registrations: Vec<RuleRegistration>) {
    RuleRegistry::builder(registrations)
        .durability(salsa::Durability::HIGH)
        .new(db);
}
```

Fake rule and the tests:

```rust
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

#[test]
fn an_unset_registry_is_the_empty_path() {
    let (db, file, configuration) = test_setup("<?php echo 1;");
    assert!(syntax_phase_diagnostics(&db, file, configuration).is_empty());
}

#[test]
fn an_active_syntax_rule_reports_through_the_phase() {
    let (db, file, configuration) = test_setup("<?php echo 1;");
    register(&db, vec![fake_registration(true)]);
    let diagnostics = syntax_phase_diagnostics(&db, file, configuration);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].id, DiagnosticId::new("CEL9998"));
    assert_eq!(diagnostics[0].severity, Severity::Error);
}

#[test]
fn an_inactive_rule_is_skipped() {
    let (db, file, configuration) = test_setup("<?php echo 1;");
    register(&db, vec![fake_registration(false)]);
    assert!(syntax_phase_diagnostics(&db, file, configuration).is_empty());
}

#[test]
fn the_output_is_sorted_by_the_diagnostic_total_order() {
    // Two rules registered in reverse positional order; the phase
    // sorts, so the result is position-ordered regardless.
    ...
}
```

`fake_registration(active)` wraps `EmitAt(TextRange::new(TextSize::from(6), TextSize::from(10)))` with metadata declaring `CEL9998` at `Error`. Unit tests for `resolved_diagnostic` cover the other anchors directly (constructing `Finding` is crate-visible here):

```rust
#[test]
fn a_declaration_anchor_resolves_through_the_ast_id_map() {
    let (db, file, _configuration) = test_setup("<?php function demo() { echo 1; }");
    let map = celerrate_semantics::ast_id_map(&db, file);
    // Index 0 is the file's first declaration in tree order.
    let ast_id = AstId { file: FileId::new(0), index: 0 };
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
fn a_declaration_anchor_of_another_file_is_dropped() { /* file mismatch -> None */ }

#[test]
fn an_expression_anchor_resolves_through_the_body_source_map() {
    // A body with one expression; ExpressionId::from_index(0) resolves.
    ...
}

#[test]
fn a_dangling_anchor_is_dropped_never_a_panic() {
    // AstId { index: u32::MAX } on a real file -> None.
    ...
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p celerrate_rules phases`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
use celerrate_db::SourceFile;
use celerrate_diagnostics::Diagnostic;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::BodyQuery;
use celerrate_source::FileId;

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
```

Add `mod phases;` and `pub use phases::syntax_phase_diagnostics;` to `lib.rs`. (If `AstIdMap::pointer`'s accessor differs in name or the pointer exposes `text_range()` directly without `try_to_node`, follow the crate's actual API — `crates/celerrate_semantics/src/ast_id.rs` — keeping the contract: pointer gone means finding dropped.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p celerrate_rules`
Expected: PASS.

- [ ] **Step 5: Run the local gates and commit**

```bash
git add crates/celerrate_rules
git commit -m "✨ feat(rules): run syntax rules through a per-file phase query"
```

---

### Task 6: The `syntax-version-gating` rule, with a parity proof

**Files:**
- Create: `crates/celerrate_rules/src/rules/mod.rs`, `crates/celerrate_rules/src/rules/syntax_version_gating.rs`
- Modify: `crates/celerrate_rules/src/lib.rs`

**Interfaces:**
- Consumes: `SyntaxContext::{php_version_range, gated_syntax_uses}`, `celerrate_semantics::SYNTAX_NOT_AVAILABLE` (imported from below **until Task 8 moves ownership**), `FindingSink::report`.
- Produces: `pub struct SyntaxVersionGating;` implementing `SyntaxRule`; `pub fn metadata() -> RuleMetadata` (name `"syntax-version-gating"`, group `Correctness`, tier `Default`, identifiers `[CEL0024 @ Error]`); `pub fn core_rules() -> Vec<(RuleMetadata, RuleImplementation)>` — the list the composition root registers and tests iterate.

- [ ] **Step 1: Write the failing tests** (in `syntax_version_gating.rs` tests module)

The decisive test is **parity**: while both paths are alive, the phase query must reproduce the legacy query byte for byte.

```rust
fn registered_setup(source: &str, minimum: PhpVersion) -> (TestDatabase, SourceFile, ProjectConfiguration) {
    let db = TestDatabase::default();
    let file = SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        minimum,
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    let identity = PluginIdentity {
        name: crate::CORE_IDENTITY_NAME.to_owned(),
        version: "test".to_owned(),
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
    RuleRegistry::builder(registrations)
        .durability(salsa::Durability::HIGH)
        .new(&db);
    (db, file, configuration)
}

#[test]
fn the_rule_reproduces_the_legacy_query_byte_for_byte() {
    let source = "<?php\nreadonly class Point {}\nclass Box { public const int X = 1; }\n";
    let (db, file, configuration) = registered_setup(source, PhpVersion::new(8, 1));
    assert_eq!(
        syntax_phase_diagnostics(&db, file, configuration),
        celerrate_semantics::syntax_version_diagnostics(&db, file, configuration),
    );
    assert_eq!(syntax_phase_diagnostics(&db, file, configuration).len(), 2);
}

#[test]
fn a_construct_within_the_range_minimum_is_silent() {
    let (db, file, configuration) =
        registered_setup("<?php readonly class Point {}", PhpVersion::new(8, 2));
    assert!(syntax_phase_diagnostics(&db, file, configuration).is_empty());
}

#[test]
fn the_message_names_the_construct_and_both_versions() {
    let (db, file, configuration) =
        registered_setup("<?php readonly class Point {}", PhpVersion::new(8, 1));
    let diagnostics = syntax_phase_diagnostics(&db, file, configuration);
    assert_eq!(
        diagnostics[0].message,
        "`readonly class` requires PHP 8.2, but the project's minimum PHP version is 8.1",
    );
}
```

(Confirm the rendered version strings against the existing `syntax_gating.rs` unit tests' expectations — `PhpVersion`'s `Display` is the source of truth.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p celerrate_rules syntax_version_gating`
Expected: FAIL to compile — the rule module does not exist.

- [ ] **Step 3: Implement**

`src/rules/syntax_version_gating.rs`:

```rust
//! The first migrated family (design section 5): the walk lives below
//! as `celerrate_semantics::gated_syntax_uses`; this rule consumes the
//! outcomes and constructs the diagnostics.

use celerrate_semantics::SYNTAX_NOT_AVAILABLE;

use crate::context::SyntaxContext;
use crate::finding::{FindingAnchor, FindingSink};
use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
use crate::traits::SyntaxRule;

/// A syntax construct newer than the range minimum.
pub struct SyntaxVersionGating;

/// The family's declarative unit.
pub fn metadata() -> RuleMetadata {
    RuleMetadata {
        name: "syntax-version-gating".to_owned(),
        group: RuleGroup::Correctness,
        identifiers: vec![RuleIdentifier {
            id: SYNTAX_NOT_AVAILABLE,
            severity: celerrate_diagnostics::Severity::Error,
        }],
        tier: Tier::Default,
    }
}

impl SyntaxRule for SyntaxVersionGating {
    fn check(&self, context: &SyntaxContext<'_>, sink: &mut FindingSink<'_>) {
        let minimum = context.php_version_range().minimum;
        for gated in context.gated_syntax_uses() {
            if gated.required > minimum {
                sink.report(
                    SYNTAX_NOT_AVAILABLE,
                    FindingAnchor::Range(gated.range),
                    format!(
                        "`{}` requires PHP {}, but the project's minimum PHP version is {minimum}",
                        gated.label, gated.required,
                    ),
                );
            }
        }
    }
}
```

`src/rules/mod.rs`:

```rust
//! The core rules. Everything here is registered at the composition
//! root under the reserved core identity.

pub mod syntax_version_gating;

use std::sync::Arc;

use crate::metadata::RuleMetadata;
use crate::registry::RuleImplementation;

/// The core rule set, in registration order.
pub fn core_rules() -> Vec<(RuleMetadata, RuleImplementation)> {
    vec![(
        syntax_version_gating::metadata(),
        RuleImplementation::Syntax(Arc::new(syntax_version_gating::SyntaxVersionGating)),
    )]
}
```

Extend `lib.rs`: `pub mod rules;` and `pub use rules::core_rules;`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p celerrate_rules`
Expected: PASS, parity test included.

- [ ] **Step 5: Run the local gates and commit**

```bash
git add crates/celerrate_rules
git commit -m "✨ feat(rules): implement syntax-version-gating as the first framework rule"
```

---

### Task 7: The swap — the CLI serves CEL0024 through the framework

**Files:**
- Modify: `crates/celerrate_cli/Cargo.toml` (add `celerrate_rules = { path = "../celerrate_rules" }`)
- Modify: `crates/celerrate_cli/src/plugins.rs`, `crates/celerrate_cli/src/session.rs`, `crates/celerrate_cli/src/analysis.rs`
- Modify: `crates/celerrate_semantics/src/syntax_gating.rs`, `crates/celerrate_semantics/src/queries.rs`, `crates/celerrate_semantics/src/lib.rs`
- Modify: `crates/celerrate_semantics/tests/invalidation_scope.rs`
- Create: `crates/celerrate_rules/tests/invalidation_scope.rs`
- Test: `crates/celerrate_cli/tests/seeded_defects.rs`

**Interfaces:**
- Consumes: `celerrate_rules::{core_rules, CORE_IDENTITY_NAME, RuleRegistration, RuleRegistry, Tier, validate_rules, syntax_phase_diagnostics}`.
- Produces: `pub fn register_core_rules(database: &AnalysisDatabase)` in `plugins.rs`; `persistable_diagnostics` extended with the syntax phase; `celerrate_semantics::syntax_version_diagnostics` **deleted** (the walk query stays); `semantic_diagnostics` reduced to the reference family.

- [ ] **Step 1: Seed the recall guard first (green on the old path)**

Add to `crates/celerrate_cli/tests/seeded_defects.rs`, following the existing `seeded` helper:

```rust
#[test]
fn cel0024_a_gated_construct_below_the_minimum_is_reported() {
    seeded(
        "CEL0024",
        r#"<?php
namespace App;
readonly class Point {}
"#,
    );
}
```

Run: `cargo test -p celerrate_cli --test seeded_defects cel0024`
Expected: PASS already (the legacy path serves it). This fixture is the family's recall gate across the swap: the corpus is clean, so only this test would catch the family silently going dark (design section 1).

Commit the fixture on its own:

```bash
git add crates/celerrate_cli/tests/seeded_defects.rs
git commit -m "✅ test(cli): seed a CEL0024 defect fixture ahead of the framework swap"
```

- [ ] **Step 2: Write the failing composition-root tests** (in `plugins.rs` tests)

```rust
#[test]
fn core_rules_register_under_the_reserved_identity_and_validate() {
    let db = AnalysisDatabase::default();
    register_core_rules(&db);
    let registry = celerrate_rules::RuleRegistry::try_get(&db)
        .expect("core rules are always registered");
    let registrations = registry.registrations(&db);
    assert!(!registrations.is_empty());
    assert!(registrations.iter().all(|registration| {
        registration.identity.name == celerrate_rules::CORE_IDENTITY_NAME
    }));
    assert_eq!(celerrate_rules::validate_rules(registrations), Ok(()));
}

#[test]
fn core_rules_never_enter_the_admitted_plugin_set() {
    let db = AnalysisDatabase::default();
    let registered = register_plugins(&db);
    register_core_rules(&db);
    assert!(registered.admitted.iter().all(|identity| {
        identity.name != celerrate_rules::CORE_IDENTITY_NAME
    }));
}

#[test]
fn a_plugin_claiming_the_reserved_core_name_is_excluded() {
    // Build a descriptor named CORE_IDENTITY_NAME through the same
    // fake-plugin path as `an_api_version_mismatch_excludes_and_reports`
    // and assert it lands in `excluded` with a reason naming the
    // reservation, not in any registry.
    ...
}
```

Run: `cargo test -p celerrate_cli plugins` — Expected: FAIL to compile (`register_core_rules` missing).

- [ ] **Step 3: Implement the registration side**

In `crates/celerrate_cli/src/plugins.rs`:

```rust
/// Registers the core rules under the reserved core identity, outside
/// the admitted plugin set: core behavior is keyed by binary identity,
/// never by the plugin-set digest (design section 2). Order here is
/// the deterministic dispatch order, like the other four registries.
pub fn register_core_rules(database: &AnalysisDatabase) {
    let identity = core_identity();
    let registrations: Vec<celerrate_rules::RuleRegistration> = celerrate_rules::core_rules()
        .into_iter()
        .map(|(metadata, implementation)| celerrate_rules::RuleRegistration {
            identity: identity.clone(),
            active: metadata.tier == celerrate_rules::Tier::Default,
            metadata,
            implementation,
        })
        .collect();
    celerrate_rules::RuleRegistry::builder(registrations)
        .durability(salsa::Durability::HIGH)
        .new(database);
}

fn core_identity() -> PluginIdentity {
    PluginIdentity {
        name: celerrate_rules::CORE_IDENTITY_NAME.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        configuration: String::new(),
    }
}
```

Extend `admission` with the reservation guard, mirroring its API-version arm's exclusion shape: a descriptor whose identity name equals `CORE_IDENTITY_NAME` is excluded with reason `"the name celerrate-core is reserved for core registrations"`.

In `Session::start` (`session.rs`), immediately after the `register_plugins` call:

```rust
register_core_rules(&database);
```

Run the Step 2 tests: PASS.

- [ ] **Step 4: Swap the pipeline**

In `crates/celerrate_cli/src/analysis.rs`, `persistable_diagnostics`, after the `semantic_diagnostics` extend and before the suppression filter:

```rust
    diagnostics.extend(
        celerrate_rules::syntax_phase_diagnostics(database, file, inputs.configuration)
            .iter()
            .cloned(),
    );
```

In `crates/celerrate_semantics/src/queries.rs`, `semantic_diagnostics` drops the gating extend (the reference family remains; keep the final `sort`), and its rustdoc says the syntax phase now owns gating. Rename and rewrite the merge test:

```rust
#[test]
fn semantic_diagnostics_carry_the_reference_family_only() {
    // Same fixture as before; now expects only CEL0018 — CEL0024 is
    // served by the rule framework's syntax phase.
    ...
    assert_eq!(identifiers, vec!["CEL0018"]);
}
```

In `crates/celerrate_semantics/src/syntax_gating.rs`: delete `syntax_version_diagnostics` and the unit tests that exercised filtering and messages (they were re-homed as rule tests in Task 6 — verify each behavior has a counterpart there before deleting: the seven-construct table test, the within-range silence, the readonly-property negative, the promoted-property positive, the non-gated clone/constant negatives; move any missing one into `celerrate_rules`' rule tests rewritten against `syntax_phase_diagnostics`). The walk tests from Task 1 stay. Update `lib.rs`:

```rust
pub use syntax_gating::{GatedSyntaxUse, SYNTAX_NOT_AVAILABLE, gated_syntax_uses};
```

Delete the parity test from Task 6 (its reference — the legacy query — is gone; the behavior tests it guarded remain).

- [ ] **Step 5: Move the invalidation pin**

Delete `a_version_range_change_re_runs_the_gating_queries` from `crates/celerrate_semantics/tests/invalidation_scope.rs` (keep any walk-only assertions there if they still compile; the gating query is gone). Create `crates/celerrate_rules/tests/invalidation_scope.rs` with the per-crate preamble (`#![allow(clippy::unwrap_used)]` etc. with the standard justification comment), a copied `executions_of`, a small fixture helper (database + one or two files + configuration + `RuleRegistry` populated from `core_rules()` exactly as `registered_setup` in Task 6), and:

```rust
#[test]
fn a_version_range_change_reruns_the_phase_but_never_the_walk() {
    // Prime with a gated construct under minimum 8.1, clear the log,
    // raise the minimum to 8.2, re-query.
    ...
    assert_eq!(diagnostics, &vec![]);
    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "syntax_phase_diagnostics"),
        1,
        "the configuration is an input of the phase query: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "gated_syntax_uses"),
        0,
        "a version change re-filters without re-walking: {log:?}",
    );
}

#[test]
fn an_edit_to_one_file_reruns_only_its_own_syntax_phase() {
    // Two files, both primed through the phase query; edit file A's
    // bytes (salsa::Setter), re-query both.
    ...
    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "syntax_phase_diagnostics"),
        1,
        "file B's phase is untouched by file A's edit: {log:?}",
    );
}
```

- [ ] **Step 6: Run the full verification for the swap**

Run, in order:
- `cargo test --workspace` — all suites, including `cache_equivalence` (warm equals cold is untouched: CEL0024 rides `StoredVerdict.diagnostics` with identical values) and `seeded_defects` (the Step 1 fixture is the recall proof that the swap did not go dark).
- `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
- `cargo xtask fetch-corpus && cargo xtask corpus` — Expected: byte-identical snapshot (`0 notices, 0 diagnostics`).
- `cargo xtask mixed-rate` — Expected: baseline unchanged (verified feasible by construction; the harness calls inference directly).

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_cli crates/celerrate_semantics crates/celerrate_rules Cargo.lock
git commit -m "♻️ refactor(cli): serve CEL0024 through the rule framework's syntax phase"
```

---

### Task 8: Move CEL0024's ownership to `celerrate_rules`

**Files:**
- Modify: `crates/celerrate_rules/src/rules/syntax_version_gating.rs`, `crates/celerrate_rules/src/lib.rs`
- Modify: `crates/celerrate_semantics/src/syntax_gating.rs`, `crates/celerrate_semantics/src/reference_checks.rs:52-60`, `crates/celerrate_semantics/src/lib.rs`
- Modify: `crates/celerrate_diagnostics/src/registry.rs:68-72`
- Modify: `crates/celerrate_cli/tests/registry.rs` (`producers()`)

**Interfaces:**
- Produces: `celerrate_rules::SYNTAX_NOT_AVAILABLE` and `celerrate_rules::ALLOCATED_IDENTIFIERS: &[DiagnosticId]`; `celerrate_semantics::ALLOCATED_IDENTIFIERS` shrinks to six entries; `REGISTRY`'s CEL0024 entry names `"celerrate_rules"`.

- [ ] **Step 1: Make the ledger red first**

In `crates/celerrate_diagnostics/src/registry.rs`, change CEL0024's owner:

```rust
    registered(
        "CEL0024",
        "syntax construct not available",
        "celerrate_rules",
    ),
```

Run: `cargo test -p celerrate_cli --test registry`
Expected: FAIL — `the_registry_names_the_producer_that_actually_allocates` (the allocator is still `celerrate_semantics`). This is the ledger doing its job.

- [ ] **Step 2: Move the allocation**

In `crates/celerrate_rules/src/rules/syntax_version_gating.rs`, replace the import with the definition:

```rust
/// A syntax construct newer than the range minimum.
pub const SYNTAX_NOT_AVAILABLE: DiagnosticId = DiagnosticId::new("CEL0024");

/// Every identifier this crate allocates, for the registry check at
/// the composition root.
pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId] = &[SYNTAX_NOT_AVAILABLE];
```

Re-export both from `celerrate_rules/src/lib.rs`. In `celerrate_semantics`: delete the constant from `syntax_gating.rs`, remove `crate::syntax_gating::SYNTAX_NOT_AVAILABLE` from `reference_checks.rs`'s `ALLOCATED_IDENTIFIERS` (and its rustdoc sentence about it), and drop it from the `lib.rs` re-export. In `crates/celerrate_cli/tests/registry.rs`, add to `producers()`:

```rust
        ("celerrate_rules", celerrate_rules::ALLOCATED_IDENTIFIERS),
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p celerrate_cli --test registry && cargo test --workspace`
Expected: PASS — producers derivation, uniqueness, registry equality, owner naming, and `every_registered_identifier_is_documented` (the `docs/diagnostics.md` entry is identifier-keyed and does not change).

- [ ] **Step 4: Run the local gates and commit**

```bash
git add crates/celerrate_rules crates/celerrate_semantics crates/celerrate_diagnostics crates/celerrate_cli
git commit -m "♻️ refactor(rules): move CEL0024 ownership to the rule crate"
```

---

### Task 9: The semantic and typed phase queries, with the per-body tier pinned

**Files:**
- Modify: `crates/celerrate_rules/src/phases.rs`, `crates/celerrate_rules/src/lib.rs`
- Modify: `crates/celerrate_cli/src/analysis.rs` (wire the semantic phase)
- Test: `crates/celerrate_rules/tests/invalidation_scope.rs`

**Interfaces:**
- Consumes: `celerrate_semantics::{semantic_context, member_tree, DeclarationKind, MemberKind, BodyQuery, body_ir}`, `celerrate_types::typed_body_context`, `resolved_diagnostic` (Task 5).
- Produces: `#[salsa::tracked(returns(ref))] pub fn semantic_phase_diagnostics(db, file: SourceFile) -> Vec<Diagnostic>`; `#[salsa::tracked(returns(ref))] pub fn typed_body_phase_diagnostics(db, file: SourceFile) -> Vec<Diagnostic>`; crate-internal per-body `body_phase_findings(db, file, body: BodyQuery) -> Vec<Finding>`. Part 4 extends these signatures with the inputs its facade methods read; it does not reshape the tiering.
- **Deliberately not CLI-wired: the typed phase.** Its CLI wiring belongs to part 4's typed migration, where the stored-verdict co-production (the `StoredTypedVerdict` half) is handled; wiring an empty query now would buy nothing and touch the warm-path serving for nothing. The semantic phase **is** wired (it contributes nothing until part 4, and part 4 then migrates content into an already-plumbed pipeline).

- [ ] **Step 1: Write the failing tests**

In `phases.rs` tests — the semantic skeleton:

```rust
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

#[test]
fn a_semantic_rule_reports_once_per_file() {
    let (db, file, _configuration) = test_setup("<?php echo 1;");
    register(&db, vec![semantic_registration()]);
    assert_eq!(semantic_phase_diagnostics(&db, file).len(), 1);
}
```

And the typed skeleton — a rule that marks every body:

```rust
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

#[test]
fn a_typed_body_rule_runs_once_per_function_and_method_body() {
    let source = "<?php\nfunction first() { echo 1; }\nclass Demo { public function second(): void { echo 2; } }\n";
    let (db, file, _configuration) = test_setup(source);
    register(&db, vec![typed_registration()]);
    let diagnostics = typed_body_phase_diagnostics(&db, file);
    assert_eq!(diagnostics.len(), 2, "one finding per body, both reconciled");
}

#[test]
fn a_trait_method_body_is_not_enumerated() {
    // Mirrors typed_file_verdicts' trait filter.
    let source = "<?php\ntrait Helper { public function inside(): void { echo 1; } }\n";
    let (db, file, _configuration) = test_setup(source);
    register(&db, vec![typed_registration()]);
    assert!(typed_body_phase_diagnostics(&db, file).is_empty());
}
```

In `tests/invalidation_scope.rs`, the two tier pins, mirroring `crates/celerrate_types/tests/invalidation_scope.rs:1888-1948` with the fake typed rule registered:

```rust
#[test]
fn a_body_edit_reruns_only_the_editing_bodys_phase() {
    // Two bodies primed through typed_body_phase_diagnostics; append
    // a statement inside the second body; re-query.
    ...
    assert_eq!(
        executions_of(&log, "body_phase_findings"),
        1,
        "editing one body never re-checks its siblings: {log:?}",
    );
}

#[test]
fn an_edit_above_a_body_reruns_no_body_phase() {
    // Prepend a comment line above every body (the offset-only edit
    // class); the range-free inner tier backdates, the reconciliation
    // moves the diagnostic.
    ...
    assert_eq!(
        executions_of(&log, "body_phase_findings"),
        0,
        "range-free findings backdate under an offset shift: {log:?}",
    );
    assert_eq!(second.len(), 2, "the diagnostics moved with their ranges");
}
```

(No semantic cross-file pin here: the skeleton's fake semantic rule reads no file content, so such a pin would measure the fake, not the framework. The semantic phase's invalidation pins arrive in part 4 with its real families — record this in the test file's module comment.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p celerrate_rules`
Expected: FAIL to compile — the two queries do not exist.

- [ ] **Step 3: Implement** (in `phases.rs`)

```rust
/// The semantic phase: one query per file. Empty until part 4
/// migrates the semantic families onto it; wired into the CLI now so
/// the migration lands in existing plumbing.
#[salsa::tracked(returns(ref))]
pub fn semantic_phase_diagnostics(db: &dyn salsa::Database, file: SourceFile) -> Vec<Diagnostic> {
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
        let context = celerrate_semantics::semantic_context(db, file);
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
/// framework preserves `body_typed_verdicts`' proven tier — editing
/// one body never re-checks its siblings. The `body_ir` guard is the
/// tier's honest content dependency: a body that does not lower has
/// nothing to check, and a body edit invalidates exactly this body's
/// query while an offset-only edit backdates it.
#[salsa::tracked(returns(ref))]
pub(crate) fn body_phase_findings<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
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
        let context = celerrate_types::typed_body_context(db, body.ast_id(db));
        let mut sink = FindingSink::new(&registration.metadata);
        rule.check(&context, &mut sink);
        findings.extend(sink.into_findings());
    }
    findings
}

/// The typed phase: aggregates the per-body tier over the file's
/// function and method bodies (the `typed_file_verdicts` enumeration,
/// traits excluded) and reconciles anchors at the tail. Not yet wired
/// into the CLI composition: part 4's typed migration wires it
/// together with the stored-verdict co-production.
#[salsa::tracked(returns(ref))]
pub fn typed_body_phase_diagnostics(db: &dyn salsa::Database, file: SourceFile) -> Vec<Diagnostic> {
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
        for finding in body_phase_findings(db, file, body) {
            if let Some(diagnostic) = resolved_diagnostic(db, file, file_id, finding.clone()) {
                diagnostics.push(diagnostic);
            }
        }
    }
    diagnostics.sort();
    diagnostics
}
```

Re-export both public queries from `lib.rs`. In `crates/celerrate_cli/src/analysis.rs`, `persistable_diagnostics`, after the syntax phase extend:

```rust
    diagnostics.extend(
        celerrate_rules::semantic_phase_diagnostics(database, file)
            .iter()
            .cloned(),
    );
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p celerrate_rules && cargo test --workspace`
Expected: PASS — including `cache_equivalence` and `seeded_defects` (the semantic phase contributes nothing yet, so output is unchanged everywhere).

- [ ] **Step 5: Run the local gates and commit**

```bash
git add crates/celerrate_rules crates/celerrate_cli
git commit -m "✨ feat(rules): add the semantic and typed phase queries with the per-body tier"
```

---

### Task 10: The facade delta and the seal proofs

**Files:**
- Modify: `crates/celerrate_plugin/Cargo.toml` (add `celerrate_rules = { path = "../celerrate_rules" }`)
- Modify: `crates/celerrate_plugin/src/lib.rs`
- Create: `crates/celerrate_plugin/tests/seal/reporting_rule_reexport.rs` (+ `.stderr`), `crates/celerrate_plugin/tests/seal/syntax_context_new.rs` (+ `.stderr`), `crates/celerrate_plugin/tests/seal/rule_registry_reexport.rs` (+ `.stderr`)

**Interfaces:**
- Produces: the facade's rule-authoring vocabulary (design section 2's enumerable delta): `SyntaxRule`, `SemanticRule`, `TypedBodyRule`, `SyntaxContext`, `SemanticContext`, `TypedBodyContext`, `FindingAnchor`, `FindingSink`, `RuleMetadata`, `RuleIdentifier`, `RuleGroup`, `Tier`, `DiagnosticId`, `Severity`, `ExplainPage`. **Not** re-exported: `ReportingRule`/`ReportingContext` (core-only this sub-project), `RuleRegistry`/`RuleRegistration`/`RuleImplementation`/`core_rules`/`CORE_IDENTITY_NAME` (composition-root vocabulary), the construction seams. Plugin rule **registration** stays unshippable by design (section 8): the traits are public API, no registration path exists for them.

- [ ] **Step 1: Add the re-exports**

`celerrate_rules/src/lib.rs` first re-exports the diagnostics vocabulary its API uses (nominal, so the facade can take everything from one crate):

```rust
pub use celerrate_diagnostics::{DiagnosticId, ExplainPage, Severity};
```

Then in `crates/celerrate_plugin/src/lib.rs`, alongside the existing blocks and their "nominal re-exports only" comment:

```rust
pub use celerrate_rules::{
    DiagnosticId, ExplainPage, FindingAnchor, FindingSink, RuleGroup, RuleIdentifier,
    RuleMetadata, SemanticRule, Severity, SyntaxContext, SyntaxRule, Tier, TypedBodyRule,
};
pub use celerrate_semantics::SemanticContext;
pub use celerrate_types::TypedBodyContext;
```

- [ ] **Step 2: Write the compile-fail cases**

`tests/seal/reporting_rule_reexport.rs`:

```rust
//! The `Reporting` phase is core-only in this sub-project: the facade
//! does not re-export it (design section 4).
use celerrate_plugin::ReportingRule;

fn main() {}
```

`tests/seal/rule_registry_reexport.rs`:

```rust
//! Rule registration is composition-root vocabulary: no plugin can
//! reach the registry through the facade (design section 8 defers
//! plugin-registered rules).
use celerrate_plugin::RuleRegistry;

fn main() {}
```

`tests/seal/syntax_context_new.rs`:

```rust
//! The syntax context is sealed: `new` is crate-private in
//! `celerrate_rules`, so the facade side cannot construct one.
fn main() {
    let _ = celerrate_plugin::SyntaxContext::new;
}
```

- [ ] **Step 3: Generate and pin the stderr files**

Run: `TRYBUILD=overwrite cargo test -p celerrate_plugin --test seal`
Then inspect each generated `.stderr`: the two re-export cases must fail with `error[E0432]: unresolved import`, the constructor case with `error[E0624]: associated function `new` is private`. Commit the `.stderr` files with the cases.

- [ ] **Step 4: Verify the dependency shape and the whole workspace**

Run: `cargo xtask dependency-shape` — Expected: PASS unchanged (`celerrate_rules` is an engine crate below the facade, not a governed plugin crate).
Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_plugin crates/celerrate_rules Cargo.lock
git commit -m "✨ feat(plugin): re-export the rule-authoring surface and pin the seal"
```

---

### Task 11: The WASM projection sketch, the CHANGELOG, and closure of this part

**Files:**
- Modify: `.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Extend the projection sketch**

Append a new section after the 2026-07-19 amendment (the design names this an acceptance artifact of sub-project 4, "extending the existing sketch"):

```markdown
## Amendment 2026-07-21: the rule-phase projection

The rule framework's plugin-facing traits project onto the same model:
plain data or opaque handles cross the boundary, contexts become host
interrogation calls, and the finding sink becomes a plain-data return
value (a list of findings — identifier string, anchor, message — needs
no handle table at all). The `Reporting` phase is core-only and does
not project in this sub-project.

| Native trait (owner) | Guest exports | Host families needed |
|---|---|---|
| `SyntaxRule` (`celerrate_rules`) | `metadata() -> rule metadata`, `check(file) -> list<finding>` | syntax outcomes: `gated_syntax_uses() -> list<(label, required_version, range)>`, `php_version_range() -> ((major, minor), (major, minor))` — plain data out |
| `SemanticRule` (`celerrate_semantics` context) | `metadata() -> rule metadata`, `check(file) -> list<finding>` | symbol lookup (family 4), resolution outcomes when part 4 fixes the context surface |
| `TypedBodyRule` (`celerrate_types` context) | `metadata() -> rule metadata`, `check(body) -> list<finding>` | type interrogation (family 2) plus body-handle interrogation when part 4 fixes the context surface |

Anchors are plain data (`range`, `declaration id`, `(body id,
expression id)`), so findings survive the boundary without host
resolution; the host reconciles them at the phase query's tail exactly
as it does for native rules. The context methods added by part 4 must
land in this table when they land in the traits.
```

- [ ] **Step 2: Record the change in the CHANGELOG**

Under `## [Unreleased]` / `### Changed`, in the file's prose style:

```markdown
- The rule framework skeleton: rules are declarative families with a
  name, a group, a closed identifier list carrying per-identifier
  default severities, and a `Default`/`Nursery` tier. Four phase
  traits (syntax, semantic, typed-body, and the core-only reporting
  phase) check through sealed contexts and report into a
  metadata-severitied finding sink; a fifth extension-point registry
  holds registrations, with core rules registered under a reserved
  core identity that never keys the plugin-set digest. The
  syntax-version-gating family (`CEL0024`) is the first migrated
  family: the gated-construct walk stays in `celerrate_semantics` as
  an outcome query, the rule constructs the diagnostics, and the
  identifier's ownership moves to `celerrate_rules`. Internal
  machinery only: reported diagnostics, exit codes, the corpus
  snapshot, and the cache format are all byte-identical.
```

- [ ] **Step 3: Final verification of every closure gate this part owns**

Run, and record the results in the task output:
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all`
- `cargo deny check`
- `cargo xtask dependency-shape`
- `cargo xtask fetch-corpus && cargo xtask corpus` — byte-identical
- `cargo xtask mixed-rate` — baseline unchanged

- [ ] **Step 4: Commit**

```bash
git add .claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md CHANGELOG.md
git commit -m "📝 docs: project the rule phases onto the WASM sketch and record the skeleton"
```

---

## Self-review against the design (performed while writing)

- **Design part-3 scope** (section 12, item 3): four phase traits ✓ (Task 3, `Reporting` included), registry ✓ (Task 4, on the four-registry template with the reserved core identity of section 2), sealed contexts ✓ (Task 3, ownership per section 4: `Semantic` in `celerrate_semantics`, `TypedBody` in `celerrate_types`, `Syntax` in `celerrate_rules` as the stated exception), phase queries with the per-body typed tier ✓ (Tasks 5 and 9, tier pinned by the two mirrored invalidation tests), one migrated family as proof ✓ (Tasks 6-8, `syntax-version-gating`).
- **Migration gates per family** (section 5): corpus byte-identical ✓ (Task 7 step 6), seeded-defect fixture for the migrated identifier ✓ (Task 7 step 1; CEL0018-0023's fixtures are part 4's semantic migration, per section 5), invalidation shape preserved ✓ (Task 7 step 5 — narrowed for the walk, never broadened), warm/cold equivalence untouched ✓, mixed-rate untouched ✓.
- **Known intentional deviations, all argued in "Design decisions"**: minimal context surfaces (decision 1), reporting execution deferred to part 5 (decision 3), typed phase not CLI-wired (decision 4), sink without symbolic suggestions (decision 5), emission-side scan deferred to closure (decision 7).
- **Type consistency spot-check**: `RuleMetadata`/`RuleIdentifier`/`Tier` names match across Tasks 2, 4, 6, 7, 10; `FindingAnchor::Declaration(AstId)` matches `TypedBodyContext::body() -> AstId`; `syntax_phase_diagnostics(db, file, configuration)` is called with that arity in Tasks 5, 6, 7; `semantic_phase_diagnostics(db, file)` and `typed_body_phase_diagnostics(db, file)` match between Task 9's definitions, tests, and CLI wiring.
- **Two named API-drift guards** (not placeholders — the contract is fully specified, only an accessor name may differ): `AstIdMap::pointer`/`try_to_node` in Task 5, and the fake-plugin construction path mirrored in Task 7's reservation-guard test; both point at the exact file to consult.
