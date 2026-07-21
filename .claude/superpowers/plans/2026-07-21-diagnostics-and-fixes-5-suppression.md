# Diagnostics and Fixes 5: Suppression (#58) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Identifier-level suppression end to end: the filter-carrying directive resolution, the foreign-to-CEL correspondence table with its triage gate, the native `@celerrate-ignore` directive with a placement-resolved scope, per-directive match records in the stored verdict (schema 7), and the two directive rules (CEL0041 unknown suppression identifier, CEL0042 unused suppression) riding the `Reporting` phase warm and cold.

**Architecture:** The directive vocabulary (`celerrate_semantics::comment_directives`) grows identifier mappings and a placement-resolved scope; `suppressed_ranges` becomes `suppression_directives`, a per-directive resolution query carrying a filter per range (`All` or `Only(sorted codes)`); the CLI's `retain_unsuppressed` matches the diagnostic identifier against the filters and records which directives admitted anything; those match records join `StoredVerdict` (per half, so a recomputed typed half never serves a stale union); the `Reporting` phase is a plain function over the records - never a salsa query, never a parse - so warm and cold runs report the same directive diagnostics byte for byte. The correspondence table lives in `celerrate_phpdoc_bridge` as plain code strings; the matcher downstream interns them through `celerrate_diagnostics::find_identifier`.

**Tech Stack:** Rust, salsa (tracked queries, singleton inputs), the part-3/4 rule framework (`RuleRegistry`, `FindingSink`, `ReportingRule`), serde + the versioned cache pack (schema bump 6 to 7).

**Reference documents:**

- Design: `.claude/superpowers/specs/2026-07-20-diagnostics-and-fixes-design.md` (sections 3, 4, 8, 11; this plan is step 5 of section 12)
- Issue #58 and its triage (the correspondence policy the design restates as normative in section 8)
- Part 3 (framework): `.claude/superpowers/plans/2026-07-21-diagnostics-and-fixes-3-framework-skeleton.md`
- Part 4 (migration): `.claude/superpowers/plans/2026-07-21-diagnostics-and-fixes-4-remaining-migration.md`

