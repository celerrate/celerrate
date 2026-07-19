# Directive-Vocabulary Seal and Refinement Dedup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Seal the comment-directive vocabulary (`#[non_exhaustive]` + constructor, issue #66) and add first-wins dedup to `StubRefinements::new` (issue #47).

**Architecture:** Two independent mechanical hardening fixes on one branch (`fix-66-47-vocabulary-seal-and-dedup`), per the design spec `.claude/superpowers/specs/2026-07-19-directive-vocabulary-and-refinement-dedup-design.md`. No behavioral change on any shipped path.

**Tech Stack:** Rust 1.94, salsa 0.27 (untouched), existing test harnesses.

## Global Constraints

- Zero panic: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` forbidden. Test modules may locally `#[allow]`.
- TDD: failing test (or failing compile for seal work) before implementation.
- Commits: gitmoji + Conventional Commits, repository-configured identity.
- Everything in English, full words.
- Final verification runs the corpus gates; zero delta expected for both fixes.

---

### Task 1: First-wins dedup in `StubRefinements::new` (#47)

**Files:**
- Modify: `crates/celerrate_stubs/src/refinements.rs:58-69` (the `new` constructor)
- Test: same file, existing `#[cfg(test)]` module (tests start near line 270)

**Interfaces:**
- Consumes: `StubRefinements::new(functions, classes)` as it exists.
- Produces: same signature; duplicate keys now collapse to the first entry after sort, matching `StubIndex::new` (`crates/celerrate_stubs/src/index.rs:45-46`: `dedup_by(|second, first| first.0 == second.0)`).

- [ ] **Step 1: Write the failing tests**

Add to the existing test module in `refinements.rs`, next to `construction_sorts_by_key`:

```rust
#[test]
fn duplicate_function_keys_collapse_to_the_first_entry() {
    let first = RefinedSignature {
        return_type: Some("int".to_owned()),
        ..RefinedSignature::default()
    };
    let second = RefinedSignature {
        return_type: Some("string".to_owned()),
        ..RefinedSignature::default()
    };
    let refinements = StubRefinements::new(
        vec![
            ("strlen".to_owned(), first.clone()),
            ("strlen".to_owned(), second),
        ],
        Vec::new(),
    );
    assert_eq!(refinements.functions, vec![("strlen".to_owned(), first)]);
}

#[test]
fn duplicate_class_keys_collapse_to_the_first_entry() {
    let first = RefinedClass {
        templates: vec![RefinedTemplate {
            name: "T".to_owned(),
            bound: None,
        }],
        ..RefinedClass::default()
    };
    let refinements = StubRefinements::new(
        Vec::new(),
        vec![
            ("iterator".to_owned(), first.clone()),
            ("iterator".to_owned(), RefinedClass::default()),
        ],
    );
    assert_eq!(refinements.classes, vec![("iterator".to_owned(), first)]);
}

#[test]
fn duplicate_method_names_collapse_to_the_first_entry_within_a_class() {
    let first = RefinedSignature {
        return_type: Some("static".to_owned()),
        ..RefinedSignature::default()
    };
    let class = RefinedClass {
        methods: vec![
            ("current".to_owned(), first.clone()),
            ("current".to_owned(), RefinedSignature::default()),
        ],
        ..RefinedClass::default()
    };
    let refinements = StubRefinements::new(Vec::new(), vec![("iterator".to_owned(), class)]);
    assert_eq!(
        refinements.classes,
        vec![(
            "iterator".to_owned(),
            RefinedClass {
                methods: vec![("current".to_owned(), first)],
                ..RefinedClass::default()
            }
        )],
    );
}
```

