# Diagnostics and Fixes Part 6: The Autofix Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `celerrate check --fix` and `--fix-suggestions`: presentation-time did-you-mean suggestions on the unknown-symbol and unknown-member families, a deterministic first-wins application pass in original snapshot coordinates, and the write path through the VFS to disk.

**Architecture:** Everything new lives in `celerrate_cli` (presentation layer), in two new modules: `suggest.rs` computes did-you-mean candidates at render and fix time, outside every memoized query, from the public symbol-index and member-surface queries of `celerrate_semantics`; `fix.rs` plans the single application pass (threshold by `Confidence`, first fix wins any overlap, losers skipped and reported) and applies per file atomically through `celerrate_edit::apply`, writing through the `Vfs` to disk. `celerrate_edit` publishes its existing `find_conflict` so the planner can ask the conflict question without applying. No phase query, no stored-verdict schema, and no salsa dependency edge changes anywhere.

**Tech Stack:** Rust, clap (existing flags surface), insta (existing snapshot suite), salsa queries consumed read-only from the CLI. No new dependencies: the bounded Levenshtein distance is ~30 lines written in `suggest.rs`, so `cargo deny` is untouched.

**Design spec:** `.claude/superpowers/specs/2026-07-20-diagnostics-and-fixes-design.md`, section 7 (plus sections 3 and 11 where cited). Read it before deviating from anything below.

## Global Constraints

- Zero panic, mechanically enforced: clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Production code returns `Result` or degrades honestly (`Option`, skip-and-report). Test modules may locally `#[allow]` these lints.
- TDD: failing test, minimal implementation, refactor. No production code without a test that demanded it.
- Strict layering: `celerrate_cli` is the top; it may consume every crate. `celerrate_edit` depends only on `celerrate_source` and `celerrate_syntax`. Nothing below the CLI learns about fixes or did-you-mean.
- Determinism: the analysis is untouched; everything added here is a deterministic pure function of the session state. No wall-clock time, no randomness.
- Exit-code contract unchanged: 0 clean, 1 any span-anchored diagnostic, 2 internal error or usage error. Fix application never changes what exit code the found diagnostics produce; a failed write adds an internal error (exit 2 dominates, consistent with `FileUnreadable` today).
- The pinned corpus snapshot stays byte-identical (`cargo xtask corpus`) and the mixed-rate baseline unchanged (`cargo xtask mixed-rate`); both need `cargo xtask fetch-corpus` first. The corpus is clean (0 diagnostics), and enrichment only touches reported diagnostics, so this holds by construction — the closure task still proves it.
- English everywhere, full words, no abbreviated names. Commits: gitmoji + Conventional Commits, repository-configured identity, no Claude attribution.
- Design decisions this plan fixes (do not relitigate mid-task):
  - Did-you-mean is presentation, not analysis (spec section 7): computed in `celerrate_cli::suggest` for the reported diagnostics only, never persisted, never inside a phase query. Clean runs therefore stay parse-free on the warm path; a run that reports findings pays the candidate search only then.
  - The member families re-derive `(member, receiver)` from the pinned message text (`` unknown method `svae` on `User` ``). Rationale: on a warm run the structured `TypedVerdict` no longer exists — the stored verdict is the post-suppression `Diagnostic` — so the message *is* the stored interface. A test pins the parser against the emitters' formats.
  - Distance policy: Levenshtein over lowercased strings (so a case-only typo like `php_eol` for `PHP_EOL` is distance 0), bound 1 for names of 4 characters or fewer, else 2. Unique minimal-distance candidate → applicable `NeedsReview` suggestion; tie → note listing the candidates; nothing in bound → untouched diagnostic.
  - All shipped fixes are `NeedsReview` (proposing a different name is never semantics-preserving), therefore `--fix` alone applies nothing at closure. That is the design's owned consequence and a test pins it honestly.
  - Virtual members (`@method`/`@property` annotations) are excluded from the member candidate pool in this sub-project: the pool can widen later without breaking anything, and excluding them keeps this plan to verified surfaces.
  - `--watch` combined with either fix flag is a usage error, enforced by clap `conflicts_with` (exit 2, clap's own message).
  - The watch path renders without enrichment in this sub-project; part 7 owns the richer watch rendering.

## File Structure

| File | Role |
| --- | --- |
| `crates/celerrate_edit/src/conflict.rs` + `src/lib.rs` | `find_conflict` becomes `pub` (modified, task 5) |
| `crates/celerrate_cli/src/suggest.rs` | New: distance core, candidate pools, `enrich` (tasks 1-3) |
| `crates/celerrate_cli/src/fix.rs` | New: `fix_threshold`, `plan`, `apply_to_disk` (tasks 5-6) |
| `crates/celerrate_cli/src/render.rs` | Help/note lines, `render_report` split, fix summary (tasks 4, 7) |
| `crates/celerrate_cli/src/session.rs` | Two `InternalError` variants (task 6) |
| `crates/celerrate_cli/src/arguments.rs` | `--fix`, `--fix-suggestions` (task 7) |
| `crates/celerrate_cli/src/lib.rs` | Module wiring, enrichment + fix in the single pass (tasks 4, 7) |
| `crates/celerrate_cli/tests/fix.rs` | New: end-to-end fix suite (task 7) |
| `crates/celerrate_rules/tests/invalidation_scope.rs` | New pin: did-you-mean out of the graph (task 8) |
| `CHANGELOG.md` | Closure entry (task 9) |

Verified API surface the tasks lean on (do not rediscover): `celerrate_diagnostics::{Diagnostic, DiagnosticId, Anchor, Suggestion, Confidence}` (crate-root re-exports; `Confidence::Safe < Confidence::NeedsReview` by `Ord`); `celerrate_source::{FileId, TextEdit, TextRange, TextSize}`; `celerrate_edit::{apply, ApplyError, EditConflict}`; `celerrate_db::{source_text, SourceFile}` where `source_text(db, file) -> &Result<SourceText, SourceTooLarge>` and `SourceText::text() -> &str`; `celerrate_semantics::{source_symbol_table(db, files) -> &SymbolTable, stub_symbol_table(db, stubs, configuration) -> &StubSymbolTable, stub_signature_table(db, stubs) -> &StubSignatureTable, linearized_class(db, files, stubs, configuration, ClassQuery) (call .as_ref() on the result), ClassQuery::new(db, String), folded_symbol_key(SymbolSpace, &str), folded_member_key(MemberKind, &str), SymbolSpace::{ClassLike, Function, Constant}, MemberKind::{Method, Property, ClassConstant, EnumCase}}`; `SymbolEntry { space, key, original, .. }`; `StubSymbolEntry { space, key, symbol: StubSymbol { name, .. } }`; `LinearizedClass { members: Vec<LinearizedMember { key, member: Member { kind, name, .. }, .. }>, ancestry: Vec<AncestorEdge { stub: Option<String>, .. }>, .. }`; `StubClassSurface { parents: Vec<String>, members: Vec<StubMember { kind: StubMemberKind, name, availability, .. }> }`; `StubAvailability::exists_in(PhpVersionRange)`; `Session { database, vfs, files, stubs, configuration, sources: BTreeMap<FileId, SourceFile>, internal_errors, .. }`.

---

### Task 1: The did-you-mean core (`suggest.rs` foundations)

**Files:**
- Create: `crates/celerrate_cli/src/suggest.rs`
- Modify: `crates/celerrate_cli/src/lib.rs` (add `pub mod suggest;` to the module list)

**Interfaces:**
- Consumes: nothing project-specific (pure string logic).
- Produces (crate-private, consumed by tasks 2-3 inside the same module): `fn bounded_distance(written: &str, candidate: &str, bound: usize) -> Option<usize>`, `fn distance_bound(name: &str) -> usize`, `enum DidYouMean { Nothing, Unique(String), Tie(Vec<String>) }`, `fn did_you_mean(written: &str, candidates: Vec<String>) -> DidYouMean`, `fn terminal_segment(name: &str) -> &str`.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_cli/src/suggest.rs` with the module documentation, an empty body, and the test module:

```rust
//! Presentation-time did-you-mean (design section 7): computed at
//! render and fix time, for the reported diagnostics only, never
//! inside a memoized query. Nothing computed here is persisted: a
//! candidate goes stale the moment a nearer name appears, and no
//! revalidation record could keep it honest. Inside a phase query the
//! candidate search would also wire the global name set into every
//! file's dependency graph; here it wires into nothing.

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

    use super::{DidYouMean, bounded_distance, did_you_mean, distance_bound, terminal_segment};

    #[test]
    fn the_distance_is_levenshtein_over_lowercased_names() {
        assert_eq!(bounded_distance("svae", "save", 2), Some(2));
        assert_eq!(bounded_distance("nmae", "name", 2), Some(2));
        assert_eq!(bounded_distance("save", "save", 2), Some(0));
        assert_eq!(bounded_distance("php_eol", "PHP_EOL", 2), Some(0));
        assert_eq!(bounded_distance("Activ", "Active", 2), Some(1));
    }

    #[test]
    fn a_distance_beyond_the_bound_is_none_not_a_number() {
        assert_eq!(bounded_distance("draft", "active", 2), None);
        assert_eq!(bounded_distance("a", "abcd", 2), None);
    }

    #[test]
    fn the_bound_is_one_for_short_names_and_two_otherwise() {
        assert_eq!(distance_bound("save"), 1);
        assert_eq!(distance_bound("saved"), 2);
        assert_eq!(distance_bound("é"), 1, "characters, not bytes");
    }

    #[test]
    fn a_unique_minimal_candidate_wins() {
        let outcome = did_you_mean(
            "svae",
            vec!["save".to_owned(), "wave".to_owned(), "unrelated".to_owned()],
        );
        // `svae` -> `save` is 2, `svae` -> `wave` is 2 as well: a tie.
        assert_eq!(
            outcome,
            DidYouMean::Tie(vec!["save".to_owned(), "wave".to_owned()]),
        );
        let outcome = did_you_mean("Activ", vec!["Active".to_owned(), "Passive".to_owned()]);
        assert_eq!(outcome, DidYouMean::Unique("Active".to_owned()));
    }

    #[test]
    fn a_nearer_candidate_replaces_a_farther_one_whatever_the_order() {
        let forward = did_you_mean("sive", vec!["salve".to_owned(), "save".to_owned()]);
        let backward = did_you_mean("sive", vec!["save".to_owned(), "salve".to_owned()]);
        assert_eq!(forward, DidYouMean::Unique("save".to_owned()));
        assert_eq!(forward, backward);
    }

    #[test]
    fn tied_candidates_are_sorted_and_deduplicated() {
        let outcome = did_you_mean(
            "sive",
            vec!["sove".to_owned(), "save".to_owned(), "sove".to_owned()],
        );
        assert_eq!(
            outcome,
            DidYouMean::Tie(vec!["save".to_owned(), "sove".to_owned()]),
        );
    }

    #[test]
    fn no_candidate_in_bound_is_nothing() {
        assert_eq!(
            did_you_mean("svae", vec!["unrelated".to_owned()]),
            DidYouMean::Nothing,
        );
        assert_eq!(did_you_mean("svae", Vec::new()), DidYouMean::Nothing);
    }

    #[test]
    fn the_terminal_segment_is_the_name_after_the_last_backslash() {
        assert_eq!(terminal_segment("Lib\\Client"), "Client");
        assert_eq!(terminal_segment("Client"), "Client");
        assert_eq!(terminal_segment("\\App\\Http\\Kernel"), "Kernel");
    }
}
```

Add `pub mod suggest;` to the module list in `crates/celerrate_cli/src/lib.rs` (alphabetical position, after `pub mod session;`).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_cli suggest`
Expected: FAIL to compile (`bounded_distance` and friends not found).