## Global Constraints

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Test modules may locally `#[allow]` these lints.
- TDD: failing test, minimal implementation, refactor. Behavior-preserving refactors (tasks 1, 2, 6) keep the existing suites green through the move; behavior changes (tasks 4, 5, 8, 9) get red tests first.
- Strict layering: `celerrate_semantics` may use `celerrate_diagnostics` (it already depends on it); the bridge depends on `celerrate_plugin` alone (the `dependency_shape` xtask gate); `celerrate_rules` sits above `celerrate_semantics`.
- Determinism: no wall-clock time, randomness, or environment reads inside queries. Providers stay deterministic pure functions of `(CommentKind, &str)`.
- Error resilience: malformed directive content yields fewer identifiers or no directive, never an error and never a panic.
- The exit-code contract does not move: 0 clean, 1 any span-anchored diagnostic (severity does not matter - CEL0041/CEL0042 warnings flip exit 1 exactly as CEL0023 does), 2 internal error. Project-anchored findings stay exit-neutral.
- Suppression direction: over-suppression is the accepted failure direction, under-suppression is the bug (parent design section 4; every fallback in this plan widens, never narrows).
- The emission-side scan governs `celerrate_semantics` and `celerrate_types`: nothing in this plan may construct a `Diagnostic` there. CEL0041/CEL0042 are constructed in `celerrate_rules` only.
- Everything in English, full words, no abbreviated names (standard acronyms fine). No em-dashes in prose or comments.
- Commits: gitmoji + Conventional Commits, authored with the repository-configured identity. Never any Claude attribution.
- Local commands: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`, `cargo deny check`. Closure additionally runs `cargo xtask fetch-corpus` then `cargo xtask corpus` and `cargo xtask mixed-rate`.

## Context: where parts 1 to 4 left the code

Read these before starting; every task below assumes them.

- `crates/celerrate_semantics/src/comment_directives.rs`: the extension point. `CommentDirective::Suppress { scope, identifiers: Vec<String> }` carries foreign identifiers "for the rule framework's identifier-level correspondence, never matched here" (the reserved comment this plan resolves). `suppressed_ranges(db, file) -> Vec<TextRange>` resolves symbolic scopes against the line index (sorted, deduplicated); `is_suppressed(suppressed, offset, text_end)` is the shared position matcher with the end-of-file exception. `resolve_scope` is the placement-aware resolution seam (it sees the token and the line index); `whole_lines` and `annotated_node_range` are its helpers.
- `crates/celerrate_phpdoc_bridge/src/directives.rs`: the foreign recognizer. `comment_directives(kind, text)` finds `@phpstan-ignore[-line|-next-line]` and `@psalm-suppress`; `identifiers_of` splits the rest of the tag's line on commas, dropping a parenthesized trailer and `*/`. Its module doc carries the second reserved comment ("Identifiers are carried, never matched").
- `crates/celerrate_cli/src/analysis.rs`: `retain_unsuppressed(database, file, suppressed, diagnostics)` filters by position only, shared by `persistable_diagnostics` (untyped half) and `typed_portion` (typed half); `composed_diagnostics` is the single recompute composition point; `served_typed_diagnostics` is the typed fork on a hit; `analyze_one` orchestrates hit, partial hit, and miss.
- `crates/celerrate_cli/src/cache/`: `pack.rs` has `CACHE_SCHEMA_VERSION: u32 = 6`. `stored.rs` has `StoredVerdict { diagnostics, records, typed: Option<StoredTypedVerdict> }`, both halves stored post-suppression; `StoredDiagnostic::to_diagnostic(file, content_length)` bounds-checks every range and re-interns the identifier through `find_identifier`, `None` discards. `verdict.rs`'s `lookup_verdict` layers untyped validation then `TypedOutcome`. `mod.rs`'s `collect_entries` reuses a fully validated entry verbatim (`stored.clone()`) only when every stored diagnostic converts, else `composed_verdict`; `composed_verdict_with_lever` composes from `persistable_diagnostics`, `resolution_records`, and `composed_typed_verdict`.
- `crates/celerrate_rules/`: `traits.rs` declares `ReportingRule` (a stub: "its execution point and context surface arrive in part 5"); `context.rs` has the placeholder `ReportingContext<'db> { _database }`; `registry.rs` has `RuleImplementation::Reporting`, `CORE_IDENTITY_NAME`, `validate_rules`; `finding.rs` has `Finding` (pub(crate)) and `FindingSink` whose severities come from metadata; `rules/mod.rs` has `ALLOCATED_IDENTIFIERS` (16 entries) and `core_rules()` (6 rules). The three existing phase queries live in `phases.rs`; `resolved_diagnostic` is the shared reconciliation tail.
- `crates/celerrate_cli/src/plugins.rs`: `register_plugins` builds the four registries (the bridge is the only comment-directive provider today; the composition-root test asserts `len() == 1`); `register_core_rules` registers `core_rules()` under `core_identity()`; `plugin_set_digest` hashes the post-admission set - core identities never enter `admitted`.
- `crates/celerrate_diagnostics/src/registry.rs`: `REGISTRY` ends at CEL0040; the gapless test asserts `previous == 40`. `find_identifier` re-interns; it is the single lookup seam (design section 8 pins the two-tier resolution shape on it: static registry now, a dynamic namespaced tier later - nothing in this plan may assume the static tier is total, so every intern goes through this function).
- Tests and gates: `crates/celerrate_cli/tests/suppressions.rs` (the product matrix for the four foreign forms), `cache_suppression.rs` (post-filter verdict stored and served equal), `cache_equivalence.rs` (`served_equals_recomputed`, the warm/cold net this plan extends to the `Reporting` phase), `registry.rs` (the allocation ledger), `documentation.rs` (every registered identifier must appear in `docs/diagnostics.md`; every suppression form in `docs/phpdoc-bridge.md`), `seeded_defects.rs`; `xtask/src/emission_scan.rs` (governed crates must not construct diagnostics); `xtask/src/dependency_shape.rs` (the bridge's dependency allowlist).
- `crates/celerrate_syntax`: `SyntaxToken` is the rowan token (`prev_token()` is available - `celerrate_edit/src/builder.rs:138` already uses it); comment trivia kinds are `LineComment`, `BlockComment`, `DocComment`.
- `PLUGIN_API_VERSION` is 0 (`crates/celerrate_plugin/src/lib.rs:18`), with a test pinning 0.

## Decisions this plan fixes (the spec leaves them to the plan)

1. **The vocabulary distinguishes four identifier fates, and the directive carries its origin.** `SuppressionIdentifier` is `Mapped { written, codes }`, `ScopeWide { written }`, `Unmapped { written }`, or `Native { written }`; `CommentDirective::Suppress` gains `origin: DirectiveOrigin` (`Foreign` or `Native`). The origin field exists because the empty-identifier case diverges: a bare foreign directive suppresses the whole scope (the #58 policy), while a bare `@celerrate-ignore` suppresses nothing (identifiers are mandatory by design) and is what CEL0042 reports. Codes travel as plain strings (`"CEL0030"`) and are interned downstream through `find_identifier` - the facade grows no identifier vocabulary (design section 8's amendment).
2. **`suppressed_ranges` becomes `suppression_directives`, one query, per directive.** It returns `Vec<ResolvedDirective>` - anchor (the carrying comment token's range), resolved scope, `SuppressionFilter` (`All` or `Only(sorted DiagnosticIds)`), the written identifiers, the origin. "Co-located filters merge by union" is satisfied semantically: a diagnostic is suppressed if and only if any directive admits it, and every admitting directive is attributed (any-match used-ness), so no merged intermediate structure exists to drift. `is_suppressed` is replaced by `ResolvedDirective::admits`, which keeps the end-of-file exception verbatim.
3. **Fallback policy, stated once and implemented once (in `filter_of`).** Foreign: empty identifier list, any `ScopeWide`, any `Unmapped`, or any code string that fails interning → `All` (widen, never under-suppress; the correspondence gate makes the failed-intern arm unreachable, it exists as the honest fallback). Native: the union of the identifiers that intern; unknown ones are silently excluded from the filter (they suppress nothing - that is CEL0041's reason to exist) and never widen.
4. **The correspondence table is data plus evidence.** The bridge holds one sorted const table per dialect (`ForeignMapping::Codes | ScopeWide | Unmapped`), exact-case binary search, and exposes `correspondence_entries(dialect)` for inspection. The published catalogues are vendored verbatim as text files next to the table (source URL, upstream version, retrieval date in a header). The triage gate lives at the composition root (`celerrate_cli/tests/suppression_correspondence.rs`, the ledger precedent): table keys equal catalogue lines exactly, both directions, per dialect; every `Codes` entry is non-empty and every code re-interns. The bridge cannot run that last check itself (it may not depend on `celerrate_diagnostics`), which is why the gate sits above.
5. **Triage guideline:** map a foreign identifier when its finding class overlaps a CEL family's; over-suppression is the accepted direction; when in doubt, `Unmapped` (the fallback widens to the whole scope, which is exactly today's behavior, so doubt is behavior-preserving).
6. **The native scope variant is `DirectiveScope::TrailingOrNextLine`.** Resolved in `resolve_scope` (the token and line index are visible there, never in a provider): code preceding the comment on the comment's first line → the comment's own line(s) (the `CurrentLine` computation); the comment alone on its line → the next line (the `NextLine` computation). "Code" is any token that is neither whitespace nor comment trivia - a preceding comment does not make the directive trailing; a `<?php` open tag does. Docblock placement maps to `AnnotatedDeclaration` in the provider (the kind is provider-visible), keeping the declaration scope.
7. **The native parser is a sibling, not a shared helper.** `native_directives` duplicates the small `ends_word`/`identifiers_of` helpers rather than sharing them with the bridge: `celerrate_semantics` sits below the bridge in the DAG, and the two grammars may diverge (the native one is Celerrate's own to evolve). About twenty lines of acknowledged duplication, noted in the module doc.
8. **Match records are stored per half.** `StoredVerdict.directives: Vec<StoredDirective>` carries each directive's identity (anchor, scope, filter, written identifiers, native flag) plus `matched: bool` for the untyped half; `StoredTypedVerdict.matched_directives: Vec<u32>` carries the typed half's admitting indexes into that same list. Rationale: the two halves validate and serve independently (a partial hit is first-class), and a stored union flag would go stale exactly when the typed half recomputes. The reporting phase consumes the union, computed at composition time from whichever source each half used. Directive order is the query's deterministic order, so indexes align between stored and fresh by construction; any mismatch on load (bounds, unknown filter code, out-of-range index) discards the verdict, like a failed diagnostic conversion - the checksum proves transport, never honesty.
9. **The `Reporting` phase is a plain function, not a salsa query.** `reporting_phase_diagnostics(db, file_id, text_end, outcomes)` runs the registered `Reporting` rules from `DirectiveOutcome` records (directive plus final matched flag). Its input is composed by the orchestration layer (stored records on a warm hit, the query on a miss), which is exactly why it cannot be keyed as a query - and why it needs no parse. Reporting diagnostics are never persisted: both paths recompute them from the records, which is cheap (directives per file are few) and keeps the stored verdict's meaning unchanged (the cache-servable halves).
10. **The one-pass suppression algorithm, exactly.** (a) Rules emit CEL0041/CEL0042 findings from the final match outcomes; a CEL0042 finding records its subject directive index. (b) One pass over those findings: a finding admitted by any directive is dropped, and every admitting directive is marked used. (c) CEL0042 findings whose subject was marked used in (b) are dropped. Nothing iterates: uses recorded in (b) do not re-open (b), and drops in (c) do not un-use anything. This is the reading under which "suppressing it counts as use" has an effect without a fixpoint.
11. **CEL0042 evaluability.** A native directive is evaluable for CEL0042 only when it matched nothing AND every identifier is known (an unknown identifier already produces CEL0041; stacking "unused" on top of "typo" reports the same mistake twice) AND no identifier belongs to an inactive rule (the design's exemption: nursery demotion must not convert existing suppressions into a warning storm). The inactive set is the identifiers claimed by registrations with `active == false`; resilience identifiers are claimed by no rule and are always emitted, so they are never inactive.
12. **CEL0041/CEL0042 anchor at the carrying comment token's range** (`ResolvedDirective.anchor`), message naming the written identifier. Per-identifier sub-ranges inside the comment would need providers to report offsets - machinery no shipped rule needs yet (the YAGNI criterion).
13. **`PLUGIN_API_VERSION` stays 0.** The vocabulary reshape is breaking, but version 0 is the declared pre-stability era, both plugins compile in-tree, and parts 3 and 4 set the precedent; the version starts moving when the WASM sub-project makes out-of-tree plugins possible.
14. **The reason trailer is parsed and dropped.** `@celerrate-ignore CEL0030 (reason)` excludes the parenthesized trailer from identifier parsing (the existing `identifiers_of` affordance). The reason text is not carried on `ResolvedDirective`: its only consumer (a verbose widened-directive channel) is sub-project 5 product surface.
15. **Rule names:** `unknown-suppression-identifier` (CEL0041) and `unused-suppression` (CEL0042), both group `Correctness`, tier `Default`, severity `Warning`, owner `celerrate_rules`. Two rules, not one family: they answer different questions and the design names them separately. Explain pages arrive with part 8's workstream (the registry's `explain` field stays `Option` until then).

## File structure

Created:

- `crates/celerrate_semantics/src/native_directive.rs` (the native provider, its parser, its tests)
- `crates/celerrate_phpdoc_bridge/src/correspondence.rs` (the two dialect tables, `ForeignMapping`, `Dialect`, lookup)
- `crates/celerrate_phpdoc_bridge/catalogues/phpstan-identifiers.txt` (vendored catalogue, pinned)
- `crates/celerrate_phpdoc_bridge/catalogues/psalm-issues.txt` (vendored catalogue, pinned)
- `crates/celerrate_rules/src/rules/unknown_suppression_identifier.rs` (CEL0041)
- `crates/celerrate_rules/src/rules/unused_suppression.rs` (CEL0042)
- `crates/celerrate_cli/tests/suppression_correspondence.rs` (the triage gate)
- `crates/celerrate_cli/tests/directive_rules.rs` (the CEL0041/CEL0042 product matrix)

Modified:

- `crates/celerrate_semantics/src/comment_directives.rs` (vocabulary, `TrailingOrNextLine`, `suppression_directives`, `ResolvedDirective`, `SuppressionFilter`, `filter_of`; `suppressed_ranges`/`is_suppressed` deleted)
- `crates/celerrate_semantics/src/lib.rs` (export updates, `native_directive` module)
- `crates/celerrate_plugin/src/lib.rs` (re-export `SuppressionIdentifier`, `DirectiveOrigin`)
- `crates/celerrate_phpdoc_bridge/src/directives.rs` (table consultation; the reserved comment resolved)
- `crates/celerrate_phpdoc_bridge/src/lib.rs` (`correspondence` module, exports)
- `crates/celerrate_cli/src/analysis.rs` (`FilteredPortion`, identifier-aware `retain_unsuppressed`, `directive_outcomes`, `reporting_portion`, composition threading)
- `crates/celerrate_cli/src/cache/stored.rs` (`StoredDirective`, `StoredSuppressionFilter`, verdict fields, conversions)
- `crates/celerrate_cli/src/cache/pack.rs` (`CACHE_SCHEMA_VERSION` 7)
- `crates/celerrate_cli/src/cache/mod.rs` (`composed_verdict_with_lever`, `composed_typed_verdict`, `collect_entries` reuse guard)
- `crates/celerrate_cli/src/plugins.rs` (native provider registration; composition-root tests)
- `crates/celerrate_rules/src/context.rs` (`ReportingContext` real surface, `DirectiveOutcome`)
- `crates/celerrate_rules/src/finding.rs` (`Finding.subject`, `report_directive`)
- `crates/celerrate_rules/src/phases.rs` (`reporting_phase_diagnostics`)
- `crates/celerrate_rules/src/rules/mod.rs` (two registrations, `ALLOCATED_IDENTIFIERS` + 2)
- `crates/celerrate_rules/src/lib.rs` (exports)
- `crates/celerrate_diagnostics/src/registry.rs` (CEL0041, CEL0042; gapless count 42)
- `crates/celerrate_cli/tests/suppressions.rs` (foreign and native matrices)
- `crates/celerrate_cli/tests/cache_suppression.rs`, `tests/cache_equivalence.rs` (records and reporting equivalence)
- `docs/diagnostics.md` (CEL0041/CEL0042 section, the native directive), `docs/phpdoc-bridge.md` (identifier-level correspondence)
- `CHANGELOG.md`

---

### Task 1: The directive vocabulary and the placement-resolved scope

Behavior-preserving reshape: richer identifier and origin vocabulary, the `TrailingOrNextLine` variant with its resolution, every provider and test updated to compile. All existing suppression behavior is unchanged (every foreign identifier is temporarily `Unmapped`, which the current matcher ignores anyway).

**Files:**

- Modify: `crates/celerrate_semantics/src/comment_directives.rs`
- Modify: `crates/celerrate_plugin/src/lib.rs`
- Modify: `crates/celerrate_phpdoc_bridge/src/directives.rs`

**Interfaces:**

- Consumes: `resolve_scope`, `whole_lines`, `LineIndex`, `SyntaxToken::prev_token()`.
- Produces: `SuppressionIdentifier` (with constructors `mapped`, `scope_wide`, `unmapped`, `native`, and accessor `written(&self) -> &str`), `DirectiveOrigin { Foreign, Native }`, `DirectiveScope::TrailingOrNextLine`, `CommentDirective::suppress(scope, origin, identifiers)`. Tasks 2, 4, 5 build on these exact names.

- [ ] **Step 1: Write the failing tests (vocabulary and placement)**

In `comment_directives.rs`'s test module, replace `the_suppress_constructor_is_field_faithful` and add placement tests. The fake provider gains a `@trailing` marker:

```rust
// In FakeProvider::directives, extend the marker table:
for (marker, scope) in [
    ("@line", DirectiveScope::CurrentLine),
    ("@next", DirectiveScope::NextLine),
    ("@both", DirectiveScope::CurrentAndNextLine),
    ("@declaration", DirectiveScope::AnnotatedDeclaration),
    ("@trailing", DirectiveScope::TrailingOrNextLine),
] {
    if text.contains(marker) {
        directives.push(CommentDirective::suppress(
            scope,
            DirectiveOrigin::Foreign,
            Vec::new(),
        ));
    }
}
```

(The `@fake` arm becomes `CommentDirective::suppress(DirectiveScope::CurrentLine, DirectiveOrigin::Foreign, vec![SuppressionIdentifier::unmapped("fake.identifier".to_owned())])`.)

```rust
#[test]
fn the_suppress_constructor_is_field_faithful() {
    let directive = CommentDirective::suppress(
        DirectiveScope::NextLine,
        DirectiveOrigin::Foreign,
        vec![SuppressionIdentifier::unmapped("method.notFound".to_owned())],
    );
    assert_eq!(
        directive,
        CommentDirective::Suppress {
            scope: DirectiveScope::NextLine,
            origin: DirectiveOrigin::Foreign,
            identifiers: vec![SuppressionIdentifier::Unmapped {
                written: "method.notFound".to_owned(),
            }],
        },
    );
}

#[test]
fn each_identifier_variant_answers_its_written_form() {
    for identifier in [
        SuppressionIdentifier::mapped("a.b".to_owned(), vec!["CEL0030".to_owned()]),
        SuppressionIdentifier::scope_wide("all".to_owned()),
        SuppressionIdentifier::unmapped("a.b".to_owned()),
        SuppressionIdentifier::native("CEL0030".to_owned()),
    ] {
        assert!(!identifier.written().is_empty());
    }
}

#[test]
fn a_trailing_directive_behind_code_covers_its_own_line() {
    let source = "<?php\n$x = 1; // @trailing\n$y = 2;\n";
    let (db, file) = fixture(source);
    assert!(suppressed_at(&db, file, source, "$x"));
    assert!(!suppressed_at(&db, file, source, "$y"));
}

#[test]
fn a_trailing_directive_alone_on_its_line_covers_the_next_line_only() {
    let source = "<?php\n// @trailing\n$x = 1;\n$y = 2;\n";
    let (db, file) = fixture(source);
    assert!(!suppressed_at(&db, file, source, "// @trailing"));
    assert!(suppressed_at(&db, file, source, "$x"));
    assert!(!suppressed_at(&db, file, source, "$y"));
}

#[test]
fn a_preceding_comment_is_not_code_for_placement_resolution() {
    // Only trivia precedes the directive on its line: it stands alone
    // and targets the next line.
    let source = "<?php\n/* note */ // @trailing\n$x = 1;\n$y = 2;\n";
    let (db, file) = fixture(source);
    assert!(suppressed_at(&db, file, source, "$x"));
    assert!(!suppressed_at(&db, file, source, "$y"));
}

#[test]
fn an_open_tag_counts_as_code_for_placement_resolution() {
    let source = "<?php // @trailing\n$x = 1;\n";
    let (db, file) = fixture(source);
    assert!(suppressed_at(&db, file, source, "<?php"));
    assert!(!suppressed_at(&db, file, source, "$x"));
}

#[test]
fn a_trailing_directive_alone_on_the_last_line_suppresses_nothing() {
    let source = "<?php\n$x = 1;\n// @trailing";
    let (db, file) = fixture(source);
    assert!(!suppressed_at(&db, file, source, "$x"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_semantics comment_directives`
Expected: compile errors (`SuppressionIdentifier` and `DirectiveOrigin` do not exist; `suppress` has two arguments).

- [ ] **Step 3: Implement the vocabulary and the resolution**

In `comment_directives.rs`, above `CommentDirective`:

```rust
/// One written identifier of a suppression directive and its fate.
/// Foreign fates come from the bridge's correspondence table; codes
/// travel as plain strings and are interned downstream through
/// `celerrate_diagnostics::find_identifier` (design section 8: the
/// facade grows no identifier vocabulary for this).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SuppressionIdentifier {
    /// A foreign identifier the correspondence table maps: it
    /// suppresses exactly these Celerrate codes.
    Mapped { written: String, codes: Vec<String> },
    /// A foreign identifier that explicitly names the whole scope
    /// (`@psalm-suppress all`): an entry, not a fallback accident.
    ScopeWide { written: String },
    /// A foreign identifier with no Celerrate correspondence: the
    /// directive falls back to scope-wide suppression, honoring the
    /// user's existing decision (the #58 triage policy).
    Unmapped { written: String },
    /// A native `CEL####` identifier, written form kept verbatim. An
    /// unknown one suppresses nothing (never widens) and is reported
    /// by CEL0041.
    Native { written: String },
}

impl SuppressionIdentifier {
    pub fn mapped(written: String, codes: Vec<String>) -> Self {
        Self::Mapped { written, codes }
    }

    pub fn scope_wide(written: String) -> Self {
        Self::ScopeWide { written }
    }

    pub fn unmapped(written: String) -> Self {
        Self::Unmapped { written }
    }

    pub fn native(written: String) -> Self {
        Self::Native { written }
    }

    /// The identifier as the user wrote it.
    pub fn written(&self) -> &str {
        match self {
            Self::Mapped { written, .. }
            | Self::ScopeWide { written }
            | Self::Unmapped { written }
            | Self::Native { written } => written,
        }
    }
}

/// Who a directive belongs to. The distinction is load-bearing for the
/// empty-identifier case: a bare foreign directive suppresses the
/// whole scope, a bare native one suppresses nothing (identifiers are
/// mandatory by design), and only native directives are subject to the
/// CEL0041/CEL0042 reporting rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DirectiveOrigin {
    Foreign,
    Native,
}
```

Reshape `CommentDirective` and its constructor:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CommentDirective {
    /// Extinguish diagnostics on the scope, filtered by the
    /// identifiers under the origin's policy (`filter_of` below is the
    /// single implementation of that policy).
    #[non_exhaustive]
    Suppress {
        scope: DirectiveScope,
        origin: DirectiveOrigin,
        identifiers: Vec<SuppressionIdentifier>,
    },
}

impl CommentDirective {
    /// Constructor for cross-crate construction: literal construction
    /// is closed by `#[non_exhaustive]`.
    pub fn suppress(
        scope: DirectiveScope,
        origin: DirectiveOrigin,
        identifiers: Vec<SuppressionIdentifier>,
    ) -> Self {
        Self::Suppress {
            scope,
            origin,
            identifiers,
        }
    }
}
```

Add the scope variant to `DirectiveScope`:

```rust
    /// The line(s) the comment trails when code precedes it on the
    /// comment's first line; the next line when the comment stands
    /// alone. The native directive's placement-dependent scope,
    /// resolved where the token and its line context are visible
    /// (`resolve_scope`), never in a provider - a provider is a pure
    /// function of the comment and cannot see position.
    TrailingOrNextLine,
```

Extend `resolve_scope` with the arm and its helper:

```rust
        DirectiveScope::TrailingOrNextLine => {
            if code_precedes_on_line(token, index) {
                whole_lines(index, text_end, first_line, last_line)
            } else {
                let next = last_line.checked_add(1)?;
                whole_lines(index, text_end, next, next)
            }
        }
```

```rust
/// Whether any non-trivia token starts before `token` on the token's
/// first line: the placement question `TrailingOrNextLine` resolves
/// on. Comment trivia does not count as code (a stacked comment leaves
/// the directive alone on its line); anything else, the `<?php` open
/// tag included, does.
fn code_precedes_on_line(token: &SyntaxToken, index: &LineIndex) -> bool {
    let first_line = index.line_column(token.text_range().start()).line;
    let Some(line_start) = index.offset(LineColumn {
        line: first_line,
        column: 0,
    }) else {
        return false;
    };
    let mut current = token.prev_token();
    while let Some(previous) = current {
        if previous.text_range().end() <= line_start {
            return false;
        }
        if !matches!(
            previous.kind(),
            SyntaxKind::Whitespace
                | SyntaxKind::LineComment
                | SyntaxKind::BlockComment
                | SyntaxKind::DocComment
        ) {
            return true;
        }
        current = previous.prev_token();
    }
    false
}
```

`suppressed_ranges`'s match arm destructures the new shape (`CommentDirective::Suppress { scope, .. }` still compiles unchanged). Update the module doc's "never matched here" sentence to: "identifier-level matching lives in `suppression_directives`' filter computation (task 2 of the part-5 plan resolves the long-standing reservation)". The full doc rewrite lands in task 2 with the query itself.

- [ ] **Step 4: Update the bridge and the facade to compile**

In `crates/celerrate_phpdoc_bridge/src/directives.rs`: import `DirectiveOrigin` and `SuppressionIdentifier` from `celerrate_plugin`; change the private helper and `identifiers_of` call sites so every foreign identifier is temporarily unmapped:

```rust
fn suppress(scope: DirectiveScope, identifiers: Vec<String>) -> CommentDirective {
    CommentDirective::suppress(
        scope,
        DirectiveOrigin::Foreign,
        identifiers
            .into_iter()
            .map(SuppressionIdentifier::unmapped)
            .collect(),
    )
}
```

Update the bridge's test helper `suppress(scope, &[..])` the same way so its assertions compare `Unmapped` identifiers. In `crates/celerrate_plugin/src/lib.rs`, extend the semantics re-export list:

```rust
pub use celerrate_semantics::{
    CommentDirective, CommentDirectiveProvider, CommentKind, DirectiveOrigin, DirectiveScope,
    PluginIdentity, SuppressionIdentifier, VirtualMember, VirtualMemberKind, VirtualParameter,
    VirtualSymbolProvider,
};
```

In `crates/celerrate_semantics/src/lib.rs`, add `DirectiveOrigin` and `SuppressionIdentifier` to the `comment_directives` re-export.

- [ ] **Step 5: Run the workspace tests**

Run: `cargo test --workspace`
Expected: PASS (the new placement tests included; every existing suppression test green - the reshape is behavior-preserving).

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_semantics crates/celerrate_plugin crates/celerrate_phpdoc_bridge
git commit -m "✨ feat(semantics): grow the directive vocabulary and placement-resolved scope"
```

---

### Task 2: `suppression_directives`, the filter-carrying resolution query

`suppressed_ranges` becomes a per-directive resolution: anchor, scope, filter, written identifiers, origin. The CLI matcher becomes identifier-aware and attributes matches. Still behavior-preserving: every foreign identifier is `Unmapped` (filter `All`) until task 4, and no native provider is registered until task 5.

**Files:**

- Modify: `crates/celerrate_semantics/src/comment_directives.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`
- Modify: `crates/celerrate_cli/src/analysis.rs`

**Interfaces:**

- Consumes: task 1's vocabulary; `celerrate_diagnostics::find_identifier`; the existing walk in `suppressed_ranges`.
- Produces: `SuppressionFilter { All, Only(Vec<DiagnosticId>) }`, `ResolvedDirective { anchor, scope, filter, identifiers: Vec<String>, origin }` with `admits(&self, id, offset, text_end) -> bool`, the tracked query `suppression_directives(db, file) -> Vec<ResolvedDirective>` (`returns(ref)`), and the CLI's `retain_unsuppressed(database, file, directives, diagnostics) -> Vec<u32>` (sorted admitting indexes). Tasks 6 to 9 consume these exact names. `suppressed_ranges` and `is_suppressed` are deleted.

- [ ] **Step 1: Write the failing tests**

In `comment_directives.rs` tests (the fixture helpers change: `suppressed_at` now goes through `admits`):

```rust
fn suppressed_at(
    db: &TestDatabase,
    file: celerrate_db::SourceFile,
    source: &str,
    needle: &str,
) -> bool {
    let directives = suppression_directives(db, file);
    let offset = offset_of(source, needle);
    let text_end = TextSize::of(source);
    // The test marker directives carry no mapped identifier, so any
    // registered identifier probes the position logic.
    let id = celerrate_diagnostics::find_identifier("CEL0018").unwrap();
    directives
        .iter()
        .any(|directive| directive.admits(id, offset, text_end))
}
```

New tests:

```rust
#[test]
fn a_foreign_directive_with_only_mapped_identifiers_narrows_to_their_union() {
    let identifiers = vec![
        SuppressionIdentifier::mapped("arguments.count".to_owned(), vec![
            "CEL0036".to_owned(),
            "CEL0037".to_owned(),
        ]),
        SuppressionIdentifier::mapped("class.notFound".to_owned(), vec!["CEL0018".to_owned()]),
    ];
    let filter = filter_of(DirectiveOrigin::Foreign, &identifiers);
    assert_eq!(
        filter,
        SuppressionFilter::Only(vec![
            celerrate_diagnostics::find_identifier("CEL0018").unwrap(),
            celerrate_diagnostics::find_identifier("CEL0036").unwrap(),
            celerrate_diagnostics::find_identifier("CEL0037").unwrap(),
        ]),
    );
}

#[test]
fn a_bare_foreign_directive_suppresses_the_whole_scope() {
    assert_eq!(
        filter_of(DirectiveOrigin::Foreign, &[]),
        SuppressionFilter::All
    );
}

#[test]
fn any_unmapped_foreign_identifier_widens_to_the_whole_scope() {
    let identifiers = vec![
        SuppressionIdentifier::mapped("class.notFound".to_owned(), vec!["CEL0018".to_owned()]),
        SuppressionIdentifier::unmapped("something.else".to_owned()),
    ];
    assert_eq!(
        filter_of(DirectiveOrigin::Foreign, &identifiers),
        SuppressionFilter::All
    );
}

#[test]
fn an_explicit_scope_wide_identifier_widens_to_the_whole_scope() {
    let identifiers = vec![SuppressionIdentifier::scope_wide("all".to_owned())];
    assert_eq!(
        filter_of(DirectiveOrigin::Foreign, &identifiers),
        SuppressionFilter::All
    );
}

#[test]
fn a_native_directive_unions_its_known_identifiers_and_drops_unknown_ones() {
    let identifiers = vec![
        SuppressionIdentifier::native("CEL0030".to_owned()),
        SuppressionIdentifier::native("CEL9999".to_owned()),
        SuppressionIdentifier::native("CEL0018".to_owned()),
    ];
    assert_eq!(
        filter_of(DirectiveOrigin::Native, &identifiers),
        SuppressionFilter::Only(vec![
            celerrate_diagnostics::find_identifier("CEL0018").unwrap(),
            celerrate_diagnostics::find_identifier("CEL0030").unwrap(),
        ]),
    );
}

#[test]
fn a_bare_native_directive_suppresses_nothing() {
    assert_eq!(
        filter_of(DirectiveOrigin::Native, &[]),
        SuppressionFilter::Only(Vec::new()),
    );
}

#[test]
fn an_only_filter_admits_exactly_its_codes_on_its_scope() {
    let directive = ResolvedDirective {
        anchor: TextRange::new(TextSize::from(10), TextSize::from(30)),
        scope: TextRange::new(TextSize::from(0), TextSize::from(31)),
        filter: SuppressionFilter::Only(vec![
            celerrate_diagnostics::find_identifier("CEL0018").unwrap(),
        ]),
        identifiers: vec!["class.notFound".to_owned()],
        origin: DirectiveOrigin::Foreign,
    };
    let text_end = TextSize::from(100);
    let inside = TextSize::from(5);
    let outside = TextSize::from(50);
    let cel0018 = celerrate_diagnostics::find_identifier("CEL0018").unwrap();
    let cel0019 = celerrate_diagnostics::find_identifier("CEL0019").unwrap();
    assert!(directive.admits(cel0018, inside, text_end));
    assert!(!directive.admits(cel0019, inside, text_end));
    assert!(!directive.admits(cel0018, outside, text_end));
}

#[test]
fn the_end_of_file_exception_survives_in_admits() {
    let end = TextSize::from(20);
    let directive = ResolvedDirective {
        anchor: TextRange::new(TextSize::from(8), TextSize::from(20)),
        scope: TextRange::new(TextSize::from(6), end),
        filter: SuppressionFilter::All,
        identifiers: Vec::new(),
        origin: DirectiveOrigin::Foreign,
    };
    let cel0007 = celerrate_diagnostics::find_identifier("CEL0007").unwrap();
    assert!(directive.admits(cel0007, end, end));
    assert!(!directive.admits(cel0007, end, TextSize::from(40)));
}

#[test]
fn the_query_resolves_anchor_scope_and_origin_per_directive() {
    let source = "<?php\n$x = 1; // @line\n";
    let (db, file) = fixture(source);
    let directives = suppression_directives(&db, file);
    assert_eq!(directives.len(), 1);
    let directive = &directives[0];
    assert_eq!(directive.origin, DirectiveOrigin::Foreign);
    assert_eq!(directive.filter, SuppressionFilter::All);
    let comment_start = offset_of(source, "// @line");
    assert_eq!(directive.anchor.start(), comment_start);
}
```

Keep and re-target the existing scope-resolution tests (`a_current_line_directive_covers_its_whole_line_and_only_it`, the placement tests of task 1, and so on): the `suppressed_at` helper above keeps them meaningful unchanged. Re-target `identical_resolved_ranges_deduplicate` (two markers in one comment produce two identical `ResolvedDirective` values → deduplicated to one) and the backdate test (rename `suppression_count` to read `suppression_directives(db, file).len()`; the assertion text keeps its meaning: the own-tree read re-runs on any edit, an identical directive set backdates).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_semantics comment_directives`
Expected: compile errors (`SuppressionFilter`, `ResolvedDirective`, `filter_of`, `suppression_directives` do not exist).

- [ ] **Step 3: Implement the resolution query**

In `comment_directives.rs` (also add `celerrate_diagnostics` to imports; the crate already depends on it):

```rust
use celerrate_diagnostics::DiagnosticId;

/// What a directive's identifier list resolved to: the matcher input.
/// Design section 8's mechanics - a filter per range, `All` or
/// `Only(sorted codes)`; co-location merges by union semantically,
/// because a diagnostic is suppressed exactly when any directive
/// admits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuppressionFilter {
    /// Every diagnostic family on the scope.
    All,
    /// Exactly these identifiers, sorted and deduplicated (binary
    /// search relies on the order).
    Only(Vec<DiagnosticId>),
}

