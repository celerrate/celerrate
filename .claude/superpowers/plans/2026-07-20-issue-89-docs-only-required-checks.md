# Docs-Only Required Status Checks Implementation Plan (issue #89)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every required branch-protection context report on every pull request, so a documentation-only pull request goes from open to merged by auto-merge alone, with zero manual intervention and `enforce_admins` untouched.

**Architecture:** The `changes` job in each workflow stays the single source of truth for "did code move". The eight jobs whose contexts are required by branch protection stop skipping at the **job level** and instead gate every **step** on the `changes` output: the job always starts (so the `test` matrix always expands and every required context always reports) and no-ops in seconds on a documentation-only change. Non-required jobs keep the cheaper job-level skip.

**Tech Stack:** GitHub Actions YAML, `gh` CLI, `actionlint` (via Docker) for validation. No Rust code is touched.

## Context: the bug being fixed

PR #88 (documentation only) sat permanently `BLOCKED` with auto-merge armed. The CI path filter skips the `test` job at the job level on documentation-only changes, so the OS matrix never expands: the run reports one skipped `test` context, and the three required per-OS contexts `test (ubuntu-latest)`, `test (macos-latest)`, `test (windows-latest)` are **never reported at all**. GitHub waits forever for checks that will never arrive (`3 of 10 required status checks are expected`). Because `enforce_admins` is enabled, even `gh pr merge --admin` is refused.

Key mechanics an engineer needs to know:

- A **skipped non-matrix job** still reports its context with conclusion `skipped`, which branch protection accepts — that is why 7 of the 10 required checks were satisfied on #88.
- A **skipped matrix job** never expands, so its per-OS contexts are never created — that is the deadlock.
- Issue #89 directs converting **all eight required jobs** (not just `test`) to step-level gating, for a uniform contract: every required context reports success on every pull request, whatever the paths touched.
- On `pull_request` events GitHub runs the workflow definitions from the pull request itself, so the fix PR exercises its own new logic. Since it touches `.github/workflows/*.yml` (not in the inert-documentation allowlist), `changes` resolves `code=true` and the full suite runs on it.
- A step whose `if` is false is skipped without failing, and subsequent steps still evaluate their own conditions (a skipped step does not break `success()`). Composite-action post-steps (`Swatinem/rust-cache`, `actions/cache`) do not run for skipped steps, so a no-op job saves nothing to caches. This is why gating each step individually is both sufficient and cheap.

## Global Constraints

- The ten required contexts on `main` (read from the live branch protection on 2026-07-20, app_id 15368) are exactly: `bench`, `deny`, `fuzz`, `lint`, `phpdoc-cases`, `snapshot`, `stubs`, `test (macos-latest)`, `test (ubuntu-latest)`, `test (windows-latest)`.
- Branch protection settings are read-only for this plan: never edit the required-context list, never toggle `enforce_admins` (it stays `enabled: true` throughout — that is part of the acceptance criterion).
- The `changes` job bodies stay byte-for-byte identical in `ci.yml`, `corpus.yml`, and `fuzz.yml` (only the comment block above the job changes). The inert-documentation allowlist (`*.md|LICENSE*|.gitignore|.gitattributes|.editorconfig`) is not modified.
- The non-required corpus jobs `ground-truth`, `mixed-rate`, `memory` keep their job-level skip unchanged.
- `.github/workflows/release.yml` is not touched (tag-triggered, no required contexts).
- Toolchain pins stay as they are: `toolchain: "1.94"` everywhere, `nightly` for fuzz.
- Commits: gitmoji + Conventional Commits, repository-configured git identity, no Claude attribution anywhere. Everything in English, full words.
- Repository: `celerrate/celerrate`. Merge method: merge commit (`--merge`), matching the existing history.

## File Structure

