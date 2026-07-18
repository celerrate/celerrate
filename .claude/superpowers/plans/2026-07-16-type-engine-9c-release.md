# Type Engine 9c — Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `v0.0.3`, the type-engine preview: the release ledger
decided (guillotine, cache lever, editorial calls), the workspace
version bumped, the identifier reference and the bridge tables
published as repository documentation with drift tests, the `0.0.3`
changelog entry, the README and benchmark SVGs rewritten around the
new published numbers, the tag pushed through the existing release
workflow, and the sub-project closed in the specs.

**Architecture:** No engine change: everything the release contains
already merged with plans 1a through 9b. This plan is release
decisions, documentation, and product surface. The release
infrastructure from the semantic core's 8c plan is reused unchanged
(`release.yml` on `v*` tags, five targets, `cargo xtask release-notes`
extracting the notes from the changelog, the three-OS test matrix).
The one new tested seam is a pair of documentation drift tests at the
composition root: every registered identifier must appear in
`docs/diagnostics.md`, and every suppression form in
`docs/phpdoc-bridge.md` — so the pages cannot silently fall behind
the registry.

**Tech Stack:** Rust 1.94 (one small integration-test file), GitHub
Actions (existing workflows, untouched), Keep a Changelog, Markdown,
hand-edited SVG.

**Branch:** `type-engine-9c-release`, from `main`.