/// One directive, resolved against the file: where it sits (the
/// carrying comment token - where CEL0041/CEL0042 anchor), what it
/// covers, what it admits, and what the reporting rules need to speak
/// about it. The reason trailer is deliberately not carried: its only
/// consumer (a verbose widened-directive channel) is sub-project 5
/// product surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDirective {
    pub anchor: TextRange,
    pub scope: TextRange,
    pub filter: SuppressionFilter,
    /// The written identifiers, verbatim, in written order.
    pub identifiers: Vec<String>,
    pub origin: DirectiveOrigin,
}

impl ResolvedDirective {
    /// Whether this directive admits (suppresses) a diagnostic of `id`
    /// anchored at `offset`. Position matching is by the diagnostic's
    /// start, end-exclusive, except at the very end of the file: a
    /// diagnostic anchored exactly at the text's end (an
    /// unexpected-end-of-file parse error) belongs to the last line
    /// and must be suppressible from it (the rule `is_suppressed`
    /// carried, preserved verbatim).
    pub fn admits(&self, id: DiagnosticId, offset: TextSize, text_end: TextSize) -> bool {
        let in_scope = offset >= self.scope.start()
            && (offset < self.scope.end()
                || (offset == self.scope.end() && self.scope.end() == text_end));
        if !in_scope {
            return false;
        }
        match &self.filter {
            SuppressionFilter::All => true,
            SuppressionFilter::Only(codes) => codes.binary_search(&id).is_ok(),
        }
    }
}

/// The single implementation of the correspondence policy (design
/// section 8, fixed by the #58 triage). Foreign: a bare list, any
/// scope-wide or unmapped identifier, or a mapped code that fails
/// interning widens to `All` - over-suppression, never
/// under-suppression (the correspondence gate makes the failed-intern
/// arm unreachable; it is the honest fallback, not a code path).
/// Native: the union of the identifiers that intern; unknown ones are
/// excluded and never widen (they suppress nothing - CEL0041's reason
/// to exist).
pub(crate) fn filter_of(
    origin: DirectiveOrigin,
    identifiers: &[SuppressionIdentifier],
) -> SuppressionFilter {
    let mut codes: Vec<DiagnosticId> = Vec::new();
    match origin {
        DirectiveOrigin::Foreign => {
            if identifiers.is_empty() {
                return SuppressionFilter::All;
            }
            for identifier in identifiers {
                match identifier {
                    SuppressionIdentifier::Mapped { codes: mapped, .. } => {
                        for code in mapped {
                            match celerrate_diagnostics::find_identifier(code) {
                                Some(id) => codes.push(id),
                                None => return SuppressionFilter::All,
                            }
                        }
                    }
                    _ => return SuppressionFilter::All,
                }
            }
        }
        DirectiveOrigin::Native => {
            for identifier in identifiers {
                if let SuppressionIdentifier::Native { written } = identifier
                    && let Some(id) = celerrate_diagnostics::find_identifier(written)
                {
                    codes.push(id);
                }
            }
        }
    }
    codes.sort();
    codes.dedup();
    SuppressionFilter::Only(codes)
}

/// The file's directives, resolved: every comment handed to every
/// registered provider, symbolic scopes resolved against the line
/// index, filters computed under the correspondence policy, sorted and
/// deduplicated. An own-tree read for strictly-local output.
/// `Eq`-comparable: a comment edit that leaves the directive set
/// unchanged backdates, and dependents never re-run.
#[salsa::tracked(returns(ref))]
pub fn suppression_directives(
    db: &dyn salsa::Database,
    file: SourceFile,
) -> Vec<ResolvedDirective> {
    let Some(registry) = CommentDirectiveRegistry::try_get(db) else {
        return Vec::new();
    };
    let registrations = registry.registrations(db);
    if registrations.is_empty() {
        return Vec::new();
    }
    let root = celerrate_db::parse(db, file).tree();
    let index = celerrate_db::line_index(db, file);
    let text_end = root.text_range().end();
    let mut directives = Vec::new();
    for element in root.descendants_with_tokens() {
        let Some(token) = element.as_token() else {
            continue;
        };
        let Some(kind) = comment_kind(token.kind()) else {
            continue;
        };
        for registration in registrations {
            for directive in registration.provider.directives(kind, token.text()) {
                match directive {
                    CommentDirective::Suppress {
                        scope,
                        origin,
                        identifiers,
                    } => {
                        let Some(resolved) = resolve_scope(scope, token, index, text_end) else {
                            continue;
                        };
                        directives.push(ResolvedDirective {
                            anchor: token.text_range(),
                            scope: resolved,
                            filter: filter_of(origin, &identifiers),
                            identifiers: identifiers
                                .iter()
                                .map(|identifier| identifier.written().to_owned())
                                .collect(),
                            origin,
                        });
                    }
                }
            }
        }
    }
    directives.sort_by(|left, right| {
        (
            left.anchor.start(),
            left.anchor.end(),
            left.scope.start(),
            left.scope.end(),
        )
            .cmp(&(
                right.anchor.start(),
                right.anchor.end(),
                right.scope.start(),
                right.scope.end(),
            ))
            .then_with(|| format!("{left:?}").cmp(&format!("{right:?}")))
    });
    directives.dedup();
    directives
}
```

(The `format!`-based tiebreak is deliberate minimalism: full determinism for directives sharing all four offsets without hand-deriving `Ord` over the whole struct; it runs only on ties, which one comment produces at most a handful of.)

Delete `suppressed_ranges` and `is_suppressed`. Rewrite the module doc: the vocabulary paragraph now states that identifier-level correspondence is resolved here (`filter_of`), closing the reservation the old comment carried.

- [ ] **Step 4: Re-target the CLI matcher**

In `crates/celerrate_cli/src/analysis.rs`, replace `retain_unsuppressed` (imports change from `is_suppressed`/`suppressed_ranges` to `suppression_directives`/`ResolvedDirective`):

```rust
/// Filters `diagnostics` down to what no suppression directive
/// admits, and answers the sorted indexes (into
/// `suppression_directives(db, file)`) of every directive that
/// admitted at least one diagnostic - any-match attribution: a
/// diagnostic admitted by several co-located directives marks them
/// all used (design section 4). Shared by `persistable_diagnostics`
/// and `typed_portion` so the two composers apply the exact same
/// filter.
fn retain_unsuppressed(
    database: &dyn salsa::Database,
    file: SourceFile,
    directives: &[celerrate_semantics::ResolvedDirective],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<u32> {
    let text_end = celerrate_db::source_text(database, file)
        .as_ref()
        .map(|text| TextSize::of(text.text()))
        .unwrap_or_default();
    let mut matched = std::collections::BTreeSet::new();
    diagnostics.retain(|diagnostic| {
        let Some((_, range)) = diagnostic.span() else {
            return true;
        };
        let mut suppressed = false;
        for (index, directive) in directives.iter().enumerate() {
            if directive.admits(diagnostic.id, range.start(), text_end) {
                suppressed = true;
                if let Ok(index) = u32::try_from(index) {
                    matched.insert(index);
                }
            }
        }
        !suppressed
    });
    matched.into_iter().collect()
}
```

Both call sites (`persistable_diagnostics`, `typed_portion`) change their local from `suppressed` to `directives = celerrate_semantics::suppression_directives(database, file)` and discard the return for now (`let _ = retain_unsuppressed(...)`); the threading is task 6. Touch up the two prose mentions of `suppressed_ranges` in module docs (`crates/celerrate_cli/src/cache/stored.rs`, `crates/celerrate_cli/tests/cache_suppression.rs`) to name `suppression_directives` - the suppression-note semantics they describe are unchanged.

- [ ] **Step 5: Run the workspace tests**

Run: `cargo test --workspace`
Expected: PASS. Every product suppression test (`suppressions.rs`, `cache_suppression.rs`) stays green: all foreign identifiers are still `Unmapped`, so every filter is `All` - today's behavior exactly.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_semantics crates/celerrate_cli
git commit -m "♻️ refactor(semantics): resolve directives with filters and attribution"
```

---

### Task 3: The correspondence table and its vendored catalogues

The triage workstream, scheduled as work (the stub-curation lesson). Pure data plus unit tests in the bridge; no behavior change yet (nothing consults the table until task 4). The composition-root gate arrives in the same task so the table can never drift from the catalogues unnoticed.

**Files:**

- Create: `crates/celerrate_phpdoc_bridge/src/correspondence.rs`
- Create: `crates/celerrate_phpdoc_bridge/catalogues/phpstan-identifiers.txt`
- Create: `crates/celerrate_phpdoc_bridge/catalogues/psalm-issues.txt`
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs`
- Create: `crates/celerrate_cli/tests/suppression_correspondence.rs`

**Interfaces:**

- Consumes: nothing from the workspace (plain data; the bridge stays facade-only in `[dependencies]`).
- Produces: `Dialect { Phpstan, Psalm }`, `ForeignMapping { Codes(&'static [&'static str]), ScopeWide, Unmapped }`, `foreign_mapping(dialect, identifier) -> ForeignMapping` (exact-case; a miss answers `Unmapped`), `correspondence_entries(dialect) -> &'static [(&'static str, ForeignMapping)]`. Task 4 wires the provider through `foreign_mapping`.

- [ ] **Step 1: Fetch and vendor the published catalogues**

Fetch both catalogues, pinning source and date in a `#`-comment header (parsers below skip `#` lines and blanks). PHPStan publishes its identifier catalogue at `https://phpstan.org/error-identifiers` (backing list in the `phpstan/phpstan-src` repository); Psalm enumerates its issue types in its documentation (`https://psalm.dev/docs/running_psalm/issues/`) and exhaustively in `config.xsd` of the `vimeo/psalm` repository. Suggested extraction (verify the output by eye - a page layout change is a data bug here, not a tooling one):