If `RefinedSignature`/`RefinedTemplate`/`RefinedClass` field names differ from the above, mirror the ones the existing tests in this file use — the assertion shape (first entry wins, second vanishes) is the requirement.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p celerrate_stubs duplicate`
Expected: the three new tests FAIL (the duplicate survives as an adjacent entry today).

- [ ] **Step 3: Implement the dedup**

In `StubRefinements::new`, mirror `StubIndex::new`'s pattern after each sort:

```rust
pub fn new(
    mut functions: Vec<(String, RefinedSignature)>,
    mut classes: Vec<(String, RefinedClass)>,
) -> Self {
    functions.sort_by(|left, right| left.0.cmp(&right.0));
    functions.dedup_by(|second, first| first.0 == second.0);
    classes.sort_by(|left, right| left.0.cmp(&right.0));
    classes.dedup_by(|second, first| first.0 == second.0);
    for (_, class) in &mut classes {
        class.methods.sort_by(|left, right| left.0.cmp(&right.0));
        class.methods.dedup_by(|second, first| first.0 == second.0);
    }
    Self { functions, classes }
}
```

Extend the rustdoc on the struct (lines 48-51) with one sentence: duplicate keys collapse to the first entry after the sort, matching `StubIndex::new`; the sole production producer already rejects duplicates, so this is defense in depth for programmatic callers.

- [ ] **Step 4: Run the crate suite**

Run: `cargo test -p celerrate_stubs`
Expected: all tests PASS, including the three new ones and the existing `construction_sorts_*` pair.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_stubs/src/refinements.rs
git commit -m "🐛 fix(stubs): duplicate refinement keys collapse to the first entry (#47)"
```

---

### Task 2: Seal the directive vocabulary in `celerrate_semantics` (#66)

**Files:**
- Modify: `crates/celerrate_semantics/src/comment_directives.rs:23-63` (the three type declarations)
- Test: same file, existing `#[cfg(test)]` module

**Interfaces:**
- Consumes: nothing new.
- Produces: `CommentDirective::suppress(scope: DirectiveScope, identifiers: Vec<String>) -> CommentDirective` — the constructor Task 3's bridge migration calls. The three types gain `#[non_exhaustive]`; `CommentDirective::Suppress` (the variant) gains it too.

- [ ] **Step 1: Write the constructor test**

Add to the test module in `comment_directives.rs`:

```rust
#[test]
fn the_suppress_constructor_is_field_faithful() {
    let directive = CommentDirective::suppress(
        DirectiveScope::NextLine,
        vec!["method.notFound".to_owned()],
    );
    assert_eq!(
        directive,
        CommentDirective::Suppress {
            scope: DirectiveScope::NextLine,
            identifiers: vec!["method.notFound".to_owned()],
        },
    );
}
```

(The literal on the right stays legal: `#[non_exhaustive]` does not restrict the declaring crate.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p celerrate_semantics the_suppress_constructor`
Expected: FAIL to compile — no method `suppress` on `CommentDirective`.

- [ ] **Step 3: Seal the three types and add the constructor**

In `comment_directives.rs`, add `#[non_exhaustive]` to `CommentKind` (line 24), `DirectiveScope` (line 35), `CommentDirective` (line 53), and to the `Suppress` variant itself; add the constructor mirroring `ParsedAssertion::new`'s shape (`crates/celerrate_types/src/type_syntax.rs:38-54`):

```rust
/// One structured directive a comment carries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CommentDirective {
    /// Extinguish every diagnostic family on the scope. The
    /// identifiers are the foreign diagnostic names the written form
    /// carried (`@phpstan-ignore method.notFound`), carried for the
    /// rule framework's identifier-level correspondence, never matched
    /// here (design section 5).
    #[non_exhaustive]
    Suppress {
        scope: DirectiveScope,
        identifiers: Vec<String>,
    },
}

impl CommentDirective {
    /// Constructor for cross-crate construction: literal construction
    /// is closed by `#[non_exhaustive]`.
    pub fn suppress(scope: DirectiveScope, identifiers: Vec<String>) -> Self {
        Self::Suppress { scope, identifiers }
    }
}
```

Add one rustdoc sentence to each of `CommentKind` and `DirectiveScope` noting the additive-extension promise (same wording as the sealed types of PR #65 carry, for example `ParsedTemplate`).

- [ ] **Step 4: Run the crate suite**

Run: `cargo test -p celerrate_semantics`
Expected: PASS (in-crate literals and matches stay legal).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_semantics/src/comment_directives.rs
git commit -m "✨ feat(semantics): seal the comment-directive vocabulary (#66)"
```

