# Type Engine 3 — Declared Types Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve declared types into the lattice: native signature
resolution through per-member queries, the docblock-inheritance and
trust machinery (with the annotation layer as a seam plan 4 fills), the
stub signature payload as per-version deltas with the
intersection/union range rule, and the stub compiler extension that
produces it.

**Architecture:** All new resolution queries live in `celerrate_types`
(new modules `written.rs` and `declared.rs`), which already sits above
`celerrate_semantics` in the DAG. `celerrate_semantics` gains the
source-side prerequisites (free-function signatures, promoted
properties, the stub class graph joining linearization and member
lookup). `celerrate_stubs` gains the versioned signature model, the
`SECTION_SIGNATURES` blob section, and the compiler extension over
phpstorm-stubs. `celerrate_cli` records the format change by bumping
the cache pack schema.

**Tech Stack:** Rust, salsa 0.27 (`#[salsa::tracked]` free functions,
`#[salsa::interned]` keys, `dyn salsa::Database`), rowan-based typed
AST from `celerrate_syntax`, hand-written little-endian blob encoding.

**Design source:** `.claude/superpowers/specs/2026-07-14-type-engine-design.md`
sections 2 (default values, member `ItemTree`), 3 (lattice and declared
types — the core of this plan), 6 (unannotated parameters are `mixed`),
and 11 (plan "3 — Declared"). Parent spec section 2 (the `[min, max]`
range rule) and section 3 (stubs).

## Global Constraints

- **Zero panic, mechanically enforced**: Clippy denies `unwrap_used`,
  `expect_used`, `indexing_slicing`, `panic` workspace-wide;
  `unsafe_code` is forbidden. Production code returns `Result` or
  `Option`; test modules open with
  `#![allow(clippy::unwrap_used)]` (add `indexing_slicing`/`panic`
  allows only where a test needs them).
- **TDD**: failing test → minimal implementation → refactor. No
  production code without a test that demanded it.
- **Strict layering**: `celerrate_stubs` must not depend on
  `celerrate_semantics` or `celerrate_types`; `celerrate_semantics`
  must not depend on `celerrate_types`. `celerrate_types` already
  depends on `celerrate_semantics`, `celerrate_stubs`,
  `celerrate_project`, `celerrate_db`, `celerrate_source`.
- **Determinism**: no wall clock, no environment reads, no
  iteration-order-dependent results inside queries. Every new
  collection consulted for an answer is sorted or walked in a
  deterministically recorded order.
- **Error resilience**: no user input (source text, docblock text,
  stub text, blob bytes) may ever panic the tool. Malformed input
  degrades to `None`/`mixed`/skip, never to an error the user sees
  (docblock diagnostics are explicitly out of scope this sub-project).
- Everything in English, full words, no abbreviated names (standard
  acronyms fine).
- Commits: gitmoji + Conventional Commits
  (`✨ feat(types): …`, `🐛 fix(stubs): …`, `📝 docs(plans): …`), authored
  with the repository-configured identity — never override git
  identity, no Claude attribution anywhere.
- Local commands: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`, `cargo deny check`.
- Salsa pattern: free `#[salsa::tracked]` functions over
  `db: &dyn salsa::Database` plus the three inputs
  (`AnalyzedFileSet`, `StubIndexInput`, `ProjectConfiguration`);
  interned key structs with `#[salsa::interned(debug)]` and
  `#[returns(ref)]` on `String` fields. Follow the existing shapes in
  `crates/celerrate_semantics/src/lookup.rs` and
  `crates/celerrate_types/src/judgments.rs`.

## Design decisions fixed by this plan (with two recorded deviations)

1. **Blob version policy (deviation, recorded).** The design spec says
   "the blob takes a schema version bump". The implemented blob policy
   (`crates/celerrate_stubs/src/blob.rs:14-16`) says the format version
   is "bumped only on incompatible layout changes; additive evolution
   goes through new sections, which old readers skip" — and
   `SECTION_SIGNATURES = 2` was reserved for exactly this plan. This
   plan follows the implemented policy: `BLOB_FORMAT_VERSION` stays 1,
   the signatures land as the reserved additive section, and the
   **cache pack schema** (`CACHE_SCHEMA_VERSION`, the named reviewable
   record of deliberate format breaks, `celerrate_cli/src/cache/pack.rs`)
   bumps 2 → 3. The pack header's `stub_blob` blake3 hash already
   invalidates every cache built on the old blob. This satisfies "the
   header field pinned by the cache-audit tests evolves with it" at
   the pack level, where the pinning tests live.
2. **Source free-function signatures (scope addition, recorded).** The
   design's section 3 names the member `ItemTree`; free functions are
   not named. But the stub side of this very plan compiles *function*
   signatures (the stdlib is the headline consumer of the range rule),
   and plans 5/6/8 need the source-side symmetric surface
   (`declared_function_signature` must be total). The `MemberTree`
   projection therefore gains a `functions` list — signature-granular,
   exactly like class members, so the invalidation story is identical.
3. **Bare `callable` lowers to `mixed` (documented sound widening).**
   The lattice's `Callable` requires a parameter list, and no encoding
   of "top of all callables" judges correctly under the shipped
   contravariance rule. `mixed` is sound for all three families
   (silence, never a false positive). Recorded debt: a first-class
   bare-callable form, revisited when plan 8 measures the silence.
4. **Trust is a recorded fact.** Every declared element carries a
   `Trust` verdict (`NativeOnly`, `Refined`, `RefinedUnproven`,
   `RejectedAnnotation`) — the trace the design requires for
   cannot-prove refinements, and the hook the ground-truth harness
   (plan 6) reads.
5. **The annotation layer is a seam.** `member_annotations` exists and
   answers `MemberAnnotations::default()` until plan 4a's bridge fills
   it through the type-syntax registry. Precedence, the trust rule, and
   the inheritance walk are built and unit-tested NOW against injected
   readers, so plan 4 swaps one query body and changes nothing else.
6. **Parameter "intersection across the range" is implemented as
   most-restrictive-or-silenced.** For the finite set of per-version
   parameter types: all equal → that type; one is a proven subtype of
   all others → that one (the most restrictive form, per the parent
   spec's own wording); otherwise the parameter is **silenced**
   (`parameter_type: None`) — which implements the design's degenerate
   empty-intersection guard without ever fabricating an uninhabited
   lattice intersection. Returns are the plain union.
7. **Stub parent names resolve by qualification only.** Inside stub
   files, an ancestor name is absolutized by the declaring namespace
   (leading `\` respected); `use` imports inside stub files are not
   consulted. phpstorm-stubs overwhelmingly declares in the global
   namespace with fully qualified references; recorded debt if the
   corpus proves otherwise.

## Out of scope (deliberately)

- Docblock/annotation *parsing* — plan 4 (the bridge). Here the
  annotation layer is the seam of decision 5.
- Template types from annotations, `class-string<T>` binding — the
  native grammar cannot write them; they arrive with plan 4.
- Verifying a body honors its declared return — future work.
- The refinements overlay and `SECTION_OVERLAYS` — plan 7.
- Function-level annotation inheritance — functions do not inherit.
- Call-site substitution of `self`/`static`/`parent` placeholders —
  plan 6. Native lowering produces the placeholders, nothing more.
- Stub member flags beyond visibility and staticness
  (abstract/final/readonly) — no consumer in this sub-project.
- `MagicMarkers` for `stdClass` dynamic properties — plan 8 (reads
  `stub_ancestors`, which this plan makes transitive).

## Task sizing note

Tasks below follow the required step pattern (failing test → verify
fail → implement → verify pass → commit). Where a task lists several
tests, write them all in the "failing test" step and watch them all
fail together. Run `cargo fmt --all` before every commit;
run `cargo clippy --workspace --all-targets -- -D warnings` at least
before each commit's final test run.

---

### Task 1: The written-type parser (`celerrate_types/src/written.rs`)

The member `ItemTree` stores every declared type as written text,
tokens joined with no separator (`"Foo\\Bar|null"`, `"?Logger"`,
`"(A&B)|C"` — produced by `celerrate_syntax::ast::type_text`). This
task parses that text into a small structural form. Pure: no database,
no resolution — names stay strings.

**Files:**
- Create: `crates/celerrate_types/src/written.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (add `mod written;` —
  no public re-export; the module is `pub(crate)`)

**Interfaces:**
- Consumes: nothing (pure text in).
- Produces: `pub(crate) enum WrittenType { Name(String),
  Nullable(Box<WrittenType>), Union(Vec<WrittenType>),
  Intersection(Vec<WrittenType>) }` and
  `pub(crate) fn parse_written(text: &str) -> Option<WrittenType>`.
  Task 2 lowers `WrittenType` to `TypeId`; task 11 reuses the parser
  for stub type texts.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_types/src/written.rs` containing only the
test module for now:

```rust
//! Parsing the written form of a native PHP type: the token-joined
//! text the member `ItemTree` carries (`Foo\Bar|null`, `?Logger`,
//! `(A&B)|C`). Grammar only: names stay unresolved strings, keywords
//! are ordinary names until lowering. Tolerant: malformed text is
//! `None`, never a panic.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WrittenType {
    /// A (possibly qualified) name or keyword, exactly as written.
    Name(String),
    Nullable(Box<WrittenType>),
    Union(Vec<WrittenType>),
    Intersection(Vec<WrittenType>),
}

pub(crate) fn parse_written(_text: &str) -> Option<WrittenType> {
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{WrittenType, parse_written};

    fn name(text: &str) -> WrittenType {
        WrittenType::Name(text.to_owned())
    }

    #[test]
    fn a_plain_name_parses() {
        assert_eq!(parse_written("int"), Some(name("int")));
        assert_eq!(parse_written("Foo\\Bar"), Some(name("Foo\\Bar")));
        assert_eq!(parse_written("\\DateTime"), Some(name("\\DateTime")));
    }

    #[test]
    fn nullable_unions_and_intersections_parse() {
        assert_eq!(
            parse_written("?Logger"),
            Some(WrittenType::Nullable(Box::new(name("Logger")))),
        );
        assert_eq!(
            parse_written("Foo\\Bar|null"),
            Some(WrittenType::Union(vec![name("Foo\\Bar"), name("null")])),
        );
        assert_eq!(
            parse_written("Countable&Iterator"),
            Some(WrittenType::Intersection(vec![
                name("Countable"),
                name("Iterator"),
            ])),
        );
    }

    #[test]
    fn disjunctive_normal_form_parses_with_parentheses() {
        assert_eq!(
            parse_written("(A&B)|C"),
            Some(WrittenType::Union(vec![
                WrittenType::Intersection(vec![name("A"), name("B")]),
                name("C"),
            ])),
        );
    }

    #[test]
    fn unions_flatten_across_their_own_nesting() {
        // `A|B|C` is one three-part union, not a nested pair.
        assert_eq!(
            parse_written("A|B|C"),
            Some(WrittenType::Union(vec![name("A"), name("B"), name("C")])),
        );
    }

    #[test]
    fn malformed_text_is_none_never_a_panic() {
        for garbage in [
            "", "|", "?", "(", ")", "A|", "|A", "A&", "?(", "((A)", "A B",
            "A||B", "1nt", "\\", "A\\", "?|A", "A(B)",
        ] {
            assert_eq!(parse_written(garbage), None, "input {garbage:?}");
        }
    }

    #[test]
    fn every_ascii_soup_is_parsed_or_rejected_without_panicking() {
        // A cheap fuzz floor: three-byte soups over a hostile alphabet.
        let alphabet = b"A?|&()\\1_ ";
        for a in alphabet {
            for b in alphabet {
                for c in alphabet {
                    let text: String =
                        [*a, *b, *c].iter().map(|&byte| byte as char).collect();
                    let _ = parse_written(&text);
                }
            }
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types written -- --nocapture`
Expected: FAIL — every positive test asserts `Some(..)` against the
stub `None`.

- [ ] **Step 3: Implement the parser**

Replace the `parse_written` stub with a lexer plus a recursive-descent
parser. Complete implementation:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Name(String),
    Question,
    Pipe,
    Ampersand,
    OpenParenthesis,
    CloseParenthesis,
}

/// Lexes the joined text. `None` on any byte that cannot start or
/// continue a token (whitespace included: the joined form never
/// contains any).
fn lex(text: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut characters = text.chars().peekable();
    while let Some(&character) = characters.peek() {
        match character {
            '?' => {
                characters.next();
                tokens.push(Token::Question);
            }
            '|' => {
                characters.next();
                tokens.push(Token::Pipe);
            }
            '&' => {
                characters.next();
                tokens.push(Token::Ampersand);
            }
            '(' => {
                characters.next();
                tokens.push(Token::OpenParenthesis);
            }
            ')' => {
                characters.next();
                tokens.push(Token::CloseParenthesis);
            }
            _ => tokens.push(Token::Name(lex_name(&mut characters)?)),
        }
    }
    Some(tokens)
}

/// One (possibly qualified, possibly `\`-prefixed) name. PHP labels
/// start with a letter, underscore, or a byte ≥ 0x80; digits may only
/// continue a label. A trailing `\` or an empty segment is malformed.
fn lex_name(
    characters: &mut core::iter::Peekable<core::str::Chars<'_>>,
) -> Option<String> {
    let mut name = String::new();
    if characters.peek() == Some(&'\\') {
        characters.next();
        name.push('\\');
    }
    loop {
        let mut segment = String::new();
        while let Some(&character) = characters.peek() {
            let continues = character.is_ascii_alphanumeric()
                || character == '_'
                || !character.is_ascii();
            if !continues {
                break;
            }
            if segment.is_empty() && character.is_ascii_digit() {
                return None;
            }
            segment.push(character);
            characters.next();
        }
        if segment.is_empty() {
            return None;
        }
        name.push_str(&segment);
        if characters.peek() == Some(&'\\') {
            characters.next();
            name.push('\\');
        } else {
            return Some(name);
        }
    }
}

pub(crate) fn parse_written(text: &str) -> Option<WrittenType> {
    let tokens = lex(text)?;
    let mut cursor = 0usize;
    let parsed = parse_union(&tokens, &mut cursor)?;
    (cursor == tokens.len()).then_some(parsed)
}

/// union := intersection (`|` intersection)*
fn parse_union(tokens: &[Token], cursor: &mut usize) -> Option<WrittenType> {
    let mut parts = vec![parse_intersection(tokens, cursor)?];
    while tokens.get(*cursor) == Some(&Token::Pipe) {
        *cursor += 1;
        parts.push(parse_intersection(tokens, cursor)?);
    }
    Some(if parts.len() == 1 {
        parts.remove(0)
    } else {
        WrittenType::Union(parts)
    })
}

/// intersection := atom (`&` atom)*
fn parse_intersection(tokens: &[Token], cursor: &mut usize) -> Option<WrittenType> {
    let mut parts = vec![parse_atom(tokens, cursor)?];
    while tokens.get(*cursor) == Some(&Token::Ampersand) {
        *cursor += 1;
        parts.push(parse_atom(tokens, cursor)?);
    }
    Some(if parts.len() == 1 {
        parts.remove(0)
    } else {
        WrittenType::Intersection(parts)
    })
}

/// atom := `?` atom | `(` union `)` | name
fn parse_atom(tokens: &[Token], cursor: &mut usize) -> Option<WrittenType> {
    match tokens.get(*cursor)? {
        Token::Question => {
            *cursor += 1;
            Some(WrittenType::Nullable(Box::new(parse_atom(tokens, cursor)?)))
        }
        Token::OpenParenthesis => {
            *cursor += 1;
            let inner = parse_union(tokens, cursor)?;
            if tokens.get(*cursor) != Some(&Token::CloseParenthesis) {
                return None;
            }
            *cursor += 1;
            Some(inner)
        }
        Token::Name(name) => {
            let name = name.clone();
            *cursor += 1;
            Some(WrittenType::Name(name))
        }
        Token::Pipe | Token::Ampersand | Token::CloseParenthesis => None,
    }
}
```

Note `parts.remove(0)`: `remove` on index 0 of a non-empty vector is
not `indexing_slicing`; if clippy objects anyway, use
`parts.pop()` on the single-element case
(`if parts.len() == 1 { parts.pop() } else { … }` returning
`Option`). Recursion depth in `parse_atom` is bounded by the input
length (each recursion consumes a token), and member type texts are
short; no explicit depth guard is needed.

Add `mod written;` to `crates/celerrate_types/src/lib.rs` next to the
other module declarations.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types written`
Expected: PASS (all 6 tests).
Then: `cargo clippy --package celerrate_types --all-targets -- -D warnings`
and `cargo fmt --all`.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/written.rs crates/celerrate_types/src/lib.rs
git commit -m "✨ feat(types): parse the written form of native type text"
```

---

### Task 2: The native lowering table (`celerrate_types/src/declared.rs`, part 1)

Lower a `WrittenType` to a `TypeId`: keywords through a fixed table,
class names qualified at their declaring site (namespace + `use`
tables) or in the global context (stub texts). This is the design's
"total lowering table" for the native grammar.

**Files:**
- Create: `crates/celerrate_types/src/declared.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (add `mod declared;`
  and `pub use declared::…` as listed below)

**Interfaces:**
- Consumes: `crate::written::{WrittenType, parse_written}` (Task 1);
  `celerrate_semantics::{UseTables, resolve_candidates, SymbolSpace}`;
  `TypeId` constructors from `crate::construction`
  (`mixed`, `never`, `void`, `null`, `object`, `resource`, `bool`,
  `bool_literal`, `int`, `float`, `string`, `array`, `iterable`,
  `union`, `intersection`, `class`, `static_placeholder`,
  `self_placeholder`, `parent_placeholder`).
- Produces (crate-internal, consumed by tasks 4 and 11):
  - `pub(crate) enum NameSite<'a> { Source { namespace: &'a str,
    tables: &'a UseTables }, Global }`
  - `pub(crate) fn lower_written_text<'db>(db: &'db dyn
    salsa::Database, site: &NameSite<'_>, text: &str) ->
    Option<TypeId<'db>>` — parse + lower; `None` on malformed text
    (callers fall back to `mixed`).
  - `pub(crate) fn lower_written<'db>(db: &'db dyn salsa::Database,
    site: &NameSite<'_>, written: &WrittenType) -> TypeId<'db>`

**The lowering table** (keywords are matched case-insensitively, only
when the name is unqualified and has no leading backslash — a
qualified `Foo\int` is a class name):

| written | lattice |
|---|---|
| `int` | `TypeId::int` |
| `float` | `TypeId::float` |
| `string` | `TypeId::string` |
| `bool` | `TypeId::bool` |
| `true` / `false` | `TypeId::bool_literal` |
| `null` | `TypeId::null` |
| `mixed` | `TypeId::mixed` |
| `never` | `TypeId::never` |
| `void` | `TypeId::void` |
| `object` | `TypeId::object` |
| `resource` | `TypeId::resource` (stub texts carry it) |
| `array` | `TypeId::array(int\|string, mixed)` |
| `iterable` | `TypeId::iterable(mixed, mixed)` |
| `callable` | `TypeId::mixed` — decision 3, documented sound widening |
| `self` | `TypeId::self_placeholder` |
| `static` | `TypeId::static_placeholder` |
| `parent` | `TypeId::parent_placeholder` |
| anything else | `TypeId::class(qualified name, vec![])` |

`?T` lowers to `union(T, null)`; unions and intersections lower
member-wise through `TypeId::union` / `TypeId::intersection`
(canonicalization is theirs).

**Name qualification**: `NameSite::Source` takes the **first**
candidate of `resolve_candidates(written, SymbolSpace::ClassLike,
namespace, tables)` — PHP class-name resolution is static (imports and
namespace prefixing decide the fully qualified name; existence does
not participate), so the first candidate is the answer whether or not
the class exists; an unresolvable name still lowers to a class type
and the judgment layer answers `CannotProve` on it. If the candidate
list is empty (defensive; it never is for a lexable name), fall back
to the written text with the leading backslash trimmed.
`NameSite::Global` trims the leading backslash and uses the text
verbatim. `TypeId::class` folds internally — never pre-fold here.

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_types/src/declared.rs`:

```rust
//! Declared types: lowering written native type text into the lattice
//! at a declaring site. The keyword table is total over the native
//! grammar; unknown names lower to class types (the judgment layer
//! answers `CannotProve` for unresolvable classes). Bare `callable`
//! lowers to `mixed`: a documented sound widening (no top-of-callables
//! form exists in the lattice; recorded debt, revisited by plan 8).

use celerrate_semantics::{SymbolSpace, UseTables, resolve_candidates};

use crate::representation::TypeId;
use crate::written::{WrittenType, parse_written};

/// Where a written name qualifies: a source declaring site (namespace
/// plus `use` tables) or the global context (stub type texts).
pub(crate) enum NameSite<'a> {
    Source {
        namespace: &'a str,
        tables: &'a UseTables,
    },
    Global,
}

pub(crate) fn lower_written_text<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    text: &str,
) -> Option<TypeId<'db>> {
    Some(lower_written(db, site, &parse_written(text)?))
}

pub(crate) fn lower_written<'db>(
    _db: &'db dyn salsa::Database,
    _site: &NameSite<'_>,
    _written: &WrittenType,
) -> TypeId<'db> {
    unimplemented_lowering()
}