```bash
mkdir -p crates/celerrate_phpdoc_bridge/catalogues
# PHPStan: one identifier per line from the published catalogue page.
curl -sL https://phpstan.org/error-identifiers | grep -oE '<code>[a-zA-Z]+(\.[a-zA-Z]+)+</code>' | sed -E 's|</?code>||g' | sort -u > /tmp/phpstan-raw.txt
# Psalm: issue types from the repository's config.xsd enumeration.
curl -sL https://raw.githubusercontent.com/vimeo/psalm/master/config.xsd | grep -oE 'name="[A-Za-z]+"' | sed -E 's/name="|"//g' | sort -u > /tmp/psalm-raw.txt
```

Hand-inspect both raw lists (the Psalm extraction over-matches non-issue element names such as `psalm`, `plugins`, `issueHandlers`; strip anything that is not an issue type - issue types are the elements documented on the issues page). Then write each catalogue file with a header of this exact shape before the identifier lines:

```text
# PHPStan error identifiers, https://phpstan.org/error-identifiers
# Upstream version: <the PHPStan version the page states>, retrieved 2026-07-21.
# One identifier per line. The correspondence table in
# src/correspondence.rs must cover this list exactly; the gate is
# crates/celerrate_cli/tests/suppression_correspondence.rs.
```

Add `all` as a line of the Psalm catalogue if the extraction did not produce it (`@psalm-suppress all` is documented suppression surface).

- [ ] **Step 2: Write the failing bridge unit tests**

Create `crates/celerrate_phpdoc_bridge/src/correspondence.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn both_tables_are_sorted_and_unique() {
        for dialect in [Dialect::Phpstan, Dialect::Psalm] {
            let entries = correspondence_entries(dialect);
            for pair in entries.windows(2) {
                assert!(
                    pair[0].0 < pair[1].0,
                    "{dialect:?}: {} must sort strictly before {}",
                    pair[0].0,
                    pair[1].0,
                );
            }
        }
    }

    #[test]
    fn lookup_is_exact_case_and_a_miss_is_unmapped() {
        assert_ne!(
            foreign_mapping(Dialect::Psalm, "UndefinedClass"),
            ForeignMapping::Unmapped,
        );
        assert_eq!(
            foreign_mapping(Dialect::Psalm, "undefinedclass"),
            ForeignMapping::Unmapped,
        );
        assert_eq!(
            foreign_mapping(Dialect::Phpstan, "no.suchIdentifier"),
            ForeignMapping::Unmapped,
        );
    }

    #[test]
    fn psalm_all_is_an_explicit_scope_wide_entry() {
        assert_eq!(
            foreign_mapping(Dialect::Psalm, "all"),
            ForeignMapping::ScopeWide,
        );
    }

    #[test]
    fn the_multi_code_example_of_the_design_holds() {
        // PHPStan's single arguments.count covers findings that span
        // several CEL codes (design section 8).
        assert_eq!(
            foreign_mapping(Dialect::Phpstan, "arguments.count"),
            ForeignMapping::Codes(&["CEL0036", "CEL0037"]),
        );
    }

    #[test]
    fn no_codes_entry_is_empty() {
        for dialect in [Dialect::Phpstan, Dialect::Psalm] {
            for (identifier, mapping) in correspondence_entries(dialect) {
                if let ForeignMapping::Codes(codes) = mapping {
                    assert!(!codes.is_empty(), "{identifier} maps to an empty set");
                }
            }
        }
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p celerrate_phpdoc_bridge correspondence`
Expected: compile errors (the types and tables do not exist).

- [ ] **Step 4: Implement the table**

The module head:

```rust
//! The foreign-to-CEL correspondence table (design section 8): every
//! published identifier of both bridge dialects, triaged - mapped to a
//! non-empty set of Celerrate code strings, explicitly scope-wide, or
//! explicitly unmapped. Codes are plain strings; the matcher
//! downstream interns them, so this crate never grows an identifier
//! dependency. The vendored catalogues under `catalogues/` are the
//! evidence that "every" holds; the gate at the composition root
//! (`celerrate_cli/tests/suppression_correspondence.rs`) asserts table
//! and catalogue are the same set, both directions.
//!
//! Triage guideline: map when the foreign finding class overlaps a CEL
//! family's; over-suppression is the accepted direction; when in
//! doubt, `Unmapped` (the fallback widens to the whole scope, exactly
//! the pre-correspondence behavior).

/// The two written dialects the bridge recognizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Phpstan,
    Psalm,
}

/// One foreign identifier's triage verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignMapping {
    /// Suppresses exactly these Celerrate codes.
    Codes(&'static [&'static str]),
    /// Explicitly suppresses the whole scope (`@psalm-suppress all`).
    ScopeWide,
    /// Triaged: Celerrate has no corresponding diagnostic (yet).
    Unmapped,
}

/// The triage verdict for one written identifier. Exact-case, per
/// dialect; an identifier absent from the table (a foreign tool's
/// newer release, a typo the foreign tool would reject too) is
/// unmapped, so the directive falls back to scope-wide suppression -
/// the user's decision is honored either way.
pub fn foreign_mapping(dialect: Dialect, identifier: &str) -> ForeignMapping {
    let table = correspondence_entries(dialect);
    table
        .binary_search_by(|(name, _)| (*name).cmp(identifier))
        .ok()
        .and_then(|position| table.get(position))
        .map(|(_, mapping)| *mapping)
        .unwrap_or(ForeignMapping::Unmapped)
}

/// The raw table, for the composition-root gate.
pub fn correspondence_entries(dialect: Dialect) -> &'static [(&'static str, ForeignMapping)] {
    match dialect {
        Dialect::Phpstan => PHPSTAN,
        Dialect::Psalm => PSALM,
    }
}
```

Then the two const tables. Seed the mapped entries below; every remaining catalogue identifier gets an explicit `Unmapped` line (generate mechanically from the catalogue files, then splice the seeds in and re-sort - the tables must be sorted by identifier for the binary search). **Verify every seed against the vendored catalogue**: if a seeded name is absent upstream, drop it; if the catalogue's description contradicts the mapping, adjust and note the adjustment in a comment. Seeds:

```rust
const PHPSTAN: &[(&str, ForeignMapping)] = &[
    // ... Unmapped entries interleaved in sorted order ...
    ("argument.type", ForeignMapping::Codes(&["CEL0035"])),
    ("argument.unknown", ForeignMapping::Codes(&["CEL0038"])),
    ("arguments.count", ForeignMapping::Codes(&["CEL0036", "CEL0037"])),
    ("class.deprecated", ForeignMapping::Codes(&["CEL0023"])),
    ("class.notFound", ForeignMapping::Codes(&["CEL0018"])),
    ("classConstant.deprecated", ForeignMapping::Codes(&["CEL0023"])),
    ("classConstant.notFound", ForeignMapping::Codes(&["CEL0032"])),
    ("constant.deprecated", ForeignMapping::Codes(&["CEL0023"])),
    ("constant.notFound", ForeignMapping::Codes(&["CEL0020"])),
    ("function.deprecated", ForeignMapping::Codes(&["CEL0023"])),
    ("function.notFound", ForeignMapping::Codes(&["CEL0019"])),
    ("method.deprecated", ForeignMapping::Codes(&["CEL0023"])),
    ("method.nonObject", ForeignMapping::Codes(&["CEL0034"])),
    ("method.notFound", ForeignMapping::Codes(&["CEL0030"])),
    ("property.deprecated", ForeignMapping::Codes(&["CEL0023"])),
    ("property.nonObject", ForeignMapping::Codes(&["CEL0034"])),
    ("property.notFound", ForeignMapping::Codes(&["CEL0031"])),
    ("staticMethod.deprecated", ForeignMapping::Codes(&["CEL0023"])),
    ("staticMethod.notFound", ForeignMapping::Codes(&["CEL0030"])),
    ("staticProperty.notFound", ForeignMapping::Codes(&["CEL0031"])),
];

const PSALM: &[(&str, ForeignMapping)] = &[
    // ... Unmapped entries interleaved in sorted order ...
    ("DeprecatedClass", ForeignMapping::Codes(&["CEL0023"])),
    ("DeprecatedConstant", ForeignMapping::Codes(&["CEL0023"])),
    ("DeprecatedFunction", ForeignMapping::Codes(&["CEL0023"])),
    ("DeprecatedMethod", ForeignMapping::Codes(&["CEL0023"])),
    ("DeprecatedProperty", ForeignMapping::Codes(&["CEL0023"])),
    ("InvalidArgument", ForeignMapping::Codes(&["CEL0035"])),
    ("InvalidNamedArgument", ForeignMapping::Codes(&["CEL0038"])),
    ("PossiblyInvalidArgument", ForeignMapping::Codes(&["CEL0035"])),
    ("PossiblyNullPropertyFetch", ForeignMapping::Codes(&["CEL0034"])),
    ("PossiblyNullReference", ForeignMapping::Codes(&["CEL0034"])),
    ("PossiblyUndefinedMethod", ForeignMapping::Codes(&["CEL0030"])),
    ("TooFewArguments", ForeignMapping::Codes(&["CEL0036"])),
    ("TooManyArguments", ForeignMapping::Codes(&["CEL0037"])),
    ("UndefinedClass", ForeignMapping::Codes(&["CEL0018"])),
    ("UndefinedConstant", ForeignMapping::Codes(&["CEL0020"])),
    ("UndefinedFunction", ForeignMapping::Codes(&["CEL0019"])),
    ("UndefinedMagicMethod", ForeignMapping::Codes(&["CEL0030"])),
    ("UndefinedMagicPropertyFetch", ForeignMapping::Codes(&["CEL0031"])),
    ("UndefinedMethod", ForeignMapping::Codes(&["CEL0030"])),
    ("UndefinedPropertyFetch", ForeignMapping::Codes(&["CEL0031"])),
    ("UndefinedThisPropertyFetch", ForeignMapping::Codes(&["CEL0031"])),
    ("all", ForeignMapping::ScopeWide),
];
```

Register the module and exports in `src/lib.rs`: `mod correspondence;` plus `pub use correspondence::{Dialect, ForeignMapping, correspondence_entries, foreign_mapping};`.

- [ ] **Step 5: Run the bridge tests**

Run: `cargo test -p celerrate_phpdoc_bridge correspondence`
Expected: PASS.

- [ ] **Step 6: Write the composition-root gate**

Create `crates/celerrate_cli/tests/suppression_correspondence.rs`:

```rust
//! The correspondence-table triage gate (design section 8's closure
//! gate): both dialects' published catalogues fully triaged - table
//! and catalogue are the same set, both directions - and every mapped
//! code re-interns, so silent widening through table incompleteness is
//! bounded by review, never by accident. Lives at the composition root
//! because the bridge may not depend on `celerrate_diagnostics` (the
//! dependency-shape gate) and so cannot check its own code strings.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use celerrate_phpdoc_bridge::{Dialect, ForeignMapping, correspondence_entries};

fn catalogue(file: &str) -> BTreeSet<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../celerrate_phpdoc_bridge/catalogues")
        .join(file);
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is unreadable: {error}", path.display()));
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn table_keys(dialect: Dialect) -> BTreeSet<String> {
    correspondence_entries(dialect)
        .iter()
        .map(|(identifier, _)| (*identifier).to_owned())
        .collect()
}

#[test]
fn the_phpstan_table_covers_its_catalogue_exactly() {
    assert_eq!(
        table_keys(Dialect::Phpstan),
        catalogue("phpstan-identifiers.txt"),
        "table and catalogue must be the same set: an identifier in one \
         but not the other is an untriaged entry or a stale catalogue",
    );
}

#[test]
fn the_psalm_table_covers_its_catalogue_exactly() {
    assert_eq!(table_keys(Dialect::Psalm), catalogue("psalm-issues.txt"));
}

#[test]
fn every_mapped_code_is_a_registered_identifier() {
    for dialect in [Dialect::Phpstan, Dialect::Psalm] {
        for (identifier, mapping) in correspondence_entries(dialect) {
            if let ForeignMapping::Codes(codes) = mapping {
                for code in *codes {
                    assert!(
                        celerrate_diagnostics::find_identifier(code).is_some(),
                        "{dialect:?} {identifier} maps to unregistered {code}",
                    );
                }
            }
        }
    }
}
```

Add `celerrate_phpdoc_bridge` to `celerrate_cli`'s `[dev-dependencies]` if it is not already there (check `crates/celerrate_cli/Cargo.toml`; the composition root already depends on the bridge as a regular dependency, in which case nothing changes).

- [ ] **Step 7: Run the gate and the workspace**

Run: `cargo test -p celerrate_cli --test suppression_correspondence` then `cargo test --workspace`
Expected: PASS. Iterate on the table until the set-equality tests are green - that iteration IS the triage.

- [ ] **Step 8: Commit**

```bash
git add crates/celerrate_phpdoc_bridge crates/celerrate_cli
git commit -m "✨ feat(phpdoc-bridge): triage both dialect catalogues into the correspondence table"
```

---

### Task 4: The bridge consults the table - identifier-precise foreign suppression