**Design source:** `.claude/superpowers/specs/2026-07-14-type-engine-design.md`
sections 1 (the minimum shippable set, the guillotine inheritance
rule), 8 (the families and their stances), 9 (the preview product,
the fallback lever, the product surface at closure) and 11 (plan 14,
"9c — Release: the `v0.0.x` release, README, CHANGELOG, identifier
documentation, the published conflict tables").

**Prerequisites:** Plans 6 (interprocedural), 7 (providers), 8
(checks), 9a (cache), and 9b (corpus and benchmark) are merged and
green on `main`. In particular: the registry holds `CEL0001` through
`CEL0038`, `benchmarks/PROTOCOL.md` carries the re-published numbers
(five scenarios, peak memory, the substance number), and the closing
memos of plans 8, 9a, and 9b exist in their plan files — Task 1 reads
them.

## Global Constraints

- **Zero panic, mechanically enforced**: workspace lints deny
  `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`;
  `unsafe_code` is forbidden. The new test file carries the usual
  test-module `#![allow(...)]`.
- **The workspace version becomes `0.0.3`; the tag is `v0.0.3`.**
  `v0.0.1` and `v0.0.2` exist; the release workflow's tag-version
  check (`grep "^version = ..." Cargo.toml`) is generic and needs no
  change.
- **Identifiers are permanent from publication** (design section 8):
  `docs/diagnostics.md` states it, and a guillotined family's
  identifiers stay allocated and documented as not emitted — never
  removed, never reused.
- **Publication says honestly what is enabled** (design sections 1
  and 9): every number in the README and the changelog comes from
  `benchmarks/PROTOCOL.md` as re-published by plan 9b, and a cut
  family is named in the release notes as a v0.1 blocker inherited by
  sub-project 5 — never silently absent.
- **No user-side off-switch exists yet** (design section 1): the
  changelog and `docs/diagnostics.md` name inline suppression as the
  only per-site answer to a disputed diagnostic in this preview.
- **The `mixed-rate` and `ground-truth` subcommands stay hidden**
  (plan 7's rule, reconfirmed by plan 9b's handoff); the substance
  number is published as a number with its reproduction command, not
  as a documented product surface.
- **Everything in English, full words** (standard acronyms fine). No
  em-dashes in newly generated content; pre-existing committed text
  carried over verbatim keeps its punctuation.
- **Commits**: gitmoji + Conventional Commits, repository-configured
  identity, no AI attribution of any kind.
- Local gate for every task:
  `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
  all green.

## Fixed decisions

1. **The version is `0.0.3`.** The preview series increments the
   patch number (`0.0.1` the semantic core, `0.0.2` its fix release);
   under SemVer every `0.0.x` release may change anything, and the
   design names the milestone "a public `v0.0.x` preview". The
   release binary discards every on-disk cache wholesale through the
   pack header's binary identity: the blake3 hash of the running
   executable's own bytes
   (`crates/celerrate_cli/src/cache/identity.rs`; keying on
   `CARGO_PKG_VERSION` alone was removed as audit finding I1, and the
   version string is only the unreadable-executable fallback), so any
   rebuilt binary, the version bump included, never reads the
   previous build's packs. The designed mechanism, not a problem.

2. **The release ledger is a task, not an assumption.** Plans 8, 9a,
   and 9b each escalate decisions here by name: the family guillotine
   (plan 8, decision 15), the `PERSIST_TYPED_ARTIFACTS` fallback
   lever (plan 9a; design section 9), and whether the release notes
   mention the substance number (plan 9b's handoff). Task 1 reads the
   three closing memos, applies the decision rules written below, and
   records the outcomes in a ledger appended to this plan file before
   any user-facing text is written — the changelog and README wording
   depend on the outcomes.

3. **The guillotine default is: all three families ship.** A cut
   happens only if plan 8's closing memo names a guillotine candidate
   that stayed unresolved through plans 9a and 9b. The cut itself is
   one line in the family-walk composition
   (`crates/celerrate_types/src/checks/mod.rs`, dropping the family's
   walk from the composed verdict set — not `analysis.rs`'s composition
   point, which would leave cache-served typed diagnostics unfiltered),
   plus the honesty trail: the
   family's rows in `docs/diagnostics.md` marked "allocated, not
   emitted in v0.0.3", the changelog naming it a v0.1 blocker
   inherited by sub-project 5, the seeded-defect suite's fixtures for
   that family inverted to pin the silence (with a comment naming the
   blocker), and the spec amendment recording the inheritance. The
   identifiers stay allocated and in the registry.

4. **The `PERSIST_TYPED_ARTIFACTS` default is: stays `true`.** The
   lever flips only if plan 9b recorded a warm-scenario miss (a
   median at or above 1.0 second on the reference hardware) whose
   cache-statistics lines show the typed cache itself as the cause
   (revalidation dominating, `typed_served` low). If flipped, the
   warm numbers converge toward cold-with-inference, the protocol is
   re-run and re-published with the honest numbers, and the README
   and changelog carry them — the design's escalation path, taken
   visibly, never silently.

5. **The substance number appears in the changelog.** One sentence in
   the `0.0.3` entry: the residual `mixed` rate on the corpus's
   expressions, with `benchmarks/PROTOCOL.md` as the source. Honest
   substance is the recall-side story of this preview; hiding the
   number would gate substance and then not say what it measured.

6. **The repository documentation is `docs/`.** Two new pages:
   `docs/diagnostics.md` (the identifier reference — the interim home
   until the rule framework ships `celerrate explain`) and
   `docs/phpdoc-bridge.md` (the publication home of the bridge's
   conflict, precedence, suppression, and lowering tables, plus the
   pinned-reference coverage statement). The rustdoc in
   `celerrate_phpdoc_bridge` stays the authoritative source; the
   pages say so, and the interim-home sentences in the crate's
   rustdoc are updated to point at the published page.

7. **The documentation cannot drift silently.** A new integration
   test at the composition root
   (`crates/celerrate_cli/tests/documentation.rs`) asserts that every
   `celerrate_diagnostics::REGISTRY` entry appears in
   `docs/diagnostics.md` and that every suppression form appears in
   `docs/phpdoc-bridge.md`. The composition root is the right home
   for the same reason the identifier-uniqueness test lives there: it
   is the only layer that sees every producer at once. A future plan
   that allocates `CEL0039` without documenting it fails this test.

8. **The README's flagship number becomes warm body-edit** — plan
   9b's decision 8, named there as a 9c handoff: the comment-append
   edit is the class the body IR's early cutoff neutralizes, so as a
   flagship it would be near-tautological. The hero paragraph, the
   performance table, and both benchmark SVGs carry the protocol's
   re-published numbers; the SVGs keep their two-bar layout with the
   warm bar re-labeled and re-scaled.

9. **No new workflows.** `release.yml` (five targets, `SHA256SUMS`,
   notes from `cargo xtask release-notes`), the three-OS test matrix,
   and `corpus.yml` are reused exactly as the semantic core's 8c plan
   built them. The only YAML this plan touches is none.

## File structure

```
.claude/superpowers/plans/2026-07-16-type-engine-9c-release.md
                                   modify (task 1): the release ledger appended
crates/celerrate_types/src/checks/mod.rs
                                   conditionally modify (task 1): the guillotine cut
crates/celerrate_cli/src/cache/mod.rs
                                   conditionally modify (task 1): PERSIST_TYPED_ARTIFACTS
crates/celerrate_cli/tests/seeded_defects.rs
                                   conditionally modify (task 1): a cut family's fixtures
Cargo.toml                         modify (task 2): version 0.0.2 -> 0.0.3
Cargo.lock                         regenerated by cargo (task 2)
crates/celerrate_cli/tests/documentation.rs
                                   create (tasks 3, 4): the drift tests
docs/diagnostics.md                create (task 3): the identifier reference
docs/phpdoc-bridge.md              create (task 4): the published bridge tables
crates/celerrate_phpdoc_bridge/src/lib.rs
                                   modify (task 4): the publication-home sentence
crates/celerrate_phpdoc_bridge/src/dialect/mod.rs
                                   modify (task 4): the conflict-table heading
CHANGELOG.md                       modify (task 5): the 0.0.3 entry
README.md                          modify (task 6): full rewrite
assets/benchmark-light.svg         modify (task 6): the new numbers
assets/benchmark-dark.svg          modify (task 6): the new numbers
.claude/superpowers/specs/2026-07-14-type-engine-design.md
                                   modify (task 7, on main): the closing amendment
.claude/superpowers/specs/2026-07-09-celerrate-design.md
                                   modify (task 7, on main): the second-milestone release outcome
```

## Notes for the implementer

- **The `«...»` convention.** Values wrapped in `«»` exist only after
  plan 9b's measurements and cannot be pre-written honestly. Each
  names its exact source; fill it from that source, never from
  memory. The sources are: the Results section of
  `benchmarks/PROTOCOL.md` (the five scenario medians, peak memory,
  the substance number) and the closing memos appended to the plan
  files of plans 8, 9a, and 9b.
- The repository's `origin` is `git@github.com:celerrate/celerrate.git`;
  `gh` is authenticated for it. A workflow observation command
  (`gh pr checks`, `gh run watch`) is part of its task: the release is
  not done until it has been watched to green.
- Tags `v0.0.1` and `v0.0.2` exist; `v0.0.3` follows the same
  mechanics. The release workflow's publish job checks the tag
  against `Cargo.toml`'s version line, extracts the notes with
  `cargo xtask release-notes 0.0.3`, and uploads five archives plus
  `SHA256SUMS` — all pre-existing, verified twice already.
- Diagnostic message shapes quoted in the documentation come from
  `crates/celerrate_types/src/checks/mod.rs` (plan 8) and
  `crates/celerrate_semantics/src/reference_checks.rs`; if a quoted
  example drifts from the source, the source wins — fix the page.
- The two docs pages are product documentation and belong in `docs/`;
  they are not superpowers artifacts.

---

### Task 1: The release ledger

The decisions the sibling plans escalated here, made and recorded
before any user-facing text depends on them. The default path (no
escalations recorded) is documentation-only; each contingency, if
taken, is its own gated commit.

**Files:**

- Modify: `.claude/superpowers/plans/2026-07-16-type-engine-9c-release.md`
  (the ledger, appended at the end)
- Conditionally modify: `crates/celerrate_types/src/checks/mod.rs`
  (decision 3's cut), `crates/celerrate_cli/src/cache/mod.rs`
  (decision 4's lever), `crates/celerrate_cli/tests/seeded_defects.rs`
  and `xtask/src/corpus.rs` (a cut family's fixtures and refusal list)

**Interfaces:**

- Consumes: the closing memos in
  `.claude/superpowers/plans/2026-07-16-type-engine-8-checks.md`,
  `...-9a-cache.md`, and `...-9b-corpus-benchmark.md`; the Results
  section of `benchmarks/PROTOCOL.md`.
- Produces: the `## Release ledger` section of this plan file — the
  single place tasks 3, 5, and 6 read the guillotine and lever
  outcomes from.

- [ ] **Step 1: Create the branch**

```bash
git checkout main && git pull
git checkout -b type-engine-9c-release
```

- [ ] **Step 2: Read the three closing memos and the protocol**

Read, in full:

- the closing memo / triage memo of plan 8 (guillotine candidates,
  blessed true positives),
- the closing memo of plan 9a (the `PERSIST_TYPED_ARTIFACTS` stance,
  the watch-persist outcome),
- the closing memo of plan 9b (the five medians, the acceptance
  verdict, peak memory, the substance number),
- the Results section of `benchmarks/PROTOCOL.md`.

- [ ] **Step 3: Decide, by the fixed rules**

Apply fixed decisions 3, 4, and 5 of this plan:

- **Guillotine**: if no memo names an unresolved candidate, all three
  families ship — record that. If one does, cut it at the composition
  point in `crates/celerrate_types/src/checks/mod.rs` (drop the
  family's walk from the composed verdict set, with a comment naming
  this ledger), invert its seeded-defect fixtures in
  `crates/celerrate_cli/tests/seeded_defects.rs` to pin the silence
  (comment: "guillotined in v0.0.3; named v0.1 blocker"), remove its
  identifiers from any hard-refusal list in `xtask/src/corpus.rs` if
  they were added, and re-bless the corpus snapshot. Each change in
  its own commit, full gate plus `cargo xtask corpus` green.
- **`PERSIST_TYPED_ARTIFACTS`**: if plan 9b recorded no warm miss, it
  stays `true` — record that. If a miss is recorded and the
  statistics lines implicate typed revalidation, flip the constant in
  `crates/celerrate_cli/src/cache/mod.rs` to `false`, re-run
  `cargo xtask bench` on the reference hardware, and hand the honest
  numbers to tasks 5 and 6 via the ledger.
- **Substance number**: published in the changelog (fixed decision
  5) — record the number.

- [ ] **Step 4: Append the ledger to this plan file**

```markdown
## Release ledger

Decided «date», before any user-facing text was written.

- Families shipping in v0.0.3: «all three / the list», per plan 8's
  closing memo («one line: clean triage, or the candidate and why it
  was cut»). «If cut: the family is a named v0.1 blocker inherited by
  sub-project 5; its identifiers stay allocated.»
- `PERSIST_TYPED_ARTIFACTS`: «true, unchanged / flipped to false»,
  per plan 9b's memo («the scenario and median that decided it»).
- Substance number: «N» % residual `mixed` rate on corpus
  expressions, published in the changelog entry.
- Published numbers (from `benchmarks/PROTOCOL.md`): cold full
  «N» s, warm no-change «N» s, warm one-edit «N» s, warm body-edit
  «N» s (flagship), warm signature-edit «N» s; peak memory cold
  «N» MiB against the 1536 MiB budget.
```

- [ ] **Step 5: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: all green.

```bash
git add .claude/superpowers/plans/2026-07-16-type-engine-9c-release.md
git commit -m "📝 docs(plans): record the 9c release ledger"
```

(Contingency commits from Step 3, if any, precede this one and use:
`🔥 fix(checks): withhold the «family» family from the v0.0.3 composition`
and
`⏪ revert(cache): disable typed-artifact persistence for v0.0.3`.)

---

### Task 2: The version bump and the draft pull request

`0.0.2` becomes `0.0.3`, and the branch goes up early so every later
task's push is observed by the full CI surface (three-OS matrix,
corpus, dependency shape).