---

### Task 3: Migrate the bridge to the sealed vocabulary (#66)

**Files:**
- Modify: `crates/celerrate_phpdoc_bridge/src/directives.rs` (the `suppress` helper at line 86 and the `psalm_directive` match at lines 75-84; the test-module helper at lines 124-129)

**Interfaces:**
- Consumes: `CommentDirective::suppress(...)` from Task 2 (re-exported through `celerrate_plugin`).
- Produces: nothing new — the bridge compiles against the sealed vocabulary; behavior identical.

- [ ] **Step 1: Observe the compile failures**

Run: `cargo check -p celerrate_phpdoc_bridge --all-targets`
Expected: FAIL — cross-crate literal construction of `Suppress` in the `suppress` helpers (production and test), and a non-exhaustive `match` on `CommentKind` in `psalm_directive`.

- [ ] **Step 2: Migrate construction and add the wildcard arm**

The production helper (line 86) becomes:

```rust
fn suppress(scope: DirectiveScope, identifiers: Vec<String>) -> CommentDirective {
    CommentDirective::suppress(scope, identifiers)
}
```

`psalm_directive`'s match (lines 79-82) becomes:

```rust
let scope = match kind {
    CommentKind::Docblock => DirectiveScope::AnnotatedDeclaration,
    CommentKind::Line | CommentKind::Block => DirectiveScope::CurrentAndNextLine,
    // A comment kind this bridge does not know yet: the both-lines
    // superset, the same over-suppression-never-under-suppression
    // resolution the bare form uses (design section 5).
    _ => DirectiveScope::CurrentAndNextLine,
};
```

The test-module helper (lines 124-129) becomes:

```rust
fn suppress(scope: DirectiveScope, identifiers: &[&str]) -> CommentDirective {
    CommentDirective::suppress(
        scope,
        identifiers.iter().map(|s| (*s).to_owned()).collect(),
    )
}
```

Also update the semantics-side `FakeProvider` in
`crates/celerrate_semantics/src/comment_directives.rs` tests only if
Step 1 of Task 2 surfaced errors there — it should not (same crate).

- [ ] **Step 3: Run both crates' suites**

Run: `cargo test -p celerrate_phpdoc_bridge -p celerrate_semantics`
Expected: PASS, zero behavioral change (every existing directive test green).

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_phpdoc_bridge/src/directives.rs
git commit -m "♻️ refactor(phpdoc-bridge): construct directives through the sealed vocabulary (#66)"
```

---

### Task 4: Verification

**Files:** none (verification only).

- [ ] **Step 1: Full local gates**

Run, each expected clean:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

- [ ] **Step 2: Corpus gates**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: both match the committed snapshot/baseline exactly (zero delta — neither fix touches analysis semantics). Any delta is a bug in this branch; stop and investigate.

- [ ] **Step 3: Changelog**

Add under the Unreleased heading of `CHANGELOG.md`, matching its existing entry style: the sealed comment-directive vocabulary (#66) and the refinement dedup (#47).

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the vocabulary seal and refinement dedup (#66, #47)"
```

- [ ] **Step 4: Push and open the PR**

```bash
git push -u origin fix-66-47-vocabulary-seal-and-dedup
gh pr create --title "🔒 fix(semantics, stubs): seal the directive vocabulary and dedup refinements (#66, #47)" --body "Implements the design in .claude/superpowers/specs/2026-07-19-directive-vocabulary-and-refinement-dedup-design.md. Closes #66, closes #47."
```