- Modify: `.github/workflows/ci.yml` — jobs `lint`, `test`, `deny`, `stubs` (all required) move to step-level gating; comment above `changes` rewritten.
- Modify: `.github/workflows/corpus.yml` — jobs `snapshot`, `bench`, `phpdoc-cases` (required) move to step-level gating; `ground-truth`, `mixed-rate`, `memory` (not required) untouched; header comment rewritten.
- Modify: `.github/workflows/fuzz.yml` — job `fuzz` (required) moves to step-level gating through a job-level `env.RUN_FUZZ` variable (its condition is compound, so one evaluation point beats eleven copies); header comment rewritten.
- Create (Task 5 only): this plan file itself is committed as the documentation-only acceptance pull request.

## Validation tooling

`actionlint` is not installed locally, but Docker is. Validate with:

```bash
docker run --rm -v "$PWD":/repo -w /repo rhysd/actionlint:latest -color
```

Expected: **no output, exit code 0** (actionlint is silent on success; the first invocation pulls the image). If Docker is unavailable in the execution environment, skip the actionlint steps and rely on the grep assertions plus GitHub's own workflow parsing on push — a parse error surfaces immediately as a failed run.

---

### Task 1: ci.yml — required jobs always report

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: the existing `changes` job output `needs.changes.outputs.code` (string `'true'` or `'false'`).
- Produces: jobs `lint`, `test`, `deny`, `stubs` that always start and report; every step guarded by `if: ${{ needs.changes.outputs.code == 'true' }}`. Tasks 2 and 3 follow the same convention; Task 4 ships all three files in one pull request.

- [ ] **Step 1: Create the working branch and record the failing baseline**

Create/enter the worktree branch (superpowers:using-git-worktrees at execution time):

```bash
git checkout -b fix-89-docs-only-required-checks
grep -cE "^    if:" .github/workflows/ci.yml
```

Expected: `4` (the four job-level gates on `lint`, `test`, `deny`, `stubs` — the configuration this task removes). The live failure itself is documented on issue #89 and PR #88; it cannot be reproduced locally.

- [ ] **Step 2: Rewrite `.github/workflows/ci.yml`**

Replace the entire file with exactly this content (the `on`/`permissions`/`concurrency`/`env` header and the `changes` job body are unchanged; the comment above `changes` is rewritten and the four downstream jobs are converted):

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}

env:
  CARGO_TERM_COLOR: always