- [ ] **Step 3: Write the implementation**

Above the test module in `suggest.rs`:

```rust
/// Levenshtein distance over lowercased characters, abandoned as soon
/// as it provably exceeds `bound`. Lowercasing makes a case-only typo
/// distance 0, which is exactly the fix the case-sensitive spaces
/// (constants, properties, enum cases) want suggested.
fn bounded_distance(written: &str, candidate: &str, bound: usize) -> Option<usize> {
    let written: Vec<char> = written.to_lowercase().chars().collect();
    let candidate: Vec<char> = candidate.to_lowercase().chars().collect();
    if written.len().abs_diff(candidate.len()) > bound {
        return None;
    }
    let mut previous: Vec<usize> = (0..=candidate.len()).collect();
    for (row, written_character) in written.iter().enumerate() {
        let mut current: Vec<usize> = Vec::with_capacity(candidate.len() + 1);
        current.push(row + 1);
        for (column, candidate_character) in candidate.iter().enumerate() {
            // The `get` fallbacks are unreachable (the rows are dense
            // by construction); they exist because indexing is denied
            // and a wrong answer here is caught by the tests anyway.
            let substitution = previous.get(column).copied().unwrap_or(usize::MAX - 1)
                + usize::from(written_character != candidate_character);
            let insertion = current.get(column).copied().unwrap_or(usize::MAX - 1) + 1;
            let deletion = previous.get(column + 1).copied().unwrap_or(usize::MAX - 1) + 1;
            current.push(substitution.min(insertion).min(deletion));
        }
        if current.iter().min().copied().unwrap_or(0) > bound {
            return None;
        }
        previous = current;
    }
    previous
        .last()
        .copied()
        .filter(|&distance| distance <= bound)
}

/// The bound the design calls "bounded edit distance": tight for short
/// names (almost anything is within 2 of a 3-letter name), 2 otherwise.
fn distance_bound(name: &str) -> usize {
    if name.chars().count() <= 4 { 1 } else { 2 }
}

/// The ambiguity discipline (design section 7): a unique
/// minimal-distance candidate becomes an applicable suggestion; a tie
/// is listed in a note instead, because bulk `--fix-suggestions` must
/// never apply a guess the engine itself knows is ambiguous.
#[derive(Debug, PartialEq, Eq)]
enum DidYouMean {
    Nothing,
    Unique(String),
    Tie(Vec<String>),
}

fn did_you_mean(written: &str, candidates: Vec<String>) -> DidYouMean {
    let bound = distance_bound(written);
    let mut minimum: Option<usize> = None;
    let mut names: Vec<String> = Vec::new();
    for candidate in candidates {
        let Some(distance) = bounded_distance(written, &candidate, bound) else {
            continue;
        };
        match minimum {
            Some(best) if distance > best => {}
            Some(best) if distance == best => {
                if !names.contains(&candidate) {
                    names.push(candidate);
                }
            }
            _ => {
                minimum = Some(distance);
                names = vec![candidate];
            }
        }
    }
    names.sort();
    match names.len() {
        0 => DidYouMean::Nothing,
        1 => names.pop().map_or(DidYouMean::Nothing, DidYouMean::Unique),
        _ => DidYouMean::Tie(names),
    }
}

/// The last segment of a qualified name: `Lib\Client` -> `Client`.
fn terminal_segment(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}
```

Note: `rsplit` always yields at least one segment, and the row vectors are dense; the `unwrap_or` fallbacks satisfy the denied-lint discipline without ever firing.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_cli suggest`
Expected: PASS (8 tests). If `dead_code` warnings fail the build (the helpers have no production caller until task 2), add `#[allow(dead_code)]` on each item with a `// Consumed by enrich (task 2); the allow dies with that task.` comment — and task 2's checklist removes them.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p celerrate_cli --all-targets -- -D warnings
git add crates/celerrate_cli/src/suggest.rs crates/celerrate_cli/src/lib.rs
git commit -m "✨ feat(cli): add the bounded-distance did-you-mean core"
```

---

### Task 2: Unknown-symbol enrichment and the `enrich` entry point

**Files:**
- Modify: `crates/celerrate_cli/src/suggest.rs`

**Interfaces:**
- Consumes: task 1's helpers; `Session` fields (`database`, `files`, `stubs`, `configuration`, `sources`); `celerrate_semantics::{source_symbol_table, stub_symbol_table, folded_symbol_key, SymbolSpace}`; `celerrate_db::source_text`.
- Produces: `pub fn enrich(session: &Session, diagnostics: &[Diagnostic]) -> Vec<Diagnostic>` — same length and order as its input; each CEL0018/CEL0019/CEL0020 diagnostic may gain exactly one suggestion (`Confidence::NeedsReview`, one same-file `TextEdit` replacing the written name's terminal segment) or one tie note. Tasks 4 and 7 call this.

- [ ] **Step 1: Write the failing tests**

Append to the test module in `suggest.rs` (alongside task 1's tests):

```rust
    use celerrate_diagnostics::{Confidence, Diagnostic};

    use crate::analysis;
    use crate::session::Session;

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

    /// Analyzes a fixture project and enriches its report, exactly as
    /// the single-pass path will (task 4).
    fn enriched(files: &[(&str, &str)]) -> (tempfile::TempDir, Vec<Diagnostic>) {
        let root = project(files);
        let session = Session::start(root.path());
        let inputs = session.inputs();
        let outcome = analysis::analyze(&inputs).unwrap_or_default();
        let enriched = super::enrich(&session, &outcome.diagnostics);
        (root, enriched)
    }

    const MANIFEST: &str =
        r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;

    #[test]
    fn an_unknown_class_with_one_near_declaration_gains_an_applicable_suggestion() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Gateway.php",
                "<?php\nnamespace App;\nclass PaymentGateway {}\n",
            ),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\nnew PaymentGatewya();\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.id.as_str(), "CEL0018");
        assert_eq!(diagnostic.suggestions.len(), 1);
        let suggestion = &diagnostic.suggestions[0];
        assert_eq!(suggestion.message, "did you mean `PaymentGateway`?");
        assert_eq!(suggestion.confidence, Confidence::NeedsReview);
        assert_eq!(suggestion.edits.len(), 1);
        // The edit replaces exactly the written name, in the
        // diagnostic's own file.
        let (file, range) = diagnostic.span().unwrap();
        assert_eq!(suggestion.edits[0].file, file);
        assert_eq!(suggestion.edits[0].range, range);
        assert_eq!(suggestion.edits[0].replacement, "PaymentGateway");
    }

    #[test]
    fn a_qualified_spelling_keeps_its_prefix_and_replaces_the_terminal_segment_only() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Gateway.php",
                "<?php\nnamespace App\\Billing;\nclass PaymentGateway {}\n",
            ),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\nnew Billing\\PaymentGatewya();\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        let suggestion = &diagnostics[0].suggestions[0];
        // The span covers `Billing\PaymentGatewya`; the edit covers
        // only `PaymentGatewya`.
        let (_, span) = diagnostics[0].span().unwrap();
        assert!(suggestion.edits[0].range.start() > span.start());
        assert_eq!(suggestion.edits[0].range.end(), span.end());
        assert_eq!(suggestion.edits[0].replacement, "PaymentGateway");
    }

    #[test]
    fn a_case_only_constant_typo_suggests_the_declared_spelling() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/constants.php",
                "<?php\ndefine('DATABASE_TIMEOUT_LIMIT', 30);\necho database_timeout_limit;\n",
            ),
        ]);
        let constant: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.id.as_str() == "CEL0020")
            .collect();
        assert_eq!(constant.len(), 1);
        assert_eq!(
            constant[0].suggestions[0].message,
            "did you mean `DATABASE_TIMEOUT_LIMIT`?",
        );
    }

    #[test]
    fn a_diagnostic_with_no_near_candidate_is_returned_untouched() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\nnew CompletelyUnheardOfThing();\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].suggestions.is_empty());
        assert!(diagnostics[0].notes.is_empty());
    }

    #[test]
    fn enrichment_preserves_identity_order_and_count() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Consumer.php",
                "<?php\nnamespace App;\nnew Alpha();\nnew Beta();\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 2);
        let mut sorted = diagnostics.clone();
        sorted.sort();
        assert_eq!(diagnostics, sorted, "the total order survives enrichment");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_cli suggest`
Expected: FAIL to compile (`enrich` not found).

- [ ] **Step 3: Write the implementation**

Add to `suggest.rs` (above the test module). Imports at the top of the file:

```rust
use celerrate_db::source_text;
use celerrate_diagnostics::{Confidence, Diagnostic, Suggestion};
use celerrate_semantics::{
    SymbolSpace, folded_symbol_key, source_symbol_table, stub_symbol_table,
};
use celerrate_source::{FileId, TextEdit, TextRange, TextSize};