**Files:**

- Modify: `Cargo.toml` (workspace version)
- Regenerated: `Cargo.lock` (by cargo, not by hand)

**Interfaces:**

- Produces: `celerrate --version` prints `celerrate 0.0.3`; the
  release workflow's publish job greps `^version = "0.0.3"$` from
  `Cargo.toml` when Task 7 tags.

- [ ] **Step 1: Bump the version**

In `Cargo.toml`, change:

```toml
[workspace.package]
version = "0.0.2"
```

to:

```toml
[workspace.package]
version = "0.0.3"
```

- [ ] **Step 2: Regenerate the lock file**

Run: `cargo check --workspace`
Expected: success; `git diff Cargo.lock` shows only workspace members
moving from `0.0.2` to `0.0.3`.

- [ ] **Step 3: Verify the binary reports the new version**

Run: `cargo run --release --package celerrate_cli -- --version`
Expected output: `celerrate 0.0.3`

- [ ] **Step 4: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: all green.

```bash
git add Cargo.toml Cargo.lock
git commit -m "🔖 chore(release): bump the workspace version to 0.0.3"
```

- [ ] **Step 5: Push the branch and open the draft pull request**

```bash
git push --set-upstream origin type-engine-9c-release
gh pr create --draft --title "Type engine 9c: the v0.0.3 release" --body "Part 9c of the type-engine closure: the release ledger, version 0.0.3, the identifier reference and the bridge tables as repository documentation with drift tests, the 0.0.3 changelog entry, the README and SVG rewrite around the re-published numbers. Spec: .claude/superpowers/specs/2026-07-14-type-engine-design.md sections 1, 8, 9, 11."
gh pr checks --watch
```

Expected: all checks green.

---

### Task 3: The identifier reference (`docs/diagnostics.md`)

The user-facing documentation of every `CEL####` identifier — the
design's interim home until the rule framework ships `celerrate
explain`. TDD through the drift test: the composition root demands
that every registered identifier appears on the page.

**Files:**

- Create: `crates/celerrate_cli/tests/documentation.rs`
- Create: `docs/diagnostics.md`

**Interfaces:**

- Consumes: `celerrate_diagnostics::REGISTRY` (all 38 entries after
  plan 8), the ledger of Task 1 (a cut family's wording).
- Produces: `docs/diagnostics.md`, linked by the README (Task 6) and
  the changelog (Task 5); the test file Task 4 extends.

- [ ] **Step 1: Write the failing drift test**

Create `crates/celerrate_cli/tests/documentation.rs`:

```rust
//! Documentation drift tests. The repository documentation is the
//! interim publication home for the identifier reference and the
//! bridge tables (type-engine design, section 9); these tests live at
//! the composition root for the same reason the identifier-uniqueness
//! test does — it is the only layer that sees every producer at once.
//! A plan that allocates a new identifier without documenting it
//! fails here.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;

use celerrate_diagnostics::REGISTRY;

fn workspace_page(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is unreadable: {error}", path.display()))
}