jobs:
  # Decide whether the change touches anything the build, tests, lints or
  # corpus depend on. Everything counts as code except a small allowlist of
  # inert documentation files, so a misclassification can only run the full
  # suite unnecessarily, never skip it on a real code change. Every job below
  # carries a required branch-protection context, so none of them may skip at
  # the job level: a skipped matrix job never expands, its per-OS contexts
  # are never reported, and branch protection waits forever (issue #89).
  # Instead the jobs always start and every step gates on `code`; on a
  # documentation-only pull request each job no-ops in seconds and reports
  # success, so auto-merge can fire without manual intervention.
  changes:
    runs-on: ubuntu-latest
    outputs:
      code: ${{ steps.filter.outputs.code }}
    steps:
      - uses: actions/checkout@v7
        with:
          fetch-depth: 0
      - id: filter
        shell: bash
        env:
          EVENT_NAME: ${{ github.event_name }}
          PR_BASE: ${{ github.event.pull_request.base.sha }}
          PR_HEAD: ${{ github.event.pull_request.head.sha }}
          PUSH_BEFORE: ${{ github.event.before }}
        run: |
          if [ "$EVENT_NAME" = "pull_request" ]; then
            base="$PR_BASE"
            head="$PR_HEAD"
          else
            base="$PUSH_BEFORE"
            head="$GITHUB_SHA"
          fi
          # Run the full suite whenever the base is missing or unknown (for
          # example a new branch, whose before-sha is all zeros): never skip
          # on uncertainty.
          valid=true
          if [ -z "$base" ] || [ "$base" = "0000000000000000000000000000000000000000" ]; then
            valid=false
          elif ! git cat-file -e "${base}^{commit}" 2>/dev/null; then
            valid=false
          fi
          if [ "$valid" = false ]; then
            echo "base unavailable; running the full suite"
            echo "code=true" >> "$GITHUB_OUTPUT"
            exit 0
          fi
          files="$(git diff --name-only "$base" "$head")"
          echo "changed files:"
          printf '%s\n' "$files"
          # A change counts as code unless every changed file is inert
          # documentation. Any other path forces the full suite, so a
          # misclassification can only over-run, never skip a real change.
          code=false
          while IFS= read -r file; do
            [ -z "$file" ] && continue
            case "$file" in
              *.md|LICENSE*|.gitignore|.gitattributes|.editorconfig) ;;
              *) code=true ;;
            esac
          done <<< "$files"
          echo "resolved code=$code"
          echo "code=$code" >> "$GITHUB_OUTPUT"

  lint:
    needs: changes
    runs-on: ubuntu-latest
    steps:
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/checkout@v7
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
          components: clippy, rustfmt
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: Swatinem/rust-cache@v2
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo fmt --all --check
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo clippy --workspace --all-targets -- -D warnings
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo xtask dependency-shape

  test:
    needs: changes
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/checkout@v7
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
          components: rustfmt
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: Swatinem/rust-cache@v2
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo test --workspace

  deny:
    needs: changes
    runs-on: ubuntu-latest
    steps:
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/checkout@v7
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: EmbarkStudios/cargo-deny-action@v2

  stubs:
    needs: changes
    runs-on: ubuntu-latest
    steps:
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/checkout@v7
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
          components: clippy
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: Swatinem/rust-cache@v2
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo clippy --package celerrate_stubs --features compiler --all-targets -- -D warnings
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo xtask compile-stubs --check
```

- [ ] **Step 3: Validate with actionlint**

```bash
docker run --rm -v "$PWD":/repo -w /repo rhysd/actionlint:latest -color .github/workflows/ci.yml
```

Expected: no output, exit code 0.

- [ ] **Step 4: Assert the gating shape**

```bash
grep -cE "^    if:" .github/workflows/ci.yml || true
grep -c "needs.changes.outputs.code == 'true'" .github/workflows/ci.yml
```

Expected: `0` (no job-level gate left in this file), then `17` (step guards: 6 in `lint`, 4 in `test`, 2 in `deny`, 5 in `stubs`).

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "💚 ci: gate the required CI jobs at step level (#89)"
```

---

### Task 2: corpus.yml — required jobs always report, optional jobs keep skipping

**Files:**
- Modify: `.github/workflows/corpus.yml`

**Interfaces:**
- Consumes: the same `needs.changes.outputs.code` output and the step-guard convention from Task 1.
- Produces: `snapshot`, `bench`, `phpdoc-cases` always start and report; `ground-truth`, `mixed-rate`, `memory` keep their job-level `if` untouched.

- [ ] **Step 1: Record the failing baseline**

```bash
grep -cE "^    if:" .github/workflows/corpus.yml
```

Expected: `6` (all six downstream jobs currently gate at the job level).

- [ ] **Step 2: Rewrite `.github/workflows/corpus.yml`**

Replace the entire file with exactly this content (header and `changes` body unchanged; the comment above `changes` rewritten; `snapshot`, `bench`, `phpdoc-cases` converted; `ground-truth`, `mixed-rate`, `memory` byte-identical to before):