use crate::session::Session;
```

Then:

```rust
/// What one diagnostic gains: an applicable suggestion, or a note when
/// the engine itself knows the guess is ambiguous.
enum Enrichment {
    Suggestion(Suggestion),
    Note(String),
}

/// Adds presentation-time did-you-mean suggestions and notes to the
/// reported diagnostics. Pure presentation: the input's length and
/// order are preserved, the persisted verdicts never see the result,
/// and nothing here runs inside a salsa query.
pub fn enrich(session: &Session, diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
    diagnostics
        .iter()
        .map(|diagnostic| enrich_one(session, diagnostic.clone()))
        .collect()
}

fn enrich_one(session: &Session, mut diagnostic: Diagnostic) -> Diagnostic {
    let Some((file, range)) = diagnostic.span() else {
        return diagnostic;
    };
    // Matching on the code string is deliberate: the identifiers are
    // the frozen public contract, and the CLI must not depend on which
    // crate happens to declare each constant.
    let enrichment = match diagnostic.id.as_str() {
        "CEL0018" => symbol_did_you_mean(session, file, range, SymbolSpace::ClassLike),
        "CEL0019" => symbol_did_you_mean(session, file, range, SymbolSpace::Function),
        "CEL0020" => symbol_did_you_mean(session, file, range, SymbolSpace::Constant),
        _ => None,
    };
    match enrichment {
        Some(Enrichment::Suggestion(suggestion)) => diagnostic.suggestions.push(suggestion),
        Some(Enrichment::Note(note)) => diagnostic.notes.push(note),
        None => {}
    }
    diagnostic
}

/// The source text under a span, from the exact decoded input the
/// analysis read.
fn span_text(session: &Session, file: FileId, range: TextRange) -> Option<String> {
    let source = session.sources.get(&file)?;
    let text = source_text(&session.database, *source).as_ref().ok()?;
    text.text()
        .get(usize::from(range.start())..usize::from(range.end()))
        .map(str::to_owned)
}

fn symbol_did_you_mean(
    session: &Session,
    file: FileId,
    range: TextRange,
    space: SymbolSpace,
) -> Option<Enrichment> {
    let written = span_text(session, file, range)?;
    let terminal = terminal_segment(&written);
    // The edit replaces the terminal segment only, so an alias or a
    // qualified spelling keeps its prefix untouched.
    let prefix_length = u32::try_from(written.len() - terminal.len()).ok()?;
    let edit_range = TextRange::new(range.start() + TextSize::from(prefix_length), range.end());
    let written_key = folded_symbol_key(space, terminal);
    let candidates = symbol_candidates(session, space, &written_key);
    Some(resolve_enrichment(terminal, candidates, file, edit_range)?)
}

/// Every declared terminal segment of the space, source and stub halves
/// alike, minus anything that folds to the written key (a name that
/// folds equal would have resolved).
fn symbol_candidates(session: &Session, space: SymbolSpace, written_key: &str) -> Vec<String> {
    let db = &session.database;
    let mut names: Vec<String> = Vec::new();
    for entry in source_symbol_table(db, session.files).entries() {
        if entry.space == space {
            names.push(terminal_segment(&entry.original).to_owned());
        }
    }
    for entry in stub_symbol_table(db, session.stubs, session.configuration).entries() {
        if entry.space == space {
            names.push(terminal_segment(&entry.symbol.name).to_owned());
        }
    }
    names.retain(|name| folded_symbol_key(space, name) != written_key);
    names.sort();
    names.dedup();
    names
}

