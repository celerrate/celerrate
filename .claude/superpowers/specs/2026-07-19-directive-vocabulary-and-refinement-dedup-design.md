# Directive-Vocabulary Sealing and Refinement Dedup — Design

Date: 2026-07-19
Status: Approved (issues #66 and #47; both fix directions were fully
specified by the issues and validated at triage)

Two small, independent hardening fixes batched into one branch because
each alone is too small to carry a review cycle. No behavioral change on
any shipped path; both close cross-crate misuse windows.

## 1. Issue #66 — seal the comment-directive vocabulary

### Problem

PR #65 put `#[non_exhaustive]` on the nine plugin-boundary vocabulary
types its scope enumerated. The comment-directive vocabulary in
`crates/celerrate_semantics/src/comment_directives.rs` crosses the same
plugin boundary (through `CommentDirectiveProvider`, re-exported by
`celerrate_plugin`) but stayed exhaustively matchable and literally
constructible cross-crate: `CommentDirective`, `CommentKind`,
`DirectiveScope`. This is the vocabulary most likely to grow variants
(new directive kinds, new suppression scopes), so today every addition
is a breaking change for any plugin matching exhaustively.

### Design

Same pattern as PR #65, compiler-driven:

- `#[non_exhaustive]` on the three declarations: `CommentDirective`,
  `CommentKind`, `DirectiveScope`.
- `#[non_exhaustive]` additionally on the `CommentDirective::Suppress`
  variant, closing cross-crate literal construction of its fields.
- One constructor where a cross-crate literal actually exists, mirroring
  the `ParsedAssertion::new` field-faithful shape:
  `CommentDirective::suppress(scope: DirectiveScope, identifiers:
  Vec<String>) -> Self`. `CommentKind` and `DirectiveScope` are fieldless;
  naming a unit variant cross-crate stays legal under
  `#[non_exhaustive]`, so they need no constructors.
- The bridge (`celerrate_phpdoc_bridge/src/directives.rs`) moves to the
  constructor, and its `match` on `CommentKind` gains a wildcard arm.
  The wildcard maps an unknown future comment kind to
  `DirectiveScope::CurrentAndNextLine` — the same over-suppressing
  superset the bare `@phpstan-ignore` already uses (design section 5:
  over-suppression, never under-suppression).

Zero behavioral regression: the existing suites in both crates must pass
unchanged (test-module literals inside `celerrate_semantics` remain
legal; bridge tests construct through the constructor).

## 2. Issue #47 — dedup in `StubRefinements::new`

### Problem

`StubRefinements::new` (`crates/celerrate_stubs/src/refinements.rs`)
sorts `functions`, `classes`, and each class's `methods` by key but does
not dedup, unlike `StubIndex::new` (sort then `dedup_by`, first entry
wins). A duplicate key survives as adjacent entries, making
`function_refinement`/`class_refinement`'s `binary_search` pick an
arbitrary member of the run. Verified unreachable on any shipped path
(the sole production producer, `parse_refinement_source`, rejects
duplicate keys; three tests pin that) — this is defense in depth for a
programmatic caller.

### Design

Match the `StubIndex::new` precedent exactly: after each of the three
sorts, `dedup_by` on key equality, first entry wins. Tests pin, for each
of the three collections, that a hand-constructed duplicate collapses to
the first entry and the lookup answers unambiguously.

## Testing

TDD per fix: each new test fails before its change lands. Full local
gates (`cargo test --workspace`, clippy at deny level, `cargo fmt`,
`cargo deny check`). Corpus gates (`cargo xtask corpus`,
`cargo xtask mixed-rate` after `cargo xtask fetch-corpus`) must show no
delta: neither fix may change any analysis result.

## Out of scope

- Identifier-matched suppression (#58): deferred to sub-project 4.
- Any new directive kinds or scopes: this seals the vocabulary, it does
  not extend it.