```yaml
name: Corpus

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}

env:
  CARGO_TERM_COLOR: always

jobs:
  # See ci.yml's `changes` job. The snapshot, bench and phpdoc-cases contexts
  # are required by branch protection, so those jobs always start and gate
  # every step on `code` (issue #89): on a documentation-only pull request
  # they no-op in seconds and report success. The ground-truth, mixed-rate
  # and memory contexts are not required, so those jobs keep the cheaper
  # job-level skip.
  changes:
    runs-on: ubuntu-latest
    outputs:
      code: ${{ steps.filter.outputs.code }}
    steps:
      - uses: actions/checkout@v7
        with:
          fetch-depth: 0
      - id: filter
        shell: bash
        env:
          EVENT_NAME: ${{ github.event_name }}
          PR_BASE: ${{ github.event.pull_request.base.sha }}
          PR_HEAD: ${{ github.event.pull_request.head.sha }}
          PUSH_BEFORE: ${{ github.event.before }}
        run: |
          if [ "$EVENT_NAME" = "pull_request" ]; then
            base="$PR_BASE"
            head="$PR_HEAD"
          else
            base="$PUSH_BEFORE"
            head="$GITHUB_SHA"
          fi
          # Run the full suite whenever the base is missing or unknown (for
          # example a new branch, whose before-sha is all zeros): never skip
          # on uncertainty.
          valid=true
          if [ -z "$base" ] || [ "$base" = "0000000000000000000000000000000000000000" ]; then
            valid=false
          elif ! git cat-file -e "${base}^{commit}" 2>/dev/null; then
            valid=false
          fi
          if [ "$valid" = false ]; then
            echo "base unavailable; running the full suite"
            echo "code=true" >> "$GITHUB_OUTPUT"
            exit 0
          fi
          files="$(git diff --name-only "$base" "$head")"
          echo "changed files:"
          printf '%s\n' "$files"
          # A change counts as code unless every changed file is inert
          # documentation. Any other path forces the full suite, so a
          # misclassification can only over-run, never skip a real change.
          code=false
          while IFS= read -r file; do
            [ -z "$file" ] && continue
            case "$file" in
              *.md|LICENSE*|.gitignore|.gitattributes|.editorconfig) ;;
              *) code=true ;;
            esac
          done <<< "$files"
          echo "resolved code=$code"
          echo "code=$code" >> "$GITHUB_OUTPUT"

  snapshot:
    needs: changes
    runs-on: ubuntu-latest
    steps:
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/checkout@v7
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: Swatinem/rust-cache@v2
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/cache@v4
        with:
          path: target/corpus
          key: corpus-${{ hashFiles('xtask/corpus.pin') }}
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo xtask corpus

  ground-truth:
    needs: changes
    if: ${{ needs.changes.outputs.code == 'true' }}
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
      - run: cargo xtask ground-truth

  mixed-rate:
    needs: changes
    if: ${{ needs.changes.outputs.code == 'true' }}
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
      - run: cargo xtask mixed-rate

  bench:
    needs: changes
    runs-on: ubuntu-latest
    steps:
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/checkout@v7
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: Swatinem/rust-cache@v2
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/cache@v4
        with:
          path: target/corpus
          key: corpus-${{ hashFiles('xtask/corpus.pin') }}
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: sudo apt-get update && sudo apt-get install --yes hyperfine
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo xtask bench --ceilings

  memory:
    needs: changes
    if: ${{ needs.changes.outputs.code == 'true' }}
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

  phpdoc-cases:
    needs: changes
    runs-on: ubuntu-latest
    steps:
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/checkout@v7
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: Swatinem/rust-cache@v2
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/cache@v4
        with:
          path: target/phpdoc-parser
          key: phpdoc-parser-${{ hashFiles('xtask/phpdoc-parser.pin') }}
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo xtask phpdoc-cases --check
```

- [ ] **Step 3: Validate with actionlint**

```bash
docker run --rm -v "$PWD":/repo -w /repo rhysd/actionlint:latest -color .github/workflows/corpus.yml
```

Expected: no output, exit code 0.

- [ ] **Step 4: Assert the gating shape**

```bash
grep -cE "^    if:" .github/workflows/corpus.yml
grep -c "needs.changes.outputs.code == 'true'" .github/workflows/corpus.yml
```