#[test]
fn every_registered_identifier_is_documented() {
    let page = workspace_page("docs/diagnostics.md");
    for entry in REGISTRY {
        assert!(
            page.contains(entry.id.as_str()),
            "docs/diagnostics.md does not document {} (`{}`)",
            entry.id.as_str(),
            entry.family,
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --package celerrate_cli --test documentation`
Expected: FAIL with `docs/diagnostics.md is unreadable` (the page
does not exist yet).

- [ ] **Step 3: Write `docs/diagnostics.md`**

The content below is the default (all three families shipping). If
Task 1's ledger cut a family, keep its rows and add to its section:
"Allocated, not emitted in v0.0.3: this family is withheld from this
preview (a named v0.1 blocker); its identifiers stay reserved."

Before committing, verify the quoted message shapes against
`crates/celerrate_types/src/checks/mod.rs` and the severity of every
row against its producer. Exactly two producers use
`Severity::Warning`: `crates/celerrate_semantics/src/reference_checks.rs`
(`CEL0023`) and `crates/celerrate_project/src/notice.rs` (the notices
`CEL0025` to `CEL0029`); a repository-wide grep also matches the
cache severity round-trip in `crates/celerrate_cli/src/cache/stored.rs`
and test assertions, which are not producers. The source wins over
this plan.

````markdown
# Diagnostics

Every diagnostic Celerrate emits carries a `CEL####` identifier.
Identifiers are permanent: once published in a release an identifier
keeps its meaning forever, a retired one is never reused, and a new
diagnostic takes the next free number. This page is the identifier
reference until the rule framework ships `celerrate explain` pages.

A report line looks like:

```text
src/Controller/PostController.php:42:19 CEL0018 unknown class `App\Service\Mailer`
```

Severity is reporting weight, not exit behavior: `celerrate check`
exits 1 as soon as it reports any diagnostic, **error** and
**warning** alike. The project discovery notices
(`CEL0025` to `CEL0029`) are counted separately in the summary line
and do not affect the exit code.

To silence a single occurrence, use an inline suppression
(`@phpstan-ignore-line`, `@phpstan-ignore-next-line`,
`@phpstan-ignore`, or `@psalm-suppress`): see
[the PHPDoc bridge](phpdoc-bridge.md#suppressions). There is no
configuration file or baseline yet; inline suppression is the only
per-site switch in this preview.

## Syntax (CEL0001 to CEL0017)

Produced while reading and parsing source files. Parsing is error
resilient: a syntax diagnostic never stops the analysis of the rest
of the file or the project.

| Identifier | Severity | Meaning |
| --- | --- | --- |
| CEL0001 | error | source too large: the file exceeds the analyzable size limit |
| CEL0002 | error | unexpected character |
| CEL0003 | error | unterminated block comment |
| CEL0004 | error | unterminated string |
| CEL0005 | error | unterminated heredoc |
| CEL0006 | error | unterminated interpolation |
| CEL0007 | error | expected an expression |
| CEL0008 | error | expected a semicolon |
| CEL0009 | error | expected a specific token |
| CEL0010 | error | unexpected token |
| CEL0011 | error | nesting too deep |
| CEL0012 | error | non-associative operator chained |
| CEL0013 | error | the parser made no progress (an internal guard, never expected on real input) |
| CEL0014 | error | expected a member name |
| CEL0015 | error | expected a statement |
| CEL0016 | error | expected a type |
| CEL0017 | error | expected a declaration |

## Unknown symbols (CEL0018 to CEL0020)

References that resolve nowhere, with the project, its Composer
dependencies, and the bundled PHP stubs all considered.

| Identifier | Severity | Meaning |
| --- | --- | --- |
| CEL0018 | error | unknown class |
| CEL0019 | error | unknown function |
| CEL0020 | error | unknown constant |

## PHP version gating (CEL0021 to CEL0024)

Symbols or syntax used outside the PHP version range the project's
`composer.json` declares.

| Identifier | Severity | Meaning |
| --- | --- | --- |
| CEL0021 | error | the symbol is not available in the project's minimum PHP version |
| CEL0022 | error | the symbol was removed before the project's maximum PHP version |
| CEL0023 | warning | the symbol is deprecated within the project's version range |
| CEL0024 | error | the syntax construct is not available in the project's minimum PHP version |

## Project discovery notices (CEL0025 to CEL0029)

About the project's own configuration, reported once per run.

| Identifier | Severity | Meaning |
| --- | --- | --- |
| CEL0025 | warning | no `composer.json` found; the whole project root is analyzed |
| CEL0026 | warning | `composer.json` exists but could not be read as a Composer manifest |
| CEL0027 | warning | no PHP version configured; the latest supported stable version (currently PHP 8.5) is assumed |
| CEL0028 | warning | the PHP version constraint is unusable (unparseable, or admitting no supported version); the latest supported stable version is assumed |
| CEL0029 | warning | `vendor/composer/installed.json` could not be read; installed packages are not indexed |

## Unknown members (CEL0030 to CEL0033)

Members that do not exist on the receiver's resolved type, new in
v0.0.3. Deliberately conservative: a `mixed`, `object`, or otherwise
dynamic receiver is silent; magic methods suppress their own kind
(`__get`/`__set` for properties, `__call` for methods, `__callStatic`
for static methods), directly or by inheritance; `stdClass` and
`#[AllowDynamicProperties]` classes never report unknown properties;
members declared by `@property` or `@method` docblocks count as
existing; on a union type the member must be missing on every
non-null constituent before anything is reported.

| Identifier | Severity | Meaning | Message shape |
| --- | --- | --- | --- |
| CEL0030 | error | unknown method | ``unknown method `save` on `App\User` `` |
| CEL0031 | error | unknown property | ``unknown property `$name` on `App\User` `` |
| CEL0032 | error | unknown class constant | as above, for constants |
| CEL0033 | error | unknown enum case | as above, for enum cases |

## Nullability (CEL0034)

A method call or property access on a value that may be `null` at
that point. Flow narrowing decides what is still nullable:
`instanceof`, `null` comparisons, `isset()`/`empty()`, the `is_*`
family, truthiness, negation and boolean composition, `??`/`??=`,
`?->` chains (one null receiver short-circuits the whole chain),
`match`, `switch`, early returns, `assert()`, and assertion
annotations (`@phpstan-assert`, non-divergent `@psalm-assert`) are
all honored.

| Identifier | Severity | Meaning | Message shape |
| --- | --- | --- | --- |
| CEL0034 | error | possibly null dereference | ``accessing `save` on a possibly null `App\User|null` `` |

## Argument types (CEL0035 to CEL0038)

Each argument checked against its parameter, plus arity, named
arguments included. `mixed` passes everywhere. Coercion follows the
calling file's declared mode: under `declare(strict_types=1)` the
check is strict; in a weak-mode file, coercions PHP performs at
runtime are not reported. Argument unpacking of a value whose shape
is unknown silences arity for that call.

| Identifier | Severity | Meaning | Message shape |
| --- | --- | --- | --- |
| CEL0035 | error | argument type mismatch | ``argument 2 of `substr` expects `int`, `string` given`` |
| CEL0036 | error | too few arguments | a required parameter is bound neither positionally nor by name |
| CEL0037 | error | too many arguments | more positional arguments than parameters, no variadic |
| CEL0038 | error | unknown named argument | a named argument matches no declared parameter name |
````

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --package celerrate_cli --test documentation`
Expected: PASS.

- [ ] **Step 5: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: all green.

```bash
git add crates/celerrate_cli/tests/documentation.rs docs/diagnostics.md
git commit -m "📝 docs(diagnostics): publish the identifier reference with its drift test"
git push
```

---

### Task 4: The bridge page (`docs/phpdoc-bridge.md`)

The publication home the design's section 9 names for the bridge's
conflict and precedence tables, plus the suppression mapping, the
lowering table, and the pinned-reference coverage statement — all of
which have lived in rustdoc "until plan 9c". The rustdoc stays
authoritative; the page mirrors it and the interim-home sentences in
the crate are updated to point here.

**Files:**

- Create: `docs/phpdoc-bridge.md`
- Modify: `crates/celerrate_cli/tests/documentation.rs` (one test)
- Modify: `crates/celerrate_phpdoc_bridge/src/lib.rs` (crate doc)
- Modify: `crates/celerrate_phpdoc_bridge/src/dialect/mod.rs` (heading)

**Interfaces:**

- Consumes: the rustdoc tables in
  `crates/celerrate_phpdoc_bridge/src/dialect/mod.rs` (conflict),
  `src/lowering.rs` (total lowering), `src/directives.rs`
  (suppressions), `src/dialect/psalm.rs` (the ignored bucket), and
  the coverage statement header of
  `crates/celerrate_phpdoc_bridge/tests/phpstan_corpus/verdicts.txt`.
- Produces: `docs/phpdoc-bridge.md`, linked by `docs/diagnostics.md`
  (already, from Task 3), the README, and the changelog.

- [ ] **Step 1: Extend the drift test**

Append to `crates/celerrate_cli/tests/documentation.rs`:

```rust
#[test]
fn the_bridge_page_documents_every_suppression_form() {
    let page = workspace_page("docs/phpdoc-bridge.md");
    for form in [
        "@phpstan-ignore-line",
        "@phpstan-ignore-next-line",
        "@phpstan-ignore",
        "@psalm-suppress",
    ] {
        assert!(
            page.contains(form),
            "docs/phpdoc-bridge.md does not document `{form}`",
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --package celerrate_cli --test documentation`
Expected: the new test FAILS with `docs/phpdoc-bridge.md is
unreadable`; the Task 3 test still passes.

- [ ] **Step 3: Write `docs/phpdoc-bridge.md`**

Transcribe the four tables from their rustdoc sources named above —
the rustdoc is authoritative; drop the `//!` prefixes, keep the `\|`
escapes inside table cells, and fill the coverage numbers from the
header of `tests/phpstan_corpus/verdicts.txt`. The page:

````markdown
# The PHPDoc bridge

Celerrate ships one first-party annotation plugin, enabled by
default: `phpdoc-bridge`. It translates the inherited PHPDoc
convention family — standard PHPDoc plus the PHPStan dialect, with
Psalm synonyms — into Celerrate's internal types. This page is the
published form of the tables that govern it; the rustdoc of
`celerrate_phpdoc_bridge` is the authoritative source and this page
mirrors it at each release.

Two ground rules:

- **No docblock diagnostics.** A malformed annotation is silently
  ignored, per construct: one unsupported construct inside a docblock
  never discards the docblock.
- **Loss is a widening, never a guess.** Every construct lowers to a
  lattice value or a documented sound widening (a supertype), so a
  widening can silence a diagnostic but never mis-report working
  code.

## What is read

- **Standard PHPDoc**, complete: `@param`, `@return`, `@var`,
  `@throws`, `@property` (and `-read`/`-write`), `@method`. The last
  two declare virtual members: a member declared by `@property` or
  `@method` counts as existing for the unknown-member diagnostics.
- **The PHPStan dialect**, measured against the test corpus of
  `phpstan/phpdoc-parser` at a pinned version: «N» of «M» pinned
  inputs parse («P» %; the corpus deliberately includes invalid
  inputs, which count as rejected). The statement lives in
  `crates/celerrate_phpdoc_bridge/tests/phpstan_corpus/verdicts.txt`.
- **Psalm tags with PHPStan-coincident semantics are synonyms**,
  fully honored: `@psalm-param`, `@psalm-return`, `@psalm-var`,
  `@psalm-property(-read/-write)`, `@psalm-method`, `@psalm-extends`,
  `@psalm-implements`, `@psalm-use`, `@psalm-template`,
  `@psalm-assert`, `@psalm-assert-if-true`, `@psalm-assert-if-false`.
- **The variance markers** (`@template-covariant`,
  `@template-contravariant`) are honored as templates: the template
  itself is read, the variance marker recognized and dropped.
- **The ignored-divergent bucket**, parsed and ignored without error:
  purity tags (`@psalm-pure`, `@psalm-mutation-free`,
  `@psalm-immutable`, `@psalm-external-mutation-free`), taint
  annotations (`@psalm-taint-source`, `-sink`, `-escape`,
  `-unescape`, `-specialize`, `@psalm-flow`), and the Psalm-specific
  `this` refinements (`@psalm-if-this-is`, `@psalm-this-out`,
  `@psalm-self-out`).

## The conflict table

The dialects coexist on one docblock in real code (`@param` plus
`@psalm-param` plus `@phpstan-param` on one method). For one slot,
the tiers resolve as:

| slot | wins | over | over |
|---|---|---|---|
| return | `@phpstan-return` | `@psalm-return` | `@return` |
| param (per name) | `@phpstan-param` | `@psalm-param` | `@param` |
| var | `@phpstan-var` | `@psalm-var` | `@var` |
| property / method | `@phpstan-` form | `@psalm-` form | bare form |
| ancestor (per written head name) | `@phpstan-` form | `@psalm-` form | bare form |

Within one tier the first *parseable* tag wins; an unparseable tag
never consumes a slot. `@throws` accumulates across tiers instead of
resolving.

An annotation refines the native declaration only when the refinement
provably holds; when it provably fails, the native declaration wins;
a template-typed annotation that cannot be decided refines through
its bound. When a member declares no annotation of its own, the
nearest ancestor's annotation applies, checked against the inheriting
member's native declaration.

## Suppressions

Honored from v0.0.3 on, across **all** diagnostic families, with the
posture "over-suppression, never under-suppression":

| Written form | Comment kind | Effect |
|---|---|---|
| `@phpstan-ignore-line` | any | suppress, current line |
| `@phpstan-ignore-next-line` | any | suppress, next line |
| `@phpstan-ignore <identifiers>` | any | suppress, current and next line |
| `@psalm-suppress <identifiers>` | docblock | suppress, the annotated declaration's whole span |
| `@psalm-suppress <identifiers>` | line or block comment | suppress, current and next line |

Foreign identifiers are carried but not matched in this preview:
suppression extinguishes every family on the target scope.
Identifier-level correspondence arrives with the rule framework.

## The lowering table

Every parsed construct maps to a lattice value or a documented sound
widening. Transcribed from the rustdoc of
`crates/celerrate_phpdoc_bridge/src/lowering.rs`:

«the full table, copied verbatim from the `lowering.rs` module
rustdoc at the release commit — every row, `//!` prefixes dropped,
cell escapes kept»
````

(The `«...»` markers above are the two mechanical transcriptions this
plan cannot pin: the coverage numbers, whose committed values at plan
4b's closure were 225 of 241 (93%) but whose source of truth is
`verdicts.txt` at the release commit, and the lowering table, ~33
rows starting at "names: native keywords" and ending at "any other
name". Copy both from the sources; do not re-derive them.)

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --package celerrate_cli --test documentation`
Expected: both tests PASS.

- [ ] **Step 5: Point the rustdoc at the published page**

In `crates/celerrate_phpdoc_bridge/src/lib.rs`, change the crate
doc's parenthetical:

```text
(repository documentation is
the interim publication home until plan 9c).
```

to:

```text
(published at `docs/phpdoc-bridge.md`;
this rustdoc stays the authoritative source).
```

In `crates/celerrate_phpdoc_bridge/src/dialect/mod.rs`, change the
heading:

```text
//! # The conflict table (decision 8; published by plan 9c)
```

to:

```text
//! # The conflict table (decision 8; published at `docs/phpdoc-bridge.md`)
```

- [ ] **Step 6: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: all green.

```bash
git add docs/phpdoc-bridge.md crates/celerrate_cli/tests/documentation.rs crates/celerrate_phpdoc_bridge/src/lib.rs crates/celerrate_phpdoc_bridge/src/dialect/mod.rs
git commit -m "📝 docs(bridge): publish the conflict, suppression, and lowering tables"
git push
```

---

### Task 5: The changelog

The `0.0.3` entry, user-level, honest about what shipped and what did
not. The release workflow extracts this text verbatim as the GitHub
Release notes (`cargo xtask release-notes 0.0.3`), so this entry *is*
the release announcement.

**Files:**

- Modify: `CHANGELOG.md`

**Interfaces:**

- Consumes: the Task 1 ledger (guillotine and lever outcomes, the
  substance number), `benchmarks/PROTOCOL.md` (the numbers).
- Produces: the `## [0.0.3] - «date»` entry Task 7's release notes
  come from; the link references the README and docs pages rely on.

- [ ] **Step 1: Insert the `0.0.3` entry**

Under `## [Unreleased]` (which stays, emptied), insert the entry
below, and update the link-reference block at the bottom of the file.
Fill every `«...»` from the Task 1 ledger. If the ledger cut a
family, remove its bullet and add under a `### Known limitations`
heading: "The «name» family is withheld from this preview after
corpus triage; it is a named blocker for v0.1, and its identifiers
(`CEL00xx` to `CEL00yy`) stay reserved." If the ledger flipped
`PERSIST_TYPED_ARTIFACTS`, the cache bullet says warm runs re-infer
and carries the honest numbers.

```markdown
## [0.0.3] - «the tag's date»

The type-engine preview: the incremental engine is now type-aware.
Interprocedural type inference, docblock annotations through the
bundled PHPDoc bridge, three new diagnostic families measured on the
Symfony corpus with no visible false positive, and the incremental
numbers re-published with inference active.

### Added

- The unknown-member diagnostic family (`CEL0030` to `CEL0033`):
  methods, properties, class constants, and enum cases that do not
  exist on the receiver's resolved type. Conservative by design: a
  `mixed` or dynamic receiver is silent, magic members and
  `#[AllowDynamicProperties]` suppress their own kind, and
  `@property`/`@method` docblock members count as existing.
- The nullability diagnostic family (`CEL0034`): method calls and
  property accesses on a possibly-null value, with flow narrowing
  (`instanceof`, null comparisons, `isset()`, `??`, `?->` chains,
  `match`, early returns, assertion annotations) deciding what is
  still nullable at each use site.
- The argument-type diagnostic family (`CEL0035` to `CEL0038`):
  per-argument assignability and arity, named arguments included.
  Coercion follows the calling file's `declare(strict_types)` mode;
  `mixed` passes everywhere.
- Interprocedural type inference: declared types (native
  declarations, per-PHP-version stub signatures, docblock
  annotations) are trusted; unannotated returns are inferred from
  bodies, through mutual recursion; generics are resolved and
  propagated for precision but never reported on.
- The `phpdoc-bridge` plugin, enabled by default: standard PHPDoc,
  the PHPStan dialect, and Psalm synonyms — coverage, precedence,
  and every table published in
  [docs/phpdoc-bridge.md](https://github.com/celerrate/celerrate/blob/v0.0.3/docs/phpdoc-bridge.md).
- Inline suppressions, honored across all diagnostic families:
  `@phpstan-ignore-line`, `@phpstan-ignore-next-line`,
  `@phpstan-ignore`, and `@psalm-suppress`.
- The stdlib type provider: computation-dependent signatures
  (`array_map` from its callable, `json_decode` from its flags,
  `preg_match` matches shapes, and more) that no declarative stub
  can express.
- The persistent cache extends to typed artifacts: inferred
  signatures are persisted and revalidated recursively, and warm
  one-edit stays sub-second with inference active (median «N» s, with
  the flagship warm body-edit at «N» s), measured by the committed
  protocol
  ([benchmarks/PROTOCOL.md](https://github.com/celerrate/celerrate/blob/v0.0.3/benchmarks/PROTOCOL.md)).
- The identifier reference:
  [docs/diagnostics.md](https://github.com/celerrate/celerrate/blob/v0.0.3/docs/diagnostics.md)
  documents every `CEL####` identifier.

### Changed

- The benchmark protocol's scenario set grew from three scenarios to
  five (warm body-edit and warm signature-edit join), and the
  flagship number is now the warm body edit: cold full «N» s, warm
  body-edit «N» s, warm signature-edit «N» s on symfony/demo (9447
  PHP files, 1.3 million lines, vendor tree included).
- Substance, measured: «N» % of the corpus's expressions still
  analyze as `mixed` (the residual the stub curation and provider
  workstreams drive down); the number and its protocol are published
  so precision claims can be weighed against it.
```

And replace the link-reference block at the bottom:

```markdown
[Unreleased]: https://github.com/celerrate/celerrate/compare/v0.0.3...HEAD
[0.0.3]: https://github.com/celerrate/celerrate/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/celerrate/celerrate/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/celerrate/celerrate/releases/tag/v0.0.1
```

- [ ] **Step 2: Verify the notes extraction**

Run: `cargo xtask release-notes 0.0.3`
Expected: exactly the entry's body, starting with "The type-engine
preview", no heading, no link references.

- [ ] **Step 3: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: all green.

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): write the 0.0.3 entry describing the type-engine preview"
git push
```

---

### Task 6: The README and the benchmark SVGs

The README describes two diagnostic families and carries the
semantic-core numbers with the old flagship — the exact staleness
plan 9b's handoff names. It becomes the v0.0.3 landing page: five
families, annotations and suppressions, the re-published numbers with
warm body-edit as flagship, and links to the two new documentation
pages. The SVGs are updated in the same commit (they render the
numbers the README states; they change together).

**Files:**

- Modify: `README.md` (full replacement, content below)
- Modify: `assets/benchmark-light.svg`
- Modify: `assets/benchmark-dark.svg`

**Interfaces:**

- Consumes: `benchmarks/PROTOCOL.md` (all numbers), the Task 1
  ledger (a cut family disappears from "What works today" without a
  trace of having been promised), `docs/diagnostics.md`,
  `docs/phpdoc-bridge.md`.
- Produces: the landing page the release links to.

- [ ] **Step 1: Replace the content of `README.md`**

Fill every `«...»` from the protocol's Results section. The colons in
section bodies, the badge block, and the license text are carried
over verbatim from the committed README.

```markdown
# Celerrate

[![CI](https://github.com/celerrate/celerrate/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/celerrate/celerrate/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/celerrate/celerrate)](https://github.com/celerrate/celerrate/releases/latest)
[![Platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-555)](https://github.com/celerrate/celerrate/releases/latest)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**An extremely fast, all-in-one toolchain for PHP, written in Rust.**

Celerrate type-checks 1.3 million lines of PHP in «cold» seconds
cold, and in «body-edit» seconds after you edit a function body.
Measured end to end, protocol committed to the repository.

> **Early preview (v0.0.3).** The engine is now type-aware:
> interprocedural inference, your existing PHPDoc/PHPStan/Psalm
> annotations honored out of the box, and five diagnostic families,
> with zero configuration and without ever crashing on any input.
> The rule surface is still deliberately small, and growing.

## Quick start

Download the archive for your platform from the
[latest release](https://github.com/celerrate/celerrate/releases/latest),
unpack it, and run the binary inside a Composer project:

```sh
celerrate check .
```

There is nothing to configure. Composer discovery finds your code, your
dependencies, and your PHP version range on its own:

```text
src/Service/Search.php:27:16 CEL0021 `array_find` requires PHP 8.4, but the project's minimum PHP version is 8.1
src/Controller/PostController.php:42:19 CEL0018 unknown class `App\Service\Mailer`
src/Notification/Mailer.php:31:9 CEL0034 accessing `format` on a possibly null `DateTimeImmutable|null`

0 notices, 3 diagnostics
```

## Performance

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/benchmark-dark.svg">
  <img src="assets/benchmark-light.svg" width="720" alt="Bar chart of median wall clock on symfony/demo: cold full analysis «cold» seconds, warm with one function body edited «body-edit» seconds">
</picture>

Measured by the committed [benchmark protocol](benchmarks/PROTOCOL.md)
on symfony/demo (9447 PHP files, 1.3 million lines, vendor tree
included), with type inference active, on the hardware the protocol
names:

| Scenario | Median wall clock |
| --- | --- |
| Cold full analysis | «cold» s |
| Warm, one function body edited | **«body-edit» s** |
| Warm, one signature edited | «signature-edit» s |

All numbers are full CLI runs: process startup, cache loading,
analysis, and reporting. No comparison against other tools is
published at this scope; the protocol states why.

## What works today

`celerrate check .` reports five diagnostic families
([the identifier reference](docs/diagnostics.md)):

- **Unknown symbols**: references to classes, functions, or constants
  that resolve nowhere, with your project, your Composer dependencies,
  and the bundled PHP stubs all considered.
- **PHP version gating**: symbols or syntax used outside the PHP
  version range your `composer.json` declares, including removals and
  deprecations.
- **Unknown members**: methods, properties, class constants, and enum
  cases that do not exist on the receiver's inferred type — silent on
  anything dynamic, aware of `__call`/`__get` and
  `@property`/`@method` docblocks.
- **Nullability**: dereferencing a value that may be `null`, with
  flow narrowing (`instanceof`, `isset()`, `??`, `?->` chains,
  `match`, early returns, assertion annotations) deciding what is
  still nullable at each use site.
- **Argument types**: per-argument assignability and arity, named
  arguments included, honoring each file's `declare(strict_types)`
  mode.

Around them:

- **Your annotations, honored**: standard PHPDoc, the PHPStan
  dialect, and Psalm synonyms, through the bundled
  [PHPDoc bridge](docs/phpdoc-bridge.md) — including inline
  suppressions (`@phpstan-ignore-line`, `@psalm-suppress`, and
  friends).
- **Interprocedural inference**: declared types are trusted,
  unannotated returns are inferred across the call graph, generics
  are resolved for precision (and never reported on).
- **Zero configuration**: Composer discovery derives what to analyze
  and which PHP versions to check against. Installed dependencies are
  indexed but never reported on.
- **`--watch`**: re-analysis on every change.
- **A persistent cache** (`.celerrate/cache/`, self-ignoring): warm
  runs reuse everything that did not change, across processes —
  inferred types included.

### What it does not do yet

No lint rules, no formatter, no language server, no configuration
file, no baseline, and no output formats beyond the terminal report.
Generic mismatches are not reported (generics serve precision only),
and unannotated parameters are treated as `mixed`. Those are the next
sub-projects, in the [roadmap](#roadmap)'s order.

## One engine, a whole toolchain

Every Celerrate command is a view over the same incremental semantic
model. Index a project once; everything else is a query:

- **`celerrate check`**: static analysis with interprocedural type
  inference, plus lint, security taint, and architecture rule groups.
  One command answers "is my code OK?".
- **`celerrate format`**: an opinionated, lossless formatter.
- **`celerrate lsp`**: a language server with the same diagnostics as
  CI, at typing speed.
- **`celerrate migrate` / `celerrate generate`**: automated refactoring
  and semantic code generation.

Speed stays a feature throughout: a Rust core, parallel by default,
incremental by construction. Diagnostics are meant to teach: annotated
spans, the engine's reasoning, concrete suggestions, and safe automatic
fixes. Extensibility is designed in: first-party plugins in Rust,
community plugins through a sandboxed WASM API.

## Built to be trusted

The engineering rules behind the numbers, enforced mechanically in CI:

- **Zero panic**: clippy denies `unwrap`, `expect`, indexing, and
  `panic` across the workspace; `unsafe` code is forbidden.
- **No input can crash it**: parsers and loaders produce diagnostics,
  never failures. Fuzzing keeps them honest.
- **Deterministic**: every analysis result is a pure function of its
  inputs. Same input, same output, on any machine.
- **Incremental by construction**: invalidation is computed, not
  guessed; warm runs reuse everything that did not change.
- **Measured, not claimed**: performance numbers come from a committed,
  reproducible benchmark protocol.
- **Test-driven**: no production code without a test that demanded it.

## Compatibility

Celerrate targets PHP 8.1+ projects. It defines its own type annotation
norm and ships a first-party [PHPDoc bridge](docs/phpdoc-bridge.md),
enabled by default, so existing annotated codebases work on day 1.

## Roadmap

One pillar at a time, in this order:

1. **`celerrate check`**: the static analysis engine is the first
   public deliverable (previewed since v0.0.1, type-aware since
   v0.0.3); the lint, taint, and architecture rule groups build on it.
2. **`celerrate format`**: the formatter, once the lossless syntax tree
   is proven by the analyzer.
3. **`celerrate lsp`**: the language server, reusing the same
   incremental engine.
4. **`celerrate migrate` / `celerrate generate`**: refactoring and code
   generation, last because they lean on everything above.

## Contributing

Contributions are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md). The
engineering rules above are enforced by CI, not by review comments.

## License

Dual-licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option. "Celerrate" is a trademark of JDevelop.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
```

If the ledger cut a family, its bullet disappears from "What works
today" (four families, or three) and the "What it does not do yet"
paragraph gains: "The «name» family is withheld from this preview
after corpus triage; it returns with v0.1." The example diagnostic
line for a cut nullability family is replaced with a `CEL0030` line:
``src/Notification/Mailer.php:31:9 CEL0030 unknown method `sendAll` on `App\Notification\Mailer` ``.

- [ ] **Step 2: Update both benchmark SVGs**

Both files keep their exact structure (a title, a caption, two
labeled bars, a baseline); only the numbers, the second bar's label,
its width, and its label offset change. Geometry rule, matching the
committed files: the cold bar's total width is 640 (a `h636` path
plus the 4-unit arc); the warm bar's total width is
`w = round(640 × warm ÷ cold)`, its path is `h{w-4}` (and `h-{w-4}`
on the return), and its value label sits at `x = w + 8` (the
committed file: warm total 167, label at 175).

`assets/benchmark-light.svg`:

```xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 720 132" width="720" height="132" role="img" aria-labelledby="benchmark-title">
  <title id="benchmark-title">Median wall clock of full celerrate check runs on symfony/demo (9447 PHP files, 1.3 million lines), type inference active: cold full analysis «cold» seconds, warm with one function body edited «body-edit» seconds.</title>
  <g font-family="system-ui, -apple-system, 'Segoe UI', sans-serif">
    <text x="0" y="14" font-size="12" fill="#52514e">Median wall clock, full CLI runs, inference active · symfony/demo, 9447 PHP files, 1.3M lines</text>
    <text x="0" y="42" font-size="13" fill="#52514e">Cold full analysis</text>
    <path d="M0,48 h636 a4,4 0 0 1 4,4 v12 a4,4 0 0 1 -4,4 h-636 z" fill="#2a78d6"/>
    <text x="648" y="62.5" font-size="13" font-weight="600" fill="#0b0b0b">«cold» s</text>
    <text x="0" y="94" font-size="13" fill="#52514e">Warm, one function body edited</text>
    <path d="M0,100 h«w-4» a4,4 0 0 1 4,4 v12 a4,4 0 0 1 -4,4 h-«w-4» z" fill="#2a78d6"/>
    <text x="«w+8»" y="114.5" font-size="13" font-weight="600" fill="#0b0b0b">«body-edit» s</text>
    <line x1="0.5" y1="46" x2="0.5" y2="122" stroke="#c3c2b7" stroke-width="1"/>
  </g>
</svg>
```

`assets/benchmark-dark.svg`: the same content with the dark palette
the committed file already uses — caption and labels `#c3c2b7`, bars
`#3987e5`, value labels `#ffffff`, baseline `#383835`.

- [ ] **Step 3: Verify the three documents agree**

Check by reading: every number in `README.md` appears in
`benchmarks/PROTOCOL.md`; the changelog, the README, and the SVG
titles state the same cold and body-edit medians; the family count in
the README matches the ledger.

- [ ] **Step 4: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: all green.

```bash
git add README.md assets/benchmark-light.svg assets/benchmark-dark.svg
git commit -m "📝 docs(readme): rewrite the README as the v0.0.3 preview landing page"
git push
```

---

### Task 7: Merge, tag, verify, and close the sub-project

The endgame: the pull request merges green, the tag triggers the
existing release workflow, the release is verified as a user would
meet it — including a typed diagnostic from a downloaded binary — and
the two specs record the closure. Use
superpowers:finishing-a-development-branch for the merge decision and
superpowers:verification-before-completion before every "done" claim
below.

**Files:**

- Modify (on main, after the release):
  `.claude/superpowers/specs/2026-07-14-type-engine-design.md`
  (the closing amendment) and
  `.claude/superpowers/specs/2026-07-09-celerrate-design.md`
  (the second milestone's release outcome, appended against the
  existing 2026-07-14 entry)

**Interfaces:**

- Consumes: everything above, green on the pull request.

- [ ] **Step 1: Final local gate and pull-request readiness**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check && cargo xtask corpus && cargo xtask dependency-shape`
Expected: all green.

```bash
gh pr ready
gh pr checks --watch
```

Expected: every check green, including the three-OS test matrix and
the corpus workflow.

- [ ] **Step 2: Request review and merge**

Per superpowers:requesting-code-review, have the diff reviewed before
merging. Then merge (repository habit: merge commit) and update the
local main:

```bash
gh pr merge --merge
git checkout main
git pull
```

- [ ] **Step 3: True the changelog date, then tag**

If the `## [0.0.3]` date in `CHANGELOG.md` is no longer today,
correct it on main (one-line documentation commit, gated as usual).
Then:

```bash
git tag v0.0.3
git push origin v0.0.3
```

- [ ] **Step 4: Watch the release workflow**

```bash
gh run watch $(gh run list --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId')
```

Expected: all five `build` legs and the `publish` job pass.

- [ ] **Step 5: Verify the release page**

Run: `gh release view v0.0.3`
Expected: six assets (four `.tar.gz`, one `.zip`, `SHA256SUMS`), and
the release notes are exactly the changelog's `0.0.3` entry.

- [ ] **Step 6: Verify a binary as a user, typed families included**

On this machine (Apple Silicon):

```bash
cd "$(mktemp -d)"
gh release download v0.0.3 --repo celerrate/celerrate --pattern 'celerrate-aarch64-apple-darwin.tar.gz' --pattern 'SHA256SUMS'
grep aarch64-apple-darwin SHA256SUMS | shasum --algorithm 256 --check
tar --extract --file celerrate-aarch64-apple-darwin.tar.gz
./celerrate-aarch64-apple-darwin/celerrate --version
mkdir project
cat > project/greeter.php <<'PHP'
<?php

final class Greeter
{
    public function hello(): string
    {
        return 'hello';
    }
}

function greet(?Greeter $greeter): string
{
    return $greeter->hello();
}
PHP
./celerrate-aarch64-apple-darwin/celerrate check project; echo "exit: $?"
```

Expected: the checksum check prints `OK`; `--version` prints
`celerrate 0.0.3`; the check reports a `CEL0034` possibly-null
dereference on `$greeter->hello()` (message shape: ``accessing
`hello` on a possibly null `Greeter|null` ``) alongside the `CEL0025`
notice, and exits 1 — a downloaded binary running type inference on a
project with no `composer.json`. (If the ledger cut the nullability
family, seed a `CEL0030` defect instead: call `$greeter->helo()`
after an `instanceof` check and expect an unknown-method report.)

- [ ] **Step 7: Record the closure in both specs**

On main, in one commit:

Append a dated entry to the amendment history of
`.claude/superpowers/specs/2026-07-14-type-engine-design.md`
recording: `v0.0.3` is tagged and released; which families shipped
(and, if one was cut, its named inheritance by sub-project 5 as a
v0.1 blocker); the published numbers and the flagship scenario; the
substance number; the state of the two levers
(`PERSIST_TYPED_ARTIFACTS`, the LRU decision from plan 9b); the two
documentation pages as the interim publication homes; and that plan
9c — and with it sub-project 3, the type engine — is closed. Write it
from actual outcomes, including anything that deviated from this
plan.

Append a dated entry to the amendment history of
`.claude/superpowers/specs/2026-07-09-celerrate-design.md` recording
the release outcome of the second public milestone. The milestone
itself is already on record: the parent's 2026-07-14 amendment entry
recorded it when the type-engine design was approved. Reference that
entry rather than restating it, and record only what is new: the
preview released as `v0.0.3` with «the shipped families», leaving
sub-project 5 owing the v0.1 criterion no new diagnostics beyond
«any inherited blocker», only the product surface and the
matched-scope PHPStan comparison.

```bash
git add .claude/superpowers/specs/2026-07-14-type-engine-design.md .claude/superpowers/specs/2026-07-09-celerrate-design.md
git commit -m "📝 docs(specs): record the v0.0.3 release and close the type engine"
git push
```

---

## Self-review

Checked against the design (sections 1, 8, 9, 11) and the sibling
plans' handoffs:

- **Section 9's product surface, item by item**: the README rewrite
  (Task 6, five families, new numbers, new flagship), the CHANGELOG
  entry and release version (Tasks 2 and 5), the user-facing
  documentation of the new identifiers with the repository as
  interim home (Task 3), the publication home of the bridge's
  conflict and precedence tables (Task 4).
- **Section 1's guillotine inheritance**: a cut family is a named
  v0.1 blocker in the ledger, the changelog, the README, the docs
  page, and both spec amendments — five visible places, no orphaned
  debt (Tasks 1, 3, 5, 6, 7).
- **Section 9's fallback lever**: the `PERSIST_TYPED_ARTIFACTS`
  decision is Task 1's, by the design's own rule (flip visibly,
  publish honest numbers, never silently ship a dishonest one).
- **Plan 9b's handoffs, all three**: the README/SVG staleness
  (Task 6), the escalated release decisions (Task 1), the substance
  number's editorial call (fixed decision 5: published in the
  changelog).
- **Plan 7's and 4b's handoffs**: `mixed-rate`/`ground-truth` stay
  hidden (global constraint); the interim-home sentences in the
  bridge's rustdoc now point at the published page (Task 4).
- **The parent-spec amendment promised by the design's section 1**
  (the second milestone) already exists as the parent's 2026-07-14
  entry; Task 7 appends the release outcome against it, never a
  restatement.
- **Type consistency**: the drift tests consume
  `celerrate_diagnostics::REGISTRY` (re-exported from `lib.rs`,
  verified) and the message shapes quoted in the docs match plan 8's
  `checks/mod.rs` formats (`unknown method `{member}` on
  `{receiver}``, `accessing `{member}` on a possibly null
  `{receiver}``, `argument {position} of `{callee}` expects
  `{expected}`, `{given}` given`).
- **Placeholder scan**: every `«...»` names its exact source (the
  protocol's Results section, the three closing memos, the
  `verdicts.txt` header, the lowering rustdoc) — values that exist
  only after plans 8 through 9b execute, per the notes section; no
  "TBD" and no step without its content.

Known judgment calls, made deliberately: the release ledger is a task
rather than an assumption because three sibling plans escalate
decisions here by name; the substance number goes in the changelog
(fixed decision 5) because gating substance and then hiding the
number would be dishonest; the SVGs keep the two-bar layout (the
protocol table carries all five scenarios; the chart carries the
story); the lowering table is transcribed rather than inlined in this
plan because the rustdoc at the release commit is the authoritative
source and plans 6 through 8 may have amended rows after plan 4b
wrote them.

## Release ledger

Decided 2026-07-18, before any user-facing text was written.

- Families shipping in v0.0.3: all three (unknown members CEL0030 to
  CEL0033, nullability CEL0034, argument types CEL0035 to CEL0038),
  per plan 8's closing memo: clean triage. The corpus run diverged
  with 17 typed lines, and every one was a false positive closed by a
  family stance fix with a regression fixture, leaving no guillotine
  candidate ("Guillotine candidates: None.").
- `PERSIST_TYPED_ARTIFACTS`: true, unchanged, per plan 9b's closing
  memo: the sub-second-warm acceptance criterion holds across all five
  scenarios (flagship warm body-edit 0.521 s), so no warm-scenario
  miss was recorded and the lever was never pulled.
- Substance number: 25.0 % residual `mixed` rate on corpus expressions
  (1059 of 4233), published in the changelog entry. (Element-position
  rate: 7.4 %, 56 of 754, issue #45.)
- Published numbers (from `benchmarks/PROTOCOL.md`): cold full 1.533 s,
  warm no-change 0.434 s, warm one-edit 0.460 s, warm body-edit
  0.521 s (flagship), warm signature-edit 0.471 s; peak memory cold
  709 MiB against the 1536 MiB budget.