fn unimplemented_lowering<'db>() -> TypeId<'db> {
    // Replaced in this task's implementation step; the tests fail on
    // this stub through a compile error once the real body lands.
    panic!()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use super::{NameSite, lower_written_text};
    use crate::representation::TypeId;

    fn lower<'db>(db: &'db TestDatabase, text: &str) -> Option<TypeId<'db>> {
        lower_written_text(db, &NameSite::Global, text)
    }

    #[test]
    fn the_keyword_table_is_total_over_the_native_grammar() {
        let db = TestDatabase::default();
        let cases: &[(&str, TypeId<'_>)] = &[
            ("int", TypeId::int(&db)),
            ("INT", TypeId::int(&db)),
            ("float", TypeId::float(&db)),
            ("string", TypeId::string(&db)),
            ("bool", TypeId::bool(&db)),
            ("true", TypeId::bool_literal(&db, true)),
            ("false", TypeId::bool_literal(&db, false)),
            ("null", TypeId::null(&db)),
            ("mixed", TypeId::mixed(&db)),
            ("never", TypeId::never(&db)),
            ("void", TypeId::void(&db)),
            ("object", TypeId::object(&db)),
            ("resource", TypeId::resource(&db)),
            ("self", TypeId::self_placeholder(&db)),
            ("static", TypeId::static_placeholder(&db)),
            ("parent", TypeId::parent_placeholder(&db)),
        ];
        for (text, expected) in cases {
            assert_eq!(lower(&db, text), Some(*expected), "keyword {text}");
        }
    }

    #[test]
    fn array_iterable_and_callable_lower_to_their_documented_forms() {
        let db = TestDatabase::default();
        let array_key = TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]);
        assert_eq!(
            lower(&db, "array"),
            Some(TypeId::array(&db, array_key, TypeId::mixed(&db))),
        );
        assert_eq!(
            lower(&db, "iterable"),
            Some(TypeId::iterable(&db, TypeId::mixed(&db), TypeId::mixed(&db))),
        );
        // Decision 3: bare `callable` is a documented sound widening.
        assert_eq!(lower(&db, "callable"), Some(TypeId::mixed(&db)));
    }

    #[test]
    fn nullable_union_and_intersection_lower_through_the_lattice() {
        let db = TestDatabase::default();
        assert_eq!(
            lower(&db, "?int"),
            Some(TypeId::union(&db, [TypeId::int(&db), TypeId::null(&db)])),
        );
        assert_eq!(
            lower(&db, "int|string"),
            Some(TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)])),
        );
        let countable = TypeId::class(&db, "Countable", vec![]);
        let iterator = TypeId::class(&db, "Iterator", vec![]);
        assert_eq!(
            lower(&db, "Countable&Iterator"),
            Some(TypeId::intersection(&db, [countable, iterator])),
        );
    }

    #[test]
    fn global_names_lower_to_class_types_with_the_backslash_trimmed() {
        let db = TestDatabase::default();
        assert_eq!(
            lower(&db, "\\DateTime"),
            Some(TypeId::class(&db, "DateTime", vec![])),
        );
        // A qualified name is never a keyword.
        assert_eq!(
            lower(&db, "Foo\\int"),
            Some(TypeId::class(&db, "Foo\\int", vec![])),
        );
    }

    #[test]
    fn source_site_names_qualify_through_namespace_and_imports() {
        use celerrate_db::{AnalyzedFileSet, SourceFile};
        use celerrate_semantics::{UseTables, item_tree};
        use celerrate_source::FileId;

        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php namespace App; use Psr\\Log\\LoggerInterface as Logger; class C {}"
                .to_vec(),
        );
        let _files = AnalyzedFileSet::new(&db, vec![file]);
        let tree = item_tree(&db, file);
        let tables = UseTables::for_namespace(tree, "App");
        let site = super::NameSite::Source {
            namespace: "App",
            tables: &tables,
        };
        // The import expands.
        assert_eq!(
            lower_written_text(&db, &site, "Logger"),
            Some(TypeId::class(&db, "Psr\\Log\\LoggerInterface", vec![])),
        );
        // An unimported name qualifies into the namespace, existing or not.
        assert_eq!(
            lower_written_text(&db, &site, "Repository"),
            Some(TypeId::class(&db, "App\\Repository", vec![])),
        );
        // Absolute names ignore the namespace.
        assert_eq!(
            lower_written_text(&db, &site, "\\Throwable"),
            Some(TypeId::class(&db, "Throwable", vec![])),
        );
    }

    #[test]
    fn malformed_text_lowers_to_none() {
        let db = TestDatabase::default();
        assert_eq!(lower(&db, ""), None);
        assert_eq!(lower(&db, "A|"), None);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types declared`
Expected: FAIL (the `panic!()` stub is itself denied by clippy — the
compile/lint failure at this stage is the red state; if you prefer a
compiling red, return `TypeId::never(db)` from the stub instead and
watch the assertions fail).

- [ ] **Step 3: Implement the lowering**

Replace `lower_written` and delete `unimplemented_lowering`:

```rust
pub(crate) fn lower_written<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    written: &WrittenType,
) -> TypeId<'db> {
    match written {
        WrittenType::Nullable(inner) => TypeId::union(
            db,
            [lower_written(db, site, inner), TypeId::null(db)],
        ),
        WrittenType::Union(parts) => TypeId::union(
            db,
            parts.iter().map(|part| lower_written(db, site, part)),
        ),
        WrittenType::Intersection(parts) => TypeId::intersection(
            db,
            parts.iter().map(|part| lower_written(db, site, part)),
        ),
        WrittenType::Name(name) => lower_name(db, site, name),
    }
}

fn lower_name<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    name: &str,
) -> TypeId<'db> {
    if !name.contains('\\') {
        if let Some(keyword) = lower_keyword(db, name) {
            return keyword;
        }
    }
    TypeId::class(db, &qualified_class_name(site, name), vec![])
}

/// The keyword table: total over the native grammar (decision 3 for
/// `callable`). `None` means "an ordinary class name".
fn lower_keyword<'db>(db: &'db dyn salsa::Database, name: &str) -> Option<TypeId<'db>> {
    let folded = name.to_ascii_lowercase();
    Some(match folded.as_str() {
        "int" => TypeId::int(db),
        "float" => TypeId::float(db),
        "string" => TypeId::string(db),
        "bool" => TypeId::bool(db),
        "true" => TypeId::bool_literal(db, true),
        "false" => TypeId::bool_literal(db, false),
        "null" => TypeId::null(db),
        "mixed" => TypeId::mixed(db),
        "never" => TypeId::never(db),
        "void" => TypeId::void(db),
        "object" => TypeId::object(db),
        "resource" => TypeId::resource(db),
        "array" => TypeId::array(
            db,
            TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
            TypeId::mixed(db),
        ),
        "iterable" => TypeId::iterable(db, TypeId::mixed(db), TypeId::mixed(db)),
        // Decision 3: no top-of-callables form exists; `mixed` is the
        // documented sound widening (recorded debt for plan 8).
        "callable" => TypeId::mixed(db),
        "self" => TypeId::self_placeholder(db),
        "static" => TypeId::static_placeholder(db),
        "parent" => TypeId::parent_placeholder(db),
        _ => return None,
    })
}

/// PHP class-name resolution is static: the first candidate is the
/// fully qualified name whether or not the class exists.
fn qualified_class_name(site: &NameSite<'_>, written: &str) -> String {
    match site {
        NameSite::Source { namespace, tables } => {
            resolve_candidates(written, SymbolSpace::ClassLike, namespace, tables)
                .into_iter()
                .next()
                .unwrap_or_else(|| written.trim_start_matches('\\').to_owned())
        }
        NameSite::Global => written.trim_start_matches('\\').to_owned(),
    }
}
```

Note: a leading-backslash name never contains a keyword — the
`!name.contains('\\')` guard covers `\int` too because the lexer keeps
the backslash in the name. Add `mod declared;` to `lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types`
Expected: PASS (new module plus the whole existing suite).
Then clippy + fmt.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/src/declared.rs crates/celerrate_types/src/lib.rs
git commit -m "✨ feat(types): lower written native types at their declaring site"
```

---
### Task 3: Free functions and promoted properties in the member projection (`celerrate_semantics`)

Two source-side prerequisites. (a) The `MemberTree` gains the file's
free-function signatures (decision 2), plus a `lookup_function_declaration`
firewall. (b) Promoted constructor parameters surface as `Property`
members of their class — today they are only flagged on the parameter
(`is_promoted`), so linearization cannot answer `$object->promoted`
and declared property types would silently miss them.

**Files:**
- Modify: `crates/celerrate_semantics/src/members.rs`
- Modify: `crates/celerrate_semantics/src/lookup.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (re-export
  `FreeFunction`, `lookup_function_declaration`)

**Interfaces:**
- Consumes: `ast::FunctionDeclaration` (`name_token()`,
  `parameter_list()`, `return_type()`, `by_reference_token()`,
  `block()`), the existing `parameter_signatures` and
  `ast::{type_text, docblock_token}` helpers, `item_nodes`,
  `source_symbol_table`.
- Produces:
  - `pub struct FreeFunction { pub name: String, pub namespace: String,
    pub signature: MemberSignature, pub docblock: Option<String>,
    pub ast_id: AstId }` (derives `Debug, Clone, PartialEq, Eq`)
  - `MemberTree` gains `pub functions: Vec<FreeFunction>`
  - `#[salsa::tracked] pub fn lookup_function_declaration<'db>(db:
    &'db dyn salsa::Database, files: AnalyzedFileSet, query:
    SymbolQuery<'db>) -> Option<AstId>` — source functions only
    (`DeclarationKind::Function` with an `Item` origin).
  - Promoted parameters appear as `Member { kind: MemberKind::Property,
    … }` in their class's `ClassMembers.members`, sharing the
    constructor's `ast_id`, with `signature.type_text` = the
    parameter's type text, `signature.default_text` = the parameter's
    default text, visibility/readonly from the parameter's modifiers.

- [ ] **Step 1: Write the failing tests**

In `crates/celerrate_semantics/src/members.rs`'s test module, add:

```rust
#[test]
fn free_functions_project_their_signatures() {
    let tree = member_tree_of(
        "<?php namespace App;\n\
         /** doc */\n\
         function build(int $count, string ...$names): ?Widget { return null; }",
    );
    assert_eq!(tree.functions.len(), 1);
    let function = &tree.functions[0];
    assert_eq!(function.name, "build");
    assert_eq!(function.namespace, "App");
    assert_eq!(function.docblock.as_deref(), Some("/** doc */"));
    assert_eq!(function.signature.type_text.as_deref(), Some("?Widget"));
    assert_eq!(function.signature.parameters.len(), 2);
    assert_eq!(function.signature.parameters[0].name, "count");
    assert_eq!(
        function.signature.parameters[0].type_text.as_deref(),
        Some("int"),
    );
    assert!(function.signature.parameters[1].variadic);
}

#[test]
fn a_function_body_edit_leaves_the_member_tree_identical() {
    let before = member_tree_of("<?php function f(int $x): int { return $x; }");
    let after = member_tree_of("<?php function f(int $x): int { return $x + 1; }");
    assert_eq!(before, after);
}

#[test]
fn promoted_constructor_parameters_surface_as_properties() {
    let tree = member_tree_of(
        "<?php class Service {\n\
             public function __construct(\n\
                 private readonly ?Logger $logger = null,\n\
                 int $plain = 0,\n\
             ) {}\n\
         }",
    );
    let class = &tree.classes[0];
    let properties: Vec<&Member> = class
        .members
        .iter()
        .filter(|member| member.kind == MemberKind::Property)
        .collect();
    assert_eq!(properties.len(), 1, "only the promoted parameter");
    let promoted = properties[0];
    assert_eq!(promoted.name, "logger");
    assert_eq!(promoted.signature.type_text.as_deref(), Some("?Logger"));
    assert_eq!(promoted.signature.default_text.as_deref(), Some("null"));
    assert_eq!(promoted.flags.visibility, Visibility::Private);
    assert!(promoted.flags.is_readonly);
}
```

(`member_tree_of` is whatever local helper the existing tests use to
build a `MemberTree` from source — reuse it; if none exists inline,
build a `TestDatabase` + `SourceFile` + `crate::queries::member_tree`
call as the module's other tests do.)

In `crates/celerrate_semantics/src/lookup.rs`'s test module, add:

```rust
use crate::lookup::lookup_function_declaration;

#[test]
fn a_source_function_answers_its_declaring_identity() {
    let fixture = fixture(&["<?php namespace App; function build(): int {}"]);
    let space = SymbolSpace::Function;
    let query = SymbolQuery::new(
        &fixture.db,
        space,
        folded_symbol_key(space, "App\\build"),
    );
    let ast_id = lookup_function_declaration(&fixture.db, fixture.files, query);
    assert!(ast_id.is_some());
    // A stub function has no source declaration.
    let stub_query = SymbolQuery::new(
        &fixture.db,
        space,
        folded_symbol_key(space, "strlen"),
    );
    assert!(
        lookup_function_declaration(&fixture.db, fixture.files, stub_query)
            .is_none()
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_semantics members:: lookup::`
Expected: FAIL — `MemberTree` has no `functions` field (compile
error), `lookup_function_declaration` does not exist.

- [ ] **Step 3: Implement**

In `members.rs`:

1. Add the struct and the field:

```rust
/// One free function of the file: signature-granular, exactly like a
/// class member, so a body edit backdates and a signature edit
/// invalidates precisely the signature's dependents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeFunction {
    pub name: String,
    pub namespace: String,
    pub signature: MemberSignature,
    pub docblock: Option<String>,
    pub ast_id: AstId,
}
```

and on `MemberTree`: `pub functions: Vec<FreeFunction>,` (keep
`#[derive(… Default)]` working).

2. In the tree-building walk (the function that iterates `item_nodes`
and currently builds `classes` from class-like nodes), handle
`SyntaxKind::FunctionDeclaration` nodes that have no owner (top-level):

```rust
SyntaxKind::FunctionDeclaration => {
    let Some(function) = ast::FunctionDeclaration::cast(node.clone()) else {
        continue; // or `return` to match the surrounding shape
    };
    let Some(name) = function.name_token() else {
        continue;
    };
    tree.functions.push(FreeFunction {
        name: name.text().to_owned(),
        namespace: item.namespace.clone(),
        signature: MemberSignature {
            parameters: parameter_signatures(function.parameter_list()),
            type_text: function.return_type().map(|ty| ast::type_text(&ty)),
            default_text: None,
            by_reference: function.by_reference_token().is_some(),
        },
        docblock: ast::docblock_token(&node).map(|token| token.text().to_owned()),
        ast_id,
    });
}
```

Adapt the exact control flow (`continue` versus early return, how
`item.namespace` and `ast_id` are named) to the surrounding walk —
mirror how class-like nodes obtain theirs. Only **top-level** function
declarations project (`owner` is `None` in the `ItemNode`); function
declarations nested in bodies are body-IR territory and stay out.

3. In `lower_member`'s `SyntaxKind::MethodDeclaration` arm, after
pushing the method `Member`, surface promoted parameters as
properties. A parameter is promoted exactly when it carries modifiers
(the existing `is_promoted` rule):

```rust
for parameter in method
    .parameter_list()
    .into_iter()
    .flat_map(|list| list.parameters())
{
    let mut flags = flags_of(parameter.modifiers());
    if !parameter_is_promoted(&parameter) {
        continue;
    }
    flags.is_static = false;
    let Some(name) = parameter.name_token() else {
        continue;
    };
    group.members.push(Member {
        kind: MemberKind::Property,
        name: name.text().trim_start_matches('$').to_owned(),
        flags,
        signature: MemberSignature {
            type_text: parameter.ty().map(|ty| ast::type_text(&ty)),
            default_text: parameter
                .default_value()
                .map(|expression| ast::expression_text(&expression)),
            ..MemberSignature::default()
        },
        docblock: None,
        ast_id,
    });
}
```

with the tiny helper (mirrors `parameter_signatures`):

```rust
fn parameter_is_promoted(parameter: &ast::Parameter) -> bool {
    parameter.modifiers().next().is_some()
}
```

Guard it to constructors only (PHP allows promotion only there):
wrap the loop in
`if name.text().eq_ignore_ascii_case("__construct") { … }` using the
method's name token already in scope.

4. In `lookup.rs`, add:

```rust
/// The declaring identity of one source function: `None` for stubs,
/// non-functions, and unknown names.
#[salsa::tracked]
pub fn lookup_function_declaration<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    query: SymbolQuery<'db>,
) -> Option<AstId> {
    let entry = source_symbol_table(db, files).lookup(query.space(db), query.key(db))?;
    if entry.kind != DeclarationKind::Function {
        return None;
    }
    match entry.origin {
        SymbolOrigin::Item(ast_id) => Some(ast_id),
        SymbolOrigin::Define(_) => None,
    }
}
```

5. Re-export in `lib.rs`: add `FreeFunction` to the `members` line and
`lookup_function_declaration` to the `lookup` line.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_semantics`
Expected: PASS — including every pre-existing test (adding a field to
`MemberTree` may require updating struct literals in tests; update
them, never weaken them). Then
`cargo test --workspace` (consumers of `MemberTree` compile against
the new field), clippy, fmt.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_semantics
git commit -m "✨ feat(semantics): free-function signatures and promoted properties in the member projection"
```

---

### Task 4: The native declared-signature queries (`celerrate_types/src/declared.rs`, part 2)

The per-member query of the design's section 3: resolve one member's
written signature into lattice types at its declaring site. Also the
free-function twin, and the literal reader for constant defaults and
implicit `= null` nullability. Annotations do not participate yet
(every element is `Trust::NativeOnly` — tasks 5 and 6 wire the seam).

**Files:**
- Modify: `crates/celerrate_types/src/declared.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (re-exports)

**Interfaces:**
- Consumes: `celerrate_semantics::{MemberQuery, lookup_member,
  MemberResolution, MemberKind, lookup_class_declaration,
  lookup_function_declaration, analyzed_file_index, member_tree,
  item_tree, UseTables, SymbolQuery, SymbolSpace, folded_symbol_key}`;
  Task 2's `NameSite`/`lower_written_text`.
- Produces (all `pub`, re-exported from `lib.rs`):

```rust
/// How a declared element's final type was obtained — the trace the
/// design requires for annotation refinement (tasks 5-6 set the
/// non-native variants; the ground-truth harness of plan 6 reads it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum Trust {
    /// No annotation: the native declaration (or `mixed`) stands.
    NativeOnly,
    /// The annotation refines the native declaration (subtype: Holds).
    Refined,
    /// The annotation refines through an unproven judgment
    /// (CannotProve — template types, principally): trusted, traced.
    RefinedUnproven,
    /// The annotation contradicts the native declaration (Fails):
    /// ignored, the native declaration wins.
    RejectedAnnotation,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct DeclaredParameter<'db> {
    pub name: String,
    /// `None` silences every check on this parameter (the stub range
    /// rule's degenerate case, decision 6). An untyped parameter is
    /// `Some(mixed)`, never `None`.
    pub parameter_type: Option<TypeId<'db>>,
    pub trust: Trust,
    pub optional: bool,
    pub variadic: bool,
    pub by_reference: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct DeclaredSignature<'db> {
    /// Methods and functions; empty for properties, constants, cases.
    pub parameters: Vec<DeclaredParameter<'db>>,
    /// The return type (methods, functions), the property type, the
    /// constant type, or the enum-case type.
    pub value_type: TypeId<'db>,
    pub value_trust: Trust,
    pub by_reference: bool,
}