Expected: `3` (job-level gates kept on `ground-truth`, `mixed-rate`, `memory` only), then `19` (16 step guards: 5 in `snapshot`, 6 in `bench`, 5 in `phpdoc-cases`; plus the 3 kept job-level gates).

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/corpus.yml
git commit -m "💚 ci: gate the required corpus jobs at step level (#89)"
```

---

### Task 3: fuzz.yml — the fuzz job always reports

**Files:**
- Modify: `.github/workflows/fuzz.yml`

**Interfaces:**
- Consumes: the same `needs.changes.outputs.code` output.
- Produces: a `fuzz` job that always starts; its former compound job-level condition moves into a job-level environment variable `RUN_FUZZ`, read by every step guard (`env` is readable in step-level `if`, not in job-level `if` — that is why the variable lives on the job and the guards on the steps).

- [ ] **Step 1: Record the failing baseline**

```bash
grep -cE "^    if:" .github/workflows/fuzz.yml
```

Expected: `1` (the job-level compound gate on `fuzz`).

- [ ] **Step 2: Rewrite `.github/workflows/fuzz.yml`**

Replace the entire file with exactly this content (header, triggers, and `changes` body unchanged; comment above `changes` rewritten; the `fuzz` job converted — note the two steps that already carried conditions, which now combine them with the guard):

```yaml
name: Fuzz

on:
  push:
    branches: [main]
  pull_request:
  schedule:
    # Nightly long run at 04:42 UTC. Any off-the-hour minute avoids the
    # top-of-hour congestion on GitHub's shared cron scheduler.
    - cron: "42 4 * * *"
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  # Supersede stale pull-request runs, but never cancel a main-branch or
  # scheduled run (both resolve to refs/heads/main).
  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}

env:
  CARGO_TERM_COLOR: always

