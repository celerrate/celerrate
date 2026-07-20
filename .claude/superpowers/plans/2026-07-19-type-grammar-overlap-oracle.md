# Type-Grammar Overlap Oracle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A cross-provenance equivalence corpus over the norm/bridge overlap zone, repair of the enum-case divergence, and tightening of the norm parser to its documented v0 subset (issues #62 and #48), per `.claude/superpowers/specs/2026-07-19-type-grammar-overlap-oracle-design.md`.

**Architecture:** On branch `fix-62-48-type-grammar-overlap-oracle`. The oracle lives in `crates/celerrate_phpdoc_bridge/tests/cross_provenance.rs` — the one place both provenances are reachable, because the bridge's dev-dependencies already include `celerrate_types`, `celerrate_stubs`, `celerrate_db`, `celerrate_semantics`, and `salsa`. Each table entry drives one spelling through (a) a stub refinement (the norm path, via `celerrate_types`' declared-signature queries) and (b) a docblock (the bridge path, via the `TypeSyntax` dispatch), asserting the identical interned `TypeId` in one database. **No parser merge, no norm exposure through the seam** (spec's explicitly-rejected list).

**Tech Stack:** Rust 1.94, existing fixtures: mirror `crates/celerrate_phpdoc_bridge/tests/end_to_end.rs` (docblock → `TypeId` through a registered bridge), the `stub_refinement_fixture*` helpers in `crates/celerrate_types/src/inheritance.rs:652-824` (refinement → declared signature), and `crates/celerrate_stdlib_provider/tests/end_to_end.rs` (cross-crate registration on a test database).

## Global Constraints

- Zero panic lints at deny; `unsafe_code` forbidden; test modules may locally `#[allow]`.
- TDD: the oracle must FAIL on the enum-case divergence before its repair, and each norm-tightening rejection test must fail before the parser change.
- "Extended, never bypassed": any capability the bridge needs and `TypeContext` lacks is added to `TypeContext`, never worked around.
- Commits: gitmoji + Conventional Commits.
- Corpus gates: the enum-case repair may move typed diagnostics — a delta is hand-inspected under verify-then-accept (re-bless a verified mixed-rate precision gain with `--bless`). The norm tightening must show zero corpus delta.

---

### Task 1: The cross-provenance oracle

**Files:**
- Create: `crates/celerrate_phpdoc_bridge/tests/cross_provenance.rs`

**Interfaces:**
- Consumes: `celerrate_types`' public declared-signature query surface (as `inheritance.rs`' fixtures use), the bridge's `TypeSyntax` registration (as `end_to_end.rs` does).
- Produces: the standing oracle suite Task 2 and Task 3 run against.

- [ ] **Step 1: Write the harness and table**

The file's shape (assemble the two fixture paths by mirroring the three
precedent files named in the Tech Stack — do not invent new fixture
machinery):