The first behavior change: a foreign directive whose identifiers all map narrows to their union (this closes #58's over-suppression hole); any unmapped identifier keeps today's scope-wide fallback. The product matrix, the #58 acceptance test included, lands here.

**Files:**

- Modify: `crates/celerrate_phpdoc_bridge/src/directives.rs`
- Modify: `crates/celerrate_cli/tests/suppressions.rs`

**Interfaces:**

- Consumes: task 3's `foreign_mapping`, task 1's constructors.
- Produces: the bridge emits `Mapped`/`ScopeWide`/`Unmapped` identifiers; no signature changes anywhere.

- [ ] **Step 1: Write the failing product tests**

Append to `crates/celerrate_cli/tests/suppressions.rs`:

```rust
#[test]
fn issue_58_suppressing_one_code_keeps_the_co_located_other_reported() {
    // The acceptance test of the #58 triage: two diagnostics on one
    // line (CEL0018 and CEL0019), a directive naming only the class
    // identifier. Before identifier-level correspondence this
    // suppressed both.
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); // @phpstan-ignore class.notFound\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("CEL0018"), "{text}");
    assert!(text.contains("CEL0019"), "{text}");
}

#[test]
fn a_fully_mapped_identifier_list_suppresses_the_union() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); // @phpstan-ignore class.notFound, function.notFound\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn any_unmapped_identifier_falls_back_to_the_whole_scope() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); // @phpstan-ignore class.notFound, some.unknownIdentifier\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn psalm_suppress_all_is_scope_wide() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); /* @psalm-suppress all */\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn correspondence_lookup_is_exact_case() {
    // The properly cased identifier narrows (CEL0019 survives); the
    // miscased one is unmapped and widens to the whole scope. Both
    // honor the user's suppression; only the exact-case form is
    // precise.
    let narrowed = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); /* @psalm-suppress UndefinedClass */\n",
    )]);
    let (outcome, text) = check(narrowed.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0019"), "{text}");

    let widened = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); /* @psalm-suppress undefinedclass */\n",
    )]);
    let (outcome, text) = check(widened.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_cli --test suppressions`
Expected: `issue_58_...` and `correspondence_lookup_is_exact_case` FAIL (everything is still suppressed scope-wide); the others may already pass - keep them, they pin the policy.

- [ ] **Step 3: Wire the table into the provider**

In `crates/celerrate_phpdoc_bridge/src/directives.rs`, replace the temporary all-unmapped helper with a dialect-aware one and pass the dialect at both call sites (`phpstan_directive` → `Dialect::Phpstan`, `psalm_directive` → `Dialect::Psalm`):

```rust
use crate::correspondence::{Dialect, ForeignMapping, foreign_mapping};

/// One written identifier, marked through the correspondence table
/// (design section 8): mapped with its code strings, explicitly
/// scope-wide, or unmapped. This resolves the long-standing "carried,
/// never matched" reservation: the bridge marks, the matcher
/// downstream matches.
fn foreign_identifier(dialect: Dialect, written: String) -> SuppressionIdentifier {
    match foreign_mapping(dialect, &written) {
        ForeignMapping::Codes(codes) => SuppressionIdentifier::mapped(
            written,
            codes.iter().map(|code| (*code).to_owned()).collect(),
        ),
        ForeignMapping::ScopeWide => SuppressionIdentifier::scope_wide(written),
        ForeignMapping::Unmapped => SuppressionIdentifier::unmapped(written),
    }
}

fn suppress(dialect: Dialect, scope: DirectiveScope, identifiers: Vec<String>) -> CommentDirective {
    CommentDirective::suppress(
        scope,
        DirectiveOrigin::Foreign,
        identifiers
            .into_iter()
            .map(|written| foreign_identifier(dialect, written))
            .collect(),
    )
}
```

Update the module doc: delete the "Identifiers are carried, never matched" sentence, state the marking role instead. Update the bridge's unit tests to assert the marked forms (for example `the_bare_form_carries_identifiers_and_covers_both_placements` now expects `Mapped { written: "method.notFound", codes: ["CEL0030"] }` and `Mapped { written: "property.notFound", codes: ["CEL0031"] }`).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p celerrate_phpdoc_bridge && cargo test -p celerrate_cli --test suppressions && cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Update the bridge documentation**

In `docs/phpdoc-bridge.md`, extend the suppression section: the scope table gains a sentence stating that identifiers now filter (a directive whose identifiers all map suppresses only the union of their mapped CEL codes; any unmapped identifier keeps the scope-wide fallback; `@psalm-suppress all` is explicitly scope-wide; lookup is exact-case). The `documentation.rs` gate only requires the four written forms, which remain present.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_phpdoc_bridge crates/celerrate_cli docs/phpdoc-bridge.md
git commit -m "✨ feat(phpdoc-bridge): narrow mapped foreign directives to their CEL codes"
```

---

### Task 5: The native `@celerrate-ignore` directive

Celerrate's own directive: mandatory identifiers (no blanket form by construction), a reason trailer, the placement-resolved scope, registered unconditionally at the composition root under the reserved core identity, outside the plugin-set digest.

**Files:**

- Create: `crates/celerrate_semantics/src/native_directive.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`
- Modify: `crates/celerrate_cli/src/plugins.rs`
- Modify: `crates/celerrate_cli/tests/suppressions.rs`
- Modify: `docs/diagnostics.md`

**Interfaces:**

- Consumes: task 1's vocabulary and `TrailingOrNextLine`; `core_identity()` in `plugins.rs`.
- Produces: `NativeDirectiveProvider` (a unit struct implementing `CommentDirectiveProvider`) and `native_directives(kind, text) -> Vec<CommentDirective>`, exported from `celerrate_semantics` (not from the facade: the provider is core, not plugin vocabulary).

- [ ] **Step 1: Write the failing unit tests**

Create `crates/celerrate_semantics/src/native_directive.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::comment_directives::{CommentKind, DirectiveOrigin, DirectiveScope};

    fn native(scope: DirectiveScope, identifiers: &[&str]) -> CommentDirective {
        CommentDirective::suppress(
            scope,
            DirectiveOrigin::Native,
            identifiers
                .iter()
                .map(|written| SuppressionIdentifier::native((*written).to_owned()))
                .collect(),
        )
    }

    #[test]
    fn a_line_comment_directive_is_placement_resolved() {
        assert_eq!(
            native_directives(CommentKind::Line, "// @celerrate-ignore CEL0030, CEL0031"),
            vec![native(
                DirectiveScope::TrailingOrNextLine,
                &["CEL0030", "CEL0031"],
            )],
        );
    }

    #[test]
    fn a_docblock_directive_keeps_the_declaration_scope() {
        assert_eq!(
            native_directives(
                CommentKind::Docblock,
                "/**\n * @celerrate-ignore CEL0030\n */",
            ),
            vec![native(DirectiveScope::AnnotatedDeclaration, &["CEL0030"])],
        );
    }

    #[test]
    fn the_reason_trailer_is_excluded_from_identifier_parsing() {
        assert_eq!(
            native_directives(
                CommentKind::Line,
                "// @celerrate-ignore CEL0030, CEL0031 (nullable receiver from the legacy adapter)",
            ),
            vec![native(
                DirectiveScope::TrailingOrNextLine,
                &["CEL0030", "CEL0031"],
            )],
        );
    }

    #[test]
    fn a_bare_directive_still_parses_with_no_identifiers() {
        // No blanket form: the empty identifier list suppresses
        // nothing (filter_of answers Only(empty)); the directive still
        // exists so CEL0042 can report it.
        assert_eq!(
            native_directives(CommentKind::Line, "// @celerrate-ignore"),
            vec![native(DirectiveScope::TrailingOrNextLine, &[])],
        );
    }

    #[test]
    fn the_tag_must_end_at_a_word_boundary() {
        assert!(native_directives(CommentKind::Line, "// @celerrate-ignored").is_empty());
        assert!(native_directives(CommentKind::Line, "// @celerrate-ignores CEL0030").is_empty());
    }

    #[test]
    fn plain_prose_carries_nothing() {
        assert!(native_directives(CommentKind::Line, "// a plain remark").is_empty());
        assert!(native_directives(CommentKind::Docblock, "/** @param int $x */").is_empty());
    }

    #[test]
    fn adversarial_inputs_never_panic() {
        let inputs = [
            "@celerrate-ignore",
            "@celerrate-ignore-",
            "@celerrate-ignore ((((((",
            "@celerrate-ignore ,,,,,",
            "/* @celerrate-ignore */ trailing",
            "@celerrate-ignore \u{0} \u{7f} é漢字",
            "@@@@celerrate-ignore@celerrate-ignore",
            "@",
            "",
        ];
        for input in inputs {
            for kind in [CommentKind::Line, CommentKind::Block, CommentKind::Docblock] {
                let _ = native_directives(kind, input);
            }
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_semantics native_directive`
Expected: compile errors (module and function do not exist; add `mod native_directive;` to `lib.rs` first so the failure is the missing function, not the missing module).

- [ ] **Step 3: Implement the provider**

The module body above the tests:

```rust
//! Celerrate's own suppression directive: `@celerrate-ignore CEL0030,
//! CEL0031 (reason)` in a line comment, a block comment, or a
//! docblock. Identifiers are mandatory - there is no blanket form, so
//! the tool's own directive cannot dig a new #58-class hole by
//! construction (a bare directive parses, suppresses nothing, and is
//! CEL0042's subject). The optional parenthesized trailer is a reason,
//! excluded from identifier parsing. Docblock placement keeps the
//! annotated declaration's scope; everywhere else the scope is
//! placement-resolved (`DirectiveScope::TrailingOrNextLine`).
//!
//! The small parsing helpers (`ends_word`, `identifiers_of`) mirror
//! the bridge's: this crate sits below the bridge in the DAG, so
//! sharing is impossible without inverting a dependency, and the two
//! grammars are free to diverge (this one is Celerrate's own to
//! evolve). Malformed content yields fewer identifiers or no
//! directive, never an error.

use crate::comment_directives::{
    CommentDirective, CommentDirectiveProvider, CommentKind, DirectiveOrigin, DirectiveScope,
    SuppressionIdentifier,
};

/// The written tag.
pub const NATIVE_DIRECTIVE_TAG: &str = "@celerrate-ignore";

/// The core provider, registered unconditionally at the composition
/// root under the reserved core identity.
#[derive(Debug, Default)]
pub struct NativeDirectiveProvider;

impl CommentDirectiveProvider for NativeDirectiveProvider {
    fn directives(&self, kind: CommentKind, text: &str) -> Vec<CommentDirective> {
        native_directives(kind, text)
    }
}

/// Every native directive one comment carries, in written order. Total
/// over arbitrary input.
pub fn native_directives(kind: CommentKind, text: &str) -> Vec<CommentDirective> {
    let mut directives = Vec::new();
    let mut rest = text;
    while let Some(position) = rest.find('@') {
        let Some(tail) = rest.get(position..) else {
            break;
        };
        if let Some(after) = tail.strip_prefix(NATIVE_DIRECTIVE_TAG)
            && ends_word(after)
        {
            let scope = match kind {
                CommentKind::Docblock => DirectiveScope::AnnotatedDeclaration,
                _ => DirectiveScope::TrailingOrNextLine,
            };
            directives.push(CommentDirective::suppress(
                scope,
                DirectiveOrigin::Native,
                identifiers_of(after)
                    .into_iter()
                    .map(SuppressionIdentifier::native)
                    .collect(),
            ));
        }
        // `@` is ASCII: one past it is always a character boundary.
        rest = rest.get(position + 1..).unwrap_or("");
    }
    directives
}

/// A tag ends at a word boundary: the end of the comment, whitespace,
/// or a closing `*/`. `@celerrate-ignored` is prose, not a directive.
fn ends_word(after: &str) -> bool {
    after.is_empty()
        || after.starts_with(|character: char| character.is_whitespace())
        || after.starts_with("*/")
}

/// The identifier list after the tag: the rest of that line, the
/// parenthesized reason trailer dropped, the closing `*/` dropped,
/// comma-separated, trimmed of whitespace and docblock decoration.
fn identifiers_of(after_tag: &str) -> Vec<String> {
    let mut line = after_tag.lines().next().unwrap_or("");
    if let Some((before, _)) = line.split_once("*/") {
        line = before;
    }
    if let Some((before, _)) = line.split_once('(') {
        line = before;
    }
    line.split(',')
        .map(|identifier| identifier.trim().trim_matches('*').trim())
        .filter(|identifier| !identifier.is_empty())
        .map(str::to_owned)
        .collect()
}
```

Export from `lib.rs`: `pub use native_directive::{NATIVE_DIRECTIVE_TAG, NativeDirectiveProvider, native_directives};`.

- [ ] **Step 4: Run the unit tests**

Run: `cargo test -p celerrate_semantics native_directive`
Expected: PASS.

- [ ] **Step 5: Register at the composition root, with failing product tests first**

Append to `crates/celerrate_cli/tests/suppressions.rs`:

```rust
#[test]
fn a_trailing_native_directive_suppresses_exactly_its_codes_on_its_line() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); // @celerrate-ignore CEL0018\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("CEL0018"), "{text}");
    assert!(text.contains("CEL0019"), "{text}");
}

#[test]
fn a_native_directive_alone_on_its_line_targets_the_next_line() {
    let root = project(&[(
        "a.php",
        "<?php\n// @celerrate-ignore CEL0018\nnew MissingOne();\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn a_native_docblock_directive_covers_the_annotated_declaration() {
    let root = project(&[(
        "a.php",
        "<?php\n/** @celerrate-ignore CEL0018 */\nclass Service {\n    public function boot() { new MissingOne(); }\n}\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn a_native_reason_trailer_is_honored() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @celerrate-ignore CEL0018 (legacy fixture class)\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn an_unknown_native_identifier_suppresses_nothing() {
    // The typo does not widen: CEL0018 stays reported. Its CEL0041
    // warning arrives with the reporting phase (a later task).
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @celerrate-ignore CEL9999\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0018"), "{text}");
}

#[test]
fn co_located_native_and_foreign_directives_union() {
    // Two separate comments on one line: the native identifier list is
    // comma-separated and runs to the end of its line, so the foreign
    // directive must live in its own comment to keep both parses clean.
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); /* @celerrate-ignore CEL0018 */ // @phpstan-ignore function.notFound\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}
```

Run: `cargo test -p celerrate_cli --test suppressions`
Expected: the six new tests FAIL (no native provider is registered).

In `crates/celerrate_cli/src/plugins.rs`, at the top of `register_plugins`'s registration section (before the bridge block):

```rust
    // The native directive provider: core, registered unconditionally,
    // under the reserved core identity, outside the admitted set - it
    // never keys the plugin-set digest; binary identity already keys
    // the cache for core behavior (design sections 2 and 8).
    comment_directives.push(celerrate_semantics::CommentDirectiveRegistration {
        identity: core_identity(),
        provider: Arc::new(celerrate_semantics::NativeDirectiveProvider),
    });
```

Update `the_composition_root_registers_the_bridge_in_every_registry_it_serves`: the comment-directive registry now holds 2 registrations, index 0 named `celerrate-core`, index 1 named `phpdoc-bridge`. Add a test mirroring the core-rules digest discipline:

```rust
#[test]
fn the_native_directive_provider_never_enters_the_admitted_plugin_set() {
    let database = AnalysisDatabase::default();
    let plugins = register_plugins(&database);
    assert!(
        plugins
            .admitted
            .iter()
            .all(|identity| identity.name != celerrate_rules::CORE_IDENTITY_NAME)
    );
    let registry =
        celerrate_semantics::CommentDirectiveRegistry::try_get(&database).unwrap();
    assert_eq!(
        registry.registrations(&database)[0].identity.name,
        celerrate_rules::CORE_IDENTITY_NAME,
    );
}
```

- [ ] **Step 6: Run the workspace tests**

Run: `cargo test --workspace`
Expected: PASS, the six product tests included.

- [ ] **Step 7: Document the native directive**

In `docs/diagnostics.md`, add a "Suppressing diagnostics" section (before the identifier tables): the native form `// @celerrate-ignore CEL0030, CEL0031 (reason)`, its three comment kinds and scopes (trailing → its line; alone → the next line; docblock → the annotated declaration), mandatory identifiers with no blanket form, the reason trailer, and a pointer to `docs/phpdoc-bridge.md` for the foreign forms.

- [ ] **Step 8: Commit**

```bash
git add crates/celerrate_semantics crates/celerrate_cli docs/diagnostics.md
git commit -m "✨ feat(semantics): add the native @celerrate-ignore directive"
```

---

### Task 6: Match-outcome threading through the composition

The two filtered halves start answering which directives they used. Pure reshape: `FilteredPortion` replaces the bare `Vec<Diagnostic>` returns; every composed set stays byte-identical; the matched sets have no consumer until tasks 7 and 9.

**Files:**

- Modify: `crates/celerrate_cli/src/analysis.rs`
- Modify: `crates/celerrate_cli/src/cache/mod.rs`
- Modify: `crates/celerrate_cli/tests/cache_suppression.rs`, `tests/cache_equivalence.rs` (call-site updates only)

**Interfaces:**

- Consumes: task 2's `retain_unsuppressed` return value.
- Produces: `pub struct FilteredPortion { pub diagnostics: Vec<Diagnostic>, pub matched: Vec<u32> }`; `persistable_diagnostics` and `typed_portion` return it; `served_typed_diagnostics(inputs, file, typed_source) -> FilteredPortion` (its served arm's `matched` is empty until task 7 stores typed match indexes - a documented placeholder, invisible because nothing consumes it yet). `composed_diagnostics` keeps its `Vec<Diagnostic>` signature.

- [ ] **Step 1: Write the failing test (attribution through a portion)**

In `analysis.rs`'s test module (drive it through the product pipeline, the file's own style):

```rust
#[test]
fn a_portion_names_every_directive_that_admitted_a_diagnostic() {
    use crate::session::Session;

    // Line 2's one comment carries two directives (the foreign tag
    // first: the native identifier list runs to the end of the line,
    // so it must come last): the native one admits CEL0018, the
    // foreign blanket admits everything - any-match marks both. The
    // directive on line 4 admits nothing.
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a.php"),
        "<?php\nnew MissingOne(); // @phpstan-ignore-line @celerrate-ignore CEL0018\n$x = 1;\n// @celerrate-ignore CEL0019\n$y = 2;\n",
    )
    .unwrap();

    let session = Session::start(root.path());
    let inputs = session.inputs();
    let &file = session.sources.values().next().unwrap();
    let portion = super::persistable_diagnostics(&inputs, file);
    assert!(portion.diagnostics.is_empty(), "{:?}", portion.diagnostics);

    let directives =
        celerrate_semantics::suppression_directives(&inputs.database, file);
    assert_eq!(directives.len(), 3, "{directives:?}");
    assert_eq!(portion.matched, vec![0, 1], "{directives:?}");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p celerrate_cli a_portion_names_every_directive`
Expected: compile error (`persistable_diagnostics` answers `Vec<Diagnostic>`, no `matched` field).

- [ ] **Step 3: Implement the reshape**

In `analysis.rs`:

```rust
/// One filtered half of a file's diagnostics: what survived the
/// directive filter, and the sorted indexes (into
/// `suppression_directives(db, file)`) of every directive that
/// admitted at least one diagnostic of this half. Halves keep their
/// own matched sets because they are served independently on a
/// partial cache hit; the reporting phase consumes the union.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilteredPortion {
    pub diagnostics: Vec<Diagnostic>,
    pub matched: Vec<u32>,
}
```

- `persistable_diagnostics` and `typed_portion` build their diagnostics exactly as today, then end with:

```rust
    let directives = celerrate_semantics::suppression_directives(database, file);
    let matched = if directives.is_empty() {
        Vec::new()
    } else {
        retain_unsuppressed(database, file, directives, &mut diagnostics)
    };
    FilteredPortion {
        diagnostics,
        matched,
    }
}
```

- `served_typed_diagnostics` returns `FilteredPortion`; its served arm answers `FilteredPortion { diagnostics, matched: Vec::new() }` with the comment `// The stored typed match indexes arrive with cache schema 7 (the next task); nothing consumes matched before then.`; its recompute arm returns `typed_portion(inputs, file)` unchanged (the statistics calls stay exactly where they are).
- `composed_diagnostics` becomes:

```rust
pub fn composed_diagnostics(inputs: &AnalysisInputs, file: SourceFile) -> Vec<Diagnostic> {
    let mut portion = persistable_diagnostics(inputs, file);
    portion.diagnostics.extend(typed_portion(inputs, file).diagnostics);
    portion.diagnostics.sort();
    portion.diagnostics
}
```

- `analyze_one`'s three untyped arms wrap or unwrap accordingly (`persistable_diagnostics(inputs, file).diagnostics` on the fallback arms; the hit arm is untouched); its typed layering becomes `diagnostics.extend(served_typed_diagnostics(inputs, file, typed_source).diagnostics);`.
- `cache/mod.rs`: `composed_verdict_with_lever` reads `crate::analysis::persistable_diagnostics(inputs, file)` into a local `portion` and maps `portion.diagnostics`; `composed_typed_verdict` likewise (`portion.matched` is consumed in task 7 - bind it as `let FilteredPortion { diagnostics, matched: _ } = ...` for now so task 7's diff is small).
- Test call sites: `cache_equivalence.rs`'s `served.extend(served_typed_diagnostics(&inputs, file, typed_source));` becomes `.extend(served_typed_diagnostics(&inputs, file, typed_source).diagnostics);`; no other test touches the changed signatures directly (`cache_suppression.rs` uses `composed_diagnostics`, unchanged).

- [ ] **Step 4: Run the workspace tests**

Run: `cargo test --workspace`
Expected: PASS (byte-identical composition; the new attribution test green).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli
git commit -m "♻️ refactor(cli): thread directive match outcomes through the composition"
```

---

### Task 7: Stored directive records - cache schema 7

The per-directive match records join the stored verdict so the `Reporting` phase can run parse-free on the warm path. The untyped half stores each directive with its `matched` flag; the typed half stores its admitting indexes. Bounds and interning are validated on load like every stored range; any mismatch discards the verdict.

**Files:**

- Modify: `crates/celerrate_cli/src/cache/stored.rs`
- Modify: `crates/celerrate_cli/src/cache/pack.rs`
- Modify: `crates/celerrate_cli/src/cache/mod.rs`
- Modify: `crates/celerrate_cli/src/analysis.rs`
- Modify: `crates/celerrate_cli/tests/cache_suppression.rs`

**Interfaces:**

- Consumes: task 6's `FilteredPortion`; `celerrate_semantics::{ResolvedDirective, SuppressionFilter, DirectiveOrigin, suppression_directives}`.
- Produces: `StoredSuppressionFilter`, `StoredDirective` with `of(directive, matched)` and `to_directive(content_length) -> Option<(ResolvedDirective, bool)>`; `StoredVerdict.directives: Vec<StoredDirective>`; `StoredTypedVerdict.matched_directives: Vec<u32>`; `StoredVerdict::directives_convert(&self, content_length) -> Option<Vec<(ResolvedDirective, bool)>>` (also checks every typed `matched_directives` index is in range); `CACHE_SCHEMA_VERSION = 7`. Task 9 consumes `directives_convert` on the hit path.

- [ ] **Step 1: Write the failing unit tests**

In `stored.rs`'s test module:

```rust
#[test]
fn a_directive_record_round_trips() {
    let directive = celerrate_semantics::ResolvedDirective {
        anchor: TextRange::new(TextSize::from(10), TextSize::from(40)),
        scope: TextRange::new(TextSize::from(6), TextSize::from(41)),
        filter: SuppressionFilter::Only(vec![
            celerrate_diagnostics::find_identifier("CEL0018").unwrap(),
        ]),
        identifiers: vec!["CEL0018".to_owned()],
        origin: DirectiveOrigin::Native,
    };
    let stored = StoredDirective::of(&directive, true);
    assert_eq!(stored.to_directive(100), Some((directive, true)));
}

#[test]
fn a_directive_record_with_an_out_of_bounds_range_is_discarded() {
    let directive = celerrate_semantics::ResolvedDirective {
        anchor: TextRange::new(TextSize::from(10), TextSize::from(40)),
        scope: TextRange::new(TextSize::from(6), TextSize::from(41)),
        filter: SuppressionFilter::All,
        identifiers: Vec::new(),
        origin: DirectiveOrigin::Foreign,
    };
    let stored = StoredDirective::of(&directive, false);
    assert!(stored.to_directive(20).is_none());
}

#[test]
fn a_directive_record_with_an_unknown_filter_code_is_discarded() {
    let stored = StoredDirective {
        anchor_start: 0,
        anchor_end: 5,
        scope_start: 0,
        scope_end: 5,
        filter: StoredSuppressionFilter::Only(vec!["CEL9999".to_owned()]),
        identifiers: vec!["CEL9999".to_owned()],
        native: true,
        matched: false,
    };
    assert!(stored.to_directive(100).is_none());
}
```

And a product-level pin appended to `cache_suppression.rs`:

```rust
#[test]
fn the_pack_stores_the_directive_match_records() {
    let root = project(&[("a.php", SUPPRESSED_AND_NOT)]);
    check(root.path());

    let session = Session::start(root.path());
    let inputs = session.inputs();
    let &file = session.sources.values().next().unwrap();
    let VerdictLookup::Hit { verdict: stored, .. } = lookup_verdict(&inputs, file) else {
        panic!("the persisted verdict must revalidate on an unchanged project");
    };
    let content_length =
        u32::try_from(file.bytes(&inputs.database).len()).unwrap_or(0);
    let records = stored
        .directives_convert(content_length)
        .expect("stored directive records convert");
    assert_eq!(records.len(), 1);
    let (directive, matched) = &records[0];
    assert!(*matched, "the ignore-line directive admitted MissingOne");
    assert_eq!(
        records
            .iter()
            .map(|(directive, matched)| (directive.clone(), *matched))
            .collect::<Vec<_>>(),
        celerrate_semantics::suppression_directives(&inputs.database, file)
            .iter()
            .cloned()
            .map(|fresh| (fresh, true))
            .collect::<Vec<_>>(),
        "stored records equal the query plus the match outcome",
    );
    let _ = directive;
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_cli stored` (compile errors: the types do not exist), and the product test fails to compile too.

- [ ] **Step 3: Implement the stored forms**

In `stored.rs` (imports grow `DirectiveOrigin, ResolvedDirective, SuppressionFilter` from `celerrate_semantics`):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredSuppressionFilter {
    All,
    Only(Vec<String>),
}

/// One resolved directive with its untyped-half match outcome: what
/// the `Reporting` phase replays on the warm path without re-parsing
/// (design section 4). The typed half's own outcomes live in
/// `StoredTypedVerdict.matched_directives`, indexes into this list, so
/// a recomputed typed half never serves a stale union.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDirective {
    pub anchor_start: u32,
    pub anchor_end: u32,
    pub scope_start: u32,
    pub scope_end: u32,
    pub filter: StoredSuppressionFilter,
    pub identifiers: Vec<String>,
    pub native: bool,
    /// Whether the untyped half's filter admitted any diagnostic.
    pub matched: bool,
}

impl StoredDirective {
    pub fn of(directive: &ResolvedDirective, matched: bool) -> Self {
        Self {
            anchor_start: directive.anchor.start().into(),
            anchor_end: directive.anchor.end().into(),
            scope_start: directive.scope.start().into(),
            scope_end: directive.scope.end().into(),
            filter: match &directive.filter {
                SuppressionFilter::All => StoredSuppressionFilter::All,
                SuppressionFilter::Only(codes) => StoredSuppressionFilter::Only(
                    codes.iter().map(|code| code.as_str().to_owned()).collect(),
                ),
            },
            identifiers: directive.identifiers.clone(),
            native: directive.origin == DirectiveOrigin::Native,
            matched,
        }
    }

    /// `None` when a range is inverted or out of bounds, or a filter
    /// code no longer interns: another era's record, discarded like a
    /// failed diagnostic conversion - the checksum proves transport,
    /// never honesty.
    pub fn to_directive(&self, content_length: u32) -> Option<(ResolvedDirective, bool)> {
        let in_bounds = |start: u32, end: u32| start <= end && end <= content_length;
        if !in_bounds(self.anchor_start, self.anchor_end)
            || !in_bounds(self.scope_start, self.scope_end)
        {
            return None;
        }
        let filter = match &self.filter {
            StoredSuppressionFilter::All => SuppressionFilter::All,
            StoredSuppressionFilter::Only(codes) => {
                let mut interned = Vec::with_capacity(codes.len());
                for code in codes {
                    interned.push(find_identifier(code)?);
                }
                SuppressionFilter::Only(interned)
            }
        };
        Some((
            ResolvedDirective {
                anchor: TextRange::new(
                    TextSize::from(self.anchor_start),
                    TextSize::from(self.anchor_end),
                ),
                scope: TextRange::new(
                    TextSize::from(self.scope_start),
                    TextSize::from(self.scope_end),
                ),
                filter,
                identifiers: self.identifiers.clone(),
                origin: if self.native {
                    DirectiveOrigin::Native
                } else {
                    DirectiveOrigin::Foreign
                },
            },
            self.matched,
        ))
    }
}
```

`StoredTypedVerdict` gains `pub matched_directives: Vec<u32>`; `StoredVerdict` gains `pub directives: Vec<StoredDirective>` and:

```rust
impl StoredVerdict {
    /// Every stored directive record converted, in stored order, with
    /// the typed half's indexes checked against the list's length.
    /// `None` means the whole verdict is untrustworthy.
    pub fn directives_convert(
        &self,
        content_length: u32,
    ) -> Option<Vec<(ResolvedDirective, bool)>> {
        let records: Option<Vec<_>> = self
            .directives
            .iter()
            .map(|directive| directive.to_directive(content_length))
            .collect();
        let records = records?;
        if let Some(typed) = &self.typed
            && !typed
                .matched_directives
                .iter()
                .all(|&index| (index as usize) < records.len())
        {
            return None;
        }
        Some(records)
    }
}
```

`pack.rs`: `pub const CACHE_SCHEMA_VERSION: u32 = 7;` (the version-stamped format invalidates on the bump; no migration shim - design section 3). Update the schema comment in `stored.rs`'s module doc ("schema 6" → "schema 7", noting the directive records).

- [ ] **Step 4: Wire the persist and reuse paths**

In `cache/mod.rs`:

- `composed_verdict_with_lever`: bind `let portion = crate::analysis::persistable_diagnostics(inputs, file);` and `let directives = celerrate_semantics::suppression_directives(database, file);`, then:

```rust
    StoredVerdict {
        diagnostics: portion.diagnostics.iter().map(StoredDiagnostic::of).collect(),
        records: records.iter().map(StoredRecord::of).collect(),
        directives: directives
            .iter()
            .enumerate()
            .map(|(index, directive)| {
                let matched = u32::try_from(index)
                    .map(|index| portion.matched.binary_search(&index).is_ok())
                    .unwrap_or(false);
                StoredDirective::of(directive, matched)
            })
            .collect(),
        typed,
    }
```

- `composed_typed_verdict`: bind the portion, use `portion.diagnostics` for the stored diagnostics and `matched_directives: portion.matched` in the struct.
- `collect_entries`' verbatim-reuse guard grows the record check - the arm's condition becomes:

```rust
            } if stored
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.to_diagnostic(file_id, content_length).is_some())
                && stored.directives_convert(content_length).is_some() =>
```

In `analysis.rs`, `served_typed_diagnostics`' served arm now answers the stored indexes: `FilteredPortion { diagnostics, matched: typed.matched_directives.clone() }` (delete the task-6 placeholder comment).

Sweep the workspace for literal `StoredVerdict`/`StoredTypedVerdict` constructions outside `composed_verdict_with_lever` - the seeding-attack suite (`crates/celerrate_cli/tests/cache_seeding.rs`) and the cache module's own tests build them by hand - and give each the new fields (`directives: Vec::new()`, `matched_directives: Vec::new()`) unless the test is about the records themselves.

- [ ] **Step 5: Run the workspace tests**

Run: `cargo test --workspace`
Expected: PASS (the new unit and product tests included; the schema bump makes every pre-existing local pack a cold miss, which the suites already tolerate).

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cli): persist per-directive match records in the stored verdict"
```

---

### Task 8: CEL0041 and CEL0042 - the two directive rules and the `Reporting` phase runner

The registry grows two identifiers; `ReportingContext` gets its real surface; the two rules and the one-pass suppression algorithm (decision 10) are built and unit-tested against synthetic records. Nothing is CLI-wired yet.

**Files:**

- Modify: `crates/celerrate_diagnostics/src/registry.rs`
- Modify: `docs/diagnostics.md`
- Create: `crates/celerrate_rules/src/rules/unknown_suppression_identifier.rs`
- Create: `crates/celerrate_rules/src/rules/unused_suppression.rs`
- Modify: `crates/celerrate_rules/src/context.rs`, `src/finding.rs`, `src/phases.rs`, `src/rules/mod.rs`, `src/lib.rs`

**Interfaces:**

- Consumes: `ResolvedDirective` (task 2), `RuleRegistry`, `FindingSink`, `resolved_diagnostic`'s `Range` arm.
- Produces: `DirectiveOutcome { pub directive: ResolvedDirective, pub matched: bool }` (in `celerrate_rules::context`); `ReportingContext::new(outcomes, inactive)` with methods `outcomes()`, `is_known(written)`, `is_inactive(written)`; `Finding.subject: Option<u32>` and `FindingSink::report_directive(identifier, subject, anchor, message)` (crate-internal); `reporting_phase_diagnostics(db, file_id, text_end, outcomes) -> Vec<Diagnostic>` (a plain public function, NOT a salsa query - decision 9); constants `unknown_suppression_identifier::UNKNOWN_SUPPRESSION_IDENTIFIER` (CEL0041) and `unused_suppression::UNUSED_SUPPRESSION` (CEL0042). Task 9 calls `reporting_phase_diagnostics` from the CLI. Nothing here enters the plugin facade (`ReportingRule` stays core-only).

- [ ] **Step 1: Allocate the identifiers (failing ledger first)**

In `crates/celerrate_diagnostics/src/registry.rs`, append to `REGISTRY`:

```rust
    registered(
        "CEL0041",
        "unknown suppression identifier",
        "celerrate_rules",
    ),
    registered("CEL0042", "unused suppression", "celerrate_rules"),
```

Update the gapless test's terminal assertion: `assert_eq!(previous, 42, "forty-two identifiers allocated so far");`. Run `cargo test --workspace`: the `documentation.rs` gate now FAILS (CEL0041/CEL0042 undocumented) - that is the red test. Add to `docs/diagnostics.md`, after the argument-types section:

```markdown
## Suppression directives (CEL0041, CEL0042)

About Celerrate's own `@celerrate-ignore` directive (never about
foreign directives, which legitimately target diagnostics Celerrate
does not emit).

| Identifier | Severity | Meaning |
| --- | --- | --- |
| CEL0041 | warning | a `@celerrate-ignore` directive names an identifier Celerrate does not know (a typo must not silently suppress nothing) |
| CEL0042 | warning | a `@celerrate-ignore` directive suppressed nothing (exempt when it names an identifier of a rule not active in this run) |
```

Run `cargo test -p celerrate_diagnostics && cargo test -p celerrate_cli --test documentation`: green. The composition-root ledger (`crates/celerrate_cli/tests/registry.rs`) may now be RED if it cross-checks registry entries against each owner's `ALLOCATED_IDENTIFIERS` - that red is this task's TDD driver for the rules half and clears in step 4 when the two constants join the list.

- [ ] **Step 2: Write the failing rule and runner tests**

In a new test module at the bottom of `crates/celerrate_rules/src/phases.rs` (or extend the existing one), driving `reporting_phase_diagnostics` with synthetic records against the registered core rules:

```rust
    // ---- Reporting phase ----

    use crate::context::DirectiveOutcome;
    use crate::phases::reporting_phase_diagnostics;
    use crate::rules::{unknown_suppression_identifier, unused_suppression};
    use celerrate_semantics::{DirectiveOrigin, ResolvedDirective, SuppressionFilter};

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
        let codes = identifiers
            .iter()
            .filter_map(|written| celerrate_diagnostics::find_identifier(written))
            .collect();
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
        assert!(unknown[0].message.contains("CEL9999"), "{}", unknown[0].message);
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
        assert_eq!(range, TextRange::new(TextSize::from(10), TextSize::from(40)));
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
    fn a_suppressed_directive_diagnostic_counts_as_use_in_one_pass() {
        // Directive A (indexes 0) is unused: its CEL0042 would fire at
        // its anchor. Directive B admits CEL0042 on a scope covering
        // A's anchor: A's warning is dropped, B counts as used, and
        // B's own CEL0042 is dropped by the subject rule - one pass,
        // no iteration (decision 10).
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
```

(`NullSyntaxRule` mirrors the one in `registry.rs`'s tests; define it locally.)

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p celerrate_rules`
Expected: compile errors (`DirectiveOutcome`, the rule modules, `reporting_phase_diagnostics` do not exist).

- [ ] **Step 4: Implement the context, sink extension, rules, and runner**

`crates/celerrate_rules/src/context.rs` - replace the placeholder:

```rust
/// One directive with its final match outcome: the union of both
/// halves', composed by the orchestration layer (stored records on a
/// warm hit, the resolution query on a miss).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveOutcome {
    pub directive: celerrate_semantics::ResolvedDirective,
    pub matched: bool,
}

/// The `Reporting` phase context: directives and their match outcomes
/// (design section 4). Core-only - never re-exported by the facade.
/// Plain outcome data plus two registry questions; no database handle,
/// no tree, no parse: the same context serves the warm path.
pub struct ReportingContext<'run> {
    outcomes: &'run [DirectiveOutcome],
    inactive: &'run std::collections::BTreeSet<celerrate_diagnostics::DiagnosticId>,
}

impl<'run> ReportingContext<'run> {
    pub(crate) fn new(
        outcomes: &'run [DirectiveOutcome],
        inactive: &'run std::collections::BTreeSet<celerrate_diagnostics::DiagnosticId>,
    ) -> Self {
        Self { outcomes, inactive }
    }

    /// Every directive of the file, in resolution order, with its
    /// final match outcome.
    pub fn outcomes(&self) -> &'run [DirectiveOutcome] {
        self.outcomes
    }

    /// Whether the written form names a registered identifier - the
    /// CEL0041 knownness question, asked through the single lookup
    /// seam (`find_identifier`; design section 8's two-tier shape).
    pub fn is_known(&self, written: &str) -> bool {
        celerrate_diagnostics::find_identifier(written).is_some()
    }

    /// Whether the written form names an identifier of a rule outside
    /// the active set - the CEL0042 exemption question. Resilience
    /// identifiers are claimed by no rule and are never inactive.
    pub fn is_inactive(&self, written: &str) -> bool {
        celerrate_diagnostics::find_identifier(written)
            .is_some_and(|id| self.inactive.contains(&id))
    }
}
```

`crates/celerrate_rules/src/finding.rs`: `Finding` gains `pub subject: Option<u32>` (crate-visible; `report` sets `subject: None`), and the sink gains:

```rust
    /// A reporting-phase emission naming its subject directive, so the
    /// one-pass suppression can attribute a drop to it (the CEL0042
    /// discipline). Crate-internal: only core reporting rules exist.
    pub(crate) fn report_directive(
        &mut self,
        identifier: DiagnosticId,
        subject: u32,
        anchor: TextRange,
        message: String,
    ) {
        let Some(severity) = self.metadata.severity_of(identifier) else {
            return;
        };
        self.findings.push(Finding {
            identifier,
            severity,
            anchor: FindingAnchor::Range(anchor),
            message,
            subject: Some(subject),
        });
    }
```

`crates/celerrate_rules/src/rules/unknown_suppression_identifier.rs`:

```rust
//! CEL0041: a typo in a CEL code must not silently suppress nothing.
//! Native directives only; a known but inactive identifier is not
//! unknown (design section 8).

use celerrate_diagnostics::{DiagnosticId, Severity};
use celerrate_semantics::DirectiveOrigin;

use crate::context::ReportingContext;
use crate::finding::FindingSink;
use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
use crate::traits::ReportingRule;

pub const UNKNOWN_SUPPRESSION_IDENTIFIER: DiagnosticId = DiagnosticId::new("CEL0041");

pub fn metadata() -> RuleMetadata {
    RuleMetadata {
        name: "unknown-suppression-identifier".to_owned(),
        group: RuleGroup::Correctness,
        identifiers: vec![RuleIdentifier {
            id: UNKNOWN_SUPPRESSION_IDENTIFIER,
            severity: Severity::Warning,
        }],
        tier: Tier::Default,
    }
}

pub struct UnknownSuppressionIdentifier;

impl ReportingRule for UnknownSuppressionIdentifier {
    fn check(&self, context: &ReportingContext<'_>, sink: &mut FindingSink<'_>) {
        for (index, outcome) in context.outcomes().iter().enumerate() {
            if outcome.directive.origin != DirectiveOrigin::Native {
                continue;
            }
            let Ok(subject) = u32::try_from(index) else {
                continue;
            };
            for written in &outcome.directive.identifiers {
                if !context.is_known(written) {
                    sink.report_directive(
                        UNKNOWN_SUPPRESSION_IDENTIFIER,
                        subject,
                        outcome.directive.anchor,
                        format!(
                            "unknown diagnostic identifier `{written}` in a @celerrate-ignore directive"
                        ),
                    );
                }
            }
        }
    }
}
```

`crates/celerrate_rules/src/rules/unused_suppression.rs`:

```rust
//! CEL0042: a native directive that suppressed nothing. Exempt (not
//! evaluable) when any identifier belongs to an inactive rule - the
//! nursery-demotion storm guard - or is unknown (CEL0041 already
//! reports that mistake; decision 11 of the part-5 plan).

use celerrate_diagnostics::{DiagnosticId, Severity};
use celerrate_semantics::DirectiveOrigin;

use crate::context::ReportingContext;
use crate::finding::FindingSink;
use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
use crate::traits::ReportingRule;

pub const UNUSED_SUPPRESSION: DiagnosticId = DiagnosticId::new("CEL0042");

pub fn metadata() -> RuleMetadata {
    RuleMetadata {
        name: "unused-suppression".to_owned(),
        group: RuleGroup::Correctness,
        identifiers: vec![RuleIdentifier {
            id: UNUSED_SUPPRESSION,
            severity: Severity::Warning,
        }],
        tier: Tier::Default,
    }
}

pub struct UnusedSuppression;

impl ReportingRule for UnusedSuppression {
    fn check(&self, context: &ReportingContext<'_>, sink: &mut FindingSink<'_>) {
        for (index, outcome) in context.outcomes().iter().enumerate() {
            if outcome.directive.origin != DirectiveOrigin::Native || outcome.matched {
                continue;
            }
            let Ok(subject) = u32::try_from(index) else {
                continue;
            };
            let evaluable = outcome.directive.identifiers.iter().all(|written| {
                context.is_known(written) && !context.is_inactive(written)
            });
            if !evaluable {
                continue;
            }
            sink.report_directive(
                UNUSED_SUPPRESSION,
                subject,
                outcome.directive.anchor,
                "this @celerrate-ignore directive suppressed nothing".to_owned(),
            );
        }
    }
}
```

`crates/celerrate_rules/src/phases.rs` - the runner (decision 10's algorithm, verbatim):

```rust
/// The reporting phase: runs the registered `Reporting` rules from
/// per-directive match outcomes - never from the tree, so the warm
/// path serves the same records parse-free (design section 4). A plain
/// function, not a salsa query: its input is composed by the
/// orchestration layer, which is also why the output is recomputed on
/// both paths rather than persisted. Deterministic by construction (a
/// pure function of the registry and the outcomes).
///
/// The one additional, non-iterated suppression pass: (a) rules emit
/// findings, a CEL0042 finding naming its subject directive; (b) one
/// pass drops every finding some directive admits and marks every
/// admitting directive used; (c) findings whose subject became used in
/// (b) are dropped. Uses recorded in (b) never re-open (b), and drops
/// in (c) never un-use anything: no fixpoint.
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
        let FindingAnchor::Range(range) = finding.anchor else {
            continue;
        };
        let mut suppressed = false;
        for (index, outcome) in outcomes.iter().enumerate() {
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
```

(`TextSize` joins `phases.rs`'s imports.) `rules/mod.rs`: declare both modules, append `unknown_suppression_identifier::UNKNOWN_SUPPRESSION_IDENTIFIER` and `unused_suppression::UNUSED_SUPPRESSION` to `ALLOCATED_IDENTIFIERS`, append both registrations to `core_rules()`:

```rust
        (
            unknown_suppression_identifier::metadata(),
            RuleImplementation::Reporting(Arc::new(
                unknown_suppression_identifier::UnknownSuppressionIdentifier,
            )),
        ),
        (
            unused_suppression::metadata(),
            RuleImplementation::Reporting(Arc::new(unused_suppression::UnusedSuppression)),
        ),
```

`lib.rs`: export `context::DirectiveOutcome` and `phases::reporting_phase_diagnostics` (alongside the existing exports; still nothing reporting-related goes to `celerrate_plugin`).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p celerrate_rules && cargo test --workspace`
Expected: PASS (the composition-root ledger and `core_rules` validation absorb the two new registrations; `validate_rules` confirms no conflict).

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_diagnostics crates/celerrate_rules docs/diagnostics.md
git commit -m "✨ feat(rules): report unknown and unused native suppressions (CEL0041, CEL0042)"
```

---

### Task 9: CLI wiring - the reporting phase on both paths

`analyze_one` and `composed_diagnostics` append the reporting portion, built from stored records on a hit and from the query on a miss; the equivalence harness layers it the same way through the same shared helper. The CEL0041/CEL0042 product matrix goes green.

**Files:**

- Modify: `crates/celerrate_cli/src/analysis.rs`
- Modify: `crates/celerrate_cli/tests/cache_equivalence.rs`
- Create: `crates/celerrate_cli/tests/directive_rules.rs`

**Interfaces:**

- Consumes: task 7's `directives_convert`, task 8's `reporting_phase_diagnostics` and `DirectiveOutcome`.
- Produces: `pub fn directive_outcomes(directives: &[(ResolvedDirective, bool)], matched_typed: &[u32]) -> Vec<DirectiveOutcome>` and `pub fn reporting_portion(inputs, file, outcomes: &[DirectiveOutcome]) -> Vec<Diagnostic>` in `analysis.rs`, shared by `analyze_one`, `composed_diagnostics`, and the harness.

- [ ] **Step 1: Write the failing product tests**

Create `crates/celerrate_cli/tests/directive_rules.rs` (the `project`/`check` helpers copied from `suppressions.rs`):

```rust
//! The CEL0041/CEL0042 product matrix: the directive rules through
//! the full pipeline, native directives only, one-pass suppression
//! discipline (design sections 8 and 11).

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::{Outcome, run};

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let path = root.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    root
}

fn check(root: &Path) -> (Outcome, String) {
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()],
        &mut output,
    );
    (outcome, String::from_utf8(output).unwrap())
}

#[test]
fn a_typo_in_a_native_directive_reports_cel0041_and_the_finding_survives() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @celerrate-ignore CEL0019, CEL9999\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0041"), "{text}");
    assert!(text.contains("CEL9999"), "{text}");
    assert!(text.contains("CEL0018"), "{text}");
}

#[test]
fn an_unused_native_directive_reports_cel0042() {
    let root = project(&[(
        "a.php",
        "<?php\n$x = 1; // @celerrate-ignore CEL0018\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0042"), "{text}");
}

#[test]
fn a_used_native_directive_is_clean() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @celerrate-ignore CEL0018\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn a_bare_native_directive_reports_cel0042() {
    let root = project(&[("a.php", "<?php\n$x = 1; // @celerrate-ignore\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0042"), "{text}");
}

#[test]
fn an_unused_foreign_directive_is_never_reported() {
    let root = project(&[("a.php", "<?php\n$x = 1; // @phpstan-ignore-line\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn suppressing_a_directive_diagnostic_counts_as_use_of_the_suppressor() {
    // Line 3's directive suppressed nothing on its own scope; its
    // CEL0042 is suppressed by line 2's directive targeting the next
    // line with CEL0042 - which thereby counts as used and reports
    // nothing itself. One pass, clean run.
    let root = project(&[(
        "a.php",
        "<?php\n// @celerrate-ignore CEL0042\n$x = 1; // @celerrate-ignore CEL0018\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn cel0041_is_itself_suppressible() {
    // The foreign tag comes first (the native identifier list runs to
    // the end of the line). The foreign blanket admits the CEL0041
    // aimed at the typo, so the run is clean.
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore-line @celerrate-ignore CEL0018, CEL9999\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn any_match_attribution_marks_every_admitting_directive_used() {
    // Two co-located directives (separate comments) both admit the one
    // CEL0018: both are used, neither reports CEL0042.
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); /* @celerrate-ignore CEL0018 */ // @phpstan-ignore class.notFound\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_cli --test directive_rules`
Expected: `a_typo_...`, `an_unused_...`, `a_bare_...` FAIL (no reporting output reaches the report); the clean-path tests may already pass.

- [ ] **Step 3: Implement the shared reporting portion and wire the paths**

In `analysis.rs`:

```rust
/// Builds the reporting phase's input: converted directive records
/// (identity plus untyped matched flag) unioned with the typed half's
/// admitting indexes. Both cache paths and the recompute path funnel
/// through this one constructor so the union cannot drift.
pub fn directive_outcomes(
    directives: &[(celerrate_semantics::ResolvedDirective, bool)],
    matched_typed: &[u32],
) -> Vec<celerrate_rules::DirectiveOutcome> {
    directives
        .iter()
        .enumerate()
        .map(|(index, (directive, matched_untyped))| {
            let matched = *matched_untyped
                || u32::try_from(index)
                    .map(|index| matched_typed.binary_search(&index).is_ok())
                    .unwrap_or(false);
            celerrate_rules::DirectiveOutcome {
                directive: directive.clone(),
                matched,
            }
        })
        .collect()
}

/// The reporting portion of one file: the directive rules' output,
/// computed from final match outcomes on both the warm and the cold
/// path (design section 4). Shared by `analyze_one`,
/// `composed_diagnostics`, and the equivalence harness.
pub fn reporting_portion(
    inputs: &AnalysisInputs,
    file: SourceFile,
    outcomes: &[celerrate_rules::DirectiveOutcome],
) -> Vec<Diagnostic> {
    let database = &inputs.database;
    let file_id = file.file_id(database);
    let text_end = celerrate_db::source_text(database, file)
        .as_ref()
        .map(|text| TextSize::of(text.text()))
        .unwrap_or_default();
    celerrate_rules::reporting_phase_diagnostics(database, file_id, text_end, outcomes)
}
```

`composed_diagnostics` (the recompute truth, reporting included):

```rust
pub fn composed_diagnostics(inputs: &AnalysisInputs, file: SourceFile) -> Vec<Diagnostic> {
    let database = &inputs.database;
    let untyped = persistable_diagnostics(inputs, file);
    let typed = typed_portion(inputs, file);
    let fresh: Vec<(celerrate_semantics::ResolvedDirective, bool)> =
        celerrate_semantics::suppression_directives(database, file)
            .iter()
            .enumerate()
            .map(|(index, directive)| {
                let matched = u32::try_from(index)
                    .map(|index| untyped.matched.binary_search(&index).is_ok())
                    .unwrap_or(false);
                (directive.clone(), matched)
            })
            .collect();
    let outcomes = directive_outcomes(&fresh, &typed.matched);
    let mut diagnostics = untyped.diagnostics;
    diagnostics.extend(typed.diagnostics);
    diagnostics.extend(reporting_portion(inputs, file, &outcomes));
    diagnostics.sort();
    diagnostics
}
```

`analyze_one`: the hit arm's reuse condition additionally requires `verdict.directives_convert(content_length)` to answer `Some` (bind it; a `None` joins the `verdicts_discarded` fallback, exactly like a failed diagnostic conversion). The tail becomes:

```rust
        let (mut diagnostics, typed_source, records) = match ... {
            // Hit arm: (diagnostics, typed_source, Some(converted_records))
            // Fallback arms: (persistable_diagnostics(...).diagnostics - no:
            //   bind the portion, keep .matched) ...
        };
```

Concretely, restructure so every arm produces `(Vec<Diagnostic>, Option<&StoredTypedVerdict>, Vec<(ResolvedDirective, bool)>)`: the hit arm answers the converted stored records; each fallback arm binds `let portion = persistable_diagnostics(inputs, file);` and derives the fresh pairs from the query exactly as `composed_diagnostics` does (extract that six-line derivation into a private helper `fresh_directive_records(inputs, file, matched_untyped: &[u32]) -> Vec<(ResolvedDirective, bool)>` used by both). Then:

```rust
        let typed = served_typed_diagnostics(inputs, file, typed_source);
        let outcomes = directive_outcomes(&records, &typed.matched);
        diagnostics.extend(typed.diagnostics);
        diagnostics.extend(reporting_portion(inputs, file, &outcomes));
        diagnostics.sort();
        diagnostics
```

- [ ] **Step 4: Extend the equivalence harness**

In `cache_equivalence.rs`'s `served_equals_recomputed`, after the typed layering and before the sort:

```rust
        let records = stored
            .directives_convert(content_length)
            .expect("a revalidated verdict's directive records all convert");
        let outcomes =
            celerrate_cli::analysis::directive_outcomes(&records, &typed_half.matched);
        served.extend(celerrate_cli::analysis::reporting_portion(
            &inputs, file, &outcomes,
        ));
```

To make `typed_half` available, change the harness's typed layering (task 6 left it as a direct `.diagnostics` chain) to a binding:

```rust
        let typed_half = served_typed_diagnostics(&inputs, file, typed_source);
        served.extend(typed_half.diagnostics.iter().cloned());
```

Add a fixture that actually exercises the phase:

```rust
/// A native directive that suppressed nothing: the served CEL0042
/// must equal the recomputed one, byte for byte, from the persisted
/// match records (the design's warm/cold Reporting gate).
#[test]
fn directive_diagnostics_replay_equal() {
    let identifiers = served_equals_recomputed(&[(
        "a.php",
        "<?php\n$x = 1; // @celerrate-ignore CEL0018\nnew MissingTwo();\n",
    )]);
    assert!(identifiers.contains("CEL0042"), "{identifiers:?}");
}
```

- [ ] **Step 5: Run the workspace tests**

Run: `cargo test --workspace`
Expected: PASS - the directive-rules matrix, the extended harness, and every pre-existing suite (`cache_suppression.rs`'s `served == composed_diagnostics` assertion still holds: its foreign-only fixtures produce an empty reporting portion).

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cli): run the reporting phase from match records on both paths"
```

---

### Task 10: The warm-path product pins and the suppression matrix completion

The remaining section-11 matrix rows: a warm run reports the same directive diagnostics parse-free (through the product pipeline, statistics-verified), a directive edit restores precision, and the partial-hit union stays honest.

**Files:**

- Modify: `crates/celerrate_cli/tests/cache_suppression.rs`

**Interfaces:**

- Consumes: everything wired in task 9; the existing `check`/`project` helpers and `Session` accessors in that file.
- Produces: product pins only.

- [ ] **Step 1: Write the failing warm-path tests**

Append to `cache_suppression.rs`:

```rust
#[test]
fn a_warm_run_reports_the_same_directive_diagnostics_from_the_records() {
    // Cold: the unused native directive reports CEL0042. Warm: the
    // verdict serves, the reporting phase replays from the stored
    // match records, and the report is byte-identical.
    let root = project(&[(
        "a.php",
        "<?php\n$x = 1; // @celerrate-ignore CEL0018\n",
    )]);
    let (cold_outcome, cold_text) = check(root.path());
    assert_eq!(cold_outcome, Outcome::DiagnosticsReported, "{cold_text}");
    assert!(cold_text.contains("CEL0042"), "{cold_text}");

    let (warm_outcome, warm_text) = check(root.path());
    assert_eq!(warm_outcome, Outcome::DiagnosticsReported, "{warm_text}");
    assert_eq!(cold_text, warm_text, "warm and cold reports must be byte-identical");
}

#[test]
fn the_warm_replay_serves_the_verdict_rather_than_recomputing() {
    // The parse-free claim, checked through the counters the session
    // reports: the second run's verdict is served, not recomputed, and
    // the CEL0042 still appears - so the reporting phase ran from the
    // stored records, since the stored diagnostics never contain it.
    let root = project(&[(
        "a.php",
        "<?php\n$x = 1; // @celerrate-ignore CEL0018\n",
    )]);
    check(root.path());

    let session = Session::start(root.path());
    let inputs = session.inputs();
    let &file = session.sources.values().next().unwrap();
    let VerdictLookup::Hit { verdict: stored, .. } = lookup_verdict(&inputs, file) else {
        panic!("the persisted verdict must revalidate on an unchanged project");
    };
    assert!(
        stored
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.id != "CEL0042"),
        "reporting diagnostics are never persisted; they replay from records",
    );
    let (warm_outcome, warm_text) = check(root.path());
    assert_eq!(warm_outcome, Outcome::DiagnosticsReported, "{warm_text}");
    assert!(warm_text.contains("CEL0042"), "{warm_text}");
}

#[test]
fn editing_a_directive_identifier_is_a_plain_content_miss() {
    // Narrow the directive on a warm cache: the hash moves, the entry
    // is recomputed, and the previously suppressed finding returns
    // while the directive stops being unused.
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @celerrate-ignore CEL0018\n",
    )]);
    let (cold, _) = check(root.path());
    assert_eq!(cold, Outcome::Clean);

    std::fs::write(
        root.path().join("a.php"),
        "<?php\nnew MissingOne(); // @celerrate-ignore CEL0019\n",
    )
    .unwrap();
    let (warm, text) = check(root.path());
    assert_eq!(warm, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0018"), "{text}");
    assert!(text.contains("CEL0042"), "{text}");
}

#[test]
fn a_typed_suppression_keeps_its_directive_used_on_the_warm_path() {
    // The directive's only client is a typed-family finding (CEL0030,
    // inside a checked body - top-level code is not a body): the
    // matched attribution comes from the typed half's own records,
    // warm and cold alike - no CEL0042 on either run.
    let source = "<?php\nclass Service { public function boot(): void {} }\nfunction caller(): void {\n    $service = new Service();\n    $service->bot(); // @celerrate-ignore CEL0030\n}\n";
    let root = project(&[("a.php", source)]);
    let (cold, cold_text) = check(root.path());
    assert_eq!(cold, Outcome::Clean, "{cold_text}");
    let (warm, warm_text) = check(root.path());
    assert_eq!(warm, Outcome::Clean, "{warm_text}");
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p celerrate_cli --test cache_suppression`
Expected: PASS if tasks 7 and 9 are complete and correct - these are integration pins, red only if the wiring has a hole (a stale union, a persisted reporting diagnostic, a directive edit surviving the hash). Investigate any failure as a real defect, never weaken the assertion.

- [ ] **Step 3: Run the workspace tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✅ test(cli): pin warm-path directive replay and the suppression matrix"
```

---

### Task 11: Closure - gates, corpus, and the record

**Files:**

- Modify: `CHANGELOG.md`
- Possibly modify: `corpus/` snapshot (only under verify-then-accept), `.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md`

- [ ] **Step 1: The local gates**

Run, in order:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

Expected: all green. Also verify the two reserved comments are gone: `grep -rn "never matched" crates/celerrate_semantics/src crates/celerrate_phpdoc_bridge/src` finds nothing reserving identifier-level correspondence for later.

- [ ] **Step 2: The corpus gate, with the verify-then-accept protocol**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: `mixed-rate` unchanged by construction (the harness calls inference directly; nothing in this plan touches it). The corpus snapshot is expected byte-identical - the pinned snapshot is `0 notices, 0 diagnostics` - but this plan deliberately narrows foreign directives, so a delta is possible if the Symfony corpus contains an identifier-bearing `@phpstan-ignore`/`@psalm-suppress` that was over-suppressing a real finding. If a delta appears: hand-inspect every new diagnostic; each must trace to a directive whose identifiers all map (the narrowing) and be a true positive; anything else is a bug in this plan's matcher - fix it, do not accept. Only a verified true-positive delta is accepted by re-blessing the snapshot, with the inspection recorded in the commit message.

- [ ] **Step 3: The WASM projection sketch**

Check `.claude/superpowers/specs/2026-07-15-wasm-interface-sketch.md` covers the `Reporting` phase's shape. If the rule-trait section (added in part 3) does not mention it, append one paragraph: the reporting context projects as plain outcome data (directive records with match flags - already serializable by construction), the two registry questions become host functions, and nothing in the phase touches a tree, so it is the cheapest phase to project. No code changes.

- [ ] **Step 4: The CHANGELOG entry**

Under `## [Unreleased]` / `### Added`:

```markdown
- Identifier-level suppression (#58). A foreign directive whose
  identifiers all map now suppresses only the union of their
  corresponding `CEL` codes (`@phpstan-ignore class.notFound` no longer
  silences an unrelated unknown function on the same line); a directive
  with any unmapped identifier keeps its scope-wide effect, and
  `@psalm-suppress all` is explicitly scope-wide. The correspondence
  table triages both dialects' full published catalogues; lookup is
  exact-case.
- The native suppression directive: `// @celerrate-ignore CEL0030,
  CEL0031 (reason)` in line comments, block comments, and docblocks.
  Identifiers are mandatory (no blanket form); the parenthesized
  trailer is a reason; placement decides the scope (trailing a line of
  code: that line; alone: the next line; docblock: the annotated
  declaration).
- Two directive rules riding the new `Reporting` phase: CEL0041
  (unknown identifier in a native directive) and CEL0042 (unused native
  suppression, exempt for identifiers of inactive rules). Both warnings
  apply to native directives only, are themselves suppressible in one
  non-iterated pass, and report identically on warm and cold runs from
  per-directive match records persisted with the verdict (cache schema
  7).
```

- [ ] **Step 5: Final commit**

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record identifier-level suppression and the directive rules"
```

(If step 2 re-blessed the corpus snapshot or step 3 touched the sketch, commit those separately with their own messages: `✅ test(corpus): accept the verified narrowing delta` / `📝 docs(specs): project the reporting phase onto the WASM sketch`.)

---

## Verification summary (the design's closure gates this plan carries)

- **The #58 acceptance test**: two co-located diagnostics, suppressing one code keeps the other reported (task 4).
- **The correspondence-table triage gate**: both catalogues vendored and covered exactly, every mapped code interned (task 3's composition-root test).
- **The suppression matrix**: foreign mapped, unmapped, mixed, and bare forms; `@psalm-suppress all`; exact-case lookup (task 4); native placement forms, reason trailer, unknown-identifier non-widening, co-located native and foreign union (task 5); any-match attribution, CEL0041/CEL0042 self-suppression in one pass, the inactive-rule exemption (tasks 8 and 9).
- **Warm/cold equivalence extended to the `Reporting` phase**: the harness layers the reporting portion from stored records and asserts equality with recomputation (task 9); byte-identical warm and cold reports at the product level, records-only replay pinned (task 10).
- **Cache honesty**: schema 7, per-half match records, bounds and interning validated on load, any mismatch discards the verdict (task 7).
- **The corpus snapshot byte-identical** (or a hand-verified narrowing delta under verify-then-accept) and **the mixed-rate baseline unchanged** (task 11).
- **The layering gates untouched**: the bridge stays facade-only (its table is plain strings), `celerrate_semantics` constructs no diagnostics (the emission scan), CEL0041/CEL0042 are constructed in `celerrate_rules` and declared in the ledger.
- **Determinism**: the reporting phase is a pure function of the registry and the records; the resolution query is an own-tree read that backdates (the retargeted prose-edit pin, task 2).





