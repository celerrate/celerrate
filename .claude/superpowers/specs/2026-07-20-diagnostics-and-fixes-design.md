# Celerrate: Diagnostics and Fixes (Sub-project 4) Design

Date: 2026-07-20
Status: Closed (2026-07-23; all eight parts landed, closure gates verified; no release — the next public event is v0.1, sub-project 5)
Parent: `.claude/superpowers/specs/2026-07-09-celerrate-design.md` (sections 6
and 11)

Amendment history:

- 2026-07-20, same day: amended after a five-lens adversarial review (crate
  layering, incremental engine, rule API and extensibility, product and DX,
  migration and testing). The load-bearing corrections: the `Reporting`
  phase gets a warm-cache story (per-directive match outcomes join the
  stored verdict); secondary spans and did-you-mean suggestions leave the
  memoized analysis (symbolic labels resolved at render time,
  presentation-time candidates) so neither pierces the per-file cache
  keying nor the invalidation boundary; phase contexts are specified as
  opaque-handle interrogation surfaces to keep the WASM-projectability
  commitment true; the migration explicitly preserves the co-production of
  diagnostics and cache-revalidation records; the corpus gate is demoted to
  what it actually proves (the pinned snapshot is `0 notices,
  0 diagnostics`) and recall moves to per-identifier seeded-defect
  fixtures; the native directive gains a placement-resolved scope, a reason
  trailer, and an inactive-rule exemption for the unused-suppression rule;
  the foreign-to-CEL table transports code strings so the bridge needs no
  facade change; `TextEdit` moves to `celerrate_source`; project-anchored
  findings keep the notice contract (exit-code-neutral); plugin identifier
  allocation is explicitly deferred with its shape pinned.