```rust
//! The overlap oracle (issue #62): every spelling both grammars accept
//! must lower to the identical interned type from both provenances —
//! a stub refinement (the norm path) and a docblock (the bridge path).
//! Deliberate dialect differences are table entries too, so they are
//! documented here rather than invisible: a spelling one side rejects
//! is asserted to be rejected, and a change to either verdict fails
//! this suite.

/// One table entry: the spelling, and what each provenance must do
/// with it.
struct Entry {
    spelling: &'static str,
    norm: Verdict,
    bridge: Verdict,
}

enum Verdict {
    /// Lowers; when both sides lower, their `TypeId`s must be equal.
    Lowers,
    /// The grammar rejects the spelling (documented dialect gap).
    Rejects,
}

const TABLE: &[Entry] = &[
    // Keyword atoms and literals: full agreement expected.
    entry("int", Lowers, Lowers),
    entry("string", Lowers, Lowers),
    entry("bool", Lowers, Lowers),
    entry("mixed", Lowers, Lowers),
    entry("array-key", Lowers, Lowers),
    entry("'active'", Lowers, Lowers),
    entry("42", Lowers, Lowers),
    // Composition.
    entry("int|string", Lowers, Lowers),
    entry("Countable&Traversable", Lowers, Lowers),
    entry("?int", Lowers, Lowers),
    entry("?User|string", Lowers, Lowers),
    // Generics and their sugars.
    entry("array<int, string>", Lowers, Lowers),
    entry("array<string>", Lowers, Lowers),
    entry("list<int>", Lowers, Lowers),
    entry("non-empty-list<int>", Lowers, Lowers),
    entry("iterable<int>", Lowers, Lowers),
    entry("iterable<string, int>", Lowers, Lowers),
    // Shapes.
    entry("{id: int, name?: string}", Lowers, Rejects), // norm spelling
    entry("array{id: int, name?: string}", Rejects, Lowers), // dialect spelling
    // Callables.
    entry("callable(int, string=): void", Lowers, Lowers),
    entry("callable(int...): void", Lowers, Lowers),
    // Projections and class-string.
    entry("class-string", Lowers, Lowers),
    entry("class-string<User>", Lowers, Lowers),
    entry("key-of<array<int, string>>", Lowers, Lowers),
    entry("value-of<array<int, string>>", Lowers, Lowers),
    // Enum cases: the confirmed divergence this suite exists to catch.
    entry("Status::Active", Lowers, Lowers),
    // Documented dialect gaps (ranges spell differently by design).
    entry("int<1..5>", Lowers, Rejects),
    entry("int<1, 5>", Rejects, Lowers),
    entry("User[]", Rejects, Lowers),
];
```

