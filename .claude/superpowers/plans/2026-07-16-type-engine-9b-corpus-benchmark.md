# Type Engine 9b — Corpus and Benchmark Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the benchmark protocol with the warm body-edit and
warm signature-edit scenarios, measure peak memory on the corpus with
inference active against its budget (deciding the LRU levers), and
re-run and re-publish the protocol numbers — including the substance
number and the cold-full trajectory position.

**Architecture:** Everything lands in `xtask` (which spawns the built
binary and external tools, never links a `celerrate_*` crate) plus the
published document `benchmarks/PROTOCOL.md`. The scripted edits are
computed in Rust as variant files and applied by `cp` inside
hyperfine's `--prepare`, so no PHP source is ever shell-quoted. Peak
RSS is parsed from `/usr/bin/time` (macOS `-l` and GNU `-v` formats).
Production changes are two, both small: the element-level counters in
the hidden `mixed-rate` instrument (task 4b, issue #45), and, only if
the measurement exceeds the budget, the `lru = N` lever on two salsa
queries.

**Tech Stack:** Rust 1.94, hyperfine, `/usr/bin/time`, salsa 0.27
(`lru = N` on tracked functions), GitHub Actions.

**Design source:** `.claude/superpowers/specs/2026-07-14-type-engine-design.md`
sections 6 (memory levers), 9 (the benchmark, the typed-artifact
economics, the substance gate) and 11 (plan 13, "9b — Corpus and
benchmark: the extended scenario set, the published numbers, peak
memory against its budget").

**Prerequisites:** Plans 6 (interprocedural), 7 (providers), 8
(checks), and 9a (cache) are merged and their gates are green. The
binary this plan measures has the three typed families (CEL0030 to
CEL0038), interprocedural inference, the phpdoc bridge, the stdlib
type provider, and the typed-artifact cache active by default. Tasks 1
to 3 only touch `xtask` and can be reviewed earlier, but the
measurement tasks (4 to 6) are meaningless before the prerequisites
merge.

## Global Constraints

- **Zero panic, mechanically enforced**: workspace lints deny
  `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`;
  `unsafe_code` is forbidden. Test modules may locally `#[allow]`. No
  indexing: `.get()`, `.first()`, iterators, `.split_once()`. The
  `/usr/bin/time` output parser takes arbitrary text and must never
  panic — the same contract as every other external-input reader.
- **TDD**: failing test → minimal implementation → refactor. The pure
  helpers (`edited_variant`, `restore_prime_apply`, `peak_bytes`,
  `over_budget`) are unit-tested in `xtask`; the orchestration around
  them (spawning hyperfine, `/usr/bin/time`) follows the existing
  `bench.rs` precedent of tested-helpers-plus-thin-orchestration.
- **xtask depends on no `celerrate_*` crate**: it only spawns `git`,
  `cargo`, `composer`, `hyperfine`, `/usr/bin/time`, and the built
  `celerrate` binary, so a broken build can never prevent regenerating
  what fixes it. `cargo xtask dependency-shape` stays green.
- **Determinism**: the scripted edits are pure functions of the pinned
  corpus content (`xtask/corpus.pin`, symfony/demo at
  `03fe25671b720b15103a2ff26934e94c87bd4d82`). A moved pin fails
  loudly with an error naming the missing needle — never a silent
  measurement of nothing.
- **Published numbers come from the reference hardware only** (Apple
  M5, the machine `benchmarks/PROTOCOL.md` names). CI ceilings are
  guard rails against structural regressions on noisy shared runners;
  they are never the target and never published.
- **No product-surface change**: the `mixed-rate` and `ground-truth`
  subcommands stay hidden; README, CHANGELOG, the benchmark SVGs, and
  the release itself belong to plan 9c. This plan publishes numbers in
  `benchmarks/PROTOCOL.md` and nowhere else. Task 4b grows the hidden
  instrument's report by one line (fixed decision 12): hidden output
  with a re-blessed baseline, not product surface, and
  `corpus-snapshot.txt` must stay byte-identical.
- **No lever flips silently**: a missed acceptance criterion is
  recorded and escalated to plan 9c's release decision; this plan
  never flips `PERSIST_TYPED_ARTIFACTS` (plan 9a's constant) or cuts a
  check family (one line in `celerrate_types/src/checks/mod.rs`, plan
  9c's call) on its own.
- **Everything in English, full words** (standard acronyms fine).
- **Commits**: gitmoji + Conventional Commits, repository-configured
  identity, no AI attribution of any kind.
- Local gate for every task: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`.

## Fixed decisions

1. **Scripted edits are Rust-computed variant files applied by `cp`.**
   The existing warm one-edit scenario appends a comment with
   `printf`; an in-place statement or signature edit through `sed` or
   `perl` inside hyperfine's `--prepare` string would shell-quote PHP
   source containing `'`, `$`, and `?`. Instead, `bench.rs` reads the
   pristine target once at setup, performs the string replacement in
   Rust (`edited_variant`, unit-tested, loud on a missing needle), and
   writes the variant next to the existing `edit-target-original.bak`
   under `target/bench/`. Every in-place scenario's prepare is then
   `cp <original> <target> && (<binary> check . > /dev/null || true)
   && cp <variant> <target>` — the exact structure the existing
   scenario already uses, with `cp` replacing `printf`.

2. **The warm body-edit target is `BlogController::search`.** The
   needle `['query' => (string) $request->query->get('q', '')]`
   (unique in the file at the pinned commit) becomes
   `['query' => trim((string) $request->query->get('q', ''))]`: one
   statement's expression changes, the signature does not — the body
   IR changes and the member tree backdates, which is precisely the
   edit class the comment-append scenario neutralizes. Recorded
   deviation from design section 9, acknowledged in review and also
   published in the protocol: the design describes warm body-edit as
   exercising inference invalidation through a changed inferred
   return, but on this corpus every application-code return is
   declared or annotated, so no app-code body edit can change a
   return type callers consume — the declared-return firewall the
   design leans on (design section 1) is active in this scenario.
   Consequence, stated plainly: no scenario in the set measures the
   warm cost of the changed-inferred-return invalidation path,
   because that edit class is unreachable in application code on this
   corpus. The correctness of that path is pinned by the
   invalidation-scope tests plans 6 and 9a own (design section 10,
   harness 2); the benchmark documents the residual through the
   per-scenario cache-statistics lines (decision 10) instead of
   pretending the corpus exercises it. The closing memo (task 6)
   names this deviation for plan 9c's ledger.

3. **The warm signature-edit target is `Post::getSlug()`.** The needle
   `public function getSlug(): ?string` (unique in
   `src/Entity/Post.php` at the pinned commit) becomes
   `public function getSlug(): string`: one member signature changes
   nullability, and its call sites span three other files
   (`src/Form/PostType.php`, `src/Controller/BlogController.php`,
   `src/EventSubscriber/CommentNotificationSubscriber.php`) — a real
   dependent fan-out through the member boundary, not a
   single-file tautology. Dropping `?` adds no diagnostic on the
   pinned corpus (nothing dereferences a wider type), so the scenario
   measures invalidation, not rendering.

4. **Cross-scenario state is absorbed by the in-prepare prime.** Each
   scenario's prepare restores only its own target; whatever state an
   earlier scenario left elsewhere in the working tree is constant
   within a scenario and absorbed into the cache by the prime that
   runs before every timed run, so every timed run measures exactly
   its scenario's one edit.

5. **CI ceilings**: the two new scenarios get the same 3.0-second
   guard rail as the existing warm scenarios. `COLD_CEILING_SECONDS`
   stays 30.0; if the type-engine cold run breaches it on shared
   runners while the local number stays sane, the ceiling is raised
   with the CI measurement recorded as justification — it is a guard
   rail, never a target (the `bench.rs` doc comment already says so).

6. **Peak memory is measured externally, budget 1536 MiB cold.**
   `cargo xtask memory` runs the release binary over the pinned corpus
   under `/usr/bin/time` — cold (`.celerrate` wiped) then warm — and
   parses peak RSS from both output dialects (macOS `-l`: bytes on a
   line ending in `maximum resident set size`; GNU `-v`:
   `Maximum resident set size (kbytes): N`). The budget is
   `PEAK_MEMORY_CEILING_BYTES = 1_610_612_736` (1.5 GiB), reconducted
   from the semantic core's closure budget ("cold peak RSS at most
   1.5 GiB", which measured 507 MiB then; inference is expected to
   grow it, the budget stands). The cold number is gated with
   `--ceiling`; the warm number is recorded, not gated. External
   measurement for the same reason the protocol uses hyperfine: the
   number includes everything the process allocates, not what an
   in-process probe remembers to count.

7. **The LRU decision rule** (the lever plan 9a explicitly left here):
   if the measured cold peak is at most 1536 MiB, no `lru` is set and
   the decision is recorded with the measurement as justification — in
   this plan's measurement memo and in the standing comment at the top
   of `crates/celerrate_types/src/inference.rs`. If it exceeds the
   budget, the two subjects the design names (section 6) get
   `lru = 4096`: `body_ir` (`crates/celerrate_semantics/src/body.rs`)
   and `inferred_body_types` (`crates/celerrate_types/src/inference.rs`)
   — body IR arenas and full expression type tables. The query names
   are the anchors: plans 6 to 9a touch both files, so any line
   number recorded at review time will have drifted by execution
   time. Inferred returns
   (`inferred_function_return` and its method sibling) stay resident:
   small, hot, the fixpoint's currency. Capacity procedure: measure at
   4096; halve (2048, 1024) until under budget; then re-run
   `cargo xtask bench` and require every warm median within 10% of its
   pre-LRU value (eviction thrash is a regression, not a saving), and
   re-run the fixpoint determinism suite. Contingency, stated because
   salsa's attribute grammar is the risk: if `lru` does not compose
   with `returns(ref)` on these queries, the query switches to
   returning an owned value and callers clone — recorded in the memo
   if taken.

8. **The flagship number becomes warm body-edit.** The design (section
   9) says the comment-append edit is precisely the class the body
   IR's early cutoff neutralizes — as flagship it would be
   near-tautological. Warm one-edit stays in the protocol as the
   trivia-cutoff demonstration; the protocol names warm body-edit the
   flagship. The README still says "0.29 seconds after you edit one
   file" until plan 9c rewrites it — a named handoff, not an
   inconsistency this plan fixes.

9. **Acceptance criterion and escalation.** All three warm scenarios
   (one-edit, body-edit, signature-edit) must have a median under
   1.0 second on the reference hardware — the design's "warm one-edit
   stays sub-second on the Symfony corpus with inference active across
   the scenario set". A miss is recorded honestly in the protocol
   results and escalated in this plan's measurement memo naming the
   scenario and the number; the release decision (ship, or flip plan
   9a's `PERSIST_TYPED_ARTIFACTS`) is plan 9c's.
   This plan publishes the honest number, whatever it is.

10. **Per-scenario cache-statistics lines are published.** One manual
    run per scenario with `CELERRATE_CACHE_STATS=1` records the full
    statistics line (post-9a: trees, verdicts, typed, signatures,
    persist clauses). This is how the design's load-bearing assumption
    — declared returns keep the recursive-revalidation frontier
    shallow — is published as data (`signatures_found` /
    `signatures_absent`, `typed_served` / `typed_recomputed`) instead
    of asserted.

11. **The substance number is the mixed-rate baseline's first line.**
    `xtask/mixed-rate-baseline.txt` (plan 7) starts with
    `expressions <total>\tmixed <count>`; the published substance
    number is `<count> / <total>` as a percentage, with the command
    that reproduces it (`cargo xtask mixed-rate`). The subcommand
    itself stays hidden (plan 7's rule: "plan 9b publishes the number";
    the product surface is plan 9c's).

12. **The element-level mixed metric joins the instrument (issue
    #45).** The hidden `mixed-rate` report gains a second summary
    line, `element-positions <total>\telement-mixed <count>`, between
    the whole-expression line and the per-callee table. A position is
    each structural constituent slot reached by a recursive walk over
    an expression's type: the key and value slots of array, list, and
    iterable types, and the field-value slots of shapes, recursing
    through those same containers and through each union constituent
    (the union itself is not a position; callable parameter and return
    slots are a recorded v0 exclusion, revisited only if a refinement
    ever sharpens one). A position counts as mixed when its type
    `is_mixed`. A wholly-`mixed` expression carries no structure and
    contributes zero positions: line 1 already counts it, so the two
    lines never double-count. Motivation, from issue #45's evidence:
    curating `array_slice`/`array_unique` in plan 7 improved ground
    truth (divergences 2 to 1) while line 1 stayed byte-identical at
    `expressions 4233\tmixed 1059`; the whole-expression rate is blind
    to element-type sharpening (`array<K, mixed>` to `array<K, Tag>`),
    so decision 11's published substance number carries both rates.
    The baseline is re-blessed once, and line 1 must survive the
    re-bless byte-identical (the new counters are additive, never a
    reclassification).

## File structure

- Modify: `xtask/src/bench.rs` — edit-variant helpers, two new
  scenarios, two new ceilings (tasks 1, 2).
- Create: `xtask/src/memory.rs` — peak-RSS measurement, parser, budget
  (task 3).
- Modify: `xtask/src/lib.rs` — `pub mod memory;`, crate doc sentence
  (task 3).
- Modify: `xtask/src/main.rs` — `memory [--ceiling]` dispatch and
  usage string (task 3).
- Modify: `.github/workflows/corpus.yml` — the `memory` job (task 3).
- Conditionally modify: `crates/celerrate_semantics/src/body.rs`,
  `crates/celerrate_types/src/inference.rs` — the `lru` lever, only if
  the budget is exceeded (task 4).
- Modify: `crates/celerrate_types/src/representation.rs` — the pure
  element-position walk, owned by the crate that owns the lattice
  (task 4b).
- Modify: `crates/celerrate_cli/src/mixed_rate.rs` — the second
  summary line, accumulation and rendering, format doc updated
  (task 4b).
- Modify: `xtask/mixed-rate-baseline.txt` — re-blessed once with the
  new line, line 1 byte-identical (task 4b).
- Modify: `benchmarks/PROTOCOL.md` — the extended scenario set, the
  what-is-enabled statement, peak memory, substance, results (task 5).
- Modify: this plan file — the measurement memo and closing memo
  appended (tasks 4, 5, 6).

---

### Task 1: The warm body-edit scenario

**Files:**
- Modify: `xtask/src/bench.rs`

**Interfaces:**
- Consumes: `quoted(path: &Path) -> String`, `crate::Result`, the
  `Scenario` struct, the existing `original` backup and `edit_target`
  variables inside `run()` (all already in `bench.rs`).
- Produces: `pub fn edited_variant(pristine: &str, needle: &str,
  replacement: &str) -> Result<String>`;
  `fn restore_prime_apply(original: &Path, target: &Path,
  quoted_binary: &str, variant: &Path) -> String`; the scenario name
  `"warm body-edit"` (a published string key: the protocol table row
  and the hyperfine export `target/bench/warm-body-edit.json`); the
  constants `BODY_EDIT_NEEDLE`, `BODY_EDIT_REPLACEMENT`,
  `WARM_BODY_EDIT_CEILING_SECONDS`. Task 2 reuses both functions.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module at the bottom of `xtask/src/bench.rs`
(inside `mod tests`, which already has
`#![allow(clippy::unwrap_used)]`):

```rust
    #[test]
    fn the_scripted_edit_replaces_the_pinned_needle() {
        let variant = super::edited_variant("a needle b", "needle", "thread").unwrap();
        assert_eq!(variant, "a thread b");
    }

    #[test]
    fn a_missing_needle_is_an_error_naming_it() {
        let error = super::edited_variant("nothing here", "needle", "thread").unwrap_err();
        assert!(error.to_string().contains("needle"));
        assert!(error.to_string().contains("corpus pin"));
    }

    #[test]
    fn the_body_edit_wraps_the_pinned_query_expression() {
        // A copy of the pinned line of src/Controller/BlogController.php
        // (symfony/demo at 03fe2567): if the pin moves, `edited_variant`
        // fails loudly at run time; this pins the needle against the
        // content the pin currently names.
        let pristine = "        return $this->render('blog/search.html.twig', ['query' => (string) $request->query->get('q', '')]);\n";
        let variant = super::edited_variant(
            pristine,
            super::BODY_EDIT_NEEDLE,
            super::BODY_EDIT_REPLACEMENT,
        )
        .unwrap();
        assert!(variant.contains("trim((string) $request->query->get('q', ''))"));
    }

    #[test]
    fn the_in_place_prepare_restores_primes_and_applies() {
        let command = super::restore_prime_apply(
            std::path::Path::new("/b/orig.bak"),
            std::path::Path::new("/w/File.php"),
            "'/bin/celerrate'",
            std::path::Path::new("/b/variant.php"),
        );
        assert_eq!(
            command,
            "cp '/b/orig.bak' '/w/File.php' && ('/bin/celerrate' check . > /dev/null || true) && cp '/b/variant.php' '/w/File.php'"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package xtask edited_variant`
Expected: compilation FAILS with `cannot find function edited_variant`
(and `restore_prime_apply`, `BODY_EDIT_NEEDLE`).

- [ ] **Step 3: Write the implementation**

In `xtask/src/bench.rs`, below the existing `EDIT_TARGET` /
`EDIT_TEXT` constants (after line 17), add:

```rust
/// The body-edit scenario's scripted edit, inside
/// `BlogController::search`: one statement's expression changes, the
/// signature does not — the body IR changes, the member tree
/// backdates. This is the edit class the comment-append scenario
/// neutralizes, and the protocol's flagship.
const BODY_EDIT_NEEDLE: &str = "['query' => (string) $request->query->get('q', '')]";
const BODY_EDIT_REPLACEMENT: &str = "['query' => trim((string) $request->query->get('q', ''))]";
```

Below the existing ceiling constants (after line 26), add:

```rust
const WARM_BODY_EDIT_CEILING_SECONDS: f64 = 3.0;
```

Below `quoted` (after line 202), add the two functions:

```rust
/// Applies one scripted edit to the pristine content of its target. A
/// missing needle means the corpus pin moved without the benchmark
/// following: a loud error, never a silent no-op measurement.
pub fn edited_variant(pristine: &str, needle: &str, replacement: &str) -> Result<String> {
    if !pristine.contains(needle) {
        return Err(format!(
            "the scripted-edit needle {needle:?} is not in the pristine target; \
             the corpus pin moved without the benchmark following"
        )
        .into());
    }
    Ok(pristine.replace(needle, replacement))
}

/// The prepare command of the in-place edit scenarios: restore the
/// pristine target, prime the cache on it, then apply the edited
/// variant — all through `cp`, so no PHP source is ever shell-quoted
/// inside the command string.
fn restore_prime_apply(
    original: &Path,
    target: &Path,
    quoted_binary: &str,
    variant: &Path,
) -> String {
    format!(
        "cp {} {} && ({quoted_binary} check . > /dev/null || true) && cp {} {}",
        quoted(original),
        quoted(target),
        quoted(variant),
        quoted(target),
    )
}
```

In `run()`, directly after the `std::fs::copy(&edit_target, &original)?;`
line (line 56), add the variant computation:

```rust
    // The in-place scripted edits, applied by `cp` from variant files
    // computed here in Rust: no shell-quoting of PHP source, and a
    // moved corpus pin fails loudly instead of measuring nothing.
    let pristine = std::fs::read_to_string(&edit_target)?;
    let body_variant = bench_directory.join("edit-target-body-variant.php");
    std::fs::write(
        &body_variant,
        edited_variant(&pristine, BODY_EDIT_NEEDLE, BODY_EDIT_REPLACEMENT)?,
    )?;
```

In the `scenarios` array, after the `"warm one-edit"` entry, add:

```rust
        Scenario {
            name: "warm body-edit",
            runs: 10,
            prepare: Some(restore_prime_apply(
                &original,
                &edit_target,
                &quoted_binary,
                &body_variant,
            )),
            ceiling_seconds: WARM_BODY_EDIT_CEILING_SECONDS,
        },
```

Below the `scenarios` array, directly above the
`prime(&binary, &working)?;` call, extend the existing priming
comment with the cross-scenario state rule (decision 4):

```rust
    // Cold full's last timed run already leaves a cache behind, but
    // this explicit prime guarantees the warm scenarios a cache to
    // start from regardless of scenario order. Each in-place scenario
    // restores only its own target: whatever state an earlier scenario
    // left elsewhere is constant within a scenario and absorbed by the
    // prime inside every prepare, so each timed run measures exactly
    // its scenario's one edit.
```

Also update the doc comment above `EDIT_TARGET` (lines 13-15) so it
covers both scenarios that touch the file:

```rust
/// The file the one-edit and body-edit scenarios touch, relative to
/// the corpus root. One-edit appends a comment (spans above stay put,
/// trivia the body IR ignores); body-edit replaces a statement's
/// expression through a variant file (the body IR changes).
const EDIT_TARGET: &str = "src/Controller/BlogController.php";
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package xtask`
Expected: PASS, including the four new tests.

- [ ] **Step 5: Verify the needle against the real pinned corpus**

Run:
```sh
cargo xtask fetch-corpus
grep -Fc "['query' => (string) \$request->query->get('q', '')]" \
  target/corpus/03fe25671b720b15103a2ff26934e94c87bd4d82/src/Controller/BlogController.php
```
Expected: `1` (the needle exists exactly once at the pinned commit).
`-F` matches the literal needle, exactly what `edited_variant` does
through `str::contains`; the `\$` is shell quoting, not regex.

- [ ] **Step 6: Run the local gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green, no formatting changes staged unexpectedly.

- [ ] **Step 7: Commit**

```bash
git add xtask/src/bench.rs
git commit -m "✨ feat(xtask): the warm body-edit benchmark scenario"
```

---

### Task 2: The warm signature-edit scenario

**Files:**
- Modify: `xtask/src/bench.rs`

**Interfaces:**
- Consumes: `edited_variant`, `restore_prime_apply` (task 1),
  `quoted`, the `Scenario` struct, `working` and `bench_directory`
  inside `run()`.
- Produces: the scenario name `"warm signature-edit"` (protocol table
  row, hyperfine export `target/bench/warm-signature-edit.json`); the
  constants `SIGNATURE_TARGET`, `SIGNATURE_EDIT_NEEDLE`,
  `SIGNATURE_EDIT_REPLACEMENT`, `WARM_SIGNATURE_EDIT_CEILING_SECONDS`.

- [ ] **Step 1: Write the failing test**

Append to the `tests` module of `xtask/src/bench.rs`:

```rust
    #[test]
    fn the_signature_edit_drops_null_from_the_pinned_getter() {
        // A copy of the pinned declaration in src/Entity/Post.php
        // (symfony/demo at 03fe2567); same pinning rationale as the
        // body-edit needle test.
        let pristine = "    public function getSlug(): ?string\n    {\n        return $this->slug;\n    }\n";
        let variant = super::edited_variant(
            pristine,
            super::SIGNATURE_EDIT_NEEDLE,
            super::SIGNATURE_EDIT_REPLACEMENT,
        )
        .unwrap();
        assert!(variant.contains("public function getSlug(): string"));
        assert!(!variant.contains("?string"));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --package xtask the_signature_edit_drops_null_from_the_pinned_getter`
Expected: compilation FAILS with `cannot find value SIGNATURE_EDIT_NEEDLE`.

- [ ] **Step 3: Write the implementation**

In `xtask/src/bench.rs`, below the body-edit constants, add:

```rust
/// The signature-edit scenario's target and scripted edit:
/// `Post::getSlug()` loses its `?` — one member signature changes
/// nullability, and its call sites span three other files (PostType,
/// BlogController, CommentNotificationSubscriber): a real dependent
/// fan-out through the member boundary. Dropping `?` adds no
/// diagnostic on the pinned corpus, so the scenario measures
/// invalidation, not rendering.
const SIGNATURE_TARGET: &str = "src/Entity/Post.php";
const SIGNATURE_EDIT_NEEDLE: &str = "public function getSlug(): ?string";
const SIGNATURE_EDIT_REPLACEMENT: &str = "public function getSlug(): string";
```

Below `WARM_BODY_EDIT_CEILING_SECONDS`, add:

```rust
const WARM_SIGNATURE_EDIT_CEILING_SECONDS: f64 = 3.0;
```

In `run()`, directly after the body-variant block added by task 1,
add:

```rust
    let signature_target = working.join(SIGNATURE_TARGET);
    let signature_original = bench_directory.join("signature-target-original.bak");
    std::fs::copy(&signature_target, &signature_original)?;
    let signature_pristine = std::fs::read_to_string(&signature_target)?;
    let signature_variant = bench_directory.join("signature-target-variant.php");
    std::fs::write(
        &signature_variant,
        edited_variant(
            &signature_pristine,
            SIGNATURE_EDIT_NEEDLE,
            SIGNATURE_EDIT_REPLACEMENT,
        )?,
    )?;
```

In the `scenarios` array, after the `"warm body-edit"` entry, add:

```rust
        Scenario {
            name: "warm signature-edit",
            runs: 10,
            prepare: Some(restore_prime_apply(
                &signature_original,
                &signature_target,
                &quoted_binary,
                &signature_variant,
            )),
            ceiling_seconds: WARM_SIGNATURE_EDIT_CEILING_SECONDS,
        },
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package xtask`
Expected: PASS.

- [ ] **Step 5: Verify the needle against the real pinned corpus**

Run:
```sh
grep -Fc "public function getSlug(): ?string" \
  target/corpus/03fe25671b720b15103a2ff26934e94c87bd4d82/src/Entity/Post.php
```
Expected: `1` (`-F` for the literal needle, as in task 1).

- [ ] **Step 6: Run the local gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green.

- [ ] **Step 7: Commit**

```bash
git add xtask/src/bench.rs
git commit -m "✨ feat(xtask): the warm signature-edit benchmark scenario"
```

---

### Task 3: The peak-memory subcommand

**Files:**
- Create: `xtask/src/memory.rs`
- Modify: `xtask/src/lib.rs`
- Modify: `xtask/src/main.rs`
- Modify: `.github/workflows/corpus.yml`

**Interfaces:**
- Consumes: `crate::corpus::prepare() -> Result<PathBuf>`
  (`xtask/src/corpus.rs:28`), `crate::release_binary() -> Result<PathBuf>`
  (`xtask/src/lib.rs:49`), `crate::Result`.
- Produces: `cargo xtask memory [--ceiling]`;
  `pub fn run(check_ceiling: bool) -> Result<()>`;
  `pub fn peak_bytes(output: &str) -> Result<u64>`;
  `pub fn over_budget(cold_peak_bytes: u64) -> Option<String>`;
  `pub const PEAK_MEMORY_CEILING_BYTES: u64 = 1_610_612_736`. Task 4
  consumes the command; task 5 records its output.

- [ ] **Step 1: Write the failing tests**

Create `xtask/src/memory.rs` with only the test module (the functions
do not exist yet, so this is the red step):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{over_budget, peak_bytes, PEAK_MEMORY_CEILING_BYTES};

    #[test]
    fn the_macos_time_output_reports_bytes() {
        let output = "        1.23 real         4.56 user         0.78 sys\n           123456789  maximum resident set size\n              1111  peak memory footprint\n";
        assert_eq!(peak_bytes(output).unwrap(), 123_456_789);
    }

    #[test]
    fn the_gnu_time_output_reports_kibibytes() {
        let output = "\tCommand being timed: \"celerrate check .\"\n\tMaximum resident set size (kbytes): 524288\n\tExit status: 0\n";
        assert_eq!(peak_bytes(output).unwrap(), 524_288 * 1024);
    }

    #[test]
    fn time_output_without_a_peak_is_an_error_not_a_panic() {
        assert!(peak_bytes("").is_err());
        assert!(peak_bytes("        1.23 real  0.1 user  0.1 sys").is_err());
        assert!(peak_bytes("Maximum resident set size (kbytes): not-a-number").is_err());
    }

    #[test]
    fn a_cold_peak_over_the_budget_is_named() {
        let failure = over_budget(PEAK_MEMORY_CEILING_BYTES + 1).unwrap();
        assert!(failure.contains("1536 MiB"));
        assert!(over_budget(PEAK_MEMORY_CEILING_BYTES).is_none());
    }
}
```

Declare the module: in `xtask/src/lib.rs`, add `pub mod memory;` to
the module list (alphabetical: between `pub mod ground_truth;` and
`pub mod mixed_rate;`).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package xtask memory`
Expected: compilation FAILS with `unresolved imports` (`over_budget`,
`peak_bytes`, `PEAK_MEMORY_CEILING_BYTES` do not exist).

- [ ] **Step 3: Write the implementation**

Fill `xtask/src/memory.rs` above the test module:

```rust
//! The peak-memory measurement behind the type engine's closure
//! criterion: the pinned corpus analyzed cold, then warm, with
//! everything the shipped binary enables, peak RSS parsed from
//! `/usr/bin/time`, the cold number gated against the budget. External
//! measurement for the same reason the protocol uses hyperfine: the
//! number includes everything the process allocates, not what an
//! in-process probe remembers to count.

use std::path::Path;
use std::process::Command;

use crate::Result;

/// The cold peak-RSS budget on the corpus, in bytes: 1.5 GiB,
/// reconducted from the semantic core's closure budget. The warm
/// number is recorded, never gated: a warm run reuses the cache and
/// sits below the cold peak or the cache is doing something wrong that
/// other gates catch.
pub const PEAK_MEMORY_CEILING_BYTES: u64 = 1_610_612_736;

const MEBIBYTE: u64 = 1024 * 1024;

/// Measures the cold and warm peak RSS on the pinned corpus and prints
/// both. With `check_ceiling`, a cold peak over the budget fails the
/// run.
pub fn run(check_ceiling: bool) -> Result<()> {
    let corpus = crate::corpus::prepare()?;
    let binary = crate::release_binary()?;
    let cache = corpus.join(".celerrate");
    if cache.exists() {
        std::fs::remove_dir_all(&cache)?;
    }
    let cold = measure(&binary, &corpus)?;
    let warm = measure(&binary, &corpus)?;
    println!("{:<16} {:>12}", "run", "peak rss");
    println!("{:<16} {:>8} MiB", "cold full", cold / MEBIBYTE);
    println!("{:<16} {:>8} MiB", "warm no-change", warm / MEBIBYTE);
    if check_ceiling {
        if let Some(failure) = over_budget(cold) {
            return Err(failure.into());
        }
    }
    Ok(())
}

/// One analysis under `/usr/bin/time`, peak RSS parsed from its
/// stderr. Exit 1 means diagnostics were reported — a completed
/// analysis, exactly as the benchmark and priming runs treat it.
fn measure(binary: &Path, working: &Path) -> Result<u64> {
    let flag = if cfg!(target_os = "macos") { "-l" } else { "-v" };
    let output = Command::new("/usr/bin/time")
        .arg(flag)
        .arg(binary)
        .args(["check", "."])
        .current_dir(working)
        .stdout(std::process::Stdio::null())
        .output()?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(format!(
            "the measured run did not complete (exit {:?})",
            output.status.code()
        )
        .into());
    }
    peak_bytes(&String::from_utf8_lossy(&output.stderr))
}

/// Parses the peak resident set size, in bytes, from `/usr/bin/time`
/// output: the BSD `-l` dialect (a byte count on a line ending in
/// `maximum resident set size`) and the GNU `-v` dialect
/// (`Maximum resident set size (kbytes): N`).
pub fn peak_bytes(output: &str) -> Result<u64> {
    for line in output.lines() {
        if let Some((_, value)) = line.split_once("Maximum resident set size (kbytes):") {
            let kibibytes: u64 = value
                .trim()
                .parse()
                .map_err(|error| format!("unreadable peak RSS {value:?}: {error}"))?;
            return Ok(kibibytes.saturating_mul(1024));
        }
        if let Some(value) = line.trim().strip_suffix("maximum resident set size") {
            return value
                .trim()
                .parse()
                .map_err(|error| format!("unreadable peak RSS {value:?}: {error}").into());
        }
    }
    Err("the time output carries no maximum resident set size".into())
}

/// The budget comparison, named like `bench::over_ceiling`.
pub fn over_budget(cold_peak_bytes: u64) -> Option<String> {
    (cold_peak_bytes > PEAK_MEMORY_CEILING_BYTES).then(|| {
        format!(
            "the cold peak RSS ({} MiB) is over the {} MiB budget",
            cold_peak_bytes / MEBIBYTE,
            PEAK_MEMORY_CEILING_BYTES / MEBIBYTE
        )
    })
}
```

Wire the dispatch in `xtask/src/main.rs`: add two arms after the
`bench` arms (lines 6-7):

```rust
        (Some("memory"), None) => xtask::memory::run(false),
        (Some("memory"), Some("--ceiling")) => xtask::memory::run(true),
```

and extend the usage string (line 26) to include
`| memory [--ceiling]` after `bench [--ceilings]`.

Update the crate doc in `xtask/src/lib.rs`: in the sentence listing
what xtask spawns, add `/usr/bin/time` to the list (`git`, `cargo`,
`composer`, `hyperfine`, `/usr/bin/time`, and the built `celerrate`
binary), and extend the task list sentence with: `memory` measures
peak RSS on the corpus against its budget.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package xtask memory`
Expected: PASS (4 tests).

- [ ] **Step 5: Add the CI job**

In `.github/workflows/corpus.yml`, after the `bench` job, add:

```yaml
  memory:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
      - uses: Swatinem/rust-cache@v2
      - uses: actions/cache@v4
        with:
          path: target/corpus
          key: corpus-${{ hashFiles('xtask/corpus.pin') }}
      - run: sudo apt-get update && sudo apt-get install --yes time
      - run: cargo xtask memory --ceiling
```

(The `time` package provides GNU `/usr/bin/time` on the runner; the
shell builtin is not enough.)

- [ ] **Step 6: Run the subcommand locally as a smoke check**

Run: `cargo xtask memory`
Expected: two lines, `cold full` and `warm no-change`, each with a
peak in MiB; exit 0. (No ceiling: this is the smoke check, the
measurement of record is task 4.)

- [ ] **Step 7: Run the local gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green.

- [ ] **Step 8: Commit**

```bash
git add xtask/src/memory.rs xtask/src/lib.rs xtask/src/main.rs .github/workflows/corpus.yml
git commit -m "✨ feat(xtask): peak-memory measurement against its budget"
```

---

### Task 4: The peak-memory measurement and the LRU decision

**Files:**
- Modify: this plan file (`.claude/superpowers/plans/2026-07-16-type-engine-9b-corpus-benchmark.md`)
- Conditionally modify: `crates/celerrate_semantics/src/body.rs`,
  `crates/celerrate_types/src/inference.rs`

**Interfaces:**
- Consumes: `cargo xtask memory` (task 3); the salsa queries
  `body_ir` (`crates/celerrate_semantics/src/body.rs`) and
  `inferred_body_types` (`crates/celerrate_types/src/inference.rs`);
  the query names are the anchors, line numbers drift (decision 7).
- Produces: the recorded LRU decision (the lever plan 9a's ledger
  explicitly left to this plan) and the peak numbers task 5 publishes.

This is the inherited debt's settlement: peak memory on the corpus
with inference active, against a budget — an acceptance number, not a
data point (design sections 6 and 9).

- [ ] **Step 1: Measure**

Run, on the reference hardware, at the current commit:

```sh
cargo xtask memory
```

Record the cold and warm peaks. Append a section to the END of this
plan file:

```markdown
## Measurement memo

### Peak memory (task 4)

Measured <date>, commit `<short sha>`, reference hardware
(Apple M5, 32 GiB): cold full <N> MiB, warm no-change <N> MiB,
budget 1536 MiB.
```

- [ ] **Step 2: Decide by the rule of fixed decision 7**

**If the cold peak is at most 1536 MiB** — the expected branch, given
the semantic core measured 507 MiB and the budget triples it:

Record in the memo: `Decision: within budget; no LRU capacity set —
the levers stay named and dormant.` Then update the standing comment
at the top of `crates/celerrate_types/src/inference.rs` (the
memory-lever paragraph of the crate doc, lines 7-13 at review time,
describes LRU as a future measurement) to record the outcome, for
example:

```rust
//! ... Body IR arenas and expression type tables remain the named
//! LRU candidates (`salsa` supports `lru = N` on tracked functions);
//! plan 9b measured the corpus cold peak at <N> MiB against the
//! 1536 MiB budget and set no capacity — the lever stays dormant
//! until a measurement demands it.
```

Skip to step 5.

**If the cold peak exceeds 1536 MiB**, continue with step 3.

- [ ] **Step 3 (conditional): Apply the LRU lever**

Change the attribute of `body_ir` in
`crates/celerrate_semantics/src/body.rs` (line 481 at review time;
find `pub fn body_ir`):

```rust
#[salsa::tracked(lru = 4096, returns(ref))]
pub fn body_ir<'db>(
```

and of `inferred_body_types` in
`crates/celerrate_types/src/inference.rs` (line 125 at review time;
find `pub fn inferred_body_types`):

```rust
#[salsa::tracked(lru = 4096, returns(ref))]
pub fn inferred_body_types<'db>(
```

Inferred returns (`inferred_function_return` and its method sibling)
are NOT touched: small, hot, the fixpoint's currency (design section
6). Contingency: if the salsa macro rejects `lru` combined with
`returns(ref)`, drop `returns(ref)` on that query, return the owned
value, and adjust its callers to take the value (a mechanical change;
record it in the memo).

Re-measure and titrate: `cargo xtask memory`; if still over budget,
halve the capacity (2048, then 1024) and re-measure; stop at the first
capacity under budget.

- [ ] **Step 4 (conditional): Verify eviction did not buy the saving with thrash or nondeterminism**

Run:
```sh
cargo test --package celerrate_types --test fixpoint
cargo test --workspace
cargo xtask bench
```
Expected: fixpoint suite green (including
`thread_fan_out_answers_identically` and
`an_edit_mid_fixpoint_unwinds_cleanly_and_serves_no_provisional_value`),
workspace green, and every warm median within 10% of its pre-LRU value
(compare against the task-1/2-era numbers or re-run once at the parent
commit). A warm regression over 10% means the capacity is too small:
double it back one step and re-measure memory — if no capacity
satisfies both the budget and the 10% rule, record the conflict in the
memo and escalate to plan 9c (fixed decision 9's path); do not ship a
thrashing eviction silently.

Record the final capacity and the post-LRU peaks in the memo.

- [ ] **Step 5: Run the gated measurement**

Run: `cargo xtask memory --ceiling`
Expected: exit 0 (the budget holds, with or without the lever).

- [ ] **Step 6: Run the local gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green.

- [ ] **Step 7: Commit**

If no LRU was set:

```bash
git add .claude/superpowers/plans/2026-07-16-type-engine-9b-corpus-benchmark.md crates/celerrate_types/src/inference.rs
git commit -m "📝 docs(types): record the peak-memory measurement and the dormant LRU decision"
```

If the lever was applied:

```bash
git add .claude/superpowers/plans/2026-07-16-type-engine-9b-corpus-benchmark.md crates/celerrate_semantics/src/body.rs crates/celerrate_types/src/inference.rs
git commit -m "⚡️ perf(types): LRU-evict body IR and expression type tables under the memory budget"
```

---

### Task 4b: The element-level mixed metric (issue #45)

Implements fixed decision 12. Amendment provenance: issue #45 assigned
this measurement improvement to plan 9b at filing time (PR #44's debt
list), but the plan as first written only consumed the instrument;
this task makes it observe the element dimension the refinement
channel actually improves. It must land before task 5 reads the
baseline.

**Files:**
- Modify: `crates/celerrate_types/src/representation.rs` — the pure
  walk (the CLI instrument must not match on `TypeData`; the lattice
  crate owns the traversal).
- Modify: `crates/celerrate_cli/src/mixed_rate.rs` — accumulation,
  the second summary line, the module-doc format description.
- Modify: `xtask/mixed-rate-baseline.txt` — one `--bless`.

**Interfaces:**
- Produces: `TypeId::element_positions(database) -> ElementPositions`
  (a small `{ total: usize, mixed: usize }` struct), and report line 2
  `element-positions <total>\telement-mixed <count>`.
- Consumes: nothing new; the walk reads the interned lattice the
  instrument already holds.

- [ ] **Step 1: RED — the pure walk**

Unit tests in `celerrate_types` pinning decision 12's definition
before any implementation exists:
- `array<string, mixed>` counts 2 positions, 1 mixed;
- a wholly-`mixed` type counts 0 positions;
- a shape with one `mixed` field and one `int` field counts 2
  positions, 1 mixed;
- nesting recurses (`array<int, array<int, mixed>>` counts the outer
  key, the outer value's own key and value: 4 positions, 1 mixed);
- a union's constituents are walked but the union itself is not a
  position;
- a callable's parameter and return slots contribute 0 positions (the
  recorded v0 exclusion).

The walk is over interned, finite types; no depth guard is needed
beyond what construction already enforces, and the tests must not
invent one.

- [ ] **Step 2: GREEN — implement `element_positions`**

- [ ] **Step 3: RED — the report line**

Extend `mixed_rate.rs`'s `render_report` tests: line 2 appears between
the summary line and the per-callee table, an empty corpus prints
`element-positions 0\telement-mixed 0`, and the existing expectations
are updated rather than duplicated.

- [ ] **Step 4: GREEN — wire the accumulation**

`accumulate` folds each expression's `element_positions` into two new
running totals; `render_report` takes and prints them; the module-doc
format description gains the line-2 sentence and names decision 12 and
issue #45.

- [ ] **Step 5: Re-bless and pin the additivity**

Run `cargo xtask mixed-rate --bless`, then verify by diff that line 1
of `xtask/mixed-rate-baseline.txt` is byte-identical to its pre-bless
state (`expressions 4233\tmixed 1059` at authoring time) and that the
per-callee table is unchanged: the new metric is additive, never a
reclassification. Record both line-2 numbers for task 5's step 3.

- [ ] **Step 6: Run the local gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo xtask corpus && cargo xtask mixed-rate`
Expected: green; `corpus-snapshot.txt` untouched (the instrument never
ships a diagnostic).

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_types crates/celerrate_cli/src/mixed_rate.rs xtask/mixed-rate-baseline.txt
git commit -m "✨ feat(types): the element-level mixed metric (issue #45)"
```

Closing issue #45 is the merge's call, once task 5 has published both
rates.

---

### Task 5: The protocol run and its publication

**Files:**
- Modify: `benchmarks/PROTOCOL.md`
- Modify: this plan file (measurement memo extended)

**Interfaces:**
- Consumes: the five scenarios (tasks 1, 2); `cargo xtask memory`
  (task 3, numbers of record from task 4); the `CELERRATE_CACHE_STATS`
  statistics line (plan 9a's extended render:
  trees / verdicts / typed / signatures / persist clauses); the first
  two lines of `xtask/mixed-rate-baseline.txt`
  (`expressions <total>\tmixed <count>`, plan 7, and
  `element-positions <total>\telement-mixed <count>`, task 4b).
- Produces: the published numbers plan 9c's release notes and README
  rewrite consume; the acceptance verdict of fixed decision 9.

- [ ] **Step 1: Run the protocol**

On the reference hardware, at the commit the results will name, run
three times:

```sh
cargo xtask bench
```

Each run prints five medians (cold full, warm no-change, warm
one-edit, warm body-edit, warm signature-edit). For each scenario,
take the median of the three runs — the same aggregation the previous
protocol run recorded. Record the fifteen raw values and the five
retained medians in this plan's measurement memo under a
`### Protocol run (task 5)` heading.

- [ ] **Step 2: Record the per-scenario cache-statistics lines**

The statistics line lands on stderr when `CELERRATE_CACHE_STATS=1`.
From the bench working copy (`target/bench/corpus`, left in place by
step 1), record one line per scenario:

```sh
cd target/bench/corpus
BIN=../../release/celerrate

# Restore both edit targets first: the bench run leaves its last
# scenario's variants applied, and every statistics line must
# describe the same tree state as its timed scenario.
cp ../edit-target-original.bak src/Controller/BlogController.php
cp ../signature-target-original.bak src/Entity/Post.php

# cold full
rm -rf .celerrate
CELERRATE_CACHE_STATS=1 "$BIN" check . > /dev/null

# warm no-change
CELERRATE_CACHE_STATS=1 "$BIN" check . > /dev/null

# warm one-edit (restore, prime, append the comment, measure)
cp ../edit-target-original.bak src/Controller/BlogController.php
"$BIN" check . > /dev/null || true
printf '\n// celerrate benchmark edit\n' >> src/Controller/BlogController.php
CELERRATE_CACHE_STATS=1 "$BIN" check . > /dev/null

# warm body-edit
cp ../edit-target-original.bak src/Controller/BlogController.php
"$BIN" check . > /dev/null || true
cp ../edit-target-body-variant.php src/Controller/BlogController.php
CELERRATE_CACHE_STATS=1 "$BIN" check . > /dev/null

# warm signature-edit
cp ../signature-target-original.bak src/Entity/Post.php
"$BIN" check . > /dev/null || true
cp ../signature-target-variant.php src/Entity/Post.php
CELERRATE_CACHE_STATS=1 "$BIN" check . > /dev/null
```

(Each command may exit 1 — diagnostics reported is a completed
analysis.) Copy the five statistics lines into the measurement memo.
These lines are the published evidence for the design's load-bearing
assumption: `signatures_found` / `signatures_absent` and
`typed_served` / `typed_recomputed` document how many verdicts depend
on inferred returns (decision 10).

- [ ] **Step 3: Compute the substance numbers**

Read the first two lines of `xtask/mixed-rate-baseline.txt`
(`expressions <total>\tmixed <count>`, then
`element-positions <total>\telement-mixed <count>`, task 4b) and
compute each `<count> / <total>` as a percentage with one decimal.
Record both in the memo alongside the exact numerators and
denominators.

- [ ] **Step 4: Rewrite `benchmarks/PROTOCOL.md`**

Replace the document's Scenarios, Reproduction, and Results sections
and add the What is enabled, Peak memory, and Substance sections. The
rewrite deliberately drops the current document's paragraph about
superseding the same-day 2026-07-13 run: that history lives in git,
and the Trajectory section carries the previous numbers forward. The
complete new structure (the `«...»` slots are filled in step 5 with
the numbers recorded in steps 1 to 3 and task 4 — nothing else in the
text is a slot):

````markdown
# Benchmark protocol

Every published Celerrate performance number comes from this protocol,
run on the hardware named below, and is reproducible by a third party
following this document. A number that would not survive third-party
scrutiny is not published.

## Corpus

- Repository: https://github.com/symfony/demo
- Commit: `03fe25671b720b15103a2ff26934e94c87bd4d82` (committed in `xtask/corpus.pin`)
- Vendor tree: installed from the corpus's own `composer.lock` via
  `composer install --no-interaction --no-progress --no-scripts
  --no-plugins --ignore-platform-reqs`
- Size: 9447 PHP files, 1302218 lines of PHP, vendor tree included -
  that is the tree `celerrate check` analyzes.

symfony/demo is the corpus because it has the exact shape
`celerrate check` is aimed at: a real user project, with application
code, a real `composer.json`, and the full Symfony vendor tree
installed from its lock file.

## Hardware and toolchain

- Machine: Apple M5, 10 cores (4 performance, 6 efficiency), 32 GiB memory, 1 TB NVMe SSD
- Operating system: macOS 26.5 (build 25F71)
- Rust toolchain: 1.94 (pinned in `rust-toolchain.toml`)
- Binary: `celerrate` built with `cargo build --release`, version
  `celerrate --version` reports at the commit the results name

## What is enabled

The measured binary is the default `celerrate check`, nothing disabled
and nothing added: the parse-level syntax diagnostics (CEL0002 to
CEL0017), the unknown-symbol families of the semantic core, the
version-availability family (CEL0021 to CEL0024), the three typed
families of the type engine (unknown members CEL0030 to CEL0033,
nullability CEL0034, argument types CEL0035 to CEL0038),
interprocedural type inference, the phpdoc bridge, the stdlib type
provider, inline suppressions, and the persistent cache including the
typed artifact classes. The numbers describe what a user runs.

## Method

The harness is `cargo xtask bench`. It fetches the corpus at the
pinned commit, installs the vendor tree, builds the release binary,
copies the corpus into a disposable working tree
(`target/bench/corpus`), and measures with [hyperfine](https://github.com/sharkdp/hyperfine),
which times the full process: startup, cache loading, analysis,
rendering. The in-place scripted edits are computed in Rust from the
pinned file contents and applied by copying variant files, so the
edits are pure functions of the corpus pin; a moved pin fails the
harness loudly.

- Aggregate: the median.
- hyperfine runs with `--ignore-failure`, because `celerrate check`
  exits 1 when it reports diagnostics - a completed analysis.
- "Cold" means no Celerrate cache (`.celerrate/` removed before every
  timed run); operating-system file caches are warm after the first
  run, and the protocol does not pretend otherwise.

## Scenarios

1. **Cold full** - 5 runs. Before each timed run: `rm -rf .celerrate`.
   Timed: `celerrate check .`. The complete analysis with nothing to
   reuse.
2. **Warm no-change** - 10 runs. The cache is primed once by a full
   run; nothing changes between runs. Timed: `celerrate check .`. The
   floor of the one-shot run.
3. **Warm one-edit** - 10 runs. Before each timed run: the edit target
   (`src/Controller/BlogController.php`) is restored, a full run
   primes the cache, then one comment line is appended. Timed:
   `celerrate check .`. A comment is trivia no annotation reader
   consumes: the body representation is unchanged, and this scenario
   demonstrates that cutoff - the floor of the edit path, not its
   cost.
4. **Warm body-edit** - 10 runs. Same restore-and-prime, then one
   statement inside `BlogController::search` changes (its expression
   is wrapped in `trim(...)`): the body changes, the signature does
   not. **This is the flagship number**: a full CLI run, wall clock,
   process startup and cache loading included, on the edit class a
   save-and-rerun user actually produces. Target: sub-second.
5. **Warm signature-edit** - 10 runs. Same restore-and-prime on
   `src/Entity/Post.php`, then `Post::getSlug(): ?string` becomes
   `: string`: one member signature changes, and its dependents
   (call sites in three other files) re-check. Target: sub-second.

On this corpus every application-code return is declared or annotated,
so a body edit cannot change a return type callers consume: the
declared-return firewall the type engine's design leans on is active
in scenario 4, and the per-scenario cache statistics under Results
document the residual (how many verdicts depend on inferred returns).

## Peak memory

`cargo xtask memory` analyzes the corpus cold, then warm, under
`/usr/bin/time`, and reports the peak resident set size. The cold
number is gated (`--ceiling`) against a budget of 1536 MiB,
reconducted from the semantic core's closure budget.

| Run | Peak RSS |
| --- | --- |
| Cold full | «cold» MiB |
| Warm no-change | «warm» MiB |

«One sentence recording the LRU decision of plan 9b task 4: either
"within budget, no eviction configured" or the configured capacity.»

## Substance

Precision gates alone cannot distinguish a precise engine from a
silent one. The published substance number is the residual `mixed`
rate on the corpus's expressions: «count» of «total» expressions
(«rate» %) infer to `mixed`, measured by the committed baseline behind
`cargo xtask mixed-rate`. At element level, «element-count» of
«element-total» structural element positions («element-rate» %) are
`mixed`: the whole-expression rate is blind to element-type sharpening
(`array<K, mixed>` to `array<K, Tag>`), so both rates are published
(issue #45). The seeded-defect suite
(`cargo test --package celerrate_cli --test seeded_defects`) is the
per-family recall gate: nine known defects, each reported.

## Reproduction

```sh
# prerequisites: rust 1.94, git, composer, php, hyperfine,
# and GNU time on Linux (macOS ships /usr/bin/time)
cargo xtask bench
cargo xtask memory
```

## What is not compared

No PHPStan (or other tool) comparison is published at v0.0.x: the
preview runs a handful of diagnostic families while PHPStan runs
hundreds of rules, and a cross-scope timing comparison would be
meaningless at best and misleading at worst. The matched-scope
comparison is the v0.1 claim.

## Results

Protocol run of «date», at commit `«short sha»` (recorded as the
median of three protocol runs; the raw hyperfine exports live under
`target/bench/` and are not committed):

| Scenario | Median |
| --- | --- |
| Cold full | «» s |
| Warm no-change | «» s |
| Warm one-edit | «» s |
| Warm body-edit | «» s |
| Warm signature-edit | «» s |

The warm body-edit number is the published flagship; the README links
here.

Per-scenario cache statistics (one manual run each with
`CELERRATE_CACHE_STATS=1`, recorded verbatim):

```
cold full:            «statistics line»
warm no-change:       «statistics line»
warm one-edit:        «statistics line»
warm body-edit:       «statistics line»
warm signature-edit:  «statistics line»
```

### Trajectory

The previous protocol run (2026-07-13, semantic core, commit
`24b6950`) recorded cold full 1.11 s, warm no-change 0.28 s, warm
one-edit 0.29 s with two diagnostic families and no inference. This
run records cold full «» s with the full type engine enabled - a
«multiplier»x change against the previous cold number. The parent
design's v0.1 ambition is roughly 20x faster than PHPStan at matched
scope; that comparison is deliberately not published here (see What is
not compared), and this cold number is the trajectory data point the
v0.1 measurement will be judged against.
````

- [ ] **Step 5: Fill the slots**

Replace every `«...»` slot in the rewritten document with the values
recorded in steps 1 to 3 and task 4's memo. Verify no `«` remains:

```sh
! grep -c '«' benchmarks/PROTOCOL.md
```
Expected: `0` matches (the negated grep exits 0).

- [ ] **Step 6: Apply the acceptance criterion**

Check the three warm-edit medians (one-edit, body-edit,
signature-edit) against 1.0 second (fixed decision 9).

**If all three are under 1.0 s:** record `Acceptance: sub-second warm
across the scenario set — holds.` in the measurement memo.

**If any is not:** the number stays published exactly as measured, and
the memo gains an escalation entry:

```markdown
### Escalation (task 5)

The «scenario» median («N» s) misses the sub-second criterion. The
release decision is plan 9c's: ship with the number documented, or
flip `PERSIST_TYPED_ARTIFACTS` (plan 9a's lever — warm converges
toward cold-with-inference, the criterion is missed honestly).
Nothing was flipped in this plan.
```

- [ ] **Step 7: Run the local gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: green (documentation-only task; the gate guards against
accidental collateral edits).

- [ ] **Step 8: Commit**

```bash
git add benchmarks/PROTOCOL.md .claude/superpowers/plans/2026-07-16-type-engine-9b-corpus-benchmark.md
git commit -m "📝 docs(benchmarks): the type-engine protocol run and its published numbers"
```

---

### Task 6: Closure — the full gates and the 9c handoffs

**Files:**
- Modify: this plan file (closing memo appended)

**Interfaces:**
- Consumes: every gate the sub-project owns.
- Produces: the closed plan; the named handoffs plan 9c starts from.

- [ ] **Step 1: Run every gate**

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
cargo xtask dependency-shape
cargo xtask corpus
cargo xtask ground-truth
cargo xtask mixed-rate
cargo xtask bench --ceilings
cargo xtask memory --ceiling
```

Expected: all green. `cargo xtask corpus` must find
`xtask/corpus-snapshot.txt` byte-identical — this plan changed no
analysis behavior unless the LRU lever was applied, and eviction must
never change results, only memory (the fixpoint suite pinned that in
task 4).

- [ ] **Step 2: Append the closing memo**

Append to this plan file:

```markdown
## Closing memo

Closed «date», commit `«short sha»`.

- Scenario set: five scenarios, flagship warm body-edit («N» s).
- Recorded deviation (fixed decision 2): no scenario measures the
  warm cost of the changed-inferred-return invalidation path, because
  the corpus's declared-return coverage makes that edit class
  unreachable in application code; correctness is pinned by the
  plans 6 and 9a invalidation-scope tests, and the residual is
  published through the per-scenario cache statistics.
- Acceptance (sub-second warm across the set): «holds / escalated».
- Peak memory: cold «N» MiB against the 1536 MiB budget; LRU
  «not configured / configured at N».
- Substance: «rate» % residual whole-expression mixed rate and
  «element-rate» % element-position mixed rate (issue #45), published
  in the protocol.
- Signature-pack keying (plan 9a's ledger assigns this plan's numbers
  the revisit of its file-hash coarsening): «the coarseness does not
  matter / a per-body hash is warranted (a pack-only change) /
  re-deferred», judged from `typed_served` / `typed_recomputed` on
  the edit scenarios.

Handoffs to plan 9c (release):
- The README performance section and the benchmark SVGs
  (`assets/benchmark-dark.svg`, `assets/benchmark-light.svg`) still
  show the semantic-core numbers and the old flagship; the rewrite
  aligns them with `benchmarks/PROTOCOL.md`.
- The release decision on any escalation recorded above
  (`PERSIST_TYPED_ARTIFACTS`) — plan 9c's call, per its ledger.
- The `mixed-rate` and `ground-truth` subcommands stay hidden; whether
  the release notes mention the substance number is 9c's editorial
  decision.
```

- [ ] **Step 3: Commit**

```bash
git add .claude/superpowers/plans/2026-07-16-type-engine-9b-corpus-benchmark.md
git commit -m "📝 docs(plans): close plan 9b with the measurement memo and the 9c handoffs"
```

## Measurement memo

### Peak memory (task 4)

Measured 2026-07-18, commit `9db57ac`, reference hardware
(Apple M5, 32 GiB): cold full 709 MiB, warm no-change 342 MiB,
budget 1536 MiB.

Decision: within budget; no LRU capacity set — the levers stay named
and dormant.

### Protocol run (task 5)

Measured 2026-07-18, commit `a8382be`, reference hardware (Apple M5,
32 GiB), `cargo xtask bench`, three runs.

Raw hyperfine medians, per run (seconds):

| Scenario | Run 1 | Run 2 | Run 3 | Retained median (median of 3) |
| --- | --- | --- | --- | --- |
| Cold full | 1.532 | 1.533 | 2.438 | **1.533** |
| Warm no-change | 0.435 | 0.427 | 0.434 | **0.434** |
| Warm one-edit | 0.482 | 0.460 | 0.458 | **0.460** |
| Warm body-edit | 0.468 | 0.635 | 0.521 | **0.521** |
| Warm signature-edit | 0.471 | 0.469 | 0.538 | **0.471** |

Run 3's cold outlier (2.438 s) and Run 2's body-edit first-run-slow
(0.635 s) were both absorbed by the median-of-three.

Per-scenario cache statistics (`CELERRATE_CACHE_STATS=1`, verbatim):

```
cold full:            cache: trees 0 hit / 9341 miss; members 0 hit / 9341 miss; verdicts 0 served / 0 discarded / 46 absent; typed 217 bodies, edges 794 declared / 25 inferred / 7 provider, verdicts 0 served / 46 recomputed; persist 4 written / 0 skipped / 0 failed, 323ms
warm no-change:       cache: trees 9341 hit / 0 miss; members 9341 hit / 0 miss; verdicts 46 served / 0 discarded / 0 absent; typed 0 bodies, edges 0 declared / 0 inferred / 0 provider, verdicts 46 served / 0 recomputed; persist 0 written / 4 skipped / 0 failed, 37ms
warm one-edit:        cache: trees 9340 hit / 1 miss; members 9340 hit / 1 miss; verdicts 45 served / 0 discarded / 1 absent; typed 5 bodies, edges 23 declared / 0 inferred / 0 provider, verdicts 45 served / 1 recomputed; persist 4 written / 0 skipped / 0 failed, 63ms
warm body-edit:       cache: trees 9340 hit / 1 miss; members 9340 hit / 1 miss; verdicts 45 served / 0 discarded / 1 absent; typed 5 bodies, edges 24 declared / 0 inferred / 0 provider, verdicts 45 served / 1 recomputed; persist 4 written / 0 skipped / 0 failed, 62ms
warm signature-edit:  cache: trees 9340 hit / 1 miss; members 9340 hit / 1 miss; verdicts 45 served / 0 discarded / 1 absent; typed 65 bodies, edges 367 declared / 3 inferred / 1 provider, verdicts 37 served / 9 recomputed; persist 4 written / 0 skipped / 0 failed, 65ms
```

Substance numbers (`xtask/mixed-rate-baseline.txt`): whole-expression
mixed rate 1059 / 4233 = 25.0 %; element-position mixed rate
56 / 754 = 7.4 % (issue #45).

### Acceptance (task 5)

Acceptance: sub-second warm across the scenario set — holds.