- 2026-07-23 — sub-project closed after part 8 (`celerrate explain` and
  closure). Recorded facts:
  - Part 4 (the autofix engine) landed without a committed plan document
    (PR #95); every other part carries its plan under
    `.claude/superpowers/plans/`.
  - The executable-example exemption list is final at five identifiers,
    each with a mandatory reason in
    `celerrate_diagnostics::EXECUTABLE_EXAMPLE_EXEMPTIONS`: `CEL0001` (a
    file above the 4 GiB decoded-size engine cap cannot be committed as a
    fixture), `CEL0013` (the parser's no-progress backstop is a defensive
    guard no grammar-admitted source reaches; the reachability probe found
    only white-box no-bump loops, never a source trigger), `CEL0022` (the
    shipped stub blob carries no symbol whose removal falls inside the
    supported 8.1 to 8.5 window), and `CEL0039`/`CEL0040` (permission-based
    IO errors that cannot be committed as a fixture and do not reproduce
    under root or on Windows CI). Every exempt identifier still carries a
    full page; only the harness execution is waived.
  - The spec's forced-active provision shipped as a guard test, not
    machinery: no nursery rule exists, so building force-activation would
    be untested code. `crates/celerrate_cli/tests/explain_pages.rs` carries
    `every_core_rule_is_default_tier_so_the_default_active_set_covers_all_pages`,
    which fails the moment the first nursery rule lands and names the
    force-activation work then required. The pages-cannot-lie property
    survives: a nursery rule cannot land without tripping the guard.
  - The eight closure gates of section 1, and where each is enforced:
    (1) behavior preservation per migrated family —
    `crates/celerrate_cli/tests/seeded_defects.rs` and the byte-identical
    corpus snapshot (`cargo xtask corpus`, `0 notices, 0 diagnostics`);
    (2) an explain page for every identifier with the declared exemption
    class — the mandatory `RegisteredDiagnostic.explain` field, the
    `every_identifier_has_a_page_with_all_four_sections` content gate, the
    executable harness, and `EXECUTABLE_EXAMPLE_EXEMPTIONS` with reasons;
    (3) no check family outside the framework —
    `crates/celerrate_cli/tests/registry.rs` and `cargo xtask
    emission-scan`; (4) correspondence-table triage — the part 5 triage
    suite in `celerrate_phpdoc_bridge`; (5) the rendering snapshot suite
    including the fault-injected fallback — the part 7 snapshots;
    (6) natural fixes wired through `--fix-suggestions` — the part 6 suite;
    (7) warm/cold equivalence extended to the `Reporting` phase — the
    part 5 harness; (8) the mixed-rate baseline unchanged — `cargo xtask
    mixed-rate`. All eight closure gates were verified green at closure,
    together with the full mechanical suite that guards every change:
    `cargo test --workspace`, `cargo clippy --workspace --all-targets -D
    warnings`, `cargo fmt --all`, `cargo deny check`, and the `cargo
    xtask` gates (`dependency-shape`, `emission-scan`, `corpus`,
    `mixed-rate`). No version tagged.

Inputs this design binds to:

- Issue #58 and its triage comments: identifier-matched suppression is an
  explicit requirement of the rule framework, with the correspondence policy
  the triage fixed (recorded in section 8).
- The reserved seams left by earlier sub-projects: the deliberately minimal
  `Diagnostic` model ("the rich anatomy is sub-project 4"), the "temporary by
  design" renderer, the two source comments reserving identifier-level
  suppression correspondence for the rule framework, and the spanless-notice
  debt from the semantic-core product design ("the richer model that can
  carry a spanless finding honestly belongs to sub-project 4").

## 1. Scope and closure criterion

In scope: the rich diagnostic anatomy; the `celerrate_rules` crate (rule
framework, registry, rendering); the `celerrate_edit` crate; the autofix
engine with `celerrate check --fix` and `--fix-suggestions`; identifier-level
suppression (#58) with the foreign-to-native correspondence table and a
native suppression directive; `celerrate explain` with a written page for
every registered identifier; the migration of every existing check family
into the framework; the severity-and-tier model in rule metadata.

Out of scope: `celerrate.toml`, baseline, JSON/SARIF/GitHub output formats,
`migrate --from-phpstan` (sub-project 5); style-group rules, the WASM host,
the formatter (later sub-projects); plugin-side diagnostic identifier
allocation (deferred with its shape pinned, section 8).

**Deliverable: internal machinery, no release.** No version is tagged at
closure; the next public event is v0.1 (sub-project 5). The CLI surface built
here (`--fix`, `--fix-suggestions`, `explain`, the rich rendering) ships
functional but unannounced.

**What "full migration" means.** The check families migrate: semantic
reference checks, symbol and syntax version gating, unknown members, null
dereference, argument checks. Resilience diagnostics do not become rules:
parse errors, decode failures, and project notices are neither disableable
nor configurable by nature, so they stay produced by their crates. The
enriched model carries them; the framework does not own them.

**Closure gates:**

- **Behavior preservation per migrated family, carried by recall fixtures,
  not the corpus.** The pinned corpus snapshot is `0 notices,
  0 diagnostics`, so a byte-identical corpus run proves only "no new false
  positives on clean code"; a family that silently stopped firing would
  pass it. The gate is therefore twofold: the corpus snapshot stays
  byte-identical, **and** a seeded-defect fixture per migrated identifier
  stays green. The seeded-defect suite covers CEL0030 to CEL0038 today;
  extending it to CEL0018 to CEL0024 is scoped work of the migration
  (section 5).
- A mechanical test that every registered identifier has an explain page,
  with a declared exemption class for environment-triggered identifiers
  (section 10).
- The "no check family outside the framework" gate: the registry ownership
  ledger plus an emission-side source scan (section 5).
- The correspondence-table triage gate: both bridge dialects' published
  identifier catalogues fully triaged, mapped or explicitly unmapped
  (section 8).
- The rendering snapshot suite, including the fault-injected fallback
  (section 9).
- The natural fixes wired end to end through `--fix-suggestions`
  (section 7).
- The warm/cold equivalence harness extended to the `Reporting` phase
  (section 4): a warm run reports the same directive diagnostics as a cold
  run, byte for byte.
- The mixed-rate baseline unchanged. Verified feasible: the mixed-rate
  harness enumerates bodies and calls inference directly, with no
  dependency on any check query, so it cannot move by construction.

## 2. Crate layout and layering

Two crates are created, at the slots the parent design reserves:

- **`celerrate_edit`**, directly above `celerrate_syntax`: the
  structured-edit library. It depends on `celerrate_source` and
  `celerrate_syntax`, nothing higher. The `TextEdit { file, range,
  replacement }` record it compiles to is defined in **`celerrate_source`**:
  it is pure source vocabulary, and defining it at the bottom lets
  `celerrate_diagnostics`, `celerrate_edit`, and later the formatter and
  migrations all take it from below without the edit machinery depending on
  the reporting model.
- **`celerrate_rules`**, above `celerrate_types` and below
  `celerrate_plugin`: the rule traits and metadata, the rule registry (the
  fifth extension-point registry, on the template of the existing four),
  the renderer (behind a cargo feature, so plugin crates and the future
  `celerrate_ide`/LSP builds that want structured diagnostics rather than
  rendered text do not compile `annotate-snippets`), and the core rules
  themselves.

Core rules living in `celerrate_rules` is a consequence of the layering that
this design makes explicit: the rule traits live in `celerrate_rules`, so
implementations must sit at or above it. The check bodies therefore move out
of `celerrate_semantics` and `celerrate_types` into `celerrate_rules`, where
they consume only the sealed context surface (section 4). This is the
"first-party code exercises the same API as plugins" commitment applied to
rules.

**What moves is diagnostic construction, not the walks.** The existing
checks co-produce their diagnostics with the persistent cache's
revalidation records in single walks, deliberately, so drift between the
two is structurally impossible (`reference_outcomes` produces resolution
outcomes and `ResolutionRecord`s together; the typed checks record every
consulted class into the dependency set the typed-verdict revalidation
consumes). The migration preserves that co-production: the outcome
computation and the record production stay below, in the domain crates, as
the queries the sealed contexts consult; the rules consume **outcomes**
(resolution outcomes, membership answers, assignability judgments) and turn
them into diagnostics. Dependency recording moves inside the facade
methods themselves: every context method that consults a class records it,
structurally, so a rule cannot consult without recording. The plan for each
migrated family must enumerate which internal helpers become facade methods
and which become public queries of the domain crate (the typed checks today
reach crate-private modules: the declared-signature lookups, the
assignability judgments, the type-representation probes; publishing that
surface is named per-family plan work, not a footnote).

**Context ownership.** The `Semantic` context is owned by
`celerrate_semantics` and the `TypedBody` context by `celerrate_types`,
sealed on the `InvocationSite` model from the plugin-boundary sealing
(#61): a private database handle inside, public methods that delegate,
no salsa vocabulary in any rule-facing signature. The `Syntax` context is
the stated exception to the domain-ownership rule: its contents (syntax
tree access, line index, PHP version range) span `celerrate_db` and
`celerrate_project` with no single domain owner, so `celerrate_rules` owns
it, sealed the same way.

**The facade delta is enumerable.** `celerrate_plugin` re-exports,
nominally, the full rule-authoring vocabulary: the phase traits, the
sealed contexts, the finding sink and its symbolic suggestion vocabulary
(section 4), rule metadata types, `DiagnosticId`, `Severity`, and the
explain-page type. Plugin crates still depend on `celerrate_plugin` alone;
the dependency-shape check is unchanged.

**Core registrations carry a reserved core identity.** The registry
template attaches a registration identity to every entry, and the
registered-set invariant ("the identities whose registrations entered a
salsa registry", the #60 lesson) must survive core rules and the native
directive provider registering outside `register_plugins`. Core
registrations are recorded in the same registration record under a
reserved core identity, excluded from the plugin-set digest (binary
identity already keys the cache for core behavior).

Migrated identifiers change owner to `celerrate_rules` in
`celerrate_diagnostics::REGISTRY`. The registry test remains the
allocation ledger; because it constrains declarations, not emissions, the
"no check outside the framework" gate is completed by an emission-side
scan in `xtask` (the `dependency_shape` pattern): domain-crate sources
must not construct diagnostics outside their declared resilience lists.

## 3. Diagnostic anatomy

`Diagnostic` is enriched in `celerrate_diagnostics`, preserving its two
mechanical contracts: the total deterministic `Ord` (parallel collection is
sorted before rendering and comparison) and the cheap deterministic `Eq` that
salsa early cutoff depends on.

- **Anchor.** The single mandatory span becomes an anchor that admits a
  project-level, spanless form. Project notices (missing or unreadable
  Composer manifest, no PHP constraint) enter the shared model honestly;
  anchoring them to a fictional `composer.json:1:1` remains forbidden.
  Project-anchored findings order before span-anchored ones in the total
  order. **They keep the notice contract**: a project-anchored finding
  never affects the exit code and renders under the notice vocabulary, so
  the pinned product tests (notices alone are not a failure; a notice
  announces itself as a notice) do not change. Exit 1 means at least one
  span-anchored diagnostic, exactly as today.
- **Labeled spans.** One primary span plus zero or more labeled secondary
  spans ("the parameter is declared `int` here"). Secondary spans in
  **other files are carried symbolically** (a stable identifier of the
  referenced declaration, plus the label text) and resolved to concrete
  ranges **at render time, outside queries**. Rationale, and it is
  load-bearing: an absolute range of another file embedded in a per-file
  artifact keyed by this file's content hash goes stale invisibly (surface
  digests hash names and signatures, never positions), and resolving
  another file's range inside a phase query would pierce the range-free
  invalidation boundary. Same-file secondary spans may carry concrete
  ranges. Labels are restricted to VFS-backed files: a declaration that
  lives in the compiled stubs has no source to excerpt, so the label
  degrades to a note naming the declaration.
- **Notes.** Free-form reasoning lines ("the inferred type is `string|null`
  because this path returns `null`").
- **Suggestions.** Message, confidence (`Safe` or `NeedsReview`), and
  same-file `TextEdit`s (the `celerrate_source` shape). Two provenances:
  suggestions embedded by rules at analysis time (compiled from symbolic
  form at the reconciliation tail, section 4), and presentation-time
  suggestions computed outside the memoized analysis (the did-you-mean
  family, section 7). Cross-file suggestion edits are out of scope for
  this sub-project. Stored suggestions are bounds-validated on cache load
  like diagnostic ranges are today.
- **Severity.** `Warning` and `Error`, unchanged, and the exit-code
  contract does not move: 0 clean, 1 any span-anchored diagnostic, 2
  internal error. The **tier** (`Default` or `Nursery`) is rule metadata,
  not diagnostic data: a nursery rule is removed from the default-enabled
  set computed at the composition root. Demotion under the
  anti-false-positive policy is a one-line metadata change; the
  `celerrate.toml` of sub-project 5 maps user configuration over this
  model without reshaping it.

**Cache consequences, stated rather than waved at.** The stored-verdict
schema changes: the enriched anatomy (symbolic labels, notes, embedded
suggestions), the per-directive match outcomes the `Reporting` phase needs
on the warm path (section 4), and per-edit bounds validation on load. The
version-stamped format invalidates on binary version, so no migration shim
is needed. The active set is a pure function of the binary today and is
covered by its identity hash; one header field is **reserved** for an
active-set digest, because the moment sub-project 5 makes the active set
configurable, it must join the cache key.

## 4. The rule framework

**The declarative unit.** A rule is a coherent family, not a single
identifier: `unknown-symbols` emits CEL0018 to CEL0020, `argument-checks`
emits CEL0035 to CEL0038. Rule metadata carries:

- a stable kebab-case name;
- the group (`correctness` for everything migrated here);
- the closed list of identifiers the rule may emit;
- the default severity **per identifier** (existing families already mix
  `Error` and `Warning` within one family);
- the tier (`Default` or `Nursery`) **per rule**;
- an explain page **per identifier** (section 10).

The registry enforces mechanically: every emitted identifier is declared,
every declared identifier has a page, and no identifier is claimed by two
rules. Conflict units are fixed: a plugin registration that conflicts is
excluded **whole**, through the existing exclusion record and
degraded-run reporting (the `SymbolClaim` model); a core-versus-core
conflict is a bug and fails the composition-root test in CI, never a
runtime degradation.

**Four phase traits, one registry.** The check body is typed by phase:

- `SyntaxRule`: syntax tree access, line index, PHP version range.
- `SemanticRule`: reference resolution outcomes, symbol index.
- `TypedBodyRule`: body IR interrogation, inferred types.
- `ReportingRule`: directives and their match outcomes (below). Core-only
  in this sub-project; declared as a trait with a context like the other
  three so the registry model and the ownership gate see it, but not part
  of the plugin-facing surface yet.

**Contexts are opaque-handle interrogation surfaces.** This is the parent
design's WASM-projectability commitment applied before the traits freeze,
not after (the #61 lesson). Rules hold plain-data handles (`AstId`,
`ExpressionId`, `TypeId`, symbol identifiers) and interrogate the context
through query methods: kind probes, child and operand access, membership
and assignability questions. No context hands out a rowan tree to walk
freely, no context exposes the body IR arenas or the exhaustively
matchable `BodyExpression`/`BodyStatement` enums (the exact hazard the
"type representation is never exposed" commitment names, applied to the
IR that body lowering evolves). A projection sketch of the rule traits
onto the WASM host-family model is an acceptance artifact of this
sub-project, extending the existing sketch.

**Findings and the symbolic suggestion vocabulary.** Rules emit range-late
findings into a sink, anchored by `AstId` or `ExpressionId` (the
generalization of the existing `TypedVerdict` pattern), carrying labels,
notes, and suggestions in **symbolic form**: a closed vocabulary of edit
intents ("replace the name at this anchor with `save`", "insert this
comment above this anchor") rather than concrete edits, because semantic
and typed-body rules have neither trees nor ranges at emission time. The
phase query compiles symbolic suggestions through `celerrate_edit` into
`TextEdit`s at its reconciliation tail, where the source map is already
consulted and tree access is legitimate. Raw-tree dependence never enters
the rule bodies of range-late phases.

**Flow and granularity.** One salsa query per phase per file drains the
active rules of its phase in registry order, with one stated refinement:
the typed phase **preserves the per-body tracked tier**. Today
`body_typed_verdicts` is tracked per body ("editing one body never
re-checks its siblings", pinned by the invalidation-scope tests), so the
framework keeps a per-body inner query that iterates the registered
`TypedBodyRule`s, and the per-file phase query aggregates bodies, exactly
as today. Suppression filtering stays where it lives now: inside the
per-file composition, **before persistence**, now identifier-aware
(section 8). The exit-code count, the printed report, and the persisted
verdict remain the same set by construction.

**The `Reporting` phase and the warm path.** The two directive rules
(section 8) need per-directive match outcomes from the suppression filter.
On a cold run the filter produces them directly. On a warm run the filter
never executes (the stored verdict is post-suppression and served
parse-free), so **the per-directive match records join the stored
verdict**: directive identity (scope range plus filter) and whether it
matched. The `Reporting` phase runs from those records on both paths,
without re-parsing; a directive edit changes the file's content hash and
correctly invalidates the stored verdict. Reporting output then passes
through **one additional, non-iterated suppression pass**: a directive
diagnostic is itself suppressible like any other, suppressing it counts as
use of the suppressing directive, and there is no fixpoint (the pass runs
once, after match outcomes are final). Under overlapping directives,
used-ness is any-match: every directive whose filter admitted the
suppressed diagnostic counts as used.

**Active set.** Computed at the composition root: `Default`-tier rules are
active, `Nursery` rules are not. Sub-project 5's configuration adjusts this
computation; nothing else changes (and the reserved cache-header field of
section 3 absorbs it).

**Invalidation.** Per phase per file, with the typed phase per body as
stated. A registry change (plugin added or upgraded) invalidates
everything, which is rare and already true of the four existing
registries.

## 5. The migration

Six core rules take over the existing families (bounds verified against
the code):

| Rule | Identifiers | Phase |
| --- | --- | --- |
| `unknown-symbols` | CEL0018 to CEL0020 | Semantic |
| `symbol-version-gating` | CEL0021 to CEL0023 | Semantic |
| `syntax-version-gating` | CEL0024 | Syntax |
| `unknown-members` | CEL0030 to CEL0033 | TypedBody |
| `null-dereference` | CEL0034 | TypedBody |
| `argument-checks` | CEL0035 to CEL0038 | TypedBody |

(CEL0025 to CEL0029 and CEL0039 to CEL0040 are project resilience
notices; CEL0001 to CEL0017 are syntax, decode, and loader resilience.
None migrates.)

The mechanics: one family at a time, test-driven; the existing unit and
snapshot tests move with the code and stay green; after each family the
corpus snapshot is byte-identical **and** every one of the family's
identifiers has a green seeded-defect fixture (the recall side; the
corpus, being clean, only guards precision). Extending the seeded-defect
suite from CEL0030-0038 to CEL0018-0024 is part of the first semantic
migration plan. The conservative suppression on magic-method classes is
preserved and pinned by fixture.

The check modules in the domain crates are deleted; what stays below is
the outcome-and-record-producing query surface the sealed contexts
consume (section 2). Each family's plan enumerates the facade methods and
public queries it requires before any deletion. Migrated
`ALLOCATED_IDENTIFIERS` move to `celerrate_rules`; the ledger test plus
the emission-side scan (section 2) form the "no check outside the
framework" gate.

The invalidation-scope harness (which exists and is mature; only new pins
are needed) verifies the phase queries preserve the invalidation shape of
the queries they replace, per canonical edit class. A true example, to
replace a false one from the first draft of this spec: a body edit in
file A never re-runs another file's semantic phase, and an offset-only
edit above a body re-runs only the mapping, not the body's checks (both
are pinned behaviors today; a same-file body edit does re-run the
semantic per-file walk, and that stays true after migration).

## 6. `celerrate_edit`

The structured-edit library expresses edits as operations on the lossless
tree and compiles them into finalized text edits.

- **Model.** `replace`, `insert_before`, `insert_after`, `delete` over nodes
  and tokens, plus constructors for the elements the shipped fixes need. The
  terminal form is a deterministic, sorted, non-overlapping set of
  `TextEdit` values (the `celerrate_source` shape, section 2).
- **Style preservation.** An edit never touches neighboring trivia it was
  not aimed at; insertions compute indentation from the surrounding trivia.
  This is the parent spec's "the edit engine preserves surrounding style"
  made concrete.
- **Overlaps.** Two overlapping edits are an error (`Result`), never a
  silent resolution; the application layer decides what to do with
  conflicts (section 7).
- **Deliberate minimalism.** The API grows only at the pace of shipped
  fixes: token replacement and comment insertion with indentation are what
  this sub-project needs. The formatter and migrations will extend this
  crate later; their surface is not pre-built.

## 7. The autofix engine

- **Flag semantics.** `--fix` applies `Safe` suggestions only;
  `--fix-suggestions` applies `Safe` plus `NeedsReview`. Application is a
  single pass in the total diagnostic order, expressed **against the
  original snapshot coordinates, per file, applied atomically**:
  non-overlapping edits shift later offsets, so both overlap detection and
  application resolve against the pre-application text. A fix whose edits
  overlap an already-applied fix is skipped and reported (deterministic:
  the first wins). No fixpoint loop yet; re-running `check` after
  application shows what remains. Either fix flag combined with `--watch`
  is a usage error.
- **Write path.** Through the VFS to disk, on the same
  errors-never-panics path as the rest of the tool.
- **Did-you-mean is presentation, not analysis.** The candidate search
  (bounded edit distance over the symbol index) runs **outside the
  memoized phase queries**, at render and fix time, only for the
  diagnostics actually reported. Two reasons, both load-bearing: inside a
  phase query it would make every file with an unknown-symbol finding
  depend on the global name set (any rename anywhere re-runs the phase,
  contradicting section 5's invalidation gate), and a persisted candidate
  would go stale when a nearer name appears without any recorded
  revalidation answer changing (a warm `--fix` would apply a stale edit).
  Computed fresh at presentation time, neither failure mode exists and
  nothing is persisted.
- **Ambiguity discipline.** A did-you-mean suggestion is emitted as an
  applicable edit only when the minimal-distance candidate is **unique**;
  ties are listed in a note instead. Bulk `--fix-suggestions` must never
  apply a guess the engine itself knows is ambiguous.
- **Shipped fixes, and an owned consequence.** The natural fixes are the
  did-you-mean family on unknown symbols and unknown members, all
  `NeedsReview`; proposing a different name is never semantics-preserving.
  Therefore **`--fix` alone applies nothing at closure of this
  sub-project**: the `Safe` mass-application path is built and tested, but
  its first real client is the style group. That is the direct consequence
  of shipping no invented fixes, and this design states it rather than
  hiding it.

## 8. Suppression (#58) and the native directive

**Correspondence policy** (fixed by the #58 triage, restated as normative):

- a bare foreign directive (no identifiers) suppresses the whole scope,
  unchanged;
- a foreign directive whose identifiers **all** map suppresses only the
  union of their mapped CEL codes on the scope (this closes the
  over-suppression hole);
- a foreign directive with **any** unmapped identifier falls back to
  scope-wide suppression: the user's existing decision is honored rather
  than re-reported (parent design section 4).

**The table maps names to sets, and transports strings.** Foreign
families are coarser than the CEL allocation (PHPStan's single
`arguments.count` covers findings that span several CEL codes), so each
table entry maps one foreign identifier to a **non-empty set** of CEL
codes, unioned into the directive's filter. Lookup is exact-case, per
dialect; `@psalm-suppress all` is an explicit scope-wide entry, not an
accident of the fallback. The table lives in `celerrate_phpdoc_bridge`
as **plain code strings** ("CEL0030"), because the dependency-shape
check forbids the bridge everything but the facade and the facade need
not grow identifier vocabulary for this: the bridge marks each identifier
mapped (with its code strings) or unmapped, and the matcher downstream
interns the strings through `celerrate_diagnostics::find_identifier`.
The closure gate: both dialects' published identifier catalogues are
fully triaged into the table, every entry mapped or explicitly listed as
unmapped, so silent widening (a precise foreign directive degrading to a
blanket one through table incompleteness) is bounded by review, not by
accident. Surfacing widened directives to the user (a verbose channel)
is sub-project 5 product surface.

**Mechanics.** `suppressed_ranges` carries a filter per range, `All` or
`Only(sorted codes)`; co-located filters merge by union (`All` absorbs).
The CLI's `retain_unsuppressed` matches the diagnostic identifier against
the filter, in place, before persistence (section 4). The two source
comments reserving "identifier-level correspondence for the rule
framework" are resolved by this sub-project.

**The native directive, with a stance.** Syntax:
`@celerrate-ignore CEL0030, CEL0031 (reason)` in a line comment, a block
comment, or a docblock. The optional parenthesized trailer is a reason,
excluded from identifier parsing (PHPStan's affordance, kept; without it,
trailing prose would parse as identifiers and trip CEL0041).
**Identifiers are mandatory**: the native directive has no blanket form.
The foreign forms cover blanket suppression for imported codebases; the
tool's own directive cannot dig a new #58-class hole by construction.

**Placement semantics need a new scope variant, stated explicitly.** "On
a line of code, that line; alone on its line, the next line" is
placement-dependent, and the `CommentDirectiveProvider` contract is a pure
function of comment kind and text that cannot see position (which is
exactly why the bridge maps PHPStan's placement-dependent bare form onto
the two-line superset today). The directive vocabulary therefore gains a
**placement-resolved scope variant**, resolved where the token and its
line context are visible (`resolve_scope`), not in the provider. Docblock
placement on a declaration keeps the declaration scope. The native
provider is core, lives with the directive vocabulary in
`celerrate_semantics`, and is registered unconditionally at the
composition root under the reserved core identity (section 2).

**Two small rules are born with the matcher**, both riding the
`Reporting` phase (section 4), with two new identifiers (owner
`celerrate_rules`):

- **Unknown identifier in a native directive** (CEL0041, `Warning`): a
  typo in a CEL code must not silently suppress nothing. Inputs: the
  directive's parsed identifiers and the registry's known set. A known
  but inactive identifier is not unknown.
- **Unused native suppression** (CEL0042, `Warning`): a native directive
  that suppressed nothing is reported, **with the inactive-rule
  exemption**: a directive naming any identifier of a rule not active in
  this run is exempt, not evaluable. Without the exemption, the parent's
  automatic nursery demotion would convert every existing suppression of
  the demoted rule into a warning storm, mass-producing exactly the false
  positives the demotion mechanism exists to stop.

Both apply **to native directives only**. Foreign directives legitimately
target diagnostics Celerrate does not emit yet; reporting them would be
the false-positive storm the parent design forbids.

**Plugin identifiers: deferred, with the shape pinned.** A plugin cannot
allocate a CEL identifier today: the registry is a static array locked by
a gapless test, and `find_identifier` discards unknown identifiers on
cache reload. Designing a plugin identifier namespace under this
sub-project's load would freeze it prematurely, so plugin-authored
**rules** are explicitly not shippable in this sub-project (the rule
traits are plugin-facing API; the two first-party plugins register no
rules), and the debt is recorded toward the declarative-tier and WASM
sub-project. What this sub-project pins is the shape that keeps the door
open: rule registration data is owned (not `&'static`), explain pages
travel as registration data for non-core rules, and identifier lookup is
specified as a two-tier resolution (static registry now, a dynamic
namespaced tier later) so the suppression grammar and the cache
round-trip do not assume the static tier is total.

## 9. Rendering

- **Location.** The renderer lives in `celerrate_rules`, behind a cargo
  feature (section 2); the CLI calls it. It is a pure function from
  enriched diagnostics plus sources to text; symbolic labels are resolved
  to ranges here, outside queries (section 3).
- **Form.** rustc-style through an adapter over `annotate-snippets` (the
  rust-lang rendering crate; MIT OR Apache-2.0, audited by `cargo deny`):
  an `error[CEL0030]: message` header, a source excerpt with line numbers,
  labeled primary and secondary underlines, `note:` lines for reasoning,
  `help:` lines for suggestions with the rendered replacement.
  Project-anchored findings render under the notice vocabulary without an
  excerpt (section 3). The summary line stays.
- **Explain is discoverable from the output.** The report ends with one
  rustc-style pointer per distinct identifier reported: "for more
  information, run `celerrate explain CEL0030`". The forty written pages
  are reachable from the tool's primary output, not only from the
  documentation.
- **Color.** TTY detection and `NO_COLOR` handling happen in the CLI,
  outside queries, so determinism is untouched; snapshots pin the
  colorless mode.
- **Watch mode and tall output.** Rich blocks are 8 to 15 lines each and
  the watch cycle clears and reprints; unbounded output would scroll the
  useful head out of view. Watch mode caps the rendered block at the
  terminal height and ends with an "and N more diagnostics" line; the
  one-shot mode renders everything. The alternate screen buffer is a
  possible later refinement, not this sub-project's.
- **Safety net.** `annotate-snippets` is not under the workspace zero-panic
  lints, so each diagnostic renders in isolation: if rich rendering fails,
  that diagnostic falls back to the minimal one-line format
  (`path:line:column CEL#### message`; for a project-anchored finding, the
  existing notice line `notice CEL####: message`) plus an internal-error
  report, never a crash. The adapter carries a fault-injection seam so the
  fallback path is snapshot-tested (section 11), not merely asserted.
- The adapter keeps `annotate-snippets` replaceable: the mapping from the
  diagnostic anatomy to the library's input types is one module, and no
  rule or diagnostic type references the library.

## 10. `celerrate explain`

- A full-word subcommand, content embedded in the binary. A page carries:
  why the reported pattern is a problem, a failing example, a fixed
  example, configuration notes, and the owning rule.
- `RegisteredDiagnostic` gains a page pointer; the composition-root test
  requires a page for every registered identifier, resilience diagnostics
  included. The roughly forty existing pages are written in this
  sub-project as a named workstream (the stub-curation lesson: content work
  scheduled as work, not as a bullet point).
- **Pages are executable, with a declared exemption class.** A harness at
  the composition root iterates the registry, runs each failing example
  through analysis with the full registered set and the rule under test
  forced active (nursery rules are outside the default set), and asserts
  the identifier fires; runs each fixed example and asserts it does not.
  Fixtures pin their PHP version range through their own `composer.json`;
  project-level identifiers use a project-shaped fixture (a directory,
  not a snippet). Identifiers whose trigger is an **environment
  condition** rather than source content (the 4 GiB file cap; unreadable
  manifest permissions, which break under root and on Windows CI) belong
  to a mechanically declared exemption list: the page is still required,
  the executable example is waived. An explain page outside the exemption
  can neither lie nor rot.

## 11. Testing

Test-driven throughout, the parent design's five tiers, plus the harnesses
specific to this sub-project:

- the migration gate per family: corpus snapshot byte-identical (precision
  on clean code) plus a green seeded-defect fixture per migrated
  identifier (recall), with the suite extension to CEL0018-0024;
- rendering snapshots (multi-span, multi-file, notes, suggestions,
  project-anchored, unicode content), pinned colorless, **including the
  fault-injected fallback** (mixed rich and minimal output plus the
  internal-error trailer) and the explain pointer trailer;
- executable explain pages with the exemption list (section 10);
- fix application: a snapshot of the patched file, the
  fix-closes-the-diagnostic property (re-checking the patched file no
  longer reports it), original-coordinate overlap handling, and the
  ambiguity discipline (a tie produces a note, never an applicable edit);
- the suppression matrix: the #58 acceptance test (two co-located
  diagnostics, suppressing one code keeps the other reported); foreign
  mapped, unmapped, mixed, and bare forms; `@psalm-suppress all`;
  exact-case lookup; co-located native and foreign directives (filter
  union, any-match attribution); the inactive-rule exemption for CEL0042;
  suppression of CEL0041/CEL0042 themselves (one non-iterated pass,
  suppression counts as use);
- warm/cold equivalence extended to the `Reporting` phase: a warm run
  reports the same directive diagnostics as a cold run, byte for byte,
  from the persisted match records;
- invalidation-scope pins: the phase queries preserve the shape of the
  queries they replace (the per-body typed tier included), and
  did-you-mean stays out of the dependency graph (a rename in an
  unrelated file re-runs no phase query of an unaffected file);
- fuzzing: an edit-application target joins the existing fuzz targets
  (never panics, never silently resolves an overlap);
- the mixed-rate baseline unchanged.

## 12. Plan sequencing

The order proposed to the planning stage:

1. The enriched diagnostic anatomy (`TextEdit` in `celerrate_source`, the
   anchor, symbolic labels, suggestions), the explain-page store, and the
   stored-verdict schema groundwork.
2. `celerrate_edit`.
3. The framework skeleton: the four phase traits (`Reporting` included, so
   part 5 does not discover framework work mid-flight), the registry, the
   sealed contexts, the phase queries with the per-body typed tier, plus
   one migrated family as proof (`syntax-version-gating`, the smallest).
4. The remaining migration (semantic families with the seeded-defect
   extension, then typed families with the per-family facade
   enumeration).
5. Suppression #58: the identifier-aware filter, the correspondence table
   and its triage gate, the native directive with the placement-resolved
   scope, the per-directive match records in the stored verdict, and the
   two directive rules (CEL0041, CEL0042).
6. The autofix engine: application semantics, the presentation-time
   did-you-mean, the CLI flags.
7. The renderer, its fault-injection seam, and the watch-mode height cap.
8. `celerrate explain`, the explain-page workstream with the exemption
   list, and closure (gates, spec and CHANGELOG updates).

## Explicitly rejected

- **A uniform visitor API** (clippy-style single `Rule` trait with visit
  callbacks): the typed checks consume body IR and inferred types, not the
  AST; migrating them into a visitor shape would need either an obese
  context or a database leak, which the #61 sealing forbids. The shared
  AST walk may become another check phase when the style group arrives;
  nothing in this design blocks it.
- **One salsa query per rule**: the finest invalidation, but memo tables
  multiply by rule count, and plugin-registered rules would be
  second-class. Per-phase queries (per body in the typed tier) keep
  today's proven granularity.
- **Did-you-mean computed inside the phase queries**: it wires the global
  name set into every file's dependency graph and persists candidates the
  revalidation records cannot keep honest; presentation-time computation
  has neither problem (section 7).
- **Concrete cross-file ranges in labels or stored artifacts**: invisible
  staleness under the per-file cache keying, and a pierced invalidation
  boundary in memory; symbolic resolution at render time costs nothing
  measurable on the diagnostics actually shown (section 3).
- **A hand-rolled renderer**: correct multi-span rendering (unicode width,
  overlaps, wrapping, color) is a sub-project of its own and is not the
  differentiator; the adapter keeps the exit open if `annotate-snippets`
  becomes a ceiling.
- **Inventing a `Safe` fix to showcase `--fix`**: a style rule smuggled in
  early would blur the sub-project boundary; the honest statement is that
  `--fix` has no client until the style group.
- **A blanket form of the native directive**: it would recreate the #58
  hole under Celerrate's own name.
- **Unused-suppression reporting on foreign directives**: foreign
  suppressions legitimately target diagnostics Celerrate does not emit;
  reporting them re-reports what the user already silenced.
- **Designing the plugin identifier namespace now**: it would freeze a
  public naming scheme under schedule pressure, with no consumer to
  validate it; the shape is pinned instead (section 8) so nothing built
  here assumes the static tier is total.
- **Wholesale pull-forward of `celerrate.toml`**: the tier model makes
  configuration a mapping problem; the surface stays in sub-project 5.