(`entry` is a `const fn` building `Entry`; extend the table freely
where the two grammars' test suites reveal more shared spellings — the
minimum set is the spec's list.) The harness, per entry:

1. **Norm path:** build a stub refinement declaring a function whose
   return refinement text is `spelling` (the `stub_refinement_fixture`
   pattern), demand the declared signature, capture
   `Option<TypeId>`.
2. **Bridge path:** in the same database, with the bridge registered as
   the `TypeSyntax` implementation (the `end_to_end.rs` pattern),
   declare a function whose docblock is `/** @return <spelling> */`
   (with a `Status` enum and a `User` class in the fixture sources so
   named references resolve), demand the same query surface, capture
   `Option<TypeId>`.
3. Assert per the verdict pair; when both are `Lowers`, assert the two
   `TypeId`s are equal (same interner, same database — direct `==`),
   with the entry's spelling in the assertion message.

Shape-spelling caveat: the norm writes shapes bare (`{...}`) and the
dialect writes them prefixed (`array{...}`); the table encodes that as
two entries with opposite verdicts, plus a third assertion in the
harness for this pair alone: the norm's `{id: int}` and the bridge's
`array{id: int}` must lower to the SAME `TypeId` (equivalence across
different spellings is the point of documented sugar).

- [ ] **Step 2: Run and record the divergence**

Run: `cargo test -p celerrate_phpdoc_bridge --test cross_provenance`
Expected: FAIL on exactly `Status::Active` (norm: enum-case type;
bridge: `mixed`). Every other entry must pass; an unexpected second
failure is a discovered divergence — extend the table's comment and fix
it in Task 2 alongside the enum case if it is small, or record it as
its own issue if not.

- [ ] **Step 3: Commit (with the expected failure marked)**

Mark the enum-case entry's assertion with the standard
`#[should_panic]`-free TDD bridge: leave the suite red locally, do NOT
push yet — Task 2 lands in the same PR and makes it green. Commit the
harness only:

```bash
git add crates/celerrate_phpdoc_bridge/tests/cross_provenance.rs
git commit -m "✅ test(phpdoc-bridge): the cross-provenance overlap oracle (#62)"
```

---

### Task 2: Repair the enum-case divergence

**Files:**
- Modify: `crates/celerrate_phpdoc_bridge/src/lowering.rs` (the `ConstFetch` arm, near lines 34 and 132-134)
- Modify (only if the constructor is missing): `crates/celerrate_types/src/type_context.rs`

**Interfaces:**
- Consumes: `TypeContext`'s constructor set.
- Produces: `TypeContext::enum_case(...)` (if absent today) mirroring the raw `TypeId::enum_case` builder, and a bridge lowering that uses it.

- [ ] **Step 1: Establish the degradation semantics**

Read `TypeId::enum_case`'s construction and its consumers in
`celerrate_types` (the norm builds it at `norm.rs:409-414`; the member
and check layers consume it). Determine: what does an enum-case type
whose subject class is NOT an enum do downstream — does it degrade
gracefully (no false diagnostic), or does it assume enum-ness?

- **If graceful:** the bridge lowers every `Foo::BAR` const fetch
  (wildcard `Foo::*` excluded — stays `mixed`) through
  `context.enum_case(...)`, matching the norm.
- **If not graceful:** the repair narrows: the bridge keeps `mixed` for
  const fetches, and the oracle's `Status::Active` entry becomes a
  DOCUMENTED divergence (`norm: Lowers, bridge: Lowers` with an
  explicit inequality assertion and a comment naming the follow-up
  issue to file). Do not guess enum-ness in the bridge: it has no
  symbol table, and a wrong guess is a correctness bug.

Record which branch was taken in the commit message.

- [ ] **Step 2: Implement (graceful branch)**

If `TypeContext` lacks the constructor, add it next to its siblings,
delegating to the raw builder exactly as the neighboring constructors
do (the equivalence test at `type_syntax.rs:541-553` extends with one
line for it). Then the `ConstFetch` arm in `lowering.rs` becomes:

```rust
        TypeExpression::ConstFetch { class, constant } if constant != "*" => {
            context.enum_case(&site.qualify_class_name(class), constant)
        }
```

(match the file's actual AST field names; the wildcard arm keeps its
current `mixed` lowering).

- [ ] **Step 3: Run the oracle and the bridge suites**

Run: `cargo test -p celerrate_phpdoc_bridge`
Expected: the oracle PASSES entirely (enum-case entry included);
`end_to_end.rs` and the PHPStan corpus verdicts may have pinned
`Foo::BAR → mixed` — update those pins deliberately, each reviewed as
an intended improvement, never blanket-regenerated.

- [ ] **Step 4: Run the types suite**

Run: `cargo test -p celerrate_types`
Expected: PASS (the `TypeContext` extension is additive).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_phpdoc_bridge crates/celerrate_types
git commit -m "🐛 fix(phpdoc-bridge): enum-case references lower like the norm's (#62)"
```

---

### Task 3: Tighten the norm parser to the documented subset (#48)

**Files:**
- Modify: `crates/celerrate_types/src/norm.rs` — the five wide-form sites: bare collection generics (`:509-511`, `:530-531`, `:549-550`, and the `non-empty-*` bare forms near `:398-400`), bare `callable` (`:596-599`), empty shape (`:634-636`), quoted shape keys (`:641`), hyphenated name lowering (`:203-212` lexer note — the constraint lands in `named_type`, not the lexer), stacked nullable (`:351-354`)
- Test: the file's test module

**Interfaces:**
- Consumes: nothing new.
- Produces: `lower_norm_text` answering `None` for the five families; the three documented conveniences (draft §3.1) positively pinned.

- [ ] **Step 1: Write the failing rejection tests and the convenience pins**

```rust
#[test]
fn forms_outside_the_documented_subset_are_rejected() {
    // Issue #48: each of these parsed to a sound over-approximation
    // with no test pinning it; the documented subset (decision 13,
    // plan 2026-07-16-type-engine-7-providers.md) does not name them,
    // and an undocumented accepted spelling is compatibility debt in
    // a grammar that intends to freeze (norm draft, design rule 1:
    // one spelling per constructor).
    for rejected in [
        "array", "list", "iterable", "non-empty-array", "non-empty-list",
        "callable",
        "{}",
        "{'a': int}",
        "Foo-Bar",
        "??int",
    ] {
        assert_lowers_to_none(rejected);
    }
}

#[test]
fn the_documented_conveniences_lower() {
    // Norm draft §3.1: the three v0 conveniences, positively pinned.
    assert_lowers(
        "array-key",
        /* int|string, via the existing display-assertion helper */
    );
    assert_lowers("array<string>", /* array<int|string, string> */);
    assert_lowers("iterable<string>", /* iterable<mixed, string> */);
}
```

Use the module's existing assertion helpers (the tests at
`norm.rs:685-1035` show the house pattern — display-string
assertions against a test database); `assert_lowers_to_none` mirrors
`everything_outside_the_subset_answers_none_never_a_panic`
(`norm.rs:1036`).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_types norm`
Expected: the rejection test FAILS on every listed form (each parses
today); the convenience pins may already pass (they exercise existing
behavior — that is fine, they are belt-and-suspenders, the rejection
test is the red driver).

- [ ] **Step 3: Tighten**

Per family, the minimal change:

- Bare collection forms: where `generic_arguments` answers `None`, the
  affected constructors return `None` instead of defaulting
  (`array_type`, `list_type`, `iterable_type`, and the `non-empty-*`
  arms in `named_type`). The single-argument sugars stay (they are
  documented conveniences).
- Bare `callable`: requires its parenthesized signature; the atom
  without `(` answers `None`.
- Empty shape: a leading `}` in `shape_type` answers `None`.
- Quoted keys: the `Token::Text` key arm in `shape_type` answers
  `None`.
- Hyphenated names: in `named_type`'s fallthrough to a class
  reference, a name containing `-` that is not one of the known
  hyphenated keywords already handled above answers `None` (the lexer
  keeps lexing them as one token — the constraint is on lowering).
- Stacked nullable: `?` does not recurse into another `?` (peek and
  answer `None`), keeping single `?T`.

Update the module rustdoc's subset note (`norm.rs:6,37,350`) to say
the parser now rejects outside the documented subset, tested.

- [ ] **Step 4: Run the totality gate before anything else**

Run: `cargo test -p celerrate_types every_embedded_refinement_text_lowers`
(the totality test lives in `celerrate_types`, calling into
`celerrate_stubs::embedded_stub_index()`)
Expected: PASS — no embedded refinement uses a tightened form. If it
FAILS: per the spec, either rewrite the stranded spelling in
`crates/celerrate_stubs/refinements.celerrate` to its documented,
type-equivalent form (for example bare `array` becomes
`array<int|string, mixed>`), or — if the documented subset genuinely
lacks the needed form — STOP and surface the conflict for review
instead of widening silently.

- [ ] **Step 5: Run the full types and stubs suites**

Run: `cargo test -p celerrate_types -p celerrate_stubs`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_types/src/norm.rs crates/celerrate_stubs
git commit -m "🐛 fix(types): the norm parser rejects outside its documented subset (#48)"
```

---

### Task 4: Verification, changelog, PR, and the issue reframing

**Files:** `CHANGELOG.md`.

- [ ] **Step 1: Full local gates**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

Expected: all clean.

- [ ] **Step 2: Corpus gates**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: the norm tightening contributes zero delta. The enum-case
repair (if Task 2 took the graceful branch) may move typed diagnostics:
hand-inspect every changed line under verify-then-accept; accept the
snapshot only if each change is the enum-case type where `mixed` stood,
and re-bless mixed-rate only for a verified precision gain
(`cargo xtask mixed-rate --bless`).

- [ ] **Step 3: Changelog and PR**

Unreleased entries: docblock enum-case references carry their enum type
(#62), and the norm parser rejects undocumented spellings (#48).

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the overlap oracle and norm tightening (#62, #48)"
git push -u origin fix-62-48-type-grammar-overlap-oracle
gh pr create --title "✅ test(types, phpdoc-bridge): overlap oracle, enum-case repair, norm tightening (#62, #48)" --body "Implements .claude/superpowers/specs/2026-07-19-type-grammar-overlap-oracle-design.md. Closes #62, closes #48."
```

- [ ] **Step 4: The reframing comment on #62**

Post (via `gh issue comment 62`) the spec's reframing before the PR
merges, so the issue's record is honest: what PR #65 already fixed
(the compositional `TypeContext` entry point), why the parsers stay
two (two languages by design; the norm is not a v0.1 public surface),
and what this PR adds (the oracle, the enum-case repair, the #48
tightening). The comment quotes the spec's "explicitly rejected"
section verbatim.