#[salsa::tracked]
pub fn declared_member_signature<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MemberQuery<'db>,
) -> Option<DeclaredSignature<'db>>

#[salsa::interned(debug)]
pub struct FunctionQuery<'db> {
    /// Pre-folded Function-space key.
    #[returns(ref)]
    pub key: String,
}

#[salsa::tracked]
pub fn declared_function_signature<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: FunctionQuery<'db>,
) -> Option<DeclaredSignature<'db>>
```

**Element rules (native):**
- Method/function parameter: `type_text` lowered at the declaring
  site, else `mixed` (the design's monovariant-`mixed` decision);
  a default of `null` unions `null` in (implicit nullability, design
  section 2); `optional = default_text.is_some() || variadic`.
- Method/function return: `type_text` lowered, else `mixed`.
- Property: `type_text` lowered, else `mixed`.
- Class constant: `type_text` (typed constants, 8.3) lowered; else the
  literal type of its default when the default is a simple literal;
  else `mixed`.
- Enum case: `TypeId::enum_case(owner_key, case_name)`.
- Malformed type text lowers to `mixed` (never an error — resilience).

- [ ] **Step 1: Write the failing tests**

Append to `declared.rs`'s test module (extend the module started in
Task 2; add a salsa fixture mirroring `judgments.rs`'s):

```rust
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{
    MemberKind, MemberQuery, SymbolSpace, folded_member_key, folded_symbol_key,
};
use celerrate_source::FileId;
use celerrate_stubs::{StubIndex, StubIndexInput};

use super::{
    DeclaredSignature, FunctionQuery, Trust, declared_function_signature,
    declared_member_signature,
};

struct Fixture {
    db: TestDatabase,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

fn fixture(sources: &[&str]) -> Fixture {
    let db = TestDatabase::default();
    let handles: Vec<SourceFile> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let files = AnalyzedFileSet::new(&db, handles);
    let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    Fixture { db, files, stubs, configuration }
}

fn member<'db>(
    fixture: &'db Fixture,
    class_written: &str,
    kind: MemberKind,
    member_written: &str,
) -> Option<DeclaredSignature<'db>> {
    let query = MemberQuery::new(
        &fixture.db,
        folded_symbol_key(SymbolSpace::ClassLike, class_written),
        kind,
        folded_member_key(kind, member_written),
    );
    declared_member_signature(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    )
}

#[test]
fn a_method_signature_resolves_at_its_declaring_site() {
    let fixture = fixture(&[
        "<?php namespace App;\n\
         use Psr\\Log\\LoggerInterface as Logger;\n\
         class Service {\n\
             public function handle(Logger $logger, int $count = 3): ?string {}\n\
         }",
    ]);
    let signature = member(&fixture, "App\\Service", MemberKind::Method, "handle")
        .unwrap();
    let db = &fixture.db;
    assert_eq!(signature.parameters.len(), 2);
    assert_eq!(
        signature.parameters[0].parameter_type,
        Some(TypeId::class(db, "Psr\\Log\\LoggerInterface", vec![])),
    );
    assert!(!signature.parameters[0].optional);
    assert_eq!(signature.parameters[1].parameter_type, Some(TypeId::int(db)));
    assert!(signature.parameters[1].optional);
    assert_eq!(
        signature.value_type,
        TypeId::union(db, [TypeId::string(db), TypeId::null(db)]),
    );
    assert_eq!(signature.value_trust, Trust::NativeOnly);
}

#[test]
fn an_untyped_parameter_is_mixed_and_a_null_default_makes_it_nullable() {
    let fixture = fixture(&[
        "<?php class C { public function f($anything, ?Logger $log = null, Widget $w = null) {} }",
    ]);
    let signature = member(&fixture, "C", MemberKind::Method, "f").unwrap();
    let db = &fixture.db;
    assert_eq!(signature.parameters[0].parameter_type, Some(TypeId::mixed(db)));
    // `= null` on an already-nullable type changes nothing.
    let nullable_logger =
        TypeId::union(db, [TypeId::class(db, "Logger", vec![]), TypeId::null(db)]);
    assert_eq!(signature.parameters[1].parameter_type, Some(nullable_logger));
    // Implicit nullability: `Widget $w = null` admits null.
    let nullable_widget =
        TypeId::union(db, [TypeId::class(db, "Widget", vec![]), TypeId::null(db)]);
    assert_eq!(signature.parameters[2].parameter_type, Some(nullable_widget));
    // No declared return: mixed.
    assert_eq!(signature.value_type, TypeId::mixed(db));
}

#[test]
fn properties_constants_and_enum_cases_declare_their_value_types() {
    let fixture = fixture(&[
        "<?php\n\
         class C {\n\
             public ?int $count;\n\
             public $untyped;\n\
             const ACTIVE = 'active';\n\
             const int LIMIT = 10;\n\
         }\n\
         enum Status: string { case Active = 'active'; }",
    ]);
    let db = &fixture.db;
    let count = member(&fixture, "C", MemberKind::Property, "count").unwrap();
    assert_eq!(
        count.value_type,
        TypeId::union(db, [TypeId::int(db), TypeId::null(db)]),
    );
    let untyped = member(&fixture, "C", MemberKind::Property, "untyped").unwrap();
    assert_eq!(untyped.value_type, TypeId::mixed(db));
    // An untyped constant with a literal default carries the literal.
    let active = member(&fixture, "C", MemberKind::ClassConstant, "ACTIVE").unwrap();
    assert_eq!(active.value_type, TypeId::string_literal(db, "active"));
    // A typed constant (8.3) uses its written type.
    let limit = member(&fixture, "C", MemberKind::ClassConstant, "LIMIT").unwrap();
    assert_eq!(limit.value_type, TypeId::int(db));
    let case = member(&fixture, "Status", MemberKind::EnumCase, "Active").unwrap();
    assert_eq!(case.value_type, TypeId::enum_case(db, "Status", "Active"));
}

#[test]
fn an_inherited_member_resolves_in_the_declaring_class_namespace() {
    let fixture = fixture(&[
        "<?php namespace Lib; class Base { public function make(): Widget {} }",
        "<?php namespace App; class Child extends \\Lib\\Base {}",
    ]);
    let signature = member(&fixture, "App\\Child", MemberKind::Method, "make")
        .unwrap();
    // `Widget` qualifies in Lib (the declaring site), never in App.
    assert_eq!(
        signature.value_type,
        TypeId::class(&fixture.db, "Lib\\Widget", vec![]),
    );
}

#[test]
fn a_free_function_signature_resolves_like_a_method() {
    let fixture = fixture(&[
        "<?php namespace App; function build(int $count): ?Widget {}",
    ]);
    let query = FunctionQuery::new(
        &fixture.db,
        folded_symbol_key(SymbolSpace::Function, "App\\build"),
    );
    let signature = declared_function_signature(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    )
    .unwrap();
    let db = &fixture.db;
    assert_eq!(signature.parameters[0].parameter_type, Some(TypeId::int(db)));
    assert_eq!(
        signature.value_type,
        TypeId::union(db, [TypeId::class(db, "App\\Widget", vec![]), TypeId::null(db)]),
    );
}

#[test]
fn unknown_members_and_malformed_types_degrade_cleanly() {
    let fixture = fixture(&["<?php class C { public function f(): int {} }"]);
    assert!(member(&fixture, "C", MemberKind::Method, "ghost").is_none());
    assert!(member(&fixture, "Ghost", MemberKind::Method, "f").is_none());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types declared`
Expected: FAIL to compile — the queries and structs do not exist.

- [ ] **Step 3: Implement**

Add to `declared.rs` (below the lowering of Task 2). The full
implementation:

```rust
use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{
    Member, MemberKind, MemberQuery, MemberResolution, SymbolQuery,
    analyzed_file_index, item_tree, lookup_class_declaration,
    lookup_function_declaration, lookup_member, member_tree,
};
use celerrate_stubs::StubIndexInput;
```

(merge with the existing `use` lines; `MemberResolution` matching
gains its `Stub` arm in Task 10 — until then it is the current struct,
so destructure it as such and leave a `// Task 10 reshapes this into
an enum` note only if helpful.)

```rust
// … Trust, DeclaredParameter, DeclaredSignature exactly as in the
// Interfaces block above …

#[salsa::tracked]
pub fn declared_member_signature<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MemberQuery<'db>,
) -> Option<DeclaredSignature<'db>> {
    let resolution = lookup_member(db, files, stubs, configuration, query)?;
    let site_parts = declaring_site(db, files, &resolution.owner)?;
    let tables = UseTables::for_namespace(
        item_tree(db, site_parts.file),
        &site_parts.namespace,
    );
    let site = NameSite::Source {
        namespace: &site_parts.namespace,
        tables: &tables,
    };
    Some(resolve_member_signature(
        db,
        &site,
        &resolution.owner,
        &resolution.member,
    ))
}

/// The declaring site of one source class-like: its file handle and
/// namespace, found through the same firewalls linearization uses.
struct DeclaringSite {
    file: celerrate_db::SourceFile,
    namespace: String,
}

fn declaring_site(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    owner_key: &str,
) -> Option<DeclaringSite> {
    let query = SymbolQuery::new(
        db,
        celerrate_semantics::SymbolSpace::ClassLike,
        owner_key.to_owned(),
    );
    let (_, ast_id) = lookup_class_declaration(db, files, query)?;
    let index = analyzed_file_index(db, files);
    let position = index
        .binary_search_by_key(&ast_id.file, |(id, _)| *id)
        .ok()?;
    let (_, file) = *index.get(position)?;
    let namespace = member_tree(db, file)
        .classes
        .iter()
        .find(|group| group.ast_id == ast_id)?
        .namespace
        .clone();
    Some(DeclaringSite { file, namespace })
}

/// Resolves one member's written signature at its declaring site.
/// Annotations join in tasks 5-6; every element is `NativeOnly` here.
fn resolve_member_signature<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    owner_key: &str,
    member: &Member,
) -> DeclaredSignature<'db> {
    let value_type = match member.kind {
        MemberKind::EnumCase => TypeId::enum_case(db, owner_key, &member.name),
        MemberKind::ClassConstant => match member.signature.type_text.as_deref() {
            Some(text) => lowered_or_mixed(db, site, Some(text)),
            None => member
                .signature
                .default_text
                .as_deref()
                .and_then(|text| literal_type_of_default(db, text))
                .unwrap_or_else(|| TypeId::mixed(db)),
        },
        MemberKind::Method | MemberKind::Property => {
            lowered_or_mixed(db, site, member.signature.type_text.as_deref())
        }
    };
    DeclaredSignature {
        parameters: member
            .signature
            .parameters
            .iter()
            .map(|parameter| declared_parameter(db, site, parameter))
            .collect(),
        value_type,
        value_trust: Trust::NativeOnly,
        by_reference: member.signature.by_reference,
    }
}

fn declared_parameter<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    parameter: &celerrate_semantics::ParameterSignature,
) -> DeclaredParameter<'db> {
    let mut parameter_type = lowered_or_mixed(db, site, parameter.type_text.as_deref());
    // Implicit nullability (design section 2): `Type $x = null`.
    if parameter
        .default_text
        .as_deref()
        .is_some_and(|text| text.eq_ignore_ascii_case("null"))
    {
        parameter_type = TypeId::union(db, [parameter_type, TypeId::null(db)]);
    }
    DeclaredParameter {
        name: parameter.name.clone(),
        parameter_type: Some(parameter_type),
        trust: Trust::NativeOnly,
        optional: parameter.default_text.is_some() || parameter.variadic,
        variadic: parameter.variadic,
        by_reference: parameter.by_reference,
    }
}

/// Written text to lattice type: absent or malformed text is `mixed`
/// (resilience: a signature the parser mangled must never error).
fn lowered_or_mixed<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    text: Option<&str>,
) -> TypeId<'db> {
    text.and_then(|text| lower_written_text(db, site, text))
        .unwrap_or_else(|| TypeId::mixed(db))
}

/// The literal type of a comparable default text (`expression_text`
/// form: tokens joined with one space): integers (optionally `- `
/// prefixed), floats, single-quoted strings, `true`/`false`/`null`.
/// Anything else — expressions, constants, arrays — is `None`.
fn literal_type_of_default<'db>(
    db: &'db dyn salsa::Database,
    text: &str,
) -> Option<TypeId<'db>> {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return Some(TypeId::bool_literal(db, true));
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Some(TypeId::bool_literal(db, false));
    }
    if trimmed.eq_ignore_ascii_case("null") {
        return Some(TypeId::null(db));
    }
    if let Some(unquoted) = trimmed
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        // Single-quoted with no escapes: the raw content is the value.
        if !unquoted.contains('\\') && !unquoted.contains('\'') {
            return Some(TypeId::string_literal(db, unquoted));
        }
        return None;
    }
    let (negative, digits) = match trimmed.strip_prefix("- ") {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    if digits.bytes().all(|byte| byte.is_ascii_digit()) && !digits.is_empty() {
        let value = digits.parse::<i64>().ok()?;
        return Some(TypeId::int_literal(db, if negative { -value } else { value }));
    }
    if let Ok(value) = digits.parse::<f64>()
        && digits.contains('.')
    {
        return Some(TypeId::float_literal(
            db,
            if negative { -value } else { value },
        ));
    }
    None
}

#[salsa::interned(debug)]
pub struct FunctionQuery<'db> {
    /// Pre-folded Function-space key.
    #[returns(ref)]
    pub key: String,
}

#[salsa::tracked]
pub fn declared_function_signature<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: FunctionQuery<'db>,
) -> Option<DeclaredSignature<'db>> {
    let symbol_query = SymbolQuery::new(
        db,
        celerrate_semantics::SymbolSpace::Function,
        query.key(db).clone(),
    );
    let ast_id = lookup_function_declaration(db, files, symbol_query)?;
    let index = analyzed_file_index(db, files);
    let position = index
        .binary_search_by_key(&ast_id.file, |(id, _)| *id)
        .ok()?;
    let (_, file) = *index.get(position)?;
    let function = member_tree(db, file)
        .functions
        .iter()
        .find(|function| function.ast_id == ast_id)?
        .clone();
    let tables = UseTables::for_namespace(item_tree(db, file), &function.namespace);
    let site = NameSite::Source {
        namespace: &function.namespace,
        tables: &tables,
    };
    Some(DeclaredSignature {
        parameters: function
            .signature
            .parameters
            .iter()
            .map(|parameter| declared_parameter(db, &site, parameter))
            .collect(),
        value_type: lowered_or_mixed(db, &site, function.signature.type_text.as_deref()),
        value_trust: Trust::NativeOnly,
        by_reference: function.signature.by_reference,
    })
}
```

Note on the stubs/configuration parameters of
`declared_function_signature` and `declared_member_signature`: the
source arm barely reads them, but they are part of the key on purpose —
Task 10/11 adds the stub arms behind the same signatures, and the
persistent-cache plugin-set key (plan 9a) needs them recorded.

Re-export from `lib.rs`:

```rust
pub use declared::{
    DeclaredParameter, DeclaredSignature, FunctionQuery, Trust,
    declared_function_signature, declared_member_signature,
};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types`
Expected: PASS. Then `cargo test --workspace`, clippy, fmt.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): per-member declared-signature queries over native declarations"
```

---

### Task 5: The trust rule and the annotation seam

The three-valued refinement of the design's section 3 ("source
precedence"), plus the seam query plan 4a's bridge fills. Pure
functions first, wiring second, so the rule is testable today with
hand-built types.

**Files:**
- Modify: `crates/celerrate_types/src/declared.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (re-export
  `MemberAnnotations`, `member_annotations`)

**Interfaces:**
- Consumes: `subtype_of` (the shipped judgment), Task 4's structs.
- Produces:

```rust
/// The parsed annotation layer of one member. Plan 4a's bridge fills
/// this through the type-syntax registry; until then every member
/// answers the default (no annotations). The seam is a tracked query
/// so the bridge swap changes ONE body and no signatures.
#[derive(Debug, Clone, Default, PartialEq, Eq, salsa::Update)]
pub struct MemberAnnotations<'db> {
    /// `@return` / `@var`: the annotated value type.
    pub value: Option<TypeId<'db>>,
    /// `@param`: annotated parameter types by parameter name.
    pub parameters: Vec<(String, TypeId<'db>)>,
}

#[salsa::tracked]
pub fn member_annotations<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MemberQuery<'db>,
) -> MemberAnnotations<'db>

