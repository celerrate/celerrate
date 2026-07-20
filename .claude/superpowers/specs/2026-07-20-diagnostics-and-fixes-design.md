# Celerrate: Diagnostics and Fixes (Sub-project 4) Design

Date: 2026-07-20
Status: Approved (brainstorming output; plans follow)
Parent: `.claude/superpowers/specs/2026-07-09-celerrate-design.md` (sections 6
and 11)

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
the formatter (later sub-projects).

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

- The corpus snapshot is byte-identical after every migrated family (the
  proof that migration preserves behavior).
- A mechanical test that every registered identifier has an explain page.
- A mechanical test that no check family lives outside the framework (the
  registry ownership test, section 5).
- The rendering snapshot suite (section 9).
- The natural fixes wired end to end through `--fix-suggestions`
  (section 7).
- The mixed-rate baseline unchanged (no type-behavior change is permitted by
  this sub-project).

## 2. Crate layout and layering

Two crates are created, at the slots the parent design reserves:

- **`celerrate_edit`**, directly above `celerrate_syntax`: the
  structured-edit library. It depends on `celerrate_source`,
  `celerrate_diagnostics` (which defines the `TextEdit` shape suggestions
  transport, section 3), and `celerrate_syntax`, nothing higher.
- **`celerrate_rules`**, above `celerrate_types` and below
  `celerrate_plugin`: the `Rule` trait and metadata, the rule registry (the
  fifth extension-point registry, on the exact template of the existing
  four), the renderer, and the core rules themselves.

Core rules living in `celerrate_rules` is a consequence of the layering that
this design makes explicit: the `Rule` trait lives in `celerrate_rules`, so
implementations must sit at or above it. The check bodies therefore move out
of `celerrate_semantics` and `celerrate_types` into `celerrate_rules`, where
they consume only the public query surface of the crates below. This is the
"first-party code exercises the same API as plugins" commitment applied to
rules, and it forces `celerrate_types` to expose properly what the internal
`CheckContext` reached directly.