jobs:
  # See ci.yml's `changes` job. The fuzz context is required by branch
  # protection, so the job always starts and gates every step on RUN_FUZZ
  # instead of skipping at the job level (issue #89): on a documentation-only
  # pull request it no-ops in seconds and reports success. The nightly
  # schedule and manual dispatch carry no diff to filter, so the steps run
  # unconditionally for those events; only push and pull-request events can
  # no-op on documentation.
  changes:
    runs-on: ubuntu-latest
    outputs:
      code: ${{ steps.filter.outputs.code }}
    steps:
      - uses: actions/checkout@v7
        with:
          fetch-depth: 0
      - id: filter
        shell: bash
        env:
          EVENT_NAME: ${{ github.event_name }}
          PR_BASE: ${{ github.event.pull_request.base.sha }}
          PR_HEAD: ${{ github.event.pull_request.head.sha }}
          PUSH_BEFORE: ${{ github.event.before }}
        run: |
          if [ "$EVENT_NAME" = "pull_request" ]; then
            base="$PR_BASE"
            head="$PR_HEAD"
          else
            base="$PUSH_BEFORE"
            head="$GITHUB_SHA"
          fi
          # Run the full suite whenever the base is missing or unknown (for
          # example a new branch, whose before-sha is all zeros): never skip
          # on uncertainty.
          valid=true
          if [ -z "$base" ] || [ "$base" = "0000000000000000000000000000000000000000" ]; then
            valid=false
          elif ! git cat-file -e "${base}^{commit}" 2>/dev/null; then
            valid=false
          fi
          if [ "$valid" = false ]; then
            echo "base unavailable; running the full suite"
            echo "code=true" >> "$GITHUB_OUTPUT"
            exit 0
          fi
          files="$(git diff --name-only "$base" "$head")"
          echo "changed files:"
          printf '%s\n' "$files"
          # A change counts as code unless every changed file is inert
          # documentation. Any other path forces the full suite, so a
          # misclassification can only over-run, never skip a real change.
          code=false
          while IFS= read -r file; do
            [ -z "$file" ] && continue
            case "$file" in
              *.md|LICENSE*|.gitignore|.gitattributes|.editorconfig) ;;
              *) code=true ;;
            esac
          done <<< "$files"
          echo "resolved code=$code"
          echo "code=$code" >> "$GITHUB_OUTPUT"

  fuzz:
    needs: changes
    runs-on: ubuntu-latest
    # Three targets at 30 minutes each on the nightly run, plus the build.
    # 120 minutes leaves comfortable headroom.
    timeout-minutes: 120
    env:
      # The former job-level condition, evaluated once and read by every
      # step guard below (step-level `if` can read `env`; job-level `if`
      # cannot).
      RUN_FUZZ: >-
        ${{ needs.changes.outputs.code == 'true'
        || github.event_name == 'schedule'
        || github.event_name == 'workflow_dispatch' }}
    steps:
      - if: ${{ env.RUN_FUZZ == 'true' }}
        uses: actions/checkout@v7
      - if: ${{ env.RUN_FUZZ == 'true' }}
        uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: nightly
      - if: ${{ env.RUN_FUZZ == 'true' }}
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: fuzz
      - name: Cache the cargo-fuzz binary
        if: ${{ env.RUN_FUZZ == 'true' }}
        id: cargo-fuzz-cache
        uses: actions/cache@v4
        with:
          path: ~/.cargo/bin/cargo-fuzz
          # Bump the key suffix to pick up a newer cargo-fuzz release.
          key: cargo-fuzz-${{ runner.os }}-1
      - if: ${{ env.RUN_FUZZ == 'true' && steps.cargo-fuzz-cache.outputs.cache-hit != 'true' }}
        run: cargo install cargo-fuzz --locked
      # Accumulate the coverage-guided corpus across runs so coverage
      # compounds instead of restarting cold from the committed seeds each
      # time. Caches are immutable, so each run writes a fresh entry keyed by
      # run id and restore-keys pulls the most recent previous corpus. The
      # committed seeds under fuzz/corpus/<target> remain the floor.
      - name: Restore the accumulated corpus
        if: ${{ env.RUN_FUZZ == 'true' }}
        uses: actions/cache@v4
        with:
          path: fuzz/corpus
          key: fuzz-corpus-${{ runner.os }}-${{ github.run_id }}
          restore-keys: |
            fuzz-corpus-${{ runner.os }}-
      - name: Resolve the fuzz duration
        if: ${{ env.RUN_FUZZ == 'true' }}
        id: duration
        # 30 minutes per target on the nightly schedule; a 60-second smoke
        # test on push and pull requests. The nightly run carries the depth,
        # so the per-push run only needs to catch shallow regressions fast.
        run: |
          if [ "${{ github.event_name }}" = "schedule" ]; then
            echo "seconds=1800" >> "$GITHUB_OUTPUT"
          else
            echo "seconds=60" >> "$GITHUB_OUTPUT"
          fi
      # The +nightly proxy overrides the repository's rust-toolchain.toml
      # pin, which would otherwise route the sanitizer build to stable.
      # -timeout flags a single input that hangs past 25s (the termination
      # invariant under test); -rss_limit_mb bounds runaway memory.
      - if: ${{ env.RUN_FUZZ == 'true' }}
        run: cargo +nightly fuzz run lex -- -max_total_time=${{ steps.duration.outputs.seconds }} -timeout=25 -rss_limit_mb=4096
      - if: ${{ env.RUN_FUZZ == 'true' }}
        run: cargo +nightly fuzz run parse -- -max_total_time=${{ steps.duration.outputs.seconds }} -timeout=25 -rss_limit_mb=4096
      - if: ${{ env.RUN_FUZZ == 'true' }}
        run: cargo +nightly fuzz run docblock -- -max_total_time=${{ steps.duration.outputs.seconds }} -timeout=25 -rss_limit_mb=4096
      - name: Upload crash artifacts
        if: ${{ failure() && env.RUN_FUZZ == 'true' }}
        uses: actions/upload-artifact@v4
        with:
          name: fuzz-artifacts
          path: fuzz/artifacts/
          if-no-files-found: ignore