pub(crate) fn refine<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    native: TypeId<'db>,
    annotation: Option<TypeId<'db>>,
) -> (TypeId<'db>, Trust)
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_trust_rule_is_three_valued() {
    let fixture = fixture(&[
        "<?php interface Animal {} class Dog implements Animal {}",
    ]);
    let db = &fixture.db;
    let animal = TypeId::class(db, "Animal", vec![]);
    let dog = TypeId::class(db, "Dog", vec![]);
    let int = TypeId::int(db);
    let refine = |native, annotation| {
        super::refine(
            db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            native,
            annotation,
        )
    };
    // No annotation: native stands.
    assert_eq!(refine(animal, None), (animal, Trust::NativeOnly));
    // Holds: the annotation refines.
    assert_eq!(refine(animal, Some(dog)), (dog, Trust::Refined));
    // Fails: the annotation is ignored, the native declaration wins.
    assert_eq!(refine(int, Some(animal)), (int, Trust::RejectedAnnotation));
    // CannotProve (an unresolvable class): refines, traced.
    let ghost = TypeId::class(db, "Ghost", vec![]);
    assert_eq!(
        refine(animal, Some(ghost)),
        (ghost, Trust::RefinedUnproven),
    );
}

#[test]
fn the_annotation_seam_answers_the_default_until_the_bridge_lands() {
    let fixture = fixture(&["<?php class C { /** @return int */ public function f() {} }"]);
    let query = MemberQuery::new(
        &fixture.db,
        folded_symbol_key(SymbolSpace::ClassLike, "C"),
        MemberKind::Method,
        folded_member_key(MemberKind::Method, "f"),
    );
    let annotations = super::member_annotations(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    );
    assert_eq!(annotations, super::MemberAnnotations::default());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types declared`
Expected: FAIL to compile (`refine`, `member_annotations` missing).

- [ ] **Step 3: Implement**

```rust
/// The source-precedence rule of the design's section 3: an
/// annotation refines the native declaration under the three-valued
/// judgment. Holds refines; Fails is ignored (native wins);
/// CannotProve refines and is traced. Never a crash, never a silent
/// widening, never a silently dropped annotation.
pub(crate) fn refine<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    native: TypeId<'db>,
    annotation: Option<TypeId<'db>>,
) -> (TypeId<'db>, Trust) {
    let Some(annotated) = annotation else {
        return (native, Trust::NativeOnly);
    };
    match crate::judgments::subtype_of(db, files, stubs, configuration, annotated, native) {
        crate::judgments::Proof::Holds => (annotated, Trust::Refined),
        crate::judgments::Proof::CannotProve => (annotated, Trust::RefinedUnproven),
        crate::judgments::Proof::Fails => (native, Trust::RejectedAnnotation),
    }
}

#[salsa::tracked]
pub fn member_annotations<'db>(
    _db: &'db dyn salsa::Database,
    _files: AnalyzedFileSet,
    _stubs: StubIndexInput,
    _configuration: ProjectConfiguration,
    _query: MemberQuery<'db>,
) -> MemberAnnotations<'db> {
    // The seam: plan 4a's bridge replaces this body with the
    // docblock parse through the type-syntax registry. Everything
    // downstream (precedence, trust, inheritance) is already wired.
    MemberAnnotations::default()
}
```

(Salsa may reject unused parameters on tracked functions or clippy may
flag the underscores; if so, name them and add
`let _ = (files, stubs, configuration, query);` — keep the parameters:
they are the seam's contract.)

Then wire refinement into `resolve_member_signature`: change its
signature to accept the annotation layer and thread the trust through
(the full wiring including inheritance lands in Task 6; in this task,
wire **own** annotations only):

In `declared_member_signature`, before building the result:

```rust
let annotations = member_annotations(db, files, stubs, configuration, query);
```

and pass `&annotations` into `resolve_member_signature`, which now
does, for the value element:

```rust
let (value_type, value_trust) = refine(
    db, files, stubs, configuration,
    native_value_type,
    annotations.value,
);
```

and per parameter (annotation looked up by name):

```rust
let annotation = annotations
    .parameters
    .iter()
    .find(|(name, _)| *name == parameter.name)
    .map(|(_, annotated)| *annotated);
let (parameter_type, trust) = refine(
    db, files, stubs, configuration, native_parameter_type, annotation,
);
```

This changes `resolve_member_signature`'s and `declared_parameter`'s
parameter lists to carry `files`/`stubs`/`configuration` — mechanical.
Enum cases skip refinement (their type is their identity). Existing
Task 4 tests keep passing (`NativeOnly` everywhere, since the seam
answers the default).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types` — PASS; clippy; fmt.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): the three-valued trust rule and the annotation seam"
```

---

### Task 6: Docblock inheritance — the nearest-ancestor walk

Design section 3, "Declared types inherit": when a member declares no
annotation of its own, the nearest ancestor's annotation along the
linearization applies, checked by the trust rule against the
*inheriting* member's native declaration. The walk is generic over an
annotation reader so it is fully testable before the bridge exists.

**Files:**
- Modify: `crates/celerrate_types/src/declared.rs`

**Interfaces:**
- Consumes: `celerrate_semantics::{linearized_class, ClassQuery,
  LinearizedClass, lookup_class_declaration, member_tree,
  folded_member_key}`, Task 5's `MemberAnnotations`.
- Produces (crate-internal):

```rust
/// The ancestor keys of one linearized class, nearest first: the
/// resolved targets of the ancestry edges in walk order, the queried
/// class itself and duplicates removed.
fn ancestors_in_walk_order(root_key: &str, linearized: &LinearizedClass) -> Vec<String>

/// The nearest-ancestor annotations for one member, per element:
/// walking `ancestors`, the first ancestor that DECLARES the member
/// itself and annotates an element supplies that element; nearer
/// ancestors win element-wise.
fn inherited_annotations<'db>(
    own: MemberAnnotations<'db>,
    parameter_names: &[String],
    ancestors: &[String],
    declares: impl Fn(&str) -> bool,
    read: impl Fn(&str) -> MemberAnnotations<'db>,
) -> MemberAnnotations<'db>
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_nearest_ancestor_annotation_wins_element_wise() {
    let db = TestDatabase::default();
    let int = TypeId::int(&db);
    let string = TypeId::string(&db);
    let bool_type = TypeId::bool(&db);
    let annotations_of = |key: &str| -> super::MemberAnnotations<'_> {
        match key {
            // The near ancestor annotates only the parameter.
            "near" => super::MemberAnnotations {
                value: None,
                parameters: vec![("x".to_owned(), string)],
            },
            // The far ancestor annotates both.
            "far" => super::MemberAnnotations {
                value: Some(int),
                parameters: vec![("x".to_owned(), bool_type)],
            },
            _ => super::MemberAnnotations::default(),
        }
    };
    let ancestors = vec!["near".to_owned(), "far".to_owned()];
    let merged = super::inherited_annotations(
        super::MemberAnnotations::default(),
        &["x".to_owned()],
        &ancestors,
        |_| true,
        annotations_of,
    );
    // Value: the near ancestor is silent, the far one supplies it.
    assert_eq!(merged.value, Some(int));
    // Parameter: the near ancestor wins over the far one.
    assert_eq!(merged.parameters, vec![("x".to_owned(), string)]);
}

#[test]
fn own_annotations_shadow_every_ancestor_and_non_declaring_ancestors_are_skipped() {
    let db = TestDatabase::default();
    let int = TypeId::int(&db);
    let string = TypeId::string(&db);
    let own = super::MemberAnnotations {
        value: Some(int),
        parameters: vec![],
    };
    let merged = super::inherited_annotations(
        own.clone(),
        &[],
        &["ancestor".to_owned()],
        |_| true,
        |_| super::MemberAnnotations {
            value: Some(string),
            parameters: vec![],
        },
    );
    assert_eq!(merged.value, Some(int), "own annotation shadows");

    // An ancestor that does not declare the member supplies nothing.
    let merged = super::inherited_annotations(
        super::MemberAnnotations::default(),
        &[],
        &["silent".to_owned()],
        |_| false,
        |_| super::MemberAnnotations {
            value: Some(string),
            parameters: vec![],
        },
    );
    assert_eq!(merged.value, None);
}

#[test]
fn ancestors_walk_in_linearization_order_without_the_root_or_duplicates() {
    let fixture = fixture(&[
        "<?php\n\
         interface I {}\n\
         class A implements I {}\n\
         class B extends A implements I {}\n\
         class C extends B {}",
    ]);
    let key = folded_symbol_key(SymbolSpace::ClassLike, "C");
    let class = celerrate_semantics::ClassQuery::new(&fixture.db, key.clone());
    let linearized = celerrate_semantics::linearized_class(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        class,
    )
    .as_ref()
    .unwrap();
    assert_eq!(
        super::ancestors_in_walk_order(&key, linearized),
        vec!["b".to_owned(), "a".to_owned(), "i".to_owned()],
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types declared` — FAIL to
compile.

- [ ] **Step 3: Implement**

```rust
/// The ancestor keys of one linearized class, nearest first (edge
/// walk order), the root and duplicates removed. Stub ancestors take
/// no part: annotations live in source docblocks.
fn ancestors_in_walk_order(root_key: &str, linearized: &LinearizedClass) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for edge in &linearized.ancestry {
        let Some(resolved) = edge.resolved.as_deref() else {
            continue;
        };
        if resolved == root_key || seen.iter().any(|key| key == resolved) {
            continue;
        }
        seen.push(resolved.to_owned());
    }
    seen
}

/// Element-wise nearest-ancestor merge: own annotations first, then
/// each declaring ancestor in walk order fills only what is still
/// missing. `declares` gates on the ancestor's OWN member table (an
/// ancestor that merely inherits the member is not its annotation
/// site); `read` supplies the ancestor's parsed annotations.
fn inherited_annotations<'db>(
    own: MemberAnnotations<'db>,
    parameter_names: &[String],
    ancestors: &[String],
    declares: impl Fn(&str) -> bool,
    read: impl Fn(&str) -> MemberAnnotations<'db>,
) -> MemberAnnotations<'db> {
    let mut merged = own;
    for ancestor in ancestors {
        let value_missing = merged.value.is_none();
        let missing_parameters: Vec<&String> = parameter_names
            .iter()
            .filter(|name| {
                !merged
                    .parameters
                    .iter()
                    .any(|(merged_name, _)| merged_name == *name)
            })
            .collect();
        if !value_missing && missing_parameters.is_empty() {
            return merged;
        }
        if !declares(ancestor) {
            continue;
        }
        let ancestor_annotations = read(ancestor);
        if value_missing {
            merged.value = ancestor_annotations.value;
        }
        for name in missing_parameters {
            if let Some((_, annotated)) = ancestor_annotations
                .parameters
                .iter()
                .find(|(ancestor_name, _)| ancestor_name == name)
            {
                merged.parameters.push((name.clone(), *annotated));
            }
        }
    }
    merged
}
```

Then wire it into `declared_member_signature` (source arm), replacing
the plain `member_annotations` read:

```rust
let own = member_annotations(db, files, stubs, configuration, query);
let root_key = query.class_key(db);
let class = celerrate_semantics::ClassQuery::new(db, root_key.clone());
let linearized = celerrate_semantics::linearized_class(
    db, files, stubs, configuration, class,
);
let parameter_names: Vec<String> = resolution
    .member
    .signature
    .parameters
    .iter()
    .map(|parameter| parameter.name.clone())
    .collect();