Phase contexts are sealed facades owned by their domain crates, on the
`InvocationSite` model from the plugin-boundary sealing (#61): a private
database handle inside, public methods that delegate to public queries, no
salsa vocabulary in any rule-facing signature. Plugin-registered rules
receive the same contexts through `celerrate_plugin` re-exports.

Migrated identifiers change owner to `celerrate_rules` in
`celerrate_diagnostics::REGISTRY`; the composition-root registry test
continues to lock the whole allocation.

## 3. Diagnostic anatomy

`Diagnostic` is enriched in `celerrate_diagnostics`, preserving its two
mechanical contracts: the total deterministic `Ord` (parallel collection is
sorted before rendering and comparison) and the cheap deterministic `Eq` that
salsa early cutoff depends on.

- **Anchor.** The single mandatory span becomes an anchor that admits a
  project-level, spanless form. Project notices (missing or unreadable
  Composer manifest) enter the shared model honestly instead of the separate
  notice block; anchoring them to a fictional `composer.json:1:1` remains
  forbidden. Project-anchored findings order before span-anchored ones in
  the total order.
- **Labeled spans.** One primary span plus zero or more labeled secondary
  spans ("the parameter is declared `int` here"), possibly in other files.
- **Notes.** Free-form reasoning lines ("the inferred type is `string|null`
  because this path returns `null`").
- **Suggestions.** Message, confidence (`Safe` or `NeedsReview`), and edits.
  Layering constraint: `celerrate_diagnostics` sits below `celerrate_syntax`,
  so a suggestion never transports a tree. It carries finalized text edits
  (file, range, replacement text), produced upstream by `celerrate_edit`
  from tree-level operations. The model stays range-late and cheap to
  compare.
- **Severity.** `Warning` and `Error`, unchanged, and the exit-code contract
  does not move: 0 clean, 1 any diagnostic, 2 internal error. The **tier**
  (`Default` or `Nursery`) is rule metadata, not diagnostic data: a nursery
  rule is removed from the default-enabled set computed at the composition
  root. Demotion under the anti-false-positive policy is a one-line metadata
  change; the `celerrate.toml` of sub-project 5 maps user configuration over
  this model without reshaping it.

The persistent-cache artifact schema changes with the model; the
version-stamped cache format already invalidates on binary version, so no
compatibility shim is needed.

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
rules. Conflict resolution follows the `SymbolClaim` model: the
later-registered claimant is excluded, the run is reported as degraded,
never a crash.

**Three phase traits, one registry.** The check body is typed by phase:

- `SyntaxRule`: syntax tree, line index, PHP version range.
- `SemanticRule`: reference resolutions, symbol index.
- `TypedBodyRule`: body IR, inferred types.

Each context is a sealed facade owned by its domain crate (section 2). The
`RuleRegistry` is a singleton salsa input registered at the composition root
in the high durability tier; registration order is dispatch order. Plugin
rules register through the same path, re-exported by `celerrate_plugin`.

**Flow.** One salsa query per phase per file (`syntax_rule_diagnostics`,
`semantic_rule_diagnostics`, `typed_body_rule_diagnostics`) drains the
active rules of its phase in registry order. Rules emit range-late findings
into a sink, anchored by `AstId` or `ExpressionId` (the generalization of
the existing `TypedVerdict` pattern), carrying labels, notes, and
suggestions; the query reconciles findings to enriched `Diagnostic` values
through the source maps at the end, as the type checks do today. The CLI
composes: resilience diagnostics plus the three rule phases, then the
suppression filter, then the total sort, then rendering.

**A minimal fourth phase: `Reporting`.** The unused-suppression rule
(section 8) cannot ride the per-file phases: it needs the match outcomes of
the suppression filter itself. A small reporting phase runs where the filter
runs (CLI orchestration, outside salsa, deterministic because its inputs
are), receiving the file's directives and their match outcomes. Only the two
directive rules ride it for now; it is not a general extension surface in
this sub-project.

**Active set.** Computed at the composition root: `Default`-tier rules are
active, `Nursery` rules are not. Sub-project 5's configuration adjusts this
computation; nothing else changes.

**Invalidation.** Per phase per file, exactly the current shape of
`semantic_diagnostics` and `typed_diagnostics`. A registry change (plugin
added or upgraded) invalidates everything, which is rare and already true of
the four existing registries.

## 5. The migration

Six core rules take over the existing families (exact identifier bounds
confirmed at plan time):

| Rule | Identifiers | Phase |
| --- | --- | --- |
| `unknown-symbols` | CEL0018 to CEL0020 | Semantic |
| `symbol-version-gating` | CEL0021 to CEL0023 | Semantic |
| `syntax-version-gating` | CEL0024 | Syntax |
| `unknown-members` | CEL0030 to CEL0033 | TypedBody |
| `null-dereference` | CEL0034 | TypedBody |
| `argument-checks` | CEL0035 to CEL0038 | TypedBody |

The mechanics: one family at a time, test-driven; the existing unit and
snapshot tests move with the code and stay green; the corpus snapshot must
be byte-identical after each family, including the conservative suppression
on magic-method classes. The check modules in the domain crates are deleted;
what remains below is the public query surface the sealed contexts consume.
Migrated `ALLOCATED_IDENTIFIERS` move to `celerrate_rules`, and the
composition-root registry test doubles as the "no check outside the
framework" gate: domain crates own only their resilience diagnostics
afterward. The invalidation-scope harness verifies the phase queries
re-execute exactly as the old queries did per canonical edit class (a body
edit does not re-run the semantic phase).

## 6. `celerrate_edit`

The structured-edit library expresses edits as operations on the lossless
tree and compiles them into finalized text edits.

- **Model.** `replace`, `insert_before`, `insert_after`, `delete` over nodes
  and tokens, plus constructors for the elements the shipped fixes need. The
  terminal form is a deterministic, sorted, non-overlapping set of
  `TextEdit { file, range, replacement }`, the shape
  `celerrate_diagnostics` transports (section 3).
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
  single pass in the total diagnostic order; a fix whose edits overlap an
  already-applied fix is skipped and reported (deterministic: the first
  wins). No fixpoint loop yet; re-running `check` after application shows
  what remains. Either fix flag combined with `--watch` is a usage error.
- **Write path.** Through the VFS to disk, on the same
  errors-never-panics path as the rest of the tool.
- **Shipped fixes, and an owned consequence.** The natural fixes of the
  existing families are the "did you mean" suggestions on unknown symbols
  and unknown members: a candidate within a bounded edit distance over the
  symbol index, deterministic tie-break, token replacement. They are all
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
- a foreign directive whose identifiers **all** map to known CEL codes
  suppresses only those codes on the scope (this closes the
  over-suppression hole);
- a foreign directive with **any** unmapped identifier falls back to
  scope-wide suppression: the user's existing decision is honored rather
  than re-reported (parent design section 4).

The **foreign-to-CEL table** lives in `celerrate_phpdoc_bridge`, which owns
the dialects, one table per dialect: `method.notFound` maps to CEL0030 on
the PHPStan side, `UndefinedMethod` maps to CEL0030 on the Psalm side, and
so on. The table is tested and documented; the naive
`identifiers == Diagnostic.id` route the triage warned against is
structurally impossible because matching goes through the table.

**Mechanics.** `suppressed_ranges` carries a filter per range, `All` or
`Only(sorted codes)`, and the CLI's `retain_unsuppressed` matches the
diagnostic identifier against the filter. The two source comments reserving
"identifier-level correspondence for the rule framework" are resolved by
this sub-project.

**The native directive, with a stance.** Syntax:
`@celerrate-ignore CEL0030, CEL0031` in a line comment or docblock. Placed
on a line of code, it targets that line; alone on its line, the next line;
in a declaration docblock, the declaration. **Identifiers are mandatory**:
the native directive has no blanket form. The foreign forms cover blanket
suppression for imported codebases; the tool's own directive cannot dig a
new #58-class hole by construction. The native directive provider is core,
lives with the directive vocabulary in `celerrate_semantics`, and is
registered unconditionally at the composition root.

**Two small rules are born with the matcher**, because they keep the
machinery honest, with two new identifiers (CEL0041, CEL0042, owner
`celerrate_rules`):

- **Unknown identifier in a native directive** (CEL0041, `Warning`): a typo
  in a CEL code must not silently suppress nothing.
- **Unused native suppression** (CEL0042, `Warning`): a native directive
  that suppressed nothing is reported (this rides the `Reporting` phase,
  section 4).

Both apply **to native directives only**. Foreign directives legitimately
target diagnostics Celerrate does not emit yet; reporting them would be the
false-positive storm the parent design forbids.

## 9. Rendering

- **Location.** The renderer lives in `celerrate_rules` (the crate map
  assigns it "rule framework, registry, rendering"); the CLI calls it. It
  is a pure function from enriched diagnostics plus sources to text.
- **Form.** rustc-style through an adapter over `annotate-snippets` (the
  rust-lang rendering crate; MIT OR Apache-2.0, audited by `cargo deny`):
  an `error[CEL0030]: message` header, a source excerpt with line numbers,
  labeled primary and secondary underlines, `note:` lines for reasoning,
  `help:` lines for suggestions with the rendered replacement.
  Project-anchored findings render the header without an excerpt; the
  separate notice block disappears. The summary line stays; watch mode uses
  the same renderer.
- **Color.** TTY detection and `NO_COLOR` handling happen in the CLI,
  outside queries, so determinism is untouched; snapshots pin the
  colorless mode.
- **Safety net.** `annotate-snippets` is not under the workspace zero-panic
  lints, so each diagnostic renders in isolation: if rich rendering fails,
  that diagnostic falls back to the current minimal one-line format
  (`path:line:column CEL#### message`) plus an internal-error report, never
  a crash. The "temporary by design" format does not die; it becomes the
  fallback.
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
- **Pages are executable.** A harness iterates the registry, runs each
  failing example through analysis and asserts the identifier fires, runs
  each fixed example and asserts it does not. An explain page can neither
  lie nor rot; this is the codegen-freshness discipline applied to
  documentation.

## 11. Testing

Test-driven throughout, the parent design's five tiers, plus the harnesses
specific to this sub-project:

- the corpus gate at zero delta after each migrated family;
- rendering snapshots (multi-span, multi-file, notes, suggestions,
  project-anchored, unicode content), pinned colorless;
- executable explain pages (section 10);
- fix application: a snapshot of the patched file, and the
  fix-closes-the-diagnostic property (re-checking the patched file no
  longer reports it); overlap conflicts skip deterministically;
- the suppression matrix: the #58 acceptance test (two co-located
  diagnostics, suppressing one code keeps the other reported), foreign
  mapped, unmapped, and bare forms, and the two native-directive rules;
- invalidation-scope tests proving the rule phases keep the invalidation
  shape of the queries they replace;
- fuzzing of edit application (never panics, never silently resolves an
  overlap);
- the mixed-rate baseline unchanged.

## 12. Plan sequencing

The order proposed to the planning stage:

1. The enriched diagnostic anatomy and the explain-page store.
2. `celerrate_edit`.
3. The framework skeleton (traits, registry, contexts, phase queries) plus
   one migrated family as proof (`syntax-version-gating`, the smallest).
4. The remaining migration (semantic, then typed families).
5. Suppression #58: the filter, the correspondence table, the native
   directive, the two new rules (CEL0041, CEL0042).
6. The autofix engine, the did-you-mean fixes, the CLI flags.
7. The renderer.
8. `celerrate explain`, the explain-page workstream, and closure (gates,
   spec and CHANGELOG updates).

## Explicitly rejected

- **A uniform visitor API** (clippy-style single `Rule` trait with visit
  callbacks): the typed checks consume body IR and inferred types, not the
  AST; migrating them into a visitor shape would need either an obese
  context or a database leak, which the #61 sealing forbids. The shared
  AST walk may become a fourth check phase when the style group arrives;
  nothing in this design blocks it.
- **One salsa query per rule**: the finest invalidation, but memo tables
  multiply by rule count, and plugin-registered rules would be
  second-class. Per-phase queries keep today's proven granularity.
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
- **Wholesale pull-forward of `celerrate.toml`**: the tier model makes
  configuration a mapping problem; the surface stays in sub-project 5.