/// The shared tail of both families: the discipline applied to a
/// candidate list, shaped into what the diagnostic gains.
fn resolve_enrichment(
    written: &str,
    candidates: Vec<String>,
    file: FileId,
    edit_range: TextRange,
) -> Option<Enrichment> {
    match did_you_mean(written, candidates) {
        DidYouMean::Nothing => None,
        DidYouMean::Unique(candidate) => Some(Enrichment::Suggestion(Suggestion {
            message: format!("did you mean `{candidate}`?"),
            confidence: Confidence::NeedsReview,
            edits: vec![TextEdit {
                file,
                range: edit_range,
                replacement: candidate,
            }],
        })),
        DidYouMean::Tie(names) => {
            let listed = names
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ");
            Some(Enrichment::Note(format!("did you mean one of {listed}?")))
        }
    }
}
```

Remove any `#[allow(dead_code)]` markers task 1 left behind.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_cli suggest`
Expected: PASS (all task 1 + task 2 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p celerrate_cli --all-targets -- -D warnings
git add crates/celerrate_cli/src/suggest.rs
git commit -m "✨ feat(cli): suggest near names for unknown symbols"
```

---

### Task 3: Member-family enrichment (CEL0030 to CEL0033)

**Files:**
- Modify: `crates/celerrate_cli/src/suggest.rs`

**Interfaces:**
- Consumes: task 2's `enrich_one` dispatch, `span_text`, `resolve_enrichment`; `celerrate_semantics::{linearized_class, ClassQuery, stub_signature_table, folded_member_key, MemberKind}`; `celerrate_stubs::StubMemberKind`.
- Produces: the four member identifiers gain the same enrichment contract as task 2. One deviation the tests pin: a unique candidate whose token range cannot be located degrades to a note, never a guessed range.

- [ ] **Step 1: Write the failing tests**

Append to the test module:

```rust
    #[test]
    fn an_unknown_method_suggests_the_near_member_and_edits_exactly_its_token() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/User.php",
                "<?php\nnamespace App;\nclass User { public function save(): void {} }\n",
            ),
            (
                "src/Caller.php",
                "<?php\nnamespace App;\nfunction persist(User $user): void { $user->svae(); }\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.id.as_str(), "CEL0030");
        assert_eq!(diagnostic.suggestions.len(), 1);
        let suggestion = &diagnostic.suggestions[0];
        assert_eq!(suggestion.message, "did you mean `save`?");
        assert_eq!(suggestion.confidence, Confidence::NeedsReview);
        // The edit covers exactly the member token, not the whole
        // member expression the diagnostic's span covers.
        let source = std::fs::read_to_string(_root.path().join("src/Caller.php")).unwrap();
        let edit = &suggestion.edits[0];
        let start = usize::from(edit.range.start());
        let end = usize::from(edit.range.end());
        assert_eq!(&source[start..end], "svae");
        assert_eq!(edit.replacement, "save");
    }

    #[test]
    fn an_unknown_property_suggests_the_near_property() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/User.php",
                "<?php\nnamespace App;\nclass User { public string $name = ''; }\n",
            ),
            (
                "src/Caller.php",
                "<?php\nnamespace App;\nfunction read(User $user): string { return $user->nmae; }\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id.as_str(), "CEL0031");
        assert_eq!(
            diagnostics[0].suggestions[0].message,
            "did you mean `name`?",
        );
        assert_eq!(diagnostics[0].suggestions[0].edits[0].replacement, "name");
    }

    #[test]
    fn an_unknown_class_constant_and_enum_case_suggest_their_near_siblings() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Config.php",
                "<?php\nnamespace App;\nclass Config { public const LIMIT = 10; }\n",
            ),
            (
                "src/Status.php",
                "<?php\nnamespace App;\nenum Status { case Active; }\n",
            ),
            (
                "src/Caller.php",
                "<?php\nnamespace App;\nfunction f(): void { echo Config::LIMTI; $s = Status::Activ; }\n",
            ),
        ]);
        let messages: Vec<(&str, &str)> = diagnostics
            .iter()
            .filter_map(|diagnostic| {
                diagnostic
                    .suggestions
                    .first()
                    .map(|suggestion| (diagnostic.id.as_str(), suggestion.message.as_str()))
            })
            .collect();
        assert_eq!(
            messages,
            vec![
                ("CEL0032", "did you mean `LIMIT`?"),
                ("CEL0033", "did you mean `Active`?"),
            ],
        );
    }

    #[test]
    fn a_stub_inherited_member_is_a_candidate() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/Caller.php",
                "<?php\nfunction f(\\ArrayObject $a): void { $a->cout(); }\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id.as_str(), "CEL0030");
        assert_eq!(
            diagnostics[0].suggestions[0].message,
            "did you mean `count`?",
        );
    }

    #[test]
    fn a_member_with_no_near_sibling_stays_untouched() {
        let (_root, diagnostics) = enriched(&[
            ("composer.json", MANIFEST),
            (
                "src/User.php",
                "<?php\nnamespace App;\nclass User { public function save(): void {} }\n",
            ),
            (
                "src/Caller.php",
                "<?php\nnamespace App;\nfunction f(User $user): void { $user->frobnicate(); }\n",
            ),
        ]);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].suggestions.is_empty());
        assert!(diagnostics[0].notes.is_empty());
    }

    #[test]
    fn the_message_parser_matches_the_emitters_pinned_formats() {
        // These four literals are the exact formats
        // `crates/celerrate_rules/src/rules/unknown_members.rs` emits;
        // if that file changes shape, this test names the coupling.
        assert_eq!(
            super::parse_member_message("unknown method `svae` on `User`"),
            Some(("svae".to_owned(), "User".to_owned())),
        );
        assert_eq!(
            super::parse_member_message("unknown property `$nmae` on `App\\User`"),
            Some(("nmae".to_owned(), "App\\User".to_owned())),
        );
        assert_eq!(
            super::parse_member_message("unknown class constant `LIMTI` on `Config`"),
            Some(("LIMTI".to_owned(), "Config".to_owned())),
        );
        assert_eq!(
            super::parse_member_message("unknown enum case `Activ` on `Status`"),
            Some(("Activ".to_owned(), "Status".to_owned())),
        );
        assert_eq!(super::parse_member_message("no backticks here"), None);
    }

    #[test]
    fn the_member_token_is_the_operator_prefixed_occurrence() {
        use celerrate_source::TextSize;
        // `$svae->svae()`: the receiver spells the same word; the
        // token after `->` is the one the edit must cover.
        let range =
            super::member_token_range("$svae->svae()", "svae", TextSize::from(10)).unwrap();
        assert_eq!(u32::from(range.start()), 10 + 7);
        assert_eq!(u32::from(range.end()), 10 + 11);
        assert_eq!(
            super::member_token_range("Config::LIMTI", "LIMTI", TextSize::from(0)).map(
                |range| (u32::from(range.start()), u32::from(range.end())),
            ),
            Some((8, 13)),
        );
        // No operator-prefixed occurrence: no range, never a guess.
        assert_eq!(
            super::member_token_range("svae", "svae", TextSize::from(0)),
            None,
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_cli suggest`
Expected: FAIL to compile (`parse_member_message`, `member_token_range` not found).

- [ ] **Step 3: Write the implementation**

Extend the imports:

```rust
use std::collections::{HashSet, VecDeque};

use celerrate_semantics::{
    ClassQuery, MemberKind, SymbolSpace, folded_member_key, folded_symbol_key, linearized_class,
    source_symbol_table, stub_signature_table, stub_symbol_table,
};
use celerrate_stubs::StubMemberKind;
```

Extend the dispatch in `enrich_one`:

```rust
        "CEL0030" => member_did_you_mean(session, file, range, &diagnostic.message, MemberKind::Method),
        "CEL0031" => member_did_you_mean(session, file, range, &diagnostic.message, MemberKind::Property),
        "CEL0032" => member_did_you_mean(session, file, range, &diagnostic.message, MemberKind::ClassConstant),
        "CEL0033" => member_did_you_mean(session, file, range, &diagnostic.message, MemberKind::EnumCase),
```

Then the new functions:

```rust
/// Extracts the two backticked operands of the pinned member-message
/// shapes (`` unknown method `m` on `T` ``). The message is the stored
/// form of the diagnostic — on a warm run the structured verdict no
/// longer exists — so the message is the interface, and a test pins
/// this parser against the emitters' formats.
fn parse_member_message(message: &str) -> Option<(String, String)> {
    let mut segments = message.split('`');
    let _head = segments.next()?;
    let member = segments.next()?;
    let _middle = segments.next()?;
    let receiver = segments.next()?;
    let member = member.strip_prefix('$').unwrap_or(member);
    if member.is_empty() || receiver.is_empty() {
        return None;
    }
    Some((member.to_owned(), receiver.to_owned()))
}

/// The member-name token inside the diagnostic's span: the last
/// occurrence of the written member that is preceded by `->`, `?->`,
/// or `::` (whitespace allowed in between) and ends at a word
/// boundary. `None` skips the applicable edit rather than guessing.
fn member_token_range(
    span_text: &str,
    member: &str,
    span_start: TextSize,
) -> Option<TextRange> {
    let mut search_end = span_text.len();
    loop {
        let position = span_text.get(..search_end)?.rfind(member)?;
        let before = span_text.get(..position)?.trim_end();
        let after = span_text.get(position + member.len()..)?;
        let boundary_after = after
            .chars()
            .next()
            .is_none_or(|character| !character.is_alphanumeric() && character != '_');
        if boundary_after && (before.ends_with("->") || before.ends_with("::")) {
            let start = span_start + TextSize::from(u32::try_from(position).ok()?);
            let length = TextSize::from(u32::try_from(member.len()).ok()?);
            return Some(TextRange::new(start, start + length));
        }
        if position == 0 {
            return None;
        }
        search_end = position;
    }
}

fn member_did_you_mean(
    session: &Session,
    file: FileId,
    range: TextRange,
    message: &str,
    kind: MemberKind,
) -> Option<Enrichment> {
    let (member, receiver) = parse_member_message(message)?;
    let written_key = folded_member_key(kind, &member);
    let candidates = member_candidates(session, &receiver, kind, &written_key);
    if candidates.is_empty() {
        return None;
    }
    let text = span_text(session, file, range)?;
    match member_token_range(&text, &member, range.start()) {
        Some(edit_range) => resolve_enrichment(&member, candidates, file, edit_range),
        // A unique candidate without a locatable token degrades to a
        // note: an applicable edit is never guessed.
        None => match did_you_mean(&member, candidates) {
            DidYouMean::Nothing => None,
            DidYouMean::Unique(candidate) => {
                Some(Enrichment::Note(format!("did you mean `{candidate}`?")))
            }
            DidYouMean::Tie(names) => {
                let listed = names
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                Some(Enrichment::Note(format!("did you mean one of {listed}?")))
            }
        },
    }
}

/// The receiver's member surface of the queried kind: the linearized
/// source surface first, then the compiled stub graph behind its stub
/// edges (or from the key itself when the receiver is no source
/// class), breadth-first over parent links exactly like
/// `lookup_member`'s stub walk. Virtual (annotation-declared) members
/// are deliberately not in the pool in this sub-project.
fn member_candidates(
    session: &Session,
    receiver: &str,
    kind: MemberKind,
    written_key: &str,
) -> Vec<String> {
    // A display the folded key cannot round-trip (a union type, an
    // anonymous class) yields no candidates and therefore no noise.
    if receiver.contains('|') || receiver.contains('@') {
        return Vec::new();
    }
    let db = &session.database;
    let class_key = folded_symbol_key(SymbolSpace::ClassLike, receiver);
    let stub_kind = match kind {
        MemberKind::Method => StubMemberKind::Method,
        MemberKind::Property => StubMemberKind::Property,
        MemberKind::ClassConstant => StubMemberKind::ClassConstant,
        MemberKind::EnumCase => StubMemberKind::EnumCase,
    };
    let mut names: Vec<String> = Vec::new();
    let mut stub_roots: Vec<String> = Vec::new();
    let class = ClassQuery::new(db, class_key.clone());
    match linearized_class(db, session.files, session.stubs, session.configuration, class)
        .as_ref()
    {
        Some(linearized) => {
            for entry in &linearized.members {
                if entry.member.kind == kind {
                    names.push(entry.member.name.clone());
                }
            }
            for edge in &linearized.ancestry {
                if let Some(stub_key) = &edge.stub {
                    stub_roots.push(stub_key.clone());
                }
            }
        }
        None => stub_roots.push(class_key),
    }
    let table = stub_signature_table(db, session.stubs);
    let range = session.configuration.php_version_range(db);
    let mut queue: VecDeque<String> = stub_roots.into();
    let mut visited: HashSet<String> = HashSet::new();
    while let Some(key) = queue.pop_front() {
        if !visited.insert(key.clone()) {
            continue;
        }
        let Some(surface) = table.class(&key) else {
            continue;
        };
        for member in &surface.members {
            if member.kind == stub_kind && member.availability.exists_in(range) {
                names.push(member.name.clone());
            }
        }
        for parent in &surface.parents {
            queue.push_back(folded_symbol_key(SymbolSpace::ClassLike, parent));
        }
    }
    names.retain(|name| folded_member_key(kind, name) != written_key);
    names.sort();
    names.dedup();
    names
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_cli suggest`
Expected: PASS. If the `ArrayObject` test finds a tie instead of `count` (the stub surface carries many methods), inspect the actual tie members: the assertion may legitimately need a more distinctive fixture (for example the typo `getArrayCop` for `getArrayCopy`); adjust the fixture, not the policy.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p celerrate_cli --all-targets -- -D warnings
git add crates/celerrate_cli/src/suggest.rs
git commit -m "✨ feat(cli): suggest near members on the receiver's surface"
```

---

### Task 4: Render help and note lines, wire enrichment into the single pass

**Files:**
- Modify: `crates/celerrate_cli/src/render.rs`
- Modify: `crates/celerrate_cli/src/lib.rs` (the `Command::Check` single-pass arm)
- Modify (snapshots, hand-inspected): `crates/celerrate_cli/tests/snapshots/*`

**Interfaces:**
- Consumes: `suggest::enrich` (task 2).
- Produces: `pub fn render_report(output, session, outcome) -> io::Result<()>` (notices + diagnostics with `  note:`/`  help:` sub-lines + summary, no internal errors) and `render_check` re-expressed as `render_report` followed by `render_internal_errors`. Task 7 relies on this split to print the fix trailer between report and internal errors.

- [ ] **Step 1: Write the failing test**

In `crates/celerrate_cli/tests/check.rs`, add:

```rust
/// The presentation-time did-you-mean surfaces in the plain report: a
/// `help:` line under the diagnostic that owns the suggestion. This is
/// the minimal pre-part-7 rendering; the rich renderer replaces it.
#[test]
fn a_near_typo_renders_a_help_line_under_its_diagnostic() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        (
            "src/User.php",
            "<?php\nnamespace App;\nclass User { public function save(): void {} }\n",
        ),
        (
            "src/Caller.php",
            "<?php\nnamespace App;\nfunction persist(User $user): void { $user->svae(); }\n",
        ),
    ]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    insta::assert_snapshot!("help_line", normalize_location_separators(&text));
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p celerrate_cli --test check a_near_typo_renders_a_help_line`
Expected: FAIL — the snapshot does not exist yet AND the output carries no `help:` line (enrichment is not wired). Inspect the produced `.snap.new`: it must NOT contain `help:` yet.

- [ ] **Step 3: Implement the render split and the sub-lines**

In `render.rs`, rename the body of `render_check` to `render_report` and stop it before internal errors; re-express `render_check` (the watch path keeps calling it):

```rust
/// Notices, then diagnostics with their note and help sub-lines, then
/// the summary. No internal errors: the single-pass path prints those
/// last, after the fix trailer, through `render_internal_errors`.
pub fn render_report(
    output: &mut dyn Write,
    session: &Session,
    outcome: &AnalysisOutcome,
) -> io::Result<()> {
    // ... existing notice block unchanged ...

    if !outcome.diagnostics.is_empty() {
        for diagnostic in &outcome.diagnostics {
            writeln!(output, "{}", render_diagnostic(session, diagnostic))?;
            for note in &diagnostic.notes {
                writeln!(output, "  note: {note}")?;
            }
            for suggestion in &diagnostic.suggestions {
                writeln!(output, "  help: {}", suggestion.message)?;
            }
        }
        writeln!(output)?;
    }

    // ... existing summary line unchanged ...
    Ok(())
}

/// The complete check screen: the report, then the internal errors.
/// The watch cycle uses this whole; the single-pass path calls the two
/// halves itself so the fix trailer can sit between them.
pub fn render_check(
    output: &mut dyn Write,
    session: &Session,
    outcome: &AnalysisOutcome,
) -> io::Result<()> {
    render_report(output, session, outcome)?;
    render_internal_errors(output, session)
}
```

In `lib.rs`, rewrite the single-pass tail of the `Command::Check` arm to enrich before rendering (the fix flags arrive in task 7; this task only splits and enriches):

```rust
            let inputs = session.inputs();
            let outcome = single_pass(&mut session, || analysis::analyze(&inputs));
            session.absorb_outcome(&outcome);
            // Presentation only: the persisted verdicts and the exit
            // code both read `outcome`, never the enriched copy.
            let presented = analysis::AnalysisOutcome {
                diagnostics: suggest::enrich(&session, &outcome.diagnostics),
                panicked: outcome.panicked.clone(),
            };
            if render::render_report(output, &session, &presented).is_err() {
                return Outcome::InternalError;
            }
            cache::persist(&mut session, &outcome);
            if render::render_internal_errors(output, &session).is_err() {
                return Outcome::InternalError;
            }
            session.statistics.report();
            Outcome::of(outcome.diagnostics.len(), session.internal_errors.len())
```

- [ ] **Step 4: Run the CLI suite; review every changed snapshot by hand**

Run: `cargo test -p celerrate_cli`
Expected: the new `help_line` snapshot is created (accept it after checking it shows `  help: did you mean `save`?` directly under the CEL0030 line). Existing snapshots (`check.rs`, `seeded_defects.rs`, cache warm/cold suites) may gain `help:`/`note:` lines wherever a fixture's typo has a near candidate — this is verify-then-accept: inspect each `.snap.new` diff, confirm every new line is a correct suggestion for its diagnostic, then accept with `cargo insta accept` (or by moving the `.snap.new` files). A warm/cold equivalence test must never diverge (enrichment is deterministic and runs on both paths); if one does, that is a bug, not a snapshot to accept.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p celerrate_cli --all-targets -- -D warnings
git add -A crates/celerrate_cli
git commit -m "✨ feat(cli): render presentation-time help and note lines"
```

---

### Task 5: Fix planning — first wins, in the total order

**Files:**
- Modify: `crates/celerrate_edit/src/conflict.rs` (publish `find_conflict`)
- Modify: `crates/celerrate_edit/src/lib.rs` (re-export)
- Create: `crates/celerrate_cli/src/fix.rs`
- Modify: `crates/celerrate_cli/src/lib.rs` (add `pub mod fix;`)

**Interfaces:**
- Consumes: `celerrate_edit::find_conflict` (published here), `celerrate_diagnostics::{Diagnostic, Confidence}`.
- Produces: `pub fn fix_threshold(fix: bool, fix_suggestions: bool) -> Option<Confidence>`; `pub fn plan(diagnostics: &[Diagnostic], threshold: Confidence) -> PlannedFixes`; `pub struct PlannedFixes { pub edits_by_file: BTreeMap<FileId, Vec<TextEdit>>, pub accepted: usize, pub skipped: Vec<SkippedFix> }`; `pub struct SkippedFix { pub file: FileId, pub message: String, pub reason: SkipReason }`; `pub enum SkipReason { Overlap, ForeignFile }`. Tasks 6 and 7 consume all of these.

- [ ] **Step 1: Publish `find_conflict`**

In `conflict.rs`, change `pub(crate) fn find_conflict` to `pub fn find_conflict` and extend its doc comment with one sentence: `Public so an application layer can ask the conflict question before deciding what to apply.` In `lib.rs`, change the re-export line to `pub use conflict::{EditConflict, find_conflict};`. Run `cargo test -p celerrate_edit` — everything stays green.

- [ ] **Step 2: Write the failing tests**

Create `crates/celerrate_cli/src/fix.rs` with the module documentation and tests:

```rust
//! The application engine (design section 7): a single pass in the
//! total diagnostic order, expressed against the original snapshot
//! coordinates, planned per file and applied atomically. A fix whose
//! edits overlap an already-applied fix is skipped and reported —
//! deterministically, the first wins — never silently merged. No
//! fixpoint: re-running `check` after application shows what remains.

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

    use celerrate_diagnostics::{Confidence, Diagnostic, DiagnosticId, Severity, Suggestion};
    use celerrate_source::{FileId, TextEdit, TextRange, TextSize};

    use super::{SkipReason, fix_threshold, plan};

    fn suggestion(confidence: Confidence, file: u32, start: u32, end: u32) -> Suggestion {
        Suggestion {
            message: format!("did you mean `x{start}`?"),
            confidence,
            edits: vec![TextEdit {
                file: FileId::new(file),
                range: TextRange::new(TextSize::from(start), TextSize::from(end)),
                replacement: format!("x{start}"),
            }],
        }
    }

    fn diagnostic(file: u32, start: u32, end: u32, suggestions: Vec<Suggestion>) -> Diagnostic {
        let mut diagnostic = Diagnostic::spanned(
            DiagnosticId::new("CEL0030"),
            Severity::Error,
            FileId::new(file),
            TextRange::new(TextSize::from(start), TextSize::from(end)),
            "unknown method `m` on `T`".to_owned(),
        );
        diagnostic.suggestions = suggestions;
        diagnostic
    }

    #[test]
    fn the_threshold_maps_the_two_flags_onto_the_confidence_order() {
        assert_eq!(fix_threshold(false, false), None);
        assert_eq!(fix_threshold(true, false), Some(Confidence::Safe));
        assert_eq!(fix_threshold(false, true), Some(Confidence::NeedsReview));
        assert_eq!(fix_threshold(true, true), Some(Confidence::NeedsReview));
    }

    #[test]
    fn fix_applies_safe_only_and_fix_suggestions_applies_both() {
        let diagnostics = vec![
            diagnostic(0, 0, 4, vec![suggestion(Confidence::Safe, 0, 0, 4)]),
            diagnostic(0, 10, 14, vec![suggestion(Confidence::NeedsReview, 0, 10, 14)]),
        ];
        let safe_only = plan(&diagnostics, Confidence::Safe);
        assert_eq!(safe_only.accepted, 1);
        let both = plan(&diagnostics, Confidence::NeedsReview);
        assert_eq!(both.accepted, 2);
        assert!(both.skipped.is_empty());
    }

    #[test]
    fn the_first_fix_wins_an_overlap_and_the_loser_is_reported() {
        let diagnostics = vec![
            diagnostic(0, 0, 4, vec![suggestion(Confidence::NeedsReview, 0, 0, 4)]),
            diagnostic(0, 2, 6, vec![suggestion(Confidence::NeedsReview, 0, 2, 6)]),
        ];
        let planned = plan(&diagnostics, Confidence::NeedsReview);
        assert_eq!(planned.accepted, 1);
        assert_eq!(planned.skipped.len(), 1);
        assert_eq!(planned.skipped[0].reason, SkipReason::Overlap);
        assert_eq!(planned.skipped[0].file, FileId::new(0));
        // The winner is the first in the given order, so the plan is
        // deterministic by construction.
        let edits = planned.edits_by_file.get(&FileId::new(0)).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(u32::from(edits[0].range.start()), 0);
    }

    #[test]
    fn coordinates_stay_original_snapshot_coordinates_across_a_file() {
        // Two accepted fixes on one file: the second is not shifted by
        // the first — both carry pre-application offsets, and the
        // splice resolves them together.
        let diagnostics = vec![
            diagnostic(0, 0, 4, vec![suggestion(Confidence::NeedsReview, 0, 0, 4)]),
            diagnostic(0, 20, 24, vec![suggestion(Confidence::NeedsReview, 0, 20, 24)]),
        ];
        let planned = plan(&diagnostics, Confidence::NeedsReview);
        let edits = planned.edits_by_file.get(&FileId::new(0)).unwrap();
        assert_eq!(
            edits
                .iter()
                .map(|edit| u32::from(edit.range.start()))
                .collect::<Vec<_>>(),
            vec![0, 20],
        );
    }

    #[test]
    fn a_cross_file_edit_is_skipped_as_foreign() {
        let foreign = Suggestion {
            edits: vec![TextEdit {
                file: FileId::new(9),
                range: TextRange::new(TextSize::from(0), TextSize::from(1)),
                replacement: "x".to_owned(),
            }],
            ..suggestion(Confidence::NeedsReview, 0, 0, 4)
        };
        let planned = plan(&[diagnostic(0, 0, 4, vec![foreign])], Confidence::NeedsReview);
        assert_eq!(planned.accepted, 0);
        assert_eq!(planned.skipped.len(), 1);
        assert_eq!(planned.skipped[0].reason, SkipReason::ForeignFile);
    }

    #[test]
    fn an_empty_suggestion_and_a_project_finding_plan_nothing() {
        let empty = Suggestion {
            edits: Vec::new(),
            ..suggestion(Confidence::Safe, 0, 0, 4)
        };
        let project = Diagnostic::project(
            DiagnosticId::new("CEL0025"),
            Severity::Warning,
            "no composer.json found".to_owned(),
        );
        let planned = plan(
            &[diagnostic(0, 0, 4, vec![empty]), project],
            Confidence::NeedsReview,
        );
        assert_eq!(planned.accepted, 0);
        assert!(planned.skipped.is_empty());
        assert!(planned.edits_by_file.is_empty());
    }
}
```

Add `pub mod fix;` to the module list in `crates/celerrate_cli/src/lib.rs`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p celerrate_cli fix`
Expected: FAIL to compile (`plan`, `fix_threshold` not found).

- [ ] **Step 4: Write the implementation**

Above the test module in `fix.rs`:

```rust
use std::collections::BTreeMap;

use celerrate_diagnostics::{Confidence, Diagnostic};
use celerrate_edit::find_conflict;
use celerrate_source::{FileId, TextEdit};

/// What the two flags admit: every suggestion at or below the
/// threshold in the `Confidence` order (`Safe < NeedsReview`).
/// `--fix` alone is `Safe` only — and at closure of this sub-project
/// every shipped fix is `NeedsReview`, so `--fix` applies nothing;
/// that is the design's owned consequence, stated, not hidden.
pub fn fix_threshold(fix: bool, fix_suggestions: bool) -> Option<Confidence> {
    if fix_suggestions {
        Some(Confidence::NeedsReview)
    } else if fix {
        Some(Confidence::Safe)
    } else {
        None
    }
}

/// Why a fix could not join the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// An edit overlaps one an earlier fix already claimed (or the
    /// fix's own edits overlap each other). The first fix wins.
    Overlap,
    /// An edit targets a file other than the diagnostic's own.
    /// Cross-file suggestion edits are out of scope (design section 3).
    ForeignFile,
}

/// One fix that was skipped, in encounter order, for the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFix {
    pub file: FileId,
    pub message: String,
    pub reason: SkipReason,
}

/// The plan: per-file accepted edits in original-snapshot coordinates,
/// and everything skipped. One fix is one suggestion, whole: all its
/// edits enter or none do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlannedFixes {
    pub edits_by_file: BTreeMap<FileId, Vec<TextEdit>>,
    pub accepted: usize,
    pub skipped: Vec<SkippedFix>,
}

/// Plans the single application pass. `diagnostics` must already be in
/// the total diagnostic order (the analysis outcome is), so "first
/// wins" is deterministic without re-sorting anything here.
pub fn plan(diagnostics: &[Diagnostic], threshold: Confidence) -> PlannedFixes {
    let mut planned = PlannedFixes::default();
    for diagnostic in diagnostics {
        let Some((file, _)) = diagnostic.span() else {
            continue;
        };
        for suggestion in &diagnostic.suggestions {
            if suggestion.confidence > threshold || suggestion.edits.is_empty() {
                continue;
            }
            if suggestion.edits.iter().any(|edit| edit.file != file) {
                planned.skipped.push(SkippedFix {
                    file,
                    message: suggestion.message.clone(),
                    reason: SkipReason::ForeignFile,
                });
                continue;
            }
            let accepted = planned.edits_by_file.entry(file).or_default();
            let mut trial: Vec<TextEdit> = accepted.clone();
            trial.extend(suggestion.edits.iter().cloned());
            trial.sort();
            if find_conflict(&trial).is_some() {
                planned.skipped.push(SkippedFix {
                    file,
                    message: suggestion.message.clone(),
                    reason: SkipReason::Overlap,
                });
                continue;
            }
            *accepted = trial;
            planned.accepted += 1;
        }
    }
    planned
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p celerrate_cli fix && cargo test -p celerrate_edit`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add crates/celerrate_edit crates/celerrate_cli/src/fix.rs crates/celerrate_cli/src/lib.rs
git commit -m "✨ feat(cli): plan fixes first-wins in the total diagnostic order"
```

---

### Task 6: Application to disk through the VFS

**Files:**
- Modify: `crates/celerrate_cli/src/fix.rs`
- Modify: `crates/celerrate_cli/src/session.rs` (two `InternalError` variants)
- Modify: `crates/celerrate_cli/src/render.rs` (the two new match arms in `render_internal_errors`)

**Interfaces:**
- Consumes: task 5's `PlannedFixes`; `celerrate_edit::apply`; `celerrate_db::source_text`; `Session { sources, database, vfs, internal_errors }`.
- Produces: `pub fn apply_to_disk(session: &mut Session, planned: &PlannedFixes) -> AppliedFixes`; `pub struct AppliedFixes { pub files_written: usize }`; `InternalError::FixUnappliable { file: FileId, reason: String }` (a Celerrate bug: the planner admitted what the applier refused) and `InternalError::FixWriteFailed { path: PathBuf, reason: String }` (the environment's condition, like `FileUnreadable` — no bug invitation).

- [ ] **Step 1: Write the failing tests**

Append to the test module in `fix.rs`:

```rust
    use crate::session::{InternalError, Session};

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

    #[test]
    fn accepted_edits_are_spliced_once_and_written_through_the_vfs_to_disk() {
        let root = project(&[("src/App.php", "<?php aaaa(); bbbb();")]);
        let mut session = Session::start(root.path());
        let file = *session.sources.keys().next().unwrap();
        // Two original-coordinate edits: `aaaa` (6..10) and `bbbb`
        // (14..18); replacements of different lengths prove the splice
        // resolves both against the pre-application text.
        let diagnostics = vec![
            diagnostic(file.as_u32(), 6, 10, vec![Suggestion {
                message: "did you mean `a`?".to_owned(),
                confidence: Confidence::NeedsReview,
                edits: vec![TextEdit {
                    file,
                    range: TextRange::new(TextSize::from(6), TextSize::from(10)),
                    replacement: "a".to_owned(),
                }],
            }]),
            diagnostic(file.as_u32(), 14, 18, vec![Suggestion {
                message: "did you mean `bb`?".to_owned(),
                confidence: Confidence::NeedsReview,
                edits: vec![TextEdit {
                    file,
                    range: TextRange::new(TextSize::from(14), TextSize::from(18)),
                    replacement: "bb".to_owned(),
                }],
            }]),
        ];
        let planned = plan(&diagnostics, Confidence::NeedsReview);
        let applied = super::apply_to_disk(&mut session, &planned);
        assert_eq!(applied.files_written, 1);
        assert!(session.internal_errors.is_empty());
        let patched = std::fs::read_to_string(root.path().join("src/App.php")).unwrap();
        assert_eq!(patched, "<?php a(); bb();");
        // The VFS effective state followed the disk.
        assert_eq!(
            session.vfs.contents(file),
            Some("<?php a(); bb();".as_bytes()),
        );
    }

    #[test]
    fn an_out_of_bounds_edit_is_an_internal_error_not_a_panic_and_other_files_still_write() {
        let root = project(&[
            ("src/Bad.php", "<?php short();"),
            ("src/Good.php", "<?php aaaa();"),
        ]);
        let mut session = Session::start(root.path());
        let mut files = session.sources.keys().copied();
        let bad = files.next().unwrap();
        let good = files.next().unwrap();
        let mut planned = super::PlannedFixes::default();
        planned.edits_by_file.insert(bad, vec![TextEdit {
            file: bad,
            range: TextRange::new(TextSize::from(500), TextSize::from(504)),
            replacement: "x".to_owned(),
        }]);
        planned.edits_by_file.insert(good, vec![TextEdit {
            file: good,
            range: TextRange::new(TextSize::from(6), TextSize::from(10)),
            replacement: "a".to_owned(),
        }]);
        let applied = super::apply_to_disk(&mut session, &planned);
        assert_eq!(applied.files_written, 1, "the good file still writes");
        assert!(matches!(
            session.internal_errors.as_slice(),
            [InternalError::FixUnappliable { file, .. }] if *file == bad,
        ));
    }
```

(The `bad`/`good` binding order relies on `sources` being a `BTreeMap` keyed by `FileId` and the walk interning `Bad.php` before `Good.php` alphabetically; if the walk order differs, resolve the two ids through `session.vfs.path` instead — the assertion logic stays the same.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_cli fix`
Expected: FAIL to compile (`apply_to_disk`, `AppliedFixes`, `FixUnappliable` not found).

- [ ] **Step 3: Write the implementation**

In `session.rs`, add to `InternalError`:

```rust
    /// A planned fix could not be applied to its file's text. This is
    /// Celerrate's bug: the planner admitted an edit set the applier
    /// refused.
    FixUnappliable { file: FileId, reason: String },
    /// The patched text could not be written back to disk. The
    /// environment's condition, like `FileUnreadable`: named, but no
    /// bug report invited.
    FixWriteFailed { path: PathBuf, reason: String },
```

In `render.rs`, add the two arms to the `match` in `render_internal_errors` (`FixUnappliable` sets `has_celerrate_bug = true`; `FixWriteFailed` does not):

```rust
            InternalError::FixUnappliable { file, reason } => {
                has_celerrate_bug = true;
                writeln!(
                    output,
                    "internal error: the fix for {} could not be applied: {reason}",
                    display_path(session, *file),
                )?;
            }
            InternalError::FixWriteFailed { path, reason } => writeln!(
                output,
                "internal error: {} could not be written: {reason}; the fix was not applied",
                relative_path(session, path),
            )?,
```

In `fix.rs` (production section), add:

```rust
use std::path::Path;

use crate::session::{InternalError, Session};

/// The outcome of writing a plan to disk.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AppliedFixes {
    pub files_written: usize,
}

/// Applies the planned edits: per file, one splice of all accepted
/// edits against the exact decoded text the analysis read, written to
/// disk and echoed into the `Vfs` so the session's effective state
/// stays true. Every failure is an internal error the run reports and
/// survives; nothing panics, and no file is ever partially written.
pub fn apply_to_disk(session: &mut Session, planned: &PlannedFixes) -> AppliedFixes {
    let mut applied = AppliedFixes::default();
    for (&file, edits) in &planned.edits_by_file {
        let Some(&source) = session.sources.get(&file) else {
            session.internal_errors.push(InternalError::FixUnappliable {
                file,
                reason: "the file is no longer in the analyzed set".to_owned(),
            });
            continue;
        };
        let original = match celerrate_db::source_text(&session.database, source).as_ref() {
            Ok(text) => text.text().to_owned(),
            Err(_) => {
                session.internal_errors.push(InternalError::FixUnappliable {
                    file,
                    reason: "the source text could not be decoded".to_owned(),
                });
                continue;
            }
        };
        let patched = match celerrate_edit::apply(&original, edits) {
            Ok(patched) => patched,
            Err(error) => {
                session.internal_errors.push(InternalError::FixUnappliable {
                    file,
                    reason: format!("{error:?}"),
                });
                continue;
            }
        };
        let Some(path) = session.vfs.path(file).map(Path::to_path_buf) else {
            session.internal_errors.push(InternalError::FixUnappliable {
                file,
                reason: "the file has no path in the VFS".to_owned(),
            });
            continue;
        };
        if let Err(reason) = std::fs::write(&path, patched.as_bytes()) {
            session.internal_errors.push(InternalError::FixWriteFailed {
                path,
                reason: reason.to_string(),
            });
            continue;
        }
        session
            .vfs
            .set_file_contents(&path, Some(patched.into_bytes()));
        applied.files_written += 1;
    }
    applied
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p celerrate_cli`
Expected: PASS (including the untouched render tests — the new `InternalError` variants compile into the existing exhaustive match).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p celerrate_cli --all-targets -- -D warnings
git add crates/celerrate_cli/src/fix.rs crates/celerrate_cli/src/session.rs crates/celerrate_cli/src/render.rs
git commit -m "✨ feat(cli): apply planned fixes through the vfs to disk"
```

---

### Task 7: The CLI flags, the fix trailer, and the end-to-end suite

**Files:**
- Modify: `crates/celerrate_cli/src/arguments.rs`
- Modify: `crates/celerrate_cli/src/render.rs` (add `render_fix_summary`)
- Modify: `crates/celerrate_cli/src/lib.rs` (wire the flags into the single pass)
- Create: `crates/celerrate_cli/tests/fix.rs`

**Interfaces:**
- Consumes: `fix::{fix_threshold, plan, apply_to_disk, PlannedFixes, AppliedFixes, SkipReason}`; the task 4 render split.
- Produces: `Command::Check { path, watch, fix, fix_suggestions }`; `pub fn render_fix_summary(output: &mut dyn Write, session: &Session, planned: &PlannedFixes, applied: &AppliedFixes) -> io::Result<()>`. The user-facing contract: `applied N fix(es) to M file(s)` always prints under a fix flag, plus one `skipped fix in <path>: <message> (<reason>)` line per skipped fix.

- [ ] **Step 1: Write the failing argument tests**

In `arguments.rs` tests, extend the existing `Command::Check` destructurings with the two new fields and add:

```rust
    #[test]
    fn the_two_fix_flags_parse_and_default_off() {
        let arguments =
            Arguments::try_parse_from(["celerrate", "check", "src", "--fix"]).unwrap();
        let Command::Check { fix, fix_suggestions, .. } = arguments.command else {
            panic!("expected Command::Check");
        };
        assert!(fix);
        assert!(!fix_suggestions);
        let arguments =
            Arguments::try_parse_from(["celerrate", "check", "--fix-suggestions"]).unwrap();
        let Command::Check { fix, fix_suggestions, .. } = arguments.command else {
            panic!("expected Command::Check");
        };
        assert!(!fix);
        assert!(fix_suggestions);
    }

    /// Either fix flag combined with `--watch` is a usage error
    /// (design section 7): applying edits from inside a watch loop
    /// would race the watcher against its own writes.
    #[test]
    fn a_fix_flag_with_watch_is_a_usage_error() {
        assert!(Arguments::try_parse_from(["celerrate", "check", "--fix", "--watch"]).is_err());
        assert!(
            Arguments::try_parse_from(["celerrate", "check", "--fix-suggestions", "--watch"])
                .is_err()
        );
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p celerrate_cli arguments`
Expected: FAIL to compile (unknown fields `fix`, `fix_suggestions`).

- [ ] **Step 3: Implement flags, trailer, and wiring**

`arguments.rs` — extend the variant:

```rust
    Check {
        /// The project root. Defaults to the current directory.
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Re-analyze on every change, and keep reporting.
        #[arg(long)]
        watch: bool,

        /// Apply the safe suggestions and rewrite the files.
        #[arg(long, conflicts_with = "watch")]
        fix: bool,

        /// Apply safe and needs-review suggestions alike.
        #[arg(long, conflicts_with = "watch")]
        fix_suggestions: bool,
    },
```

`render.rs` — the trailer:

```rust
/// The fix trailer: what was applied, what was skipped and why.
/// Prints only under a fix flag. `applied 0 fixes to 0 files` is the
/// honest line the design requires: at closure of this sub-project
/// every shipped fix is `NeedsReview`, so `--fix` alone applies
/// nothing, visibly.
pub fn render_fix_summary(
    output: &mut dyn Write,
    session: &Session,
    planned: &crate::fix::PlannedFixes,
    applied: &crate::fix::AppliedFixes,
) -> io::Result<()> {
    writeln!(
        output,
        "applied {} to {}",
        count(planned.accepted, "fix", "fixes"),
        count(applied.files_written, "file", "files"),
    )?;
    for skipped in &planned.skipped {
        let reason = match skipped.reason {
            crate::fix::SkipReason::Overlap => "overlaps an already-applied fix",
            crate::fix::SkipReason::ForeignFile => "edits another file",
        };
        writeln!(
            output,
            "skipped fix in {}: {} ({reason})",
            display_path(session, skipped.file),
            skipped.message,
        )?;
    }
    writeln!(output)
}
```

`lib.rs` — destructure the new fields and insert between `cache::persist` and `render_internal_errors` (final single-pass order: report, persist, fix + trailer, internal errors, statistics):

```rust
        Command::Check { path, watch, fix, fix_suggestions } => {
            // ... unchanged until after cache::persist(&mut session, &outcome); ...
            if let Some(threshold) = fix::fix_threshold(fix, fix_suggestions) {
                let planned = fix::plan(&presented.diagnostics, threshold);
                let applied = fix::apply_to_disk(&mut session, &planned);
                if render::render_fix_summary(output, &session, &planned, &applied).is_err() {
                    return Outcome::InternalError;
                }
            }
            if render::render_internal_errors(output, &session).is_err() {
                return Outcome::InternalError;
            }
            session.statistics.report();
            Outcome::of(outcome.diagnostics.len(), session.internal_errors.len())
        }
```

- [ ] **Step 4: Write the end-to-end suite**

Create `crates/celerrate_cli/tests/fix.rs`:

```rust
//! The autofix engine, end to end: flags, application, the trailer,
//! and the design's honesty pins (design sections 7 and 11).

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::{Outcome, run};

const MANIFEST: &str =
    r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;

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

fn check_with(root: &Path, extra: &[&str]) -> (Outcome, String) {
    let mut arguments: Vec<std::ffi::OsString> =
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()];
    arguments.extend(extra.iter().map(Into::into));
    let mut output = Vec::new();
    let outcome = run(arguments, &mut output);
    (outcome, String::from_utf8(output).unwrap())
}

fn typo_project() -> tempfile::TempDir {
    project(&[
        ("composer.json", MANIFEST),
        (
            "src/User.php",
            "<?php\nnamespace App;\nclass User { public function save(): void {} }\n",
        ),
        (
            "src/Caller.php",
            "<?php\nnamespace App;\nfunction persist(User $user): void { $user->svae(); }\n",
        ),
    ])
}

/// The design's central promise: `--fix-suggestions` patches the file,
/// and re-checking the patched project no longer reports the fixed
/// diagnostic (the fix-closes-the-diagnostic property).
#[test]
fn fix_suggestions_patches_the_typo_and_the_recheck_is_clean() {
    let root = typo_project();
    let (outcome, text) = check_with(root.path(), &["--fix-suggestions"]);
    // The run still reports what it found: no fixpoint, exit 1.
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    assert!(text.contains("applied 1 fix to 1 file"), "{text}");
    let patched = std::fs::read_to_string(root.path().join("src/Caller.php")).unwrap();
    insta::assert_snapshot!("patched_caller", patched);
    assert!(patched.contains("$user->save()"), "{patched}");
    let (recheck, _) = check_with(root.path(), &[]);
    assert_eq!(recheck, Outcome::Clean, "the fix closes the diagnostic");
}

/// The owned consequence, pinned honestly: every natural fix is
/// `NeedsReview`, so `--fix` alone applies nothing at closure.
#[test]
fn fix_alone_applies_nothing_at_closure() {
    let root = typo_project();
    let before = std::fs::read_to_string(root.path().join("src/Caller.php")).unwrap();
    let (outcome, text) = check_with(root.path(), &["--fix"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    assert!(text.contains("applied 0 fixes to 0 files"), "{text}");
    let after = std::fs::read_to_string(root.path().join("src/Caller.php")).unwrap();
    assert_eq!(before, after, "the file is untouched");
}

/// The ambiguity discipline: a tie produces a note, never an edit, and
/// bulk application leaves the file alone.
#[test]
fn an_ambiguous_candidate_is_listed_and_never_applied() {
    let root = project(&[
        ("composer.json", MANIFEST),
        (
            "src/User.php",
            "<?php\nnamespace App;\nclass User { public function save(): void {} public function sove(): void {} }\n",
        ),
        (
            "src/Caller.php",
            "<?php\nnamespace App;\nfunction f(User $user): void { $user->sive(); }\n",
        ),
    ]);
    let before = std::fs::read_to_string(root.path().join("src/Caller.php")).unwrap();
    let (outcome, text) = check_with(root.path(), &["--fix-suggestions"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    assert!(
        text.contains("note: did you mean one of `save`, `sove`?"),
        "{text}",
    );
    assert!(text.contains("applied 0 fixes to 0 files"), "{text}");
    let after = std::fs::read_to_string(root.path().join("src/Caller.php")).unwrap();
    assert_eq!(before, after);
}

/// An unknown symbol rides the same pass: the class typo is patched
/// under `--fix-suggestions`.
#[test]
fn an_unknown_class_typo_is_patched_too() {
    let root = project(&[
        ("composer.json", MANIFEST),
        (
            "src/Gateway.php",
            "<?php\nnamespace App;\nclass PaymentGateway {}\n",
        ),
        (
            "src/Consumer.php",
            "<?php\nnamespace App;\nnew PaymentGatewya();\n",
        ),
    ]);
    let (_, text) = check_with(root.path(), &["--fix-suggestions"]);
    assert!(text.contains("applied 1 fix to 1 file"), "{text}");
    let patched = std::fs::read_to_string(root.path().join("src/Consumer.php")).unwrap();
    assert!(patched.contains("new PaymentGateway()"), "{patched}");
    let (recheck, _) = check_with(root.path(), &[]);
    assert_eq!(recheck, Outcome::Clean);
}

/// clap enforces the conflict; the product exits 2 with clap's own
/// message, like every other usage error.
#[test]
fn a_fix_flag_with_watch_exits_two() {
    let root = typo_project();
    let (outcome, text) = check_with(root.path(), &["--fix", "--watch"]);
    assert_eq!(outcome, Outcome::UsageError);
    assert!(text.contains("--watch"), "{text}");
}
```

- [ ] **Step 5: Run the whole suite**

Run: `cargo test -p celerrate_cli`
Expected: PASS; the new `patched_caller` snapshot is created — inspect it (the file must read `$user->save();` with everything else byte-identical to the fixture) and accept it.

- [ ] **Step 6: Add the trailer render test with a synthetic skip**

Append to `render.rs` tests (the natural pass cannot produce an overlap yet, so the trailer's skip line is driven synthetically):

```rust
    #[test]
    fn the_fix_trailer_names_the_skipped_fix_its_file_and_its_reason() {
        use celerrate_source::{FileId, TextEdit, TextRange, TextSize};
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.php"), "<?php echo 1;").unwrap();
        let mut session = Session::start(root.path());
        let file = *session.sources.keys().next().unwrap();
        let mut planned = crate::fix::PlannedFixes::default();
        planned.accepted = 1;
        planned.edits_by_file.insert(file, vec![TextEdit {
            file,
            range: TextRange::new(TextSize::from(6), TextSize::from(10)),
            replacement: "x".to_owned(),
        }]);
        planned.skipped.push(crate::fix::SkippedFix {
            file,
            message: "did you mean `save`?".to_owned(),
            reason: crate::fix::SkipReason::Overlap,
        });
        let applied = crate::fix::apply_to_disk(&mut session, &planned);
        let mut output = Vec::new();
        render::render_fix_summary(&mut output, &session, &planned, &applied).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("applied 1 fix to 1 file"), "{text}");
        assert!(
            text.contains("skipped fix in a.php: did you mean `save`? (overlaps an already-applied fix)"),
            "{text}",
        );
    }
```

Run: `cargo test -p celerrate_cli render`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
git add -A crates/celerrate_cli
git commit -m "✨ feat(cli): wire --fix and --fix-suggestions end to end"
```

---

### Task 8: Pin did-you-mean out of the dependency graph

**Files:**
- Modify: `crates/celerrate_rules/tests/invalidation_scope.rs`

**Interfaces:**
- Consumes: the existing harness in that file (`TestDatabase`, `register`, `configuration_for`, `executions_of`, `salsa::Setter`) and `semantic_phase_diagnostics(db, file, files, stubs, configuration)`.
- Produces: the spec section 11 pin — a rename in an unrelated file re-runs no phase query of an unaffected file. This is the graph-side half of the guarantee; the presentation side is out of the graph by construction (a plain function in `celerrate_cli`, no salsa tracking).

- [ ] **Step 1: Write the failing-or-green pin**

Append to `invalidation_scope.rs` (reuse the file's existing helpers; model the setup on `a_body_edit_reruns_only_the_editing_bodys_phase` for the `files`/`stubs` construction):

```rust
/// The did-you-mean gate (design sections 7 and 11): the candidate
/// search runs at presentation time, outside every phase query, so the
/// global name set never enters a file's dependency graph. The
/// graph-side proof: file A reports an unknown symbol; renaming a
/// declaration in file B that A never references re-runs nothing of
/// A's semantic phase. Had the phase computed candidates, A would
/// depend on the whole symbol table and any rename anywhere would
/// re-run it.
#[test]
fn a_rename_in_an_unrelated_file_reruns_no_phase_of_an_unaffected_file() {
    let mut db = TestDatabase::default();
    register(&db);
    let file_a = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php namespace App; new Cleint();".to_vec(),
    );
    let file_b = SourceFile::new(
        &db,
        FileId::new(1),
        b"<?php namespace Lib; class Helper {}".to_vec(),
    );
    let files = AnalyzedFileSet::new(&db, vec![file_a, file_b]);
    let stubs = StubIndexInput::builder(StubIndex::default())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = configuration_for(&db, PhpVersion::new(8, 1));
    let primed = semantic_phase_diagnostics(&db, file_a, files, stubs, configuration);
    assert_eq!(primed.len(), 1, "the unknown class primes A's phase: {primed:?}");
    assert_eq!(primed[0].id, DiagnosticId::new("CEL0018"));
    let _ = semantic_phase_diagnostics(&db, file_b, files, stubs, configuration);
    db.take_executed();

    // The rename: `Helper` becomes `Aide`. A never references either.
    file_b
        .set_bytes(&mut db)
        .to(b"<?php namespace Lib; class Aide {}".to_vec());
    let after = semantic_phase_diagnostics(&db, file_a, files, stubs, configuration);
    assert_eq!(after.len(), 1, "A's report is unchanged");

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "semantic_phase_diagnostics"),
        0,
        "an unrelated rename re-runs no phase of an unaffected file: {log:?}",
    );
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p celerrate_rules --test invalidation_scope a_rename_in_an_unrelated_file`
Expected: PASS immediately — the per-name lookup design already isolates A from B's rename, and did-you-mean never entered the phase. If it FAILS, that is a real regression to investigate before anything ships, not a test to weaken: it would mean some phase query grew a whole-table dependency.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy -p celerrate_rules --all-targets -- -D warnings
git add crates/celerrate_rules/tests/invalidation_scope.rs
git commit -m "✅ test(rules): pin did-you-mean out of the dependency graph"
```

---

### Task 9: Closure — gates and CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Run the full local gates**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

Expected: all green. `cargo deny` must be untouched by this plan (no dependency was added).

- [ ] **Step 2: Run the corpus gates**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: the corpus snapshot is byte-identical (the corpus is clean, enrichment touches only reported diagnostics, and no check changed) and the mixed-rate baseline unchanged. Any corpus delta is hand-inspected under verify-then-accept — but for this plan a delta means a bug, because no analysis behavior moved.

- [ ] **Step 3: Record the work in the CHANGELOG**

Add under the unreleased heading (match the file's existing entry style):

```markdown
- `celerrate check --fix` and `--fix-suggestions`: a single application
  pass in the total diagnostic order, expressed against original
  snapshot coordinates, applied atomically per file through the VFS;
  overlapping fixes are skipped and reported, first wins. Did-you-mean
  suggestions on unknown symbols (CEL0018 to CEL0020) and unknown
  members (CEL0030 to CEL0033), computed at presentation time from the
  symbol index and the receiver's member surface, never persisted; a
  unique near candidate becomes an applicable `NeedsReview` suggestion,
  a tie is listed in a note and never applied. All natural fixes are
  `NeedsReview`, so `--fix` alone applies nothing yet: its first real
  client is the style group. Either fix flag with `--watch` is a usage
  error.
```

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the autofix engine"
```

---

## Self-Review (performed while writing)

- **Spec coverage, section 7:** flag semantics (task 7), single pass in total order / original coordinates / atomic per file / first wins / skipped-and-reported (tasks 5-6), no fixpoint + re-run shows what remains (task 7 test asserts exit 1 plus clean re-check), fix+watch usage error (task 7), VFS write path with errors-never-panic (task 6), did-you-mean as presentation with both load-bearing reasons honored (tasks 1-3, pin in task 8), ambiguity discipline (tasks 1, 7), `--fix` applies nothing at closure (task 7 pin). Section 11's fix-engine bullets: patched-file snapshot, fix-closes property, original-coordinate overlap handling, ambiguity, did-you-mean out of the graph — all present; the edit-application fuzz target already exists from part 2 (`fuzz/fuzz_targets/edit_apply.rs`), nothing to add.
- **Type consistency:** `enrich(session, &[Diagnostic]) -> Vec<Diagnostic>` (tasks 2, 4, 7); `PlannedFixes { edits_by_file, accepted, skipped }` (tasks 5, 6, 7); `fix_threshold -> Option<Confidence>` (tasks 5, 7); `render_report`/`render_internal_errors` split (tasks 4, 7); `AppliedFixes { files_written }` (tasks 6, 7).
- **Known judgment calls left to the executor, named here rather than discovered:** exact import ordering per rustfmt; whether task 1's temporary `#[allow(dead_code)]` markers are needed at all (task 2 removes them); the `ArrayObject` fixture's uniqueness against the live stub surface (task 3 step 4 says how to adjust); walk-order sensitivity in task 6's two-file test (resolution path given inline).