```

- [ ] **Step 3: Validate all workflows with actionlint**

```bash
docker run --rm -v "$PWD":/repo -w /repo rhysd/actionlint:latest -color
```

Expected: no output, exit code 0 (this run covers all four workflow files, including the untouched `release.yml`).

- [ ] **Step 4: Assert the gating shape**

```bash
grep -cE "^    if:" .github/workflows/fuzz.yml || true
grep -c "env.RUN_FUZZ == 'true'" .github/workflows/fuzz.yml
```

Expected: `0` (no job-level gate left), then `11` (one guard per step of the `fuzz` job, including the two combined conditions on the cargo-fuzz install and the crash-artifact upload).

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/fuzz.yml
git commit -m "💚 ci: gate the fuzz job at step level (#89)"
```

---

### Task 4: Ship the fix through the full suite

**Files:**
- No file changes; publishes the three commits from Tasks 1-3 as one pull request.

**Interfaces:**
- Consumes: branch `fix-89-docs-only-required-checks` with the three commits.
- Produces: the fix merged into `main`. Task 5 runs on top of the updated `main`.

- [ ] **Step 1: Push and open the pull request**

The pull request body references the issue without a closing keyword: the issue only closes once the acceptance criterion is demonstrated (Task 5).

```bash
git push -u origin fix-89-docs-only-required-checks
gh pr create \
  --title "Report every required status check on documentation-only pull requests" \
  --body "$(cat <<'EOF'
Branch protection on `main` requires ten contexts. On a documentation-only
pull request the path filter used to skip the gated jobs at the job level;
a skipped matrix job never expands, so the three required `test (<os>)`
contexts were never reported and the pull request could never merge
(PR #88 sat permanently BLOCKED with auto-merge armed).

This moves the path condition from the job level to the step level in the
eight jobs whose contexts are required (`test` matrix, `lint`, `deny`,
`stubs`, `snapshot`, `bench`, `phpdoc-cases`, `fuzz`): the job always
starts, every required context always reports, and the steps no-op in
seconds when `changes` says nothing relevant moved. The `changes` detection
job stays the single source of truth, and the non-required corpus jobs
(`ground-truth`, `mixed-rate`, `memory`) keep the cheaper job-level skip.

Because this pull request touches `.github/workflows/`, it counts as code
and runs the full suite itself. The end-to-end acceptance (a
documentation-only pull request merging by auto-merge alone) is validated
by a follow-up documentation-only pull request.

Refs #89
EOF
)"
```

Expected: `gh pr create` prints the pull request URL.

- [ ] **Step 2: Arm auto-merge and watch the checks**

```bash
gh pr merge --auto --merge
gh pr checks --watch --interval 30
```

Expected: every check completes with `pass` — this pull request changes workflow files, so `changes` resolves `code=true` in all three workflows and the full suite runs (including `ground-truth`, `mixed-rate`, `memory`). Nothing may be `skipping` or stuck `pending`. Allow 30-45 minutes for the corpus and bench jobs.

- [ ] **Step 3: Confirm the merge**

```bash
gh pr view --json state,mergedAt
```

Expected: `"state": "MERGED"` with a non-null `mergedAt`, without any manual `--admin` intervention. If the pull request is still `OPEN` with all checks green, wait up to two minutes and re-check: auto-merge fires shortly after the last check reports.

---

### Task 5: Acceptance — a documentation-only pull request merges by auto-merge alone

**Files:**
- Create (in git): `.claude/superpowers/plans/2026-07-20-issue-89-docs-only-required-checks.md` — this plan document is itself the documentation-only payload, reproducing exactly the scenario that deadlocked PR #88.