let annotations = match linearized.as_ref() {
    Some(linearized) => {
        let ancestors = ancestors_in_walk_order(root_key, linearized);
        let kind = query.kind(db);
        let member_key = query.member_key(db);
        inherited_annotations(
            own,
            &parameter_names,
            &ancestors,
            |ancestor| declares_member(db, files, ancestor, kind, member_key),
            |ancestor| {
                let ancestor_query = MemberQuery::new(
                    db,
                    ancestor.to_owned(),
                    kind,
                    member_key.clone(),
                );
                member_annotations(db, files, stubs, configuration, ancestor_query)
            },
        )
    }
    None => own,
};
```

with the gate helper:

```rust
/// Whether `class_key`'s OWN member group declares a member of this
/// kind and key (inherited entries do not count: the annotation site
/// is the declaring docblock).
fn declares_member(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    class_key: &str,
    kind: MemberKind,
    member_key: &str,
) -> bool {
    let Some(site) = declaring_site(db, files, class_key) else {
        return false;
    };
    let Some((_, ast_id)) = lookup_class_declaration(
        db,
        files,
        SymbolQuery::new(
            db,
            celerrate_semantics::SymbolSpace::ClassLike,
            class_key.to_owned(),
        ),
    ) else {
        return false;
    };
    member_tree(db, site.file)
        .classes
        .iter()
        .find(|group| group.ast_id == ast_id)
        .is_some_and(|group| {
            group.members.iter().any(|member| {
                member.kind == kind
                    && celerrate_semantics::folded_member_key(kind, &member.name)
                        == member_key
            })
        })
}
```

(`declaring_site` already resolves key → file; reuse it — if the
double `lookup_class_declaration` bothers you, have `declaring_site`
also return the `ast_id` and use it here.)

The behavior is invisible today (the seam answers defaults), so also
add one integration pin:

```rust
#[test]
fn inheritance_wiring_leaves_native_results_untouched_while_the_seam_is_empty() {
    let fixture = fixture(&[
        "<?php interface Normalizer { public function normalize($data): array {} }\n\
         class UserNormalizer implements Normalizer {\n\
             public function normalize($data): array {}\n\
         }",
    ]);
    let signature = member(
        &fixture,
        "UserNormalizer",
        MemberKind::Method,
        "normalize",
    )
    .unwrap();
    assert_eq!(signature.value_trust, Trust::NativeOnly);
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types` — PASS; then
`cargo test --workspace`, clippy, fmt.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): nearest-ancestor annotation inheritance along the linearization"
```

---
### Task 7: The versioned stub signature model (`celerrate_stubs`)

The runtime data model for the signature payload: versioned type
texts, parameters, signatures, members, class surfaces, and the
`StubIndex` extension carrying them. Pure data + one semantic method
(`VersionedTypeText::at`); the blob wire format is Task 8, the
extraction Task 9.

**Files:**
- Create: `crates/celerrate_stubs/src/signature.rs`
- Modify: `crates/celerrate_stubs/src/index.rs`
- Modify: `crates/celerrate_stubs/src/lib.rs` (module + re-exports)

**Interfaces:**
- Consumes: `celerrate_project::PhpVersion`, `crate::symbol::StubAvailability`.
- Produces (all `pub`, re-exported):

```rust
/// One type text across PHP versions: a default plus ascending
/// `(from_version, text)` overrides — the compiled form of
/// phpstorm-stubs' `#[LanguageLevelTypeAware]`. `at(v)` answers the
/// text effective at `v`: the last override whose version is ≤ `v`,
/// else the default. `NONE` (no default, no overrides) means "no
/// declared type at any version".
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct VersionedTypeText {
    pub default: Option<String>,
    pub overrides: Vec<(PhpVersion, String)>,
}

impl VersionedTypeText {
    pub fn from_text(text: Option<String>) -> Self;
    pub fn at(&self, version: PhpVersion) -> Option<&str>;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StubParameter {
    pub name: String,
    pub type_text: VersionedTypeText,
    pub optional: bool,
    pub by_reference: bool,
    pub variadic: bool,
    /// The parameter's own window (`#[PhpStormStubsElementAvailable]`):
    /// a parameter added in 8.2 exists only from 8.2 on.
    pub availability: StubAvailability,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StubSignature {
    pub parameters: Vec<StubParameter>,
    pub return_type: VersionedTypeText,
    pub by_reference: bool,
}

/// Blob discriminants: fixed forever, like `StubSymbolKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StubMemberKind { Method = 0, Property = 1, ClassConstant = 2, EnumCase = 3 }
impl StubMemberKind { pub const fn as_u8(self) -> u8; pub const fn from_u8(value: u8) -> Option<Self>; }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StubVisibility { Public = 0, Protected = 1, Private = 2 }
impl StubVisibility { pub const fn as_u8(self) -> u8; pub const fn from_u8(value: u8) -> Option<Self>; }

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StubMember {
    pub kind: StubMemberKind,
    /// Original spelling; property names without the `$`.
    pub name: String,
    pub visibility: StubVisibility,
    pub is_static: bool,
    pub availability: StubAvailability,
    /// Methods only.
    pub signature: Option<StubSignature>,
    /// Properties and class constants: the declared/versioned type.
    pub type_text: VersionedTypeText,
    /// Class constants and enum cases: the literal value text, when
    /// the value is a simple literal (`'active'`, `- 1`), else `None`.
    pub value_text: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StubClassSurface {
    /// Fully qualified parent names (no leading backslash), extends
    /// first then implements, declared order — the walk order of the
    /// stub side of linearization.
    pub parents: Vec<String>,
    pub members: Vec<StubMember>,
}
```

- `StubIndex` gains two sorted payloads and accessors:

```rust
pub struct StubIndex {
    symbols: Vec<StubSymbol>,
    functions: Vec<(String, StubSignature)>,   // sorted by name, first wins
    classes: Vec<(String, StubClassSurface)>,  // sorted by name, first wins
}

impl StubIndex {
    pub fn new(
        symbols: Vec<StubSymbol>,
        functions: Vec<(String, StubSignature)>,
        classes: Vec<(String, StubClassSurface)>,
    ) -> Self;
    // from_symbols(symbols) == new(symbols, vec![], vec![]) — kept.
    pub fn functions(&self) -> &[(String, StubSignature)];
    pub fn classes(&self) -> &[(String, StubClassSurface)];
}
```

Duplicate names in `functions`/`classes` keep the **first after a
stable sort by name** — deterministic; the availability-merged
duplicates of the symbol table do not apply to signatures (recorded
simplification: phpstorm-stubs' duplicate declarations carry the same
shapes; revisit if the corpus spot checks of Task 12 disagree).

- [ ] **Step 1: Write the failing tests**

Create `crates/celerrate_stubs/src/signature.rs` with the types above
stubbed minimally enough to compile plus this test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_project::PhpVersion;

    use super::{StubMemberKind, StubVisibility, VersionedTypeText};

    #[test]
    fn a_versioned_text_answers_the_effective_text_per_version() {
        let text = VersionedTypeText {
            default: Some("int".to_owned()),
            overrides: vec![
                (PhpVersion::new(8, 0), "int|false".to_owned()),
                (PhpVersion::new(8, 3), "int|float|false".to_owned()),
            ],
        };
        assert_eq!(text.at(PhpVersion::new(7, 4)), Some("int"));
        assert_eq!(text.at(PhpVersion::new(8, 0)), Some("int|false"));
        assert_eq!(text.at(PhpVersion::new(8, 2)), Some("int|false"));
        assert_eq!(text.at(PhpVersion::new(8, 3)), Some("int|float|false"));
        assert_eq!(text.at(PhpVersion::new(8, 5)), Some("int|float|false"));
    }

    #[test]
    fn the_empty_versioned_text_is_none_everywhere() {
        assert_eq!(VersionedTypeText::default().at(PhpVersion::new(8, 1)), None);
        assert_eq!(
            VersionedTypeText::from_text(Some("string".to_owned()))
                .at(PhpVersion::new(8, 1)),
            Some("string"),
        );
        assert_eq!(VersionedTypeText::from_text(None).at(PhpVersion::new(8, 1)), None);
    }

    #[test]
    fn member_kinds_and_visibilities_round_trip_their_discriminants() {
        for kind in [
            StubMemberKind::Method,
            StubMemberKind::Property,
            StubMemberKind::ClassConstant,
            StubMemberKind::EnumCase,
        ] {
            assert_eq!(StubMemberKind::from_u8(kind.as_u8()), Some(kind));
        }
        assert_eq!(StubMemberKind::from_u8(4), None);
        for visibility in [
            StubVisibility::Public,
            StubVisibility::Protected,
            StubVisibility::Private,
        ] {
            assert_eq!(StubVisibility::from_u8(visibility.as_u8()), Some(visibility));
        }
        assert_eq!(StubVisibility::from_u8(3), None);
    }
}
```

And in `index.rs`'s test module:

```rust
#[test]
fn signature_payloads_sort_by_name_and_keep_the_first_duplicate() {
    use crate::signature::{StubClassSurface, StubSignature, VersionedTypeText};
    let first = StubSignature {
        return_type: VersionedTypeText::from_text(Some("int".to_owned())),
        ..StubSignature::default()
    };
    let second = StubSignature {
        return_type: VersionedTypeText::from_text(Some("string".to_owned())),
        ..StubSignature::default()
    };
    let index = StubIndex::new(
        vec![],
        vec![
            ("zebra".to_owned(), second.clone()),
            ("apple".to_owned(), first.clone()),
            ("apple".to_owned(), second),
        ],
        vec![("Exception".to_owned(), StubClassSurface::default())],
    );
    let names: Vec<&str> = index
        .functions()
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(names, vec!["apple", "zebra"]);
    assert_eq!(index.functions()[0].1, first, "first duplicate wins");
    assert_eq!(index.classes().len(), 1);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_stubs`
Expected: FAIL to compile (missing types/methods).

- [ ] **Step 3: Implement**

`signature.rs` — the types exactly as in the Interfaces block, plus:

```rust
impl VersionedTypeText {
    /// A plain, unversioned text (or nothing).
    pub fn from_text(text: Option<String>) -> Self {
        Self { default: text, overrides: Vec::new() }
    }

    /// The text effective at `version`: the last override whose
    /// version is not later, else the default. Overrides are kept
    /// sorted ascending by the constructors (Task 9's extractor sorts;
    /// Task 8's decoder preserves order).
    pub fn at(&self, version: PhpVersion) -> Option<&str> {
        self.overrides
            .iter()
            .rev()
            .find(|(from, _)| *from <= version)
            .map(|(_, text)| text.as_str())
            .or(self.default.as_deref())
    }
}
```

`StubMemberKind::from_u8`/`as_u8` and `StubVisibility` mirror
`StubSymbolKind`'s implementations verbatim.

`index.rs` — extend the struct, keep `from_symbols` delegating:

```rust
pub fn from_symbols(symbols: Vec<StubSymbol>) -> Self {
    Self::new(symbols, Vec::new(), Vec::new())
}

pub fn new(
    mut symbols: Vec<StubSymbol>,
    mut functions: Vec<(String, StubSignature)>,
    mut classes: Vec<(String, StubClassSurface)>,
) -> Self {
    // (move the existing sort+merge of `symbols` here)
    functions.sort_by(|left, right| left.0.cmp(&right.0));
    functions.dedup_by(|second, first| first.0 == second.0);
    classes.sort_by(|left, right| left.0.cmp(&right.0));
    classes.dedup_by(|second, first| first.0 == second.0);
    Self { symbols: merged, functions, classes }
}
```

(`Vec::dedup_by` removes the *later* of two adjacent equals, and the
sort is stable, so the first declaration wins — assert exactly that in
the test.) Add the two slice accessors. Update `lib.rs`:
`mod signature;` plus

```rust
pub use signature::{
    StubClassSurface, StubMember, StubMemberKind, StubParameter,
    StubSignature, StubVisibility, VersionedTypeText,
};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_stubs` then
`cargo test --workspace` (nothing else constructs `StubIndex` by
struct literal — everything goes through `from_symbols`, which kept
its signature). Clippy, fmt.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_stubs
git commit -m "✨ feat(stubs): the versioned signature model behind the reserved section"
```

---

### Task 8: The `SECTION_SIGNATURES` wire format, and the cache schema bump

Encode/decode the payload into the reserved section. Per decision 1:
`BLOB_FORMAT_VERSION` stays 1 (additive section, the format's own
documented policy), `CACHE_SCHEMA_VERSION` bumps 2 → 3.

**Wire format** (all integers little-endian; `string` is
`u32 length ++ UTF-8 bytes`; `availability` is the existing symbol
scheme: `u8 flags` (bit0 introduced, bit1 removed, bit2 deprecated,
bit3 since) then each present version as `[major, minor]`;
`versioned_text` is `u8 has_default (0/1)` ++ optional `string` ++
`u16 override_count` ++ per override `[major, minor] ++ string`):

```
section payload:
  u32 function_count
  per function: string name ++ signature
  u32 class_count
  per class:
    string name
    u32 parent_count ++ per parent: string
    u32 member_count ++ per member:
      u8 kind ++ u8 flags (bit0 static, bits 1-2 visibility)
      availability
      string name
      versioned_text                  (the type_text; methods: NONE)
      u8 has_value ++ optional string (value_text)
      if kind == method: signature
signature:
  u8 by_reference (0/1)
  versioned_text                      (return)
  u32 parameter_count ++ per parameter:
    u8 flags (bit0 optional, bit1 by_reference, bit2 variadic)
    availability
    string name
    versioned_text
```

**Files:**
- Modify: `crates/celerrate_stubs/src/blob.rs`
- Modify: `crates/celerrate_cli/src/cache/pack.rs` (the constant and
  its doc list)

**Interfaces:**
- Consumes: Task 7's types, the existing `Reader`, `fnv1a64`,
  encode/decode skeleton.
- Produces: `encode` writes two sections (symbol table + signatures);
  `decode` reads the signatures section when present, defaulting to
  empty payloads when absent (a version-1 blob without the section
  stays decodable — forward AND backward tolerant).

- [ ] **Step 1: Write the failing tests**

Add to `blob.rs`'s test module:

```rust
use crate::signature::{
    StubClassSurface, StubMember, StubMemberKind, StubParameter,
    StubSignature, StubVisibility, VersionedTypeText,
};

fn sample_index_with_signatures() -> StubIndex {
    let strlen = StubSignature {
        parameters: vec![StubParameter {
            name: "string".to_owned(),
            type_text: VersionedTypeText::from_text(Some("string".to_owned())),
            optional: false,
            by_reference: false,
            variadic: false,
            availability: StubAvailability::ALWAYS,
        }],
        return_type: VersionedTypeText {
            default: Some("int".to_owned()),
            overrides: vec![(PhpVersion::new(8, 0), "int|false".to_owned())],
        },
        by_reference: false,
    };
    let exception = StubClassSurface {
        parents: vec!["Throwable".to_owned()],
        members: vec![
            StubMember {
                kind: StubMemberKind::Method,
                name: "getMessage".to_owned(),
                visibility: StubVisibility::Public,
                is_static: false,
                availability: StubAvailability::ALWAYS,
                signature: Some(StubSignature {
                    parameters: vec![],
                    return_type: VersionedTypeText::from_text(Some("string".to_owned())),
                    by_reference: false,
                }),
                type_text: VersionedTypeText::default(),
                value_text: None,
            },
            StubMember {
                kind: StubMemberKind::Property,
                name: "message".to_owned(),
                visibility: StubVisibility::Protected,
                is_static: false,
                availability: StubAvailability::ALWAYS,
                signature: None,
                type_text: VersionedTypeText::from_text(Some("string".to_owned())),
                value_text: None,
            },
        ],
    };
    StubIndex::new(
        sample_index().symbols().to_vec(),
        vec![("strlen".to_owned(), strlen)],
        vec![("Exception".to_owned(), exception)],
    )
}

#[test]
fn signatures_round_trip_through_the_blob() {
    let index = sample_index_with_signatures();
    assert_eq!(decode(&encode(&index)), Ok(index));
}

#[test]
fn a_blob_without_the_signature_section_decodes_with_empty_payloads() {
    // The pre-plan-3 encoding: hand-build a one-section blob exactly
    // as the old `encode` did, and check it still decodes.
    let old_index = sample_index();
    let with_signatures = sample_index_with_signatures();
    let blob = encode(&with_signatures);
    // Sanity: the new blob decodes to the full index (covered above);
    // the OLD layout is simulated by re-encoding only symbols.
    let symbols_only = encode(&old_index);
    assert_eq!(decode(&symbols_only), Ok(old_index));
    assert_ne!(blob, symbols_only);
}

#[test]
fn a_truncated_signature_section_never_panics() {
    let blob = encode(&sample_index_with_signatures());
    for length in 0..blob.len() {
        // Every prefix is an error or a clean decode, never a panic.
        let _ = decode(&blob[..length]);
    }
}

#[test]
fn a_malformed_signature_section_is_a_clean_rejection() {
    let mut blob = encode(&sample_index_with_signatures());
    // Flip a byte deep in the payload (past the header and table),
    // then re-patch the checksum so decoding reaches the section.
    let last = blob.len() - 1;
    blob[last] ^= 0xFF;
    let checksum = fnv1a64(&blob[20..]);
    blob[12..20].copy_from_slice(&checksum.to_le_bytes());
    // Either a clean error or a decode that differs — never a panic.
    let _ = decode(&blob);
}
```

In `crates/celerrate_cli/src/cache/pack.rs`: extend the existing
schema-history doc comment on `CACHE_SCHEMA_VERSION` with a `3:` line
and change the test expectation if any test pins the literal value.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_stubs blob`
Expected: FAIL — round trip loses the payloads (encode/decode ignore
them).

- [ ] **Step 3: Implement**

In `blob.rs`:

1. `encode` gains the second section (always written, even when
empty — deterministic bytes either way):

```rust
pub fn encode(index: &StubIndex) -> Vec<u8> {
    let symbol_table = encode_symbol_table(index);
    let signatures = encode_signatures(index);
    let table_entries = 2u32;
    let symbol_offset = 24u64 + u64::from(table_entries) * 20;
    let signature_offset = symbol_offset + symbol_table.len() as u64;
    let mut blob = Vec::with_capacity(symbol_table.len() + signatures.len() + 64);
    blob.extend_from_slice(&BLOB_MAGIC);
    blob.extend_from_slice(&BLOB_FORMAT_VERSION.to_le_bytes());
    blob.extend_from_slice(&[0; 8]); // checksum, patched below
    blob.extend_from_slice(&table_entries.to_le_bytes());
    blob.extend_from_slice(&SECTION_SYMBOL_TABLE.to_le_bytes());
    blob.extend_from_slice(&symbol_offset.to_le_bytes());
    blob.extend_from_slice(&(symbol_table.len() as u64).to_le_bytes());
    blob.extend_from_slice(&SECTION_SIGNATURES.to_le_bytes());
    blob.extend_from_slice(&signature_offset.to_le_bytes());
    blob.extend_from_slice(&(signatures.len() as u64).to_le_bytes());
    blob.extend_from_slice(&symbol_table);
    blob.extend_from_slice(&signatures);
    let checksum = fnv1a64(blob.get(20..).unwrap_or_default());
    if let Some(slot) = blob.get_mut(12..20) {
        slot.copy_from_slice(&checksum.to_le_bytes());
    }
    blob
}
```

Note this changes the symbol-table offset (two table entries now):
the existing test `unknown_sections_are_skipped_for_forward_compatibility`
slices the old blob at `[44..]` — update it to `[64..]`
(24-byte header + 2 × 20-byte entries) or, better, rebuild its
symbol-table bytes from a helper that computes the offset.

2. The writers (factor small helpers; every count is checked-cast):

```rust
fn write_string(bytes: &mut Vec<u8>, text: &str) {
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
}

fn write_availability(bytes: &mut Vec<u8>, availability: StubAvailability) {
    // (extract the existing flag logic from `encode_symbol_table`
    // into this helper and call it from both places)
}

fn write_versioned_text(bytes: &mut Vec<u8>, text: &VersionedTypeText) {
    match &text.default {
        Some(default) => {
            bytes.push(1);
            write_string(bytes, default);
        }
        None => bytes.push(0),
    }
    bytes.extend_from_slice(&(text.overrides.len() as u16).to_le_bytes());
    for (version, override_text) in &text.overrides {
        bytes.extend_from_slice(&[version.major, version.minor]);
        write_string(bytes, override_text);
    }
}

fn write_signature(bytes: &mut Vec<u8>, signature: &StubSignature) {
    bytes.push(u8::from(signature.by_reference));
    write_versioned_text(bytes, &signature.return_type);
    bytes.extend_from_slice(&(signature.parameters.len() as u32).to_le_bytes());
    for parameter in &signature.parameters {
        let mut flags = 0u8;
        if parameter.optional { flags |= 1; }
        if parameter.by_reference { flags |= 1 << 1; }
        if parameter.variadic { flags |= 1 << 2; }
        bytes.push(flags);
        write_availability(bytes, parameter.availability);
        write_string(bytes, &parameter.name);
        write_versioned_text(bytes, &parameter.type_text);
    }
}

fn encode_signatures(index: &StubIndex) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(index.functions().len() as u32).to_le_bytes());
    for (name, signature) in index.functions() {
        write_string(&mut bytes, name);
        write_signature(&mut bytes, signature);
    }
    bytes.extend_from_slice(&(index.classes().len() as u32).to_le_bytes());
    for (name, surface) in index.classes() {
        write_string(&mut bytes, name);
        bytes.extend_from_slice(&(surface.parents.len() as u32).to_le_bytes());
        for parent in &surface.parents {
            write_string(&mut bytes, parent);
        }
        bytes.extend_from_slice(&(surface.members.len() as u32).to_le_bytes());
        for member in &surface.members {
            bytes.push(member.kind.as_u8());
            let mut flags = 0u8;
            if member.is_static { flags |= 1; }
            flags |= member.visibility.as_u8() << 1;
            bytes.push(flags);
            write_availability(&mut bytes, member.availability);
            write_string(&mut bytes, &member.name);
            write_versioned_text(&mut bytes, &member.type_text);
            match &member.value_text {
                Some(value) => {
                    bytes.push(1);
                    write_string(&mut bytes, value);
                }
                None => bytes.push(0),
            }
            if member.kind == StubMemberKind::Method {
                // Bind the default outside the call: a temporary
                // `&StubSignature::default()` would not live long enough.
                let default_signature = StubSignature::default();
                let signature = member.signature.as_ref().unwrap_or(&default_signature);
                write_signature(&mut bytes, signature);
            }
        }
    }
    bytes
}
```

(`unwrap_or` with a default, never `unwrap` — the lints forbid it.
A method whose `signature` is `None` encodes an empty signature; the
decoder always reconstructs `Some(signature)` for methods — pin that
in the round-trip test by never building the `None`-method case, and
note it on `StubMember::signature`'s doc.)

3. The readers, mirroring, on `Reader`: add
`fn u16(&mut self) -> Option<u16>`,
`fn string(&mut self) -> Option<String>`,
`fn availability(&mut self) -> Option<StubAvailability>` (extract from
`decode_symbol_table`),
`fn versioned_text(&mut self) -> Option<VersionedTypeText>`,
`fn stub_signature(&mut self) -> Option<StubSignature>` — each a
direct mirror of its writer, every read `?`-propagated,
`Err(StubBlobError::MalformedSection)` at the call site. In `decode`,
capture `SECTION_SIGNATURES` alongside the symbol table:

```rust
let mut signatures: Option<&[u8]> = None;
// in the section loop:
if identifier == SECTION_SIGNATURES {
    signatures = Some(section);
}
// at the end:
let symbols = decode_symbol_table(symbol_table.ok_or(StubBlobError::MissingSymbolTable)?)?;
match signatures {
    Some(section) => {
        let (functions, classes) = decode_signatures(section)?;
        Ok(StubIndex::new(symbols_vec, functions, classes))
    }
    None => Ok(StubIndex::from_symbols(symbols_vec)),
}
```

which requires `decode_symbol_table` to return `Vec<StubSymbol>`
instead of a finished `StubIndex` — a small refactor of the existing
function (its tests keep passing through `decode`).

4. `CACHE_SCHEMA_VERSION` in `celerrate_cli/src/cache/pack.rs`: bump
`2` → `3`, extend the doc history with
`/// 3: the stub blob gains SECTION_SIGNATURES (plan 3).` — the
mismatch tests are value-agnostic (`header.schema += 1`), so only the
constant and its doc change.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_stubs --package celerrate_cli`
then `cargo test --workspace`, clippy, fmt.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_stubs crates/celerrate_cli
git commit -m "✨ feat(stubs): encode the signature payload in the reserved blob section"
```

---

### Task 9: The compiler extracts signatures from phpstorm-stubs

Extend `extract.rs`: function signatures, class surfaces (parents +
members), `#[LanguageLevelTypeAware]` versioned type texts, and
per-parameter `#[PhpStormStubsElementAvailable]` windows. The
committed `stubs.bin` is recompiled in Task 12, not here — this task
is pure extraction logic over handcrafted snippets.

**Files:**
- Modify: `crates/celerrate_stubs/src/compiler/extract.rs`

**Interfaces:**
- Consumes: Task 7's types; the existing `availability_of`,
  `string_literal`, `parse_version`, `qualify` helpers; the typed AST
  (`ast::FunctionDeclaration`, `ast::ClassDeclaration`,
  `ast::InterfaceDeclaration`, `ast::EnumDeclaration`,
  `ast::MethodDeclaration`, `ast::PropertyDeclaration`,
  `ast::ConstantDeclaration`, `ast::EnumCase`, `ast::Parameter`,
  `ast::ArrayExpression`, `ast::ArrayElement`,
  `ast::type_text`, `ast::expression_text`).
- Produces: `Extraction` gains
  `pub functions: Vec<(String, StubSignature)>` and
  `pub classes: Vec<(String, StubClassSurface)>` (fully qualified
  names, matching `symbols`' spelling rules). Traits contribute
  surfaces too (their kind is in the symbol table; the surface shape
  is identical).

**Extraction rules:**
- Function/method parameters mirror
  `celerrate_semantics::parameter_signatures` (name without `$`,
  type text via `ast::type_text`, `optional` = has default, by-ref,
  variadic) — duplicated here deliberately: the DAG forbids stubs →
  semantics, and the logic is eight lines.
- A parameter's or return's `VersionedTypeText`: if a
  `#[LanguageLevelTypeAware(['8.0' => '…', …], default: '…')]`
  attribute sits on the parameter (or on the function/method for the
  return), its map becomes `overrides` (sorted ascending) and its
  `default:` becomes the default, falling back to the written type
  text when the attribute has no `default:`; otherwise the written
  type text is the unversioned default (`VersionedTypeText::from_text`).
- A parameter's availability comes from
  `#[PhpStormStubsElementAvailable]` on the parameter (the existing
  `apply_element_available` logic, reachable by making
  `availability_of` work on the parameter node — it already walks
  `node.children()` for attribute groups, which holds for parameters).
- Member availability: `availability_of(member node)` (doc tags +
  attributes), unchanged machinery.
- Parents: `extends_clause()` names then `implements_clause()` names,
  each qualified with the declaring namespace unless `\`-prefixed
  (decision 7), leading backslash trimmed.
- Members: methods (signature + staticness + visibility from
  modifiers), properties (one member per element, `$` stripped, type
  text versioned), class constants (one per element, type text +
  literal value text via `ast::expression_text`), enum cases (value
  text). Private members are kept (visibility is stored; consumers
  filter).
- Enums: their `implements_clause` parents, plus implicit
  `UnitEnum`/`BackedEnum` are NOT synthesized (recorded: the checks
  that need them are plan 8's; revisit there).

- [ ] **Step 1: Write the failing tests**

Add to `extract.rs`'s test module:

```rust
use celerrate_project::PhpVersion;

use crate::signature::{StubMemberKind, StubVisibility, VersionedTypeText};

#[test]
fn a_function_signature_is_extracted_with_its_parameters() {
    let extraction = extract(
        "<?php\n\
         function strlen(string $string): int {}\n",
    );
    let (name, signature) = &extraction.functions[0];
    assert_eq!(name, "strlen");
    assert_eq!(signature.parameters.len(), 1);
    assert_eq!(signature.parameters[0].name, "string");
    assert_eq!(
        signature.parameters[0].type_text.at(PhpVersion::new(8, 1)),
        Some("string"),
    );
    assert!(!signature.parameters[0].optional);
    assert_eq!(signature.return_type.at(PhpVersion::new(8, 1)), Some("int"));
}

#[test]
fn language_level_type_aware_becomes_a_versioned_text() {
    let extraction = extract(
        "<?php\n\
         #[LanguageLevelTypeAware(['8.0' => 'int|false', '8.3' => 'int|float|false'], default: 'int')]\n\
         function tricky(): int {}\n",
    );
    let (_, signature) = &extraction.functions[0];
    assert_eq!(signature.return_type.default.as_deref(), Some("int"));
    assert_eq!(
        signature.return_type.overrides,
        vec![
            (PhpVersion::new(8, 0), "int|false".to_owned()),
            (PhpVersion::new(8, 3), "int|float|false".to_owned()),
        ],
    );
    assert_eq!(
        signature.return_type.at(PhpVersion::new(8, 4)),
        Some("int|float|false"),
    );
}

#[test]
fn a_parameter_gains_its_own_availability_window() {
    let extraction = extract(
        "<?php\n\
         function windowed(\n\
             string $always,\n\
             #[PhpStormStubsElementAvailable(from: '8.2')] int $added = 0,\n\
         ): void {}\n",
    );
    let (_, signature) = &extraction.functions[0];
    assert_eq!(
        signature.parameters[0].availability,
        crate::symbol::StubAvailability::ALWAYS,
    );
    assert_eq!(
        signature.parameters[1].availability.introduced,
        Some(PhpVersion::new(8, 2)),
    );
    assert!(signature.parameters[1].optional);
}

#[test]
fn a_class_surface_carries_parents_and_members() {
    let extraction = extract(
        "<?php\n\
         class RuntimeException extends Exception implements Stringable {\n\
             protected string $message;\n\
             const int CODE_LIMIT = 10;\n\
             public static function create(string $text): static {}\n\
             public function getMessage(): string {}\n\
         }\n",
    );
    let (name, surface) = &extraction.classes[0];
    assert_eq!(name, "RuntimeException");
    assert_eq!(
        surface.parents,
        vec!["Exception".to_owned(), "Stringable".to_owned()],
    );
    let member_names: Vec<(&str, StubMemberKind)> = surface
        .members
        .iter()
        .map(|member| (member.name.as_str(), member.kind))
        .collect();
    assert_eq!(
        member_names,
        vec![
            ("message", StubMemberKind::Property),
            ("CODE_LIMIT", StubMemberKind::ClassConstant),
            ("create", StubMemberKind::Method),
            ("getMessage", StubMemberKind::Method),
        ],
    );
    let message = &surface.members[0];
    assert_eq!(message.visibility, StubVisibility::Protected);
    assert_eq!(message.type_text.at(PhpVersion::new(8, 1)), Some("string"));
    let constant = &surface.members[1];
    assert_eq!(constant.value_text.as_deref(), Some("10"));
    let create = &surface.members[2];
    assert!(create.is_static);
}

#[test]
fn namespaced_parents_qualify_and_absolute_parents_do_not() {
    let extraction = extract(
        "<?php\n\
         namespace Random;\n\
         class BrokenRandomEngineError extends \\RuntimeException {}\n\
         class Local extends Engine {}\n",
    );
    assert_eq!(extraction.classes[0].1.parents, vec!["RuntimeException".to_owned()]);
    assert_eq!(extraction.classes[1].1.parents, vec!["Random\\Engine".to_owned()]);
}

#[test]
fn an_enum_surface_carries_its_cases() {
    let extraction = extract(
        "<?php\n\
         enum IntervalBoundary: string {\n\
             case ClosedOpen = 'CO';\n\
             case OpenClosed = 'OC';\n\
         }\n",
    );
    let (name, surface) = &extraction.classes[0];
    assert_eq!(name, "IntervalBoundary");
    assert_eq!(surface.members.len(), 2);
    assert_eq!(surface.members[0].kind, StubMemberKind::EnumCase);
    assert_eq!(surface.members[0].name, "ClosedOpen");
    assert_eq!(surface.members[0].value_text.as_deref(), Some("'CO'"));
}
```

(Adjust the two `Extraction` fields into every existing test that
builds or matches `Extraction` literally — most go through `extract`,
so only pattern matches on the struct need `..` added.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_stubs compiler`
Expected: FAIL to compile (`Extraction` has no `functions`/`classes`).

- [ ] **Step 3: Implement**

1. Extend `Extraction`:

```rust
pub struct Extraction {
    pub symbols: Vec<StubSymbol>,
    pub functions: Vec<(String, StubSignature)>,
    pub classes: Vec<(String, StubClassSurface)>,
    pub had_parse_errors: bool,
}
```

and thread a small `&mut Extraction`-like sink through `collect`
(simplest: change `collect`'s `symbols: &mut Vec<StubSymbol>`
parameter into the three sinks, or a
`struct Sink<'a> { symbols: …, functions: …, classes: … }`).

2. Function statements additionally push the signature:

```rust
ast::Statement::FunctionDeclaration(declaration) => {
    push_named(/* unchanged symbol push */);
    if let Some(name_token) = declaration.name_token() {
        sink.functions.push((
            qualify(&namespace, name_token.text()),
            stub_signature(
                declaration.parameter_list(),
                declaration.return_type(),
                declaration.by_reference_token().is_some(),
                declaration.syntax(),
            ),
        ));
    }
}
```

3. The shared signature builder:

```rust
fn stub_signature(
    parameters: Option<ast::ParameterList>,
    return_type: Option<ast::Type>,
    by_reference: bool,
    declaration_node: &SyntaxNode,
) -> StubSignature {
    StubSignature {
        parameters: parameters
            .into_iter()
            .flat_map(|list| list.parameters())
            .filter_map(|parameter| {
                let name = parameter.name_token()?;
                Some(StubParameter {
                    name: name.text().trim_start_matches('$').to_owned(),
                    type_text: versioned_type_text(
                        parameter.syntax(),
                        parameter.ty().map(|ty| ast::type_text(&ty)),
                    ),
                    optional: parameter.default_value().is_some(),
                    by_reference: parameter.by_reference_token().is_some(),
                    variadic: parameter.variadic_token().is_some(),
                    availability: availability_of(parameter.syntax()),
                })
            })
            .collect(),
        return_type: versioned_type_text(
            declaration_node,
            return_type.map(|ty| ast::type_text(&ty)),
        ),
        by_reference,
    }
}
```

(Note: `availability_of` on a parameter node works because
`apply_attributes` walks `node.children()` for `AttributeGroup`s, and
parameter attributes are children of the `Parameter` node. The
`doc_availability` half reads the nearest preceding doc comment — for
parameters that is the function's own docblock in pathological cases;
guard it by extracting an attributes-only variant:
`fn attribute_availability(node) -> StubAvailability` =
`let mut a = StubAvailability::ALWAYS; apply_attributes(node, &mut a); a`
and use it for parameters.)

4. The versioned-text reader:

```rust
/// `#[LanguageLevelTypeAware(['8.0' => '…'], default: '…')]` on the
/// node, else the written text unversioned.
fn versioned_type_text(node: &SyntaxNode, written: Option<String>) -> VersionedTypeText {
    for group in node.children().filter_map(ast::AttributeGroup::cast) {
        for attribute in group.attributes() {
            let Some(name) = attribute.name() else { continue };
            let name = name.text();
            let simple = name
                .trim_start_matches('\\')
                .rsplit('\\')
                .next()
                .unwrap_or_default()
                .to_owned();
            if !simple.eq_ignore_ascii_case("LanguageLevelTypeAware") {
                continue;
            }
            let Some(argument_list) = attribute.argument_list() else { continue };
            let mut overrides: Vec<(PhpVersion, String)> = Vec::new();
            let mut default = None;
            for argument in argument_list.arguments() {
                match argument.label_token().map(|token| token.text().to_owned()) {
                    Some(label) if label == "default" => {
                        default = argument
                            .expression()
                            .as_ref()
                            .and_then(string_literal);
                    }
                    None => {
                        if let Some(ast::Expression::ArrayExpression(array)) =
                            argument.expression()
                        {
                            for element in array.array_elements() {
                                let version = element
                                    .key()
                                    .as_ref()
                                    .and_then(string_literal)
                                    .as_deref()
                                    .and_then(parse_version);
                                let text = element
                                    .value()
                                    .as_ref()
                                    .and_then(string_literal);
                                if let (Some(version), Some(text)) = (version, text) {
                                    overrides.push((version, text));
                                }
                            }
                        }
                    }
                    Some(_) => {}
                }
            }
            overrides.sort_by_key(|(version, _)| *version);
            return VersionedTypeText {
                default: default.or(written),
                overrides,
            };
        }
    }
    VersionedTypeText::from_text(written)
}
```

5. Class-like statements build the surface. Factor one function used
by the `ClassDeclaration`, `InterfaceDeclaration`, `TraitDeclaration`,
and `EnumDeclaration` arms:

```rust
fn class_surface(
    namespace: &str,
    extends: Option<ast::ExtendsClause>,
    implements: Option<ast::ImplementsClause>,
    member_list: Option<ast::MemberList>,
) -> StubClassSurface {
    let mut parents = Vec::new();
    for name in extends.into_iter().flat_map(|clause| clause.names()) {
        parents.push(qualify_parent(namespace, &name.text()));
    }
    for name in implements.into_iter().flat_map(|clause| clause.names()) {
        parents.push(qualify_parent(namespace, &name.text()));
    }
    let mut members = Vec::new();
    for node in member_list
        .into_iter()
        .flat_map(|list| list.syntax().children())
    {
        extract_member(&node, &mut members);
    }
    StubClassSurface { parents, members }
}

/// Decision 7: absolute names trim the backslash, everything else
/// qualifies into the declaring namespace. Stub-file imports are not
/// consulted (recorded debt; phpstorm-stubs references are almost
/// always global or absolute).
fn qualify_parent(namespace: &str, written: &str) -> String {
    if let Some(absolute) = written.strip_prefix('\\') {
        absolute.to_owned()
    } else {
        qualify(namespace, written)
    }
}
```

with `extract_member` mirroring the shape of
`celerrate_semantics::members::lower_member` (match on
`node.kind()`; `SyntaxKind::MethodDeclaration` →
`StubMemberKind::Method` with `stub_signature(...)` and
staticness/visibility from `modifiers()` tokens;
`SyntaxKind::PropertyDeclaration` → one member per element,
`versioned_type_text(property node, written type)`;
`SyntaxKind::ConstantDeclaration` → one per element with
`value_text: element.value().map(|e| ast::expression_text(&e))`;
`SyntaxKind::EnumCase` → case with
`value_text: case.value().map(|e| ast::expression_text(&e))`). The
visibility mapping:

```rust
fn stub_flags(
    modifiers: impl Iterator<Item = SyntaxToken>,
) -> (StubVisibility, bool) {
    let mut visibility = StubVisibility::Public;
    let mut is_static = false;
    for token in modifiers {
        match token.kind() {
            SyntaxKind::Protected => visibility = StubVisibility::Protected,
            SyntaxKind::Private => visibility = StubVisibility::Private,
            SyntaxKind::Static => is_static = true,
            _ => {}
        }
    }
    (visibility, is_static)
}
```

6. The compiler's assembly point (`compiler/mod.rs` or wherever the
per-file `Extraction`s are folded into a `StubIndex`) switches from
`StubIndex::from_symbols(all_symbols)` to
`StubIndex::new(all_symbols, all_functions, all_classes)`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_stubs`
Expected: PASS — note the snapshot freshness test
(`the_committed_blob_matches_a_recompilation_of_the_pinned_snapshot`)
will now FAIL if the pinned snapshot is fetched locally, because the
committed `stubs.bin` predates the new sections. That failure is
expected and belongs to Task 12; if it fires here, proceed to Task 12
immediately after committing (the two tasks land in one push). If the
snapshot is not fetched, the test skips and the suite is green.
Clippy, fmt.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_stubs
git commit -m "✨ feat(stubs): compile signatures, class surfaces, and version deltas from the snapshot"
```

---
### Task 10: The stub class graph joins linearization and member lookup (`celerrate_semantics`)

"Stub ancestors are a recorded boundary until the stub signature
payload exists (plan 3)" — this task closes that boundary. Stub
ancestry becomes transitive (walked through the blob's parent links),
member lookup falls through to stub members, magic methods on stub
ancestors count, and the hierarchy judgment learns to tell "fully
walked, absent" (`Fails`) from "opaque" (`CannotProve`).

**Files:**
- Modify: `crates/celerrate_semantics/src/index.rs` (the folded
  signature table)
- Modify: `crates/celerrate_semantics/src/linearize.rs`
- Modify: `crates/celerrate_semantics/src/member_lookup.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs` (re-exports)
- Modify: `crates/celerrate_types/src/judgments.rs` (the hierarchy
  verdict upgrade + tests)

**Interfaces:**
- Consumes: Task 7's `StubSignature`/`StubClassSurface`/`StubMember`,
  `StubIndexInput`.
- Produces:

```rust
// index.rs — the folded consultation surface over the blob payloads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StubSignatureTable {
    functions: Vec<(String, StubSignature)>,   // folded Function keys, sorted
    classes: Vec<(String, StubClassSurface)>,  // folded ClassLike keys, sorted
}
impl StubSignatureTable {
    pub fn function(&self, key: &str) -> Option<&StubSignature>;
    pub fn class(&self, key: &str) -> Option<&StubClassSurface>;
}
#[salsa::tracked(returns(ref))]
pub fn stub_signature_table(db: &dyn salsa::Database, stubs: StubIndexInput) -> StubSignatureTable

// linearize.rs
pub struct AncestorEdge {
    …existing fields…,
    /// The folded key when the edge resolved to a stub class-like;
    /// `None` for source and unresolved edges. Exactly one of
    /// `resolved`/`stub` is `Some` on a resolved edge.
    pub stub: Option<String>,
}
pub struct LinearizedClass {
    …existing fields…,   // stub_ancestors becomes TRANSITIVE
    /// A genuinely opaque boundary remains: an unresolved edge, or a
    /// stub ancestor whose surface (or parent) is missing from the
    /// compiled payload. `false` means the hierarchy is fully walked.
    pub has_opaque_edge: bool,
}

// member_lookup.rs
pub enum MemberResolution {
    Source { member: Member, owner: String, origin: MemberOrigin },
    Stub { member: StubMember, owner: String },
}
```

`lookup_member` order: the source linearized table first; then, per
ancestry edge in walk order, the stub graph behind each stub edge
(breadth-first through `parents`, visited set); when the queried class
is not a source class-like at all, the stub graph from the class key
itself. Stub member matching folds with the same
`folded_member_key` rule (methods case-insensitive) and skips members
whose `availability` fails `exists_in(range)`.

- [ ] **Step 1: Write the failing tests**

`index.rs`:

```rust
#[test]
fn the_signature_table_folds_and_answers_by_key() {
    use celerrate_stubs::{StubClassSurface, StubIndex, StubIndexInput, StubSignature};

    let index = StubIndex::new(
        vec![],
        vec![("Str\\Len".to_owned(), StubSignature::default())],
        vec![("RuntimeException".to_owned(), StubClassSurface::default())],
    );
    let db = TestDatabase::default();
    let input = StubIndexInput::builder(index)
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let table = stub_signature_table(&db, input);
    assert!(table.function("str\\len").is_some(), "function keys fold");
    assert!(table.class("runtimeexception").is_some(), "class keys fold");
    assert!(table.class("RuntimeException").is_none(), "pre-folded keys only");
}
```

`linearize.rs` (extend the fixture builder to accept an optional
signature payload, or add a second builder
`fixture_with_stub_classes(sources, classes)` that passes
`StubIndex::new(default_symbols_plus(classes_as_symbols), vec![], classes)`;
every stub class named in a surface's parents must also be a
`StubSymbol` so `resolve_ancestor` finds it):

```rust
#[test]
fn stub_ancestry_walks_transitively_through_the_blob() {
    let fixture = fixture_with_stub_classes(
        &["<?php class MyError extends RuntimeException {}"],
        vec![
            ("RuntimeException".to_owned(), StubClassSurface {
                parents: vec!["Exception".to_owned()],
                members: vec![],
            }),
            ("Exception".to_owned(), StubClassSurface {
                parents: vec!["Throwable".to_owned()],
                members: vec![],
            }),
            ("Throwable".to_owned(), StubClassSurface::default()),
        ],
    );
    let table = linearize(&fixture, "MyError").unwrap();
    assert_eq!(
        table.stub_ancestors,
        vec!["exception".to_owned(), "runtimeexception".to_owned(), "throwable".to_owned()],
    );
    assert!(!table.has_opaque_edge, "fully walked");
    assert_eq!(table.ancestry[0].stub.as_deref(), Some("runtimeexception"));
}

#[test]
fn a_missing_stub_surface_is_an_opaque_edge() {
    // The symbol exists but the payload carries no surface for it
    // (the pre-plan-3 fixtures everywhere): the boundary is recorded.
    let fixture = fixture_one("<?php class MyException extends Exception {}");
    let table = linearize(&fixture, "MyException").unwrap();
    assert_eq!(table.stub_ancestors, vec!["exception".to_owned()]);
    assert!(table.has_opaque_edge);
}

#[test]
fn magic_methods_on_a_stub_ancestor_mark_the_class() {
    let fixture = fixture_with_stub_classes(
        &["<?php class Wrapper extends MagicBase {}"],
        vec![("MagicBase".to_owned(), StubClassSurface {
            parents: vec![],
            members: vec![StubMember {
                kind: StubMemberKind::Method,
                name: "__call".to_owned(),
                visibility: StubVisibility::Public,
                is_static: false,
                availability: StubAvailability::ALWAYS,
                signature: Some(StubSignature::default()),
                type_text: VersionedTypeText::default(),
                value_text: None,
            }],
        })],
    );
    let table = linearize(&fixture, "Wrapper").unwrap();
    assert!(table.magic.has_magic_call);
}
```

`member_lookup.rs`:

```rust
#[test]
fn a_source_class_inherits_stub_members_through_the_blob() {
    let fixture = fixture_with_stub_classes(
        &["<?php class MyError extends RuntimeException {}"],
        vec![
            ("RuntimeException".to_owned(), StubClassSurface {
                parents: vec!["Exception".to_owned()],
                members: vec![],
            }),
            ("Exception".to_owned(), StubClassSurface {
                parents: vec![],
                members: vec![get_message_member()], // a `getMessage(): string` method helper
            }),
        ],
    );
    let resolution = lookup(&fixture, "MyError", MemberKind::Method, "GETMESSAGE").unwrap();
    let MemberResolution::Stub { member, owner } = resolution else {
        panic!("expected a stub member");
    };
    assert_eq!(member.name, "getMessage");
    assert_eq!(owner, "exception");
}

#[test]
fn a_stub_only_class_answers_its_own_members() {
    let fixture = fixture_with_stub_classes(
        &["<?php"],
        vec![("Exception".to_owned(), StubClassSurface {
            parents: vec![],
            members: vec![get_message_member()],
        })],
    );
    assert!(lookup(&fixture, "Exception", MemberKind::Method, "getmessage").is_some());
    assert!(lookup(&fixture, "Exception", MemberKind::Method, "ghost").is_none());
}

#[test]
fn source_members_shadow_stub_members() {
    let fixture = fixture_with_stub_classes(
        &["<?php class MyError extends Exception { public function getMessage(): string {} }"],
        vec![("Exception".to_owned(), StubClassSurface {
            parents: vec![],
            members: vec![get_message_member()],
        })],
    );
    assert!(matches!(
        lookup(&fixture, "MyError", MemberKind::Method, "getmessage"),
        Some(MemberResolution::Source { .. }),
    ));
}

#[test]
fn a_member_outside_its_availability_window_is_absent() {
    // get_message_member() but introduced in 8.6, range is 8.1-8.5.
    …build the surface with availability.introduced = Some(PhpVersion::new(8, 6))…
    assert!(lookup(&fixture, "Exception", MemberKind::Method, "getmessage").is_none());
}
```

`celerrate_types/src/judgments.rs` (extend the fixture the same way):

```rust
#[test]
fn a_transitive_stub_hierarchy_proves_and_a_fully_walked_one_refutes() {
    let fixture = fixture_with_stub_classes(
        &["<?php class MyError extends RuntimeException {}"],
        …the RuntimeException → Exception → Throwable surfaces…,
    );
    let db = &fixture.db;
    let my_error = TypeId::class(db, "MyError", vec![]);
    let exception = TypeId::class(db, "Exception", vec![]);
    let countable = TypeId::class(db, "Countable", vec![]);
    assert_eq!(judge(&fixture, my_error, exception), Proof::Holds);
    // Fully walked and absent: refuted, no longer CannotProve.
    assert_eq!(judge(&fixture, my_error, countable), Proof::Fails);
}

#[test]
fn a_stub_only_candidate_judges_through_the_blob_graph() {
    let fixture = fixture_with_stub_classes(&["<?php"], …same surfaces…);
    let db = &fixture.db;
    let runtime = TypeId::class(db, "RuntimeException", vec![]);
    let throwable = TypeId::class(db, "Throwable", vec![]);
    assert_eq!(judge(&fixture, runtime, throwable), Proof::Holds);
}

#[test]
fn a_missing_stub_surface_stays_undecidable() {
    // The pre-existing fixtures carry no surfaces: pinned unchanged.
    …assert the existing boundary test still answers CannotProve…
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --workspace` — FAIL to compile (new fields, enum
reshape) and the new assertions fail.

- [ ] **Step 3: Implement**

1. `index.rs` — build the folded table once per input:

```rust
#[salsa::tracked(returns(ref))]
pub fn stub_signature_table(
    db: &dyn salsa::Database,
    stubs: StubIndexInput,
) -> StubSignatureTable {
    let index = stubs.index(db);
    let mut functions: Vec<(String, StubSignature)> = index
        .functions()
        .iter()
        .map(|(name, signature)| {
            (folded_symbol_key(SymbolSpace::Function, name), signature.clone())
        })
        .collect();
    functions.sort_by(|left, right| left.0.cmp(&right.0));
    let mut classes: Vec<(String, StubClassSurface)> = index
        .classes()
        .iter()
        .map(|(name, surface)| {
            (folded_symbol_key(SymbolSpace::ClassLike, name), surface.clone())
        })
        .collect();
    classes.sort_by(|left, right| left.0.cmp(&right.0));
    StubSignatureTable { functions, classes }
}
```

with binary-search accessors (`binary_search_by(|(key, _)|
key.as_str().cmp(key_argument))`). Re-export `StubSignatureTable`
and `stub_signature_table` from `lib.rs`.

2. `linearize.rs`:
- Every `AncestorEdge` literal gains
  `stub: answer.stub_key()` (add `fn stub_key(&self) -> Option<String>`
  on `AncestorAnswer` mirroring `source_key`).
- After the source walk, expand the stub frontier:

```rust
let mut has_opaque_edge = ancestry
    .iter()
    .any(|edge| edge.resolved.is_none() && edge.stub.is_none());
let table = stub_signature_table(db, stubs);
let mut stub_queue: VecDeque<String> =
    stub_ancestors.iter().cloned().collect(); // the direct stub edges, walk order
let mut stub_visited: HashSet<String> = HashSet::new();
let mut transitive: Vec<String> = Vec::new();
while let Some(key) = stub_queue.pop_front() {
    if !stub_visited.insert(key.clone()) {
        continue;
    }
    let Some(surface) = table.class(&key) else {
        // A stub symbol without a compiled surface: opaque.
        has_opaque_edge = true;
        transitive.push(key);
        continue;
    };
    for member in &surface.members {
        merge_stub_magic(member, &mut magic_from_stubs);
    }
    for parent in &surface.parents {
        stub_queue.push_back(folded_symbol_key(SymbolSpace::ClassLike, parent));
    }
    transitive.push(key);
}
let mut stub_ancestors = transitive;
stub_ancestors.sort();
stub_ancestors.dedup();
```

(keep the direct-edge pushes into a walk-order `stub_ancestors`
before this block, exactly as today; the block consumes and replaces
it). `merge_stub_magic` sets `has_magic_get/set/call/callstatic` on a
small accumulator merged into `magic_markers`'s result — compare the
member's folded method key (`member.kind == StubMemberKind::Method &&
member.name.to_ascii_lowercase() == "__call"` and friends).
- Add `has_opaque_edge` to the returned `LinearizedClass` and to the
  struct definition (update every literal in tests: source-only
  fixtures with no stub edges get `false`; the old
  `a_stub_ancestor_is_a_recorded_boundary` fixture — no surfaces —
  now asserts `has_opaque_edge` instead of relying on
  `resolved.is_none()`).

3. `member_lookup.rs` — the enum and the fallthrough:

```rust
#[salsa::tracked]
pub fn lookup_member<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MemberQuery<'db>,
) -> Option<MemberResolution> {
    let class = ClassQuery::new(db, query.class_key(db).clone());
    let kind = query.kind(db);
    let key = query.member_key(db);
    let range = configuration.php_version_range(db);
    let table = stub_signature_table(db, stubs);
    match linearized_class(db, files, stubs, configuration, class).as_ref() {
        Some(linearized) => {
            if let Some(entry) = linearized
                .members
                .iter()
                .find(|entry| entry.member.kind == kind && entry.key == *key)
            {
                return Some(MemberResolution::Source {
                    member: entry.member.clone(),
                    owner: entry.owner.clone(),
                    origin: entry.origin,
                });
            }
            // Fall through to the stub graph behind each stub edge,
            // in walk order.
            for edge in &linearized.ancestry {
                if let Some(stub_key) = &edge.stub
                    && let Some(found) =
                        stub_member(table, range, stub_key, kind, key)
                {
                    return Some(found);
                }
            }
            None
        }
        // Not a source class-like: the stub graph from the key itself.
        None => stub_member(table, range, query.class_key(db), kind, key),
    }
}

/// Breadth-first over the blob's parent links from `start`, kind and
/// key folded like source members, availability-filtered.
fn stub_member(
    table: &StubSignatureTable,
    range: PhpVersionRange,
    start: &str,
    kind: MemberKind,
    key: &str,
) -> Option<MemberResolution> {
    let mut queue: VecDeque<String> = VecDeque::new();
    let mut visited: HashSet<String> = HashSet::new();
    queue.push_back(start.to_owned());
    while let Some(class_key) = queue.pop_front() {
        if !visited.insert(class_key.clone()) {
            continue;
        }
        let Some(surface) = table.class(&class_key) else {
            continue;
        };
        for member in &surface.members {
            if member_kind_of(member.kind) == kind
                && folded_member_key(kind, &member.name) == key
                && member.availability.exists_in(range)
            {
                return Some(MemberResolution::Stub {
                    member: member.clone(),
                    owner: class_key,
                });
            }
        }
        for parent in &surface.parents {
            queue.push_back(folded_symbol_key(SymbolSpace::ClassLike, parent));
        }
    }
    None
}

const fn member_kind_of(kind: StubMemberKind) -> MemberKind {
    match kind {
        StubMemberKind::Method => MemberKind::Method,
        StubMemberKind::Property => MemberKind::Property,
        StubMemberKind::ClassConstant => MemberKind::ClassConstant,
        StubMemberKind::EnumCase => MemberKind::EnumCase,
    }
}
```

4. `celerrate_types/src/judgments.rs` — `judge_class_hierarchy`:

```rust
let Some(linearized) = linearized_class(…) else {
    // Not a source class-like: judge through the stub graph.
    return judge_stub_hierarchy(db, context, candidate_name, target_name);
};
let found = …existing ancestry/stub_ancestors check…;
if found {
    return Proof::Holds;
}
if linearized.cyclic || linearized.has_opaque_edge {
    Proof::CannotProve
} else {
    Proof::Fails
}
```

with:

```rust
/// The stub-graph verdict for a candidate with no source declaration:
/// breadth-first over the compiled parent links. A key whose surface
/// is missing keeps the answer undecidable; a fully walked graph
/// without the target refutes.
fn judge_stub_hierarchy(
    db: &dyn salsa::Database,
    context: JudgmentContext,
    candidate_name: &str,
    target_name: &str,
) -> Proof {
    let table = celerrate_semantics::stub_signature_table(db, context.stubs);
    if table.class(candidate_name).is_none() {
        // Unknown class: undecidable, as before.
        return Proof::CannotProve;
    }
    let mut queue: std::collections::VecDeque<String> = Default::default();
    let mut visited: std::collections::HashSet<String> = Default::default();
    let mut opaque = false;
    queue.push_back(candidate_name.to_owned());
    while let Some(key) = queue.pop_front() {
        if !visited.insert(key.clone()) {
            continue;
        }
        if key == target_name {
            return Proof::Holds;
        }
        let Some(surface) = table.class(&key) else {
            opaque = true;
            continue;
        };
        for parent in &surface.parents {
            queue.push_back(celerrate_semantics::folded_symbol_key(
                celerrate_semantics::SymbolSpace::ClassLike,
                parent,
            ));
        }
    }
    if opaque { Proof::CannotProve } else { Proof::Fails }
}
```

5. Update Task 4's `declared_member_signature` for the enum: the
`MemberResolution::Source` arm keeps the existing body; the `Stub`
arm returns `None` **with a `// Task 11 fills the stub arm` note** —
this task leaves types compiling and green.

6. Existing-test sweep: `member_lookup` destructures update;
`a_stub_only_class_answers_none_here` in `lookup.rs` keeps its name
and meaning (it pins `lookup_class_declaration`, which is still
source-only — only its comment about "plan 3" is now stale; refresh
the comment). Every `LinearizedClass` literal in tests gains
`has_opaque_edge`; every `AncestorEdge` literal gains `stub`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace` — PASS; clippy; fmt.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_semantics crates/celerrate_types
git commit -m "✨ feat(semantics): the stub class graph joins linearization, member lookup, and the hierarchy verdict"
```

---

### Task 11: Stub declared signatures under the `[min, max]` range rule

The consultation rule of the parent spec's section 2, with the
design's degenerate-case guard (decision 6): parameters check against
the most restrictive form across the range or fall silent; returns are
the union; a parameter absent somewhere in the range is optional.

**Files:**
- Modify: `crates/celerrate_types/src/declared.rs`

**Interfaces:**
- Consumes: `celerrate_semantics::stub_signature_table`, Task 7's
  `VersionedTypeText::at`, `celerrate_project::SUPPORTED_VERSIONS`,
  Task 2's `NameSite::Global` lowering, Task 10's
  `MemberResolution::Stub`.
- Produces: the stub arms of `declared_member_signature` and
  `declared_function_signature`; crate-internal helpers:

```rust
fn versions_in_range(range: PhpVersionRange) -> Vec<PhpVersion>;  // SUPPORTED_VERSIONS ∩ [min, max]
fn parameter_type_across_range<'db>(db, files, stubs, configuration, range, text: &VersionedTypeText) -> Option<TypeId<'db>>;
fn value_type_across_range<'db>(db, range, text: &VersionedTypeText) -> TypeId<'db>;  // the union
```

- [ ] **Step 1: Write the failing tests**

Extend `declared.rs`'s test module with a stub-payload fixture
builder (`fixture_with_stub_payload(sources, functions, classes)`
mirroring Task 10's — symbols synthesized for every named function
and class, plus the payloads):

```rust
#[test]
fn a_stub_function_resolves_with_union_returns_across_the_range() {
    let strlen = StubSignature {
        parameters: vec![stub_parameter("string", Some("string"))],
        return_type: VersionedTypeText {
            default: Some("int".to_owned()),
            overrides: vec![(PhpVersion::new(8, 3), "int|false".to_owned())],
        },
        by_reference: false,
    };
    let fixture = fixture_with_stub_payload(
        &["<?php"],
        vec![("strlen".to_owned(), strlen)],
        vec![],
    );
    let db = &fixture.db;
    let query = FunctionQuery::new(db, folded_symbol_key(SymbolSpace::Function, "strlen"));
    let signature = declared_function_signature(
        db, fixture.files, fixture.stubs, fixture.configuration, query,
    )
    .unwrap();
    assert_eq!(signature.parameters[0].parameter_type, Some(TypeId::string(db)));
    // Union across 8.1..8.5: int at 8.1-8.2, int|false at 8.3+.
    assert_eq!(
        signature.value_type,
        TypeId::union(db, [TypeId::int(db), TypeId::bool_literal(db, false)]),
    );
}

#[test]
fn a_parameter_narrowing_across_the_range_takes_the_most_restrictive_form() {
    // string at 8.1, non-empty-string-like narrowing is not writable
    // natively; use int at 8.1 versus int|string at 8.2+: `int` is a
    // subtype of every per-version type, so `int` is the check type.
    let signature = StubSignature {
        parameters: vec![StubParameter {
            name: "value".to_owned(),
            type_text: VersionedTypeText {
                default: Some("int".to_owned()),
                overrides: vec![(PhpVersion::new(8, 2), "int|string".to_owned())],
            },
            optional: false,
            by_reference: false,
            variadic: false,
            availability: StubAvailability::ALWAYS,
        }],
        return_type: VersionedTypeText::from_text(Some("void".to_owned())),
        by_reference: false,
    };
    let fixture = fixture_with_stub_payload(
        &["<?php"], vec![("narrowing".to_owned(), signature)], vec![],
    );
    let db = &fixture.db;
    let declared = declared_function_signature(
        db, fixture.files, fixture.stubs, fixture.configuration,
        FunctionQuery::new(db, folded_symbol_key(SymbolSpace::Function, "narrowing")),
    )
    .unwrap();
    assert_eq!(declared.parameters[0].parameter_type, Some(TypeId::int(db)));
}

#[test]
fn a_disjoint_parameter_across_the_range_is_silenced() {
    // int at 8.1, string from 8.2: no most-restrictive form exists —
    // the design's degenerate guard silences the parameter.
    …same shape with overrides = [(8.2, "string")]…
    assert_eq!(declared.parameters[0].parameter_type, None);
    assert!(!declared.parameters[0].optional, "silencing is type-only");
}

#[test]
fn a_parameter_added_inside_the_range_is_optional() {
    …stub_parameter with availability.introduced = Some(PhpVersion::new(8, 3))…
    assert!(declared.parameters[0].optional);
}

#[test]
fn stub_member_signatures_resolve_through_the_same_rule() {
    let fixture = fixture_with_stub_payload(
        &["<?php class MyError extends Exception {}"],
        vec![],
        vec![("Exception".to_owned(), StubClassSurface {
            parents: vec![],
            members: vec![get_message_member()], // getMessage(): string
        })],
    );
    let db = &fixture.db;
    let signature = member(&fixture, "MyError", MemberKind::Method, "getMessage").unwrap();
    assert_eq!(signature.value_type, TypeId::string(db));
    assert_eq!(signature.value_trust, Trust::NativeOnly);
    // Stub types resolve in the global context.
    let direct = member(&fixture, "Exception", MemberKind::Method, "getmessage").unwrap();
    assert_eq!(direct.value_type, TypeId::string(db));
}

#[test]
fn a_point_range_never_silences() {
    // With min == max the "range" is one version: every parameter has
    // exactly one form. Build the disjoint fixture but configure
    // PhpVersionRange::point(PhpVersion::new(8, 4)) and assert the
    // parameter type is Some(string).
    …
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types declared` — FAIL (the stub
arm answers `None`; helpers missing).

- [ ] **Step 3: Implement**

```rust
use celerrate_project::{PhpVersion, PhpVersionRange, SUPPORTED_VERSIONS};
use celerrate_stubs::{StubMember, StubMemberKind, StubParameter, StubSignature, VersionedTypeText};

/// The supported versions inside the configured range, ascending.
fn versions_in_range(range: PhpVersionRange) -> Vec<PhpVersion> {
    SUPPORTED_VERSIONS
        .iter()
        .copied()
        .filter(|version| *version >= range.minimum && *version <= range.maximum)
        .collect()
}

/// The union across the range: the least restrictive reading of a
/// call's result (parent spec section 2). A version with no declared
/// text contributes `mixed`.
fn value_type_across_range<'db>(
    db: &'db dyn salsa::Database,
    range: PhpVersionRange,
    text: &VersionedTypeText,
) -> TypeId<'db> {
    TypeId::union(
        db,
        versions_in_range(range).into_iter().map(|version| {
            text.at(version)
                .and_then(|written| lower_written_text(db, &NameSite::Global, written))
                .unwrap_or_else(|| TypeId::mixed(db))
        }),
    )
}

/// The most restrictive form across the range, or silence (decision
/// 6): all per-version types equal → that type; one of them a proven
/// subtype of every other → that one; otherwise `None` — the empty
/// intersection silences the check instead of weaponizing `never`.
fn parameter_type_across_range<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    range: PhpVersionRange,
    text: &VersionedTypeText,
) -> Option<TypeId<'db>> {
    let mut types: Vec<TypeId<'db>> = versions_in_range(range)
        .into_iter()
        .map(|version| {
            text.at(version)
                .and_then(|written| lower_written_text(db, &NameSite::Global, written))
                .unwrap_or_else(|| TypeId::mixed(db))
        })
        .collect();
    types.dedup();
    let mut distinct = types;
    distinct.sort_by_key(|type_id| type_id.display(db));
    distinct.dedup();
    match distinct.as_slice() {
        [] => Some(TypeId::mixed(db)),
        [single] => Some(*single),
        several => several
            .iter()
            .copied()
            .find(|candidate| {
                several.iter().all(|other| {
                    *other == *candidate
                        || crate::judgments::subtype_of(
                            db, files, stubs, configuration, *candidate, *other,
                        ) == crate::judgments::Proof::Holds
                })
            }),
    }
}
```

(The sort by rendered form before `dedup` makes the winner independent
of override order; `display` is deterministic. Two candidates that are
each subtypes of all others are equal types, so "first found after the
sort" is stable.)

Wire the arms:

```rust
// declared_member_signature, the Stub arm:
MemberResolution::Stub { member, owner } => {
    let range = configuration.php_version_range(db);
    Some(resolve_stub_member_signature(
        db, files, stubs, configuration, range, &owner, &member,
    ))
}
```

```rust
fn resolve_stub_member_signature<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    range: PhpVersionRange,
    owner_key: &str,
    member: &StubMember,
) -> DeclaredSignature<'db> {
    let value_type = match member.kind {
        StubMemberKind::EnumCase => TypeId::enum_case(db, owner_key, &member.name),
        StubMemberKind::ClassConstant => {
            if member.type_text == VersionedTypeText::default() {
                member
                    .value_text
                    .as_deref()
                    .and_then(|text| literal_type_of_default(db, text))
                    .unwrap_or_else(|| TypeId::mixed(db))
            } else {
                value_type_across_range(db, range, &member.type_text)
            }
        }
        StubMemberKind::Method => member
            .signature
            .as_ref()
            .map(|signature| value_type_across_range(db, range, &signature.return_type))
            .unwrap_or_else(|| TypeId::mixed(db)),
        StubMemberKind::Property => value_type_across_range(db, range, &member.type_text),
    };
    let parameters = member
        .signature
        .as_ref()
        .map(|signature| {
            signature
                .parameters
                .iter()
                .map(|parameter| declared_stub_parameter(
                    db, files, stubs, configuration, range, parameter,
                ))
                .collect()
        })
        .unwrap_or_default();
    DeclaredSignature {
        parameters,
        value_type,
        value_trust: Trust::NativeOnly,
        by_reference: member
            .signature
            .as_ref()
            .is_some_and(|signature| signature.by_reference),
    }
}

fn declared_stub_parameter<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    range: PhpVersionRange,
    parameter: &StubParameter,
) -> DeclaredParameter<'db> {
    // A parameter that does not span the whole range is optional: a
    // call omitting it must be legal somewhere in the range, so arity
    // never over-reports (parameter added in 8.2, minimum 8.1).
    let spans_the_whole_range = parameter.availability.introduced.is_none_or(|v| v <= range.minimum)
        && parameter.availability.removed.is_none_or(|v| v > range.maximum);
    DeclaredParameter {
        name: parameter.name.clone(),
        parameter_type: parameter_type_across_range(
            db, files, stubs, configuration, range, &parameter.type_text,
        ),
        trust: Trust::NativeOnly,
        optional: parameter.optional || !spans_the_whole_range,
        variadic: parameter.variadic,
        by_reference: parameter.by_reference,
    }
}
```

`declared_function_signature` gains its stub arm before answering
`None`: when `lookup_function_declaration` misses, consult
`celerrate_semantics::stub_signature_table(db, stubs).function(query.key(db))`
and resolve through `resolve_stub_member_signature`'s parameter/return
helpers (factor a
`fn resolve_stub_signature(db, files, stubs, configuration, range, signature: &StubSignature) -> DeclaredSignature`
shared by methods and functions).

Untyped stub parameters (`VersionedTypeText::default()`) come out as
`Some(mixed)` through `value_type_across_range`'s `unwrap_or(mixed)`
path inside `parameter_type_across_range` — pin that with an
assertion in the first test (add an untyped parameter).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace` — PASS; clippy; fmt.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): stub signatures consult under the range rule with the degenerate-case guard"
```

---

### Task 12: Recompile the blob, spot-check the corpus surface, record the size

The committed `stubs.bin` gains the signature section; the embedded
blob is then spot-checked from the test suite (the committed blob
needs no snapshot, so these tests run everywhere).

**Files:**
- Modify: `crates/celerrate_stubs/src/stubs.bin` (generated)
- Modify: `crates/celerrate_stubs/src/lib.rs` (embedded-blob tests)

- [ ] **Step 1: Write the failing tests**

In `lib.rs`'s test module (alongside any existing embedded-blob
tests):

```rust
#[test]
fn the_embedded_blob_carries_the_signature_payload() {
    let index = crate::blob::decode(crate::EMBEDDED_STUB_BLOB).unwrap();
    assert!(!index.functions().is_empty(), "functions compiled");
    assert!(!index.classes().is_empty(), "class surfaces compiled");
}

#[test]
fn corpus_spot_checks_hold_on_the_embedded_blob() {
    use celerrate_project::PhpVersion;
    let index = crate::blob::decode(crate::EMBEDDED_STUB_BLOB).unwrap();
    let function = |name: &str| {
        index
            .functions()
            .iter()
            .find(|(function_name, _)| function_name == name)
            .map(|(_, signature)| signature)
    };
    // strlen(string $string): int
    let strlen = function("strlen").unwrap();
    assert_eq!(
        strlen.parameters[0].type_text.at(PhpVersion::new(8, 1)),
        Some("string"),
    );
    assert_eq!(strlen.return_type.at(PhpVersion::new(8, 1)), Some("int"));
    // preg_match's $matches is by reference.
    let preg_match = function("preg_match").unwrap();
    let matches = preg_match
        .parameters
        .iter()
        .find(|parameter| parameter.name == "matches")
        .unwrap();
    assert!(matches.by_reference);
    // Exception::getMessage(): string, and the parent link to Throwable.
    let (_, exception) = index
        .classes()
        .iter()
        .find(|(name, _)| name == "Exception")
        .unwrap();
    assert!(exception.parents.iter().any(|parent| parent == "Throwable"));
    let get_message = exception
        .members
        .iter()
        .find(|member| member.name == "getMessage")
        .unwrap();
    assert_eq!(
        get_message
            .signature
            .as_ref()
            .unwrap()
            .return_type
            .at(PhpVersion::new(8, 1)),
        Some("string"),
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_stubs the_embedded_blob`
Expected: FAIL — the committed blob has no signature section yet
(empty payloads).

- [ ] **Step 3: Recompile**

Run: `cargo xtask compile-stubs`
(fetches the pinned snapshot into `target/phpstorm-stubs/` if absent,
then rewrites `crates/celerrate_stubs/src/stubs.bin`).
Then record the size delta:

Run: `ls -la crates/celerrate_stubs/src/stubs.bin`
The previous size was 379.1 KB. Record the new size in the commit
message body. If the new blob exceeds **8 MB**, stop and reconsider
scope with a note in the plan (likely culprit: value texts or private
members — both are droppable); anything under that is accepted for an
embedded, zero-startup index.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS — including the snapshot freshness test (now green
against the recompiled blob), the embedded spot checks, and the
`celerrate_cli` cache tests (the pack's `stub_blob` hash changed with
the blob; nothing pins the old hash literally). If a spot check fails,
the extractor missed a shape — fix the extractor (Task 9), recompile,
re-run; never weaken the assertion to match the blob.
Clippy, fmt.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_stubs
git commit -m "✨ feat(stubs): recompile the embedded blob with the signature payload

stubs.bin size: <old> -> <new>."
```

---
### Task 13: `value-of` over backed enums (the plan-2 debt)

Plan 2 recorded: "`value_of` on enums needs member facts and stays
symbolic until plan 3" (`construction.rs:624-626`). The member facts
now exist (source enum cases carry their backing literal in
`default_text`; stub enum cases in `value_text`). The judgment layer
expands `value-of<SomeEnum>` to the union of its case backing
literals; anything non-literal or unresolvable stays symbolic.

**Files:**
- Modify: `crates/celerrate_types/src/declared.rs` (the evaluator)
- Modify: `crates/celerrate_types/src/judgments.rs` (the expansion at
  judge entry)

**Interfaces:**
- Consumes: `linearized_class` (source enums),
  `stub_signature_table` (stub enums), `literal_type_of_default`
  (Task 4), `TypeId::{value_of, union}`, `MemberKind::EnumCase`.
- Produces:

```rust
/// The union of a backed enum's case backing literals, or `None` when
/// the key is not a fully known backed enum (unresolvable class, a
/// case with a non-literal backing, a pure enum, an opaque hierarchy).
pub(crate) fn enum_backing_union<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    enum_key: &str,
) -> Option<TypeId<'db>>
```

- [ ] **Step 1: Write the failing tests**

In `judgments.rs`'s test module:

```rust
#[test]
fn value_of_a_backed_enum_expands_to_its_literal_union() {
    let fixture = fixture(&[
        "<?php enum Status: string {\n\
             case Active = 'active';\n\
             case Retired = 'retired';\n\
         }",
    ]);
    let db = &fixture.db;
    let value_of_status = TypeId::value_of(db, TypeId::class(db, "Status", vec![]));
    let literals = TypeId::union(
        db,
        [
            TypeId::string_literal(db, "active"),
            TypeId::string_literal(db, "retired"),
        ],
    );
    assert_eq!(judge(&fixture, value_of_status, literals), Proof::Holds);
    assert_eq!(
        judge(&fixture, TypeId::string_literal(db, "active"), value_of_status),
        Proof::Holds,
    );
    assert_eq!(
        judge(&fixture, TypeId::string_literal(db, "ghost"), value_of_status),
        Proof::Fails,
    );
}

#[test]
fn value_of_a_pure_or_unknown_enum_stays_symbolic() {
    let fixture = fixture(&["<?php enum Suit { case Hearts; }"]);
    let db = &fixture.db;
    let value_of_suit = TypeId::value_of(db, TypeId::class(db, "Suit", vec![]));
    // No backing values: undecidable, exactly as before this task.
    assert_eq!(
        judge(&fixture, value_of_suit, TypeId::string(db)),
        Proof::CannotProve,
    );
    let value_of_ghost = TypeId::value_of(db, TypeId::class(db, "Ghost", vec![]));
    assert_eq!(
        judge(&fixture, value_of_ghost, TypeId::string(db)),
        Proof::CannotProve,
    );
}
```

(If the second test's "exactly as before" expectation does not match
the shipped symbolic-`ValueOf` verdict — check what `judge_ground`
answers for a symbolic `ValueOf` against `string` today and pin THAT
value; the point is that non-expandable stays unchanged.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types judgments` — the first test
FAILS (symbolic `ValueOf` never proves against the literal union).

- [ ] **Step 3: Implement**

In `declared.rs`:

```rust
pub(crate) fn enum_backing_union<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    enum_key: &str,
) -> Option<TypeId<'db>> {
    let class = celerrate_semantics::ClassQuery::new(db, enum_key.to_owned());
    let mut literals: Vec<TypeId<'db>> = Vec::new();
    match celerrate_semantics::linearized_class(db, files, stubs, configuration, class)
        .as_ref()
    {
        Some(linearized) => {
            let cases = linearized
                .members
                .iter()
                .filter(|entry| entry.member.kind == MemberKind::EnumCase);
            for entry in cases {
                let backing = entry.member.signature.default_text.as_deref()?;
                literals.push(literal_type_of_default(db, backing)?);
            }
        }
        None => {
            let table = celerrate_semantics::stub_signature_table(db, stubs);
            let surface = table.class(enum_key)?;
            let cases = surface
                .members
                .iter()
                .filter(|member| member.kind == celerrate_stubs::StubMemberKind::EnumCase);
            for member in cases {
                let backing = member.value_text.as_deref()?;
                literals.push(literal_type_of_default(db, backing)?);
            }
        }
    }
    if literals.is_empty() {
        return None; // a pure enum, or no cases at all
    }
    Some(TypeId::union(db, literals))
}
```

In `judgments.rs`, at the top of `judge` (before the extremes):

```rust
let candidate = expand_value_of(db, context, candidate);
let target = expand_value_of(db, context, target);
```

```rust
/// `value-of<SomeBackedEnum>` evaluates through member facts (plan
/// 2's recorded debt, settled here); every other `value-of` stays
/// symbolic. Top-level only: nested occurrences keep their symbolic
/// (conservative) verdicts.
fn expand_value_of<'db>(
    db: &'db dyn salsa::Database,
    context: JudgmentContext,
    of: TypeId<'db>,
) -> TypeId<'db> {
    let TypeData::ValueOf { subject } = of.data(db) else {
        return of;
    };
    let TypeData::Class { name, .. } = subject.data(db) else {
        return of;
    };
    crate::declared::enum_backing_union(
        db,
        context.files,
        context.stubs,
        context.configuration,
        name,
    )
    .unwrap_or(of)
}
```

Guard against infinite recursion: `judge` recurses on constituents,
and the expansion happens per `judge` entry — an expanded union's
constituents are literals, never `ValueOf`, so the recursion
terminates structurally.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace` — PASS; clippy; fmt.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): value-of over backed enums evaluates through member facts"
```

---

### Task 14: Invalidation scope and determinism over the declared layer

The edit-class guarantees this plan advertises, pinned with
`db.take_executed()` exactly like the existing
`celerrate_types/tests/invalidation_scope.rs`.

**Files:**
- Modify: `crates/celerrate_types/tests/invalidation_scope.rs`

**Interfaces:**
- Consumes: everything shipped by tasks 1-13;
  `celerrate_db::testing::TestDatabase` (`take_executed`),
  `salsa::Setter` (`file.set_bytes(&mut db).to(…)`).

**The pinned edit classes:**
1. A **method body edit** never re-runs `declared_member_signature`
   (the member tree is body-blind).
2. A **docblock prose edit** re-runs `declared_member_signature` (the
   member payload carries the docblock — accepted, by design) but its
   result is identical, so a downstream consumer (a `subtype_of`
   verdict against the declared return) is spared — the early cutoff
   this plan's dependents live on.
3. A **signature edit** (return type `int` → `string`) re-runs the
   query and changes the answer.
4. A **default-value edit** (`= null` → `= 1`) invalidates the
   signature's dependents (implicit nullability is part of the
   projection — design section 2's pinned requirement, extended to
   the declared level).
5. An **unrelated member's signature edit** in the same class re-runs
   the linearization but leaves this member's declared signature
   backdated, sparing its dependents.

- [ ] **Step 1: Write the failing tests**

Follow the existing file's fixture shape (inputs built inline,
`take_executed` drained before the probe, executed-query names
asserted by `.contains("declared_member_signature")`). Representative
skeleton for (1) and (2) — write all five:

```rust
#[test]
fn a_body_edit_never_recomputes_a_declared_signature() {
    // …build db, one file:
    // "<?php class C { public function f(): int { return 1; } }"
    // …resolve the declared signature once, drain take_executed…
    file.set_bytes(&mut db)
        .to(b"<?php class C { public function f(): int { return 2; } }".to_vec());
    let _ = declared_member_signature(&db, files, stubs, configuration, query);
    let executed = db.take_executed();
    assert!(
        !executed.iter().any(|name| name.contains("declared_member_signature")),
        "a body edit must not reach the declared layer: {executed:?}",
    );
}

#[test]
fn a_docblock_prose_edit_recomputes_but_dependents_are_spared() {
    // …same shape; the edit adds "/** prose */" above the method;
    // probe a subtype_of(declared return, int) verdict instead:
    // declared_member_signature re-runs (assert it did), the verdict
    // query does not (assert no "subtype_of" in executed).
}
```

For (4), probe `parameters[0].parameter_type` nullability before and
after, and assert the dependent verdict DID re-run. For (5), two
methods in one class; edit `g`'s return type; assert
`declared_member_signature` re-ran (the linearized table changed) but
a verdict depending on `f`'s declared signature did not.

Note on (5): whether `declared_member_signature(f)` itself re-executes
depends on `lookup_member(f)` backdating — the member firewall already
guarantees the lookup backdates; assert at minimum that the
**dependent verdict** is spared, and additionally assert
`declared_member_signature` backdated if the firewall makes it so
(check what `take_executed` reports and pin the stronger true fact).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types --test invalidation_scope`
Expected: the *assertions* pass or fail depending on wiring mistakes —
the red state here is any failing pin. If all five pass immediately,
verify each probe actually executes (drain-and-assert on the first
computation) so the tests cannot pass vacuously, then treat this step
as green.

- [ ] **Step 3: Fix whatever a failing pin reveals**

The likely culprits, in order: `declared_member_signature` reading a
whole `member_tree` instead of going through `lookup_member`
(dependency too wide), `UseTables::for_namespace(item_tree(…))`
making the query depend on the item tree (unavoidable and fine — the
item tree is name-level and cuts off on unrelated edits; do NOT
"fix" this), or a fixture mistake. Adjust production code only if a
pin exposes a genuinely too-wide dependency.

- [ ] **Step 4: Run the whole gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_types/tests/invalidation_scope.rs
git commit -m "✅ test(types): pin the declared layer's invalidation scope over the new edit classes"
```

---

### Task 15: Closure — documentation, re-export audit, the debt ledger

**Files:**
- Modify: `crates/celerrate_types/src/lib.rs` (module documentation)
- Modify: `crates/celerrate_stubs/src/lib.rs` (module documentation)
- Modify: `.claude/superpowers/plans/2026-07-14-type-engine-3-declared.md`
  (the debt ledger appended at the bottom)

- [ ] **Step 1: Documentation and the re-export audit**

`celerrate_types/src/lib.rs` module doc gains a "Declared types"
paragraph: the per-member/per-function queries, the source-precedence
rule and `Trust` trace, the annotation seam (plan 4a fills it), the
range rule and its silencing guard, and the bare-`callable` widening
debt. Verify the final public surface is exactly:

```rust
pub use declared::{
    DeclaredParameter, DeclaredSignature, FunctionQuery, MemberAnnotations,
    Trust, declared_function_signature, declared_member_signature,
    member_annotations,
};
pub use judgments::{Nullability, Proof, assignable_to, nullability, subtype_of};
pub use representation::{CallableParameter, FloatBits, ShapeField, ShapeKey, TypeId};
pub use widening::{STRUCTURAL_DEPTH_CAP, UNION_ARITY_CAP, join, widened_literals};
```

`celerrate_stubs/src/lib.rs` module doc: the signature payload is
live (section 2 of the blob), what it stores, and the version-delta
consultation contract (`at`, the range rule living upstairs in
`celerrate_types`).

- [ ] **Step 2: Write the debt ledger**

Append to this plan file a section `## Accepted debt at closure`
listing at minimum (plus whatever execution surfaced):
- Bare `callable` lowers to `mixed` (decision 3) — plan 8 measures.
- Stub parent names ignore stub-file `use` imports (decision 7).
- Duplicate stub declarations keep the first signature after the sort
  (Task 7) rather than merging.
- Function annotations have no seam yet (plan 4 adds it when the
  bridge parses function docblocks).
- Implicit `UnitEnum`/`BackedEnum` parents are not synthesized for
  stub enums (Task 9) — plan 8 revisits with the interface checks.
- `value-of` expansion is top-level only (Task 13).
- Stub member flags beyond visibility/static are not compiled.

- [ ] **Step 3: The full gate, one last time**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo deny check`
Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_types/src/lib.rs crates/celerrate_stubs/src/lib.rs .claude/superpowers/plans/2026-07-14-type-engine-3-declared.md
git commit -m "📝 docs(types): the declared-type layer, its seam, and the closure debt ledger"
```

---

## Verification against the design (for the final reviewer)

- Design §3 "Native declared types … per-member queries" → Tasks 1, 2,
  4. "Shortest path from the member boundary to a useful judgment."
- Design §3 "The stub signature payload … per-version deltas …
  intersection … union … empty intersection silences" → Tasks 7, 8, 9,
  11, 12. Schema-bump wording → decision 1 (recorded deviation).
- Design §3 "Source precedence … three-valued … never a crash, never a
  silent widening, never a silently dropped template annotation" →
  Task 5 (`Trust` traces every outcome; templates arrive with plan 4
  through the same `refine`).
- Design §3 "Declared types inherit … nearest ancestor … checked
  against the inheriting member's native declaration" → Task 6.
- Design §2 "editing a default value invalidates that signature's
  dependents" + implicit `= null` nullability → Tasks 4, 14.
- Plan-2 debts inherited: `value_of` on enums (Task 13), template
  scope convention (deferred to plan 4 with the bridge — no template
  is constructible from native text), `resource` atom consumed
  (Task 2).
- Linearize's recorded boundary "until the stub signature payload
  exists (plan 3)" → Task 10.
- `lookup.rs` "The stub side has no member tree until plan 3" →
  Task 10 (comment refreshed).