**Interfaces:**
- Consumes: `main` containing the merged fix from Task 4; this plan file present on disk (if executing from a fresh worktree that lacks it, copy it from the main checkout at the same relative path).
- Produces: issue #89 closed by the acceptance pull request, with evidence recorded on the issue.

- [ ] **Step 1: Branch from the updated main and commit the plan**

```bash
git checkout main
git pull
git checkout -b docs-89-acceptance-plan
git add .claude/superpowers/plans/2026-07-20-issue-89-docs-only-required-checks.md
git commit -m "📝 docs(plans): write the docs-only required checks plan (#89)"
git show --stat --format= HEAD
```

Expected: the commit touches exactly one file, ending in `.md` (inert documentation per the allowlist). If the plan file had already reached `main` some other way, substitute any genuinely useful documentation-only change (a `*.md` file) — never a throwaway edit.

- [ ] **Step 2: Open the pull request and arm auto-merge immediately**

Arming auto-merge before the checks finish is the point: the merge must happen with zero manual intervention afterwards.

Later steps select this pull request by its branch name `docs-89-acceptance-plan` (shell variables do not survive across separate command invocations; the branch name does).

```bash
git push -u origin docs-89-acceptance-plan
gh pr create \
  --title "Docs-only acceptance for issue #89: the implementation plan" \
  --body "$(cat <<'EOF'
Documentation-only pull request: commits the implementation plan for
issue #89 and doubles as the end-to-end acceptance test. Under the fixed
workflows, every required context must report success as a no-op and this
pull request must go from open to merged by auto-merge alone, with zero
manual intervention and `enforce_admins` untouched.

Fixes #89
EOF
)"
gh pr merge --auto --merge docs-89-acceptance-plan
```

Expected: `gh pr create` prints the pull request URL and auto-merge is armed while checks are still running.

- [ ] **Step 3: Watch the required contexts report as no-ops**

```bash
gh pr checks docs-89-acceptance-plan --watch --interval 15
```

Expected within a few minutes: the ten required contexts — `bench`, `deny`, `fuzz`, `lint`, `phpdoc-cases`, `snapshot`, `stubs`, `test (macos-latest)`, `test (ubuntu-latest)`, `test (windows-latest)` — all `pass` (each job starts, its steps skip, it reports success in seconds); the three `changes` jobs `pass`; `ground-truth`, `mixed-rate`, `memory` show as skipped. No context may remain in the "expected" state.

- [ ] **Step 4: Verify the merge happened by auto-merge alone**

Do not touch the pull request; only observe it.

```bash
for i in $(seq 1 30); do
  state=$(gh pr view docs-89-acceptance-plan --json state --jq .state)
  [ "$state" = "MERGED" ] && break
  sleep 10
done
gh pr view docs-89-acceptance-plan --json state,mergedAt,autoMergeRequest
```

Expected: `"state": "MERGED"`. This is the acceptance criterion of issue #89. If it stays `OPEN` past five minutes with all checks green, the fix has failed: stop, capture `gh pr view docs-89-acceptance-plan --json statusCheckRollup`, and report — do not merge manually.

- [ ] **Step 5: Verify enforce_admins was never touched and the issue closed**

```bash
gh api repos/celerrate/celerrate/branches/main/protection --jq '.enforce_admins.enabled'
gh issue view 89 --json state --jq .state
```

Expected: `true`, then `CLOSED` (closed automatically by the `Fixes #89` keyword when the acceptance pull request merged).

- [ ] **Step 6: Record the evidence on the issue**

```bash
PR_URL=$(gh pr view docs-89-acceptance-plan --json url --jq .url)
gh issue comment 89 --body "Acceptance demonstrated: the documentation-only pull request $PR_URL went from open to merged by auto-merge alone — every required context reported success as a no-op, no manual intervention, and enforce_admins stayed enabled throughout."
```

Expected: the comment URL is printed.
