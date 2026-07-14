# Type Engine 2 - Lattice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create `celerrate_types`, the type lattice crate: the full type representation interned in canonical form behind an opaque `TypeId`, structural canonical ordering (never interner-handle ordering), deterministic widening with union arity and structural depth caps, deterministic rendering, and the three-valued judgments (subtyping, assignability, nullability) as salsa queries, class hierarchy included through `linearized_class`. The Celerrate norm draft is written alongside the lattice.

**Architecture:** `TypeData` is a crate-private enum inside a private module; the public surface is exactly the interned handle `TypeId<'db>` (a `#[salsa::interned]` struct), constructors that canonicalize before interning (flatten, absorb, deduplicate, structurally sort, cap), and interrogation query methods; the representation is never exposed as a matchable enum, the parent spec's plugin commitment. Judgments are `#[salsa::tracked]` functions taking the same salsa inputs the semantics queries take (`AnalyzedFileSet`, `StubIndexInput`, `ProjectConfiguration`) so the class-versus-class rule can consult `linearized_class`. Two determinism invariants are load-bearing: canonical ordering is structural (by rank, name, and shape), never by interner handle (interning order is timing-dependent under parallel fan-out), and `TypeId` values never escape the process (no serde on any lattice type; persistence is plan 9a's structural serialization).

**Tech Stack:** Rust workspace, salsa 0.27 (interned and tracked queries), `celerrate_semantics` (folding helpers, `linearized_class`), plain assertions (no insta), TDD with `cargo test`.

**Spec:** `.claude/superpowers/specs/2026-07-14-type-engine-design.md` section 3 (the lattice and declared types, the lattice half only), the three-valued judgment paragraph, section 7 (the norm draft), and section 11 item 3. Read those sections before starting. Native declaration resolution, stub deltas, and docblock sources are plan 3; do not build them here.

## Global Constraints

- Zero panic, mechanically enforced: Clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` is forbidden. Production code returns `Option`/`Result` and uses total accessors (`.get(...)`, `.first()`); test modules open with `#![allow(clippy::unwrap_used)]` (add `panic`, `indexing_slicing` only when used).
- TDD: every task writes its failing tests first, watches them fail, then implements minimally.
- Strict layering: `celerrate_types` sits above `celerrate_semantics` and below `celerrate_rules`. It may depend on `celerrate_semantics`, `celerrate_stubs`, `celerrate_project`, `celerrate_db`, `celerrate_source`, and salsa, never on `celerrate_rules`, `celerrate_plugin`, or `celerrate_cli`.
- Determinism: no wall clock, no randomness, no environment reads inside queries. Every canonicalization and every judgment is a pure function of interned structure (plus the named salsa inputs).
- No serde derives and no `Serialize`/`Deserialize` on any type in this crate: `TypeId` is a process-local interner handle and never hits disk (spec section 3; plan 9a serializes structurally).
- Everything in English, full words (no abbreviated identifiers; standard acronyms fine).
- Commits: gitmoji + Conventional Commits (`✨ feat(types): …`, `✅ test(types): …`, `📝 docs(types): …`). Never add Claude attribution of any kind.
- Run before every commit: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`; run `cargo fmt --all` and re-stage when it changes files.
- No em-dashes in any generated content (documentation, comments, messages).

## Scope decisions fixed by this plan

- **Opacity through module privacy.** `TypeData` is declared `pub` inside the private module `representation` and never re-exported: unreachable from outside the crate, so the macro-generated `TypeId::new(db, data)` and `.data(db)` stay crate-internal in practice. `lib.rs` re-exports only `TypeId`, the public auxiliary structs (`ShapeKey`, `ShapeField`, `CallableParameter`), the judgment vocabulary (`Proof`, `Nullability`), the judgment queries, the widening operations, and the two cap constants.
- **No derived `Ord` on `TypeData`, ever.** A derived comparison would order child `TypeId`s by interner handle, which is timing-dependent under parallel fan-out. Structural ordering lives in `ordering.rs` as a recursive function over the data. A doc comment on `TypeData` states this prohibition.
- **Handle equality is structural equality.** Constructors canonicalize bottom-up before interning, so two structurally identical types always intern to the same `TypeId`. Deduplication and rule 1 of the judgment (`candidate == target`) lean on this invariant; a test pins it (same union built in two different orders yields the same handle).
- **Class names are stored pre-folded.** The `Class`, `EnumCase`, and `Template`-scope strings hold folded keys; the `class` and `enum_case` constructors fold internally with `celerrate_semantics::folded_symbol_key(SymbolSpace::ClassLike, …)` so spelling variants intern to one type. Consequence, accepted and recorded: `display` renders the folded (lowercased) key in this plan; diagnostic-grade original-spelling rendering is a recorded debt for plan 8, which can recover the spelling through `SymbolEntry::original`.
- **`Resource` is one atom beyond the spec's list.** The spec's section 3 enumeration does not name `resource`, but the stub signature payload (plan 3) carries it on the classic I/O functions and a one-variant atom now is cheaper than reshaping the enum later. Recorded as a deliberate, minimal extension.
- **Unified integers.** `int`, integer literals, `int<1, max>`, and `positive-int` are one representation: an inclusive range with optional bounds. A literal is a singleton range; an inverted range canonicalizes to `never`.
- **Canonical union rules are minimal and closed.** Flatten, drop `never`, absorb into `mixed`, deduplicate by handle, collapse the `true`/`false` literal pair to `bool`, sort structurally, unwrap singletons, and cap. **No subsumption elimination** (`int|int<1,3>` keeps both constituents): judgments remain correct, rendering may show redundancy, and the rule set stays order-independent by construction. Intersections are the dual (drop `mixed`, absorb into `never`), capped by keeping the first `UNION_ARITY_CAP` intersectands after the structural sort (deterministic because sorted; a sound over-approximation).
- **The caps are named constants**: `UNION_ARITY_CAP = 32` and `STRUCTURAL_DEPTH_CAP = 16` (both `pub`). A union beyond the arity cap collapses to the deterministic pairwise join of all constituents (common supertype, `mixed` at worst), never a truncated subset. A composite whose depth would exceed the depth cap replaces each child sitting at the cap with `mixed`, guaranteeing the result stays at the cap regardless of construction order.
- **`join` is hierarchy-blind in this plan.** Unrelated class types join to `mixed`; a hierarchy-aware least upper bound can refine it later without changing any signature. Same-name classes join argumentwise (same arity) or drop to the unparameterized class.
- **The judgment's `Fails` means refuted, `CannotProve` means undecidable.** `Fails` is only answered where value-set inclusion is definitionally refutable (`mixed <: int`, disjoint scalar kinds, fully resolved unrelated classes). Anything a template bound, a symbolic form, a placeholder, an invokable object, a stub boundary, or a broken hierarchy could change answers `CannotProve`. Consumers (plan 8) state their posture; nothing here silently discards a `CannotProve`.
- **Shapes are sealed** (the PHPStan default): a candidate shape with a key the target shape does not name fails the shape-versus-shape judgment. Duplicate keys handed to the shape constructor keep the last occurrence (PHP array-literal write semantics), then sort.
- **`iterable<K, V>` is a constructor, not a variant**: it desugars at construction to `array<K, V>|Traversable<K, V>` (spec section 3).
- **Generic arguments compare invariantly**: identical arguments participate in the class rule; differing arguments answer `CannotProve` (variance is out of scope; generics are inference-only in this sub-project). An unparameterized target erases: `Collection<User> <: Collection` holds when the hierarchy rule holds.
- **Float literals intern by bit pattern** (`FloatBits`, a `u64` newtype; every NaN pattern canonicalizes to one). `0.0` and `-0.0` are distinct interned literals; their join is `float`. Recorded, deterministic, and irrelevant to the three families.
- **`nullability(void)` is `AlwaysNull`**: reading a void call's value yields `null` in PHP.
- **Templates carry an opaque scope string** (`scope: String`): the declaring symbol's folded key, by convention `<class key>::<member key>` or a function key, produced by plans 3 and 4. Two templates with the same name in different declarations must never intern to one type; the scope is the discriminator.
- **Salsa contingency, stated once:** if `#[salsa::interned]` demands extra bounds on the field type (`salsa::Update` on `TypeData`), add salsa's derive (`#[derive(salsa::Update)]`) to `TypeData` and its auxiliary structs rather than reshaping the design.

## The canonical data model

This is the complete, final shape (tasks grow it incrementally; this section is the authority for names and types). Representation types live in `crates/celerrate_types/src/representation.rs` and derive `Debug, Clone, PartialEq, Eq, Hash` unless stated otherwise.

```rust
/// A float literal by bit pattern: `Eq`/`Hash`-safe, every NaN canonical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FloatBits(u64);
impl FloatBits {
    pub fn from_value(value: f64) -> Self;  // canonicalizes NaN to f64::NAN.to_bits()
    pub fn value(self) -> f64;
}

/// The string family: one variant per PHPStan string subtype the spec pins.
pub(crate) enum StringConstraint {
    General,             // string
    NonEmpty,            // non-empty-string
    Numeric,             // numeric-string
    LiteralMarker,       // literal-string (written in source, value unknown)
    Literal(String),     // 'active'
}

/// One array-shape key. Derives PartialOrd, Ord (Integer sorts before String).
pub enum ShapeKey { Integer(i64), String(String) }        // public

pub struct ShapeField<'db> {                              // public
    pub key: ShapeKey,
    pub optional: bool,
    pub value: TypeId<'db>,
}

pub struct CallableParameter<'db> {                       // public
    pub parameter_type: TypeId<'db>,
    pub optional: bool,
    pub variadic: bool,
    pub by_reference: bool,
}

/// The lattice representation. NEVER derive Ord/PartialOrd (children would
/// compare by interner handle); ordering.rs owns the structural order.
pub(crate)-visible enum TypeData<'db> {   // `pub` inside the private module
    Mixed, Never, Void, Null, Object, Resource,
    Bool { literal: Option<bool> },
    Int { minimum: Option<i64>, maximum: Option<i64> },   // literals are singleton ranges
    Float { literal: Option<FloatBits> },
    String { constraint: StringConstraint },
    ClassString { argument: Option<TypeId<'db>> },        // class-string / class-string<T>
    Array { key: TypeId<'db>, value: TypeId<'db>, is_list: bool, non_empty: bool },
    Shape { fields: Vec<ShapeField<'db>> },               // sorted by key, sealed
    Class { name: String, arguments: Vec<TypeId<'db>> },  // name pre-folded
    EnumCase { enum_name: String, case_name: String },    // enum_name pre-folded, case verbatim
    Callable { parameters: Vec<CallableParameter<'db>>, return_type: TypeId<'db> },
    Template { scope: String, name: String, bound: TypeId<'db> },
    KeyOf { subject: TypeId<'db> },
    ValueOf { subject: TypeId<'db> },
    Conditional { subject: TypeId<'db>, matches: TypeId<'db>,
                  then_branch: TypeId<'db>, otherwise_branch: TypeId<'db>, negated: bool },
    SelfPlaceholder, ParentPlaceholder, StaticPlaceholder,
}

/// The opaque interned handle: the entire public identity of a type.
#[salsa::interned(debug)]
pub struct TypeId<'db> {
    #[returns(ref)]
    pub(crate)-visible data: TypeData<'db>,
}
```

Constructors (inherent `impl TypeId<'db>` in `construction.rs`; every one canonicalizes before interning):

```rust
// atoms
pub fn mixed(db) -> Self;      pub fn never(db) -> Self;   pub fn void(db) -> Self;
pub fn null(db) -> Self;       pub fn object(db) -> Self;  pub fn resource(db) -> Self;
pub fn bool(db) -> Self;       pub fn bool_literal(db, value: bool) -> Self;
pub fn int(db) -> Self;        pub fn int_literal(db, value: i64) -> Self;
pub fn int_range(db, minimum: Option<i64>, maximum: Option<i64>) -> Self;  // inverted -> never
pub fn float(db) -> Self;      pub fn float_literal(db, value: f64) -> Self;
pub fn string(db) -> Self;     pub fn non_empty_string(db) -> Self;
pub fn numeric_string(db) -> Self;  pub fn literal_string_type(db) -> Self;
pub fn string_literal(db, value: &str) -> Self;
// composites
pub fn union(db, constituents: impl IntoIterator<Item = TypeId<'db>>) -> Self;
pub fn intersection(db, intersectands: impl IntoIterator<Item = TypeId<'db>>) -> Self;
pub fn array(db, key: TypeId<'db>, value: TypeId<'db>) -> Self;
pub fn non_empty_array(db, key: TypeId<'db>, value: TypeId<'db>) -> Self;
pub fn list(db, value: TypeId<'db>) -> Self;              // key forced to int
pub fn non_empty_list(db, value: TypeId<'db>) -> Self;
pub fn shape(db, fields: Vec<ShapeField<'db>>) -> Self;   // last duplicate wins, then sort
pub fn iterable(db, key: TypeId<'db>, value: TypeId<'db>) -> Self;  // desugars to a union
pub fn class(db, name: &str, arguments: Vec<TypeId<'db>>) -> Self;  // folds the name
pub fn enum_case(db, enum_name: &str, case_name: &str) -> Self;     // folds the enum name
pub fn callable(db, parameters: Vec<CallableParameter<'db>>, return_type: TypeId<'db>) -> Self;
pub fn class_string(db, argument: Option<TypeId<'db>>) -> Self;
pub fn template(db, scope: &str, name: &str, bound: TypeId<'db>) -> Self;
pub fn key_of(db, subject: TypeId<'db>) -> Self;    // evaluates decidable subjects
pub fn value_of(db, subject: TypeId<'db>) -> Self;  // evaluates decidable subjects
pub fn conditional(db, subject, matches, then_branch, otherwise_branch, negated: bool) -> Self;
pub fn static_placeholder(db) -> Self;  pub fn self_placeholder(db) -> Self;
pub fn parent_placeholder(db) -> Self;
```

Interrogation query methods (inherent `impl TypeId<'db>`, grown task by task):

```rust
pub fn is_mixed(self, db) -> bool;   pub fn is_never(self, db) -> bool;
pub fn is_null(self, db) -> bool;    pub fn is_void(self, db) -> bool;
pub fn contains_null(self, db) -> bool;
pub fn without_null(self, db) -> TypeId<'db>;   // null alone becomes never
pub fn constituents(self, db) -> Vec<TypeId<'db>>;   // union parts; singleton otherwise
pub fn intersectands(self, db) -> Vec<TypeId<'db>>;
pub fn bool_literal_value(self, db) -> Option<bool>;
pub fn int_literal_value(self, db) -> Option<i64>;
pub fn int_bounds(self, db) -> Option<(Option<i64>, Option<i64>)>;
pub fn float_literal_value(self, db) -> Option<f64>;
pub fn string_literal_value(self, db) -> Option<String>;
pub fn class_name(self, db) -> Option<String>;        // the folded key
pub fn class_arguments(self, db) -> Vec<TypeId<'db>>;
pub fn enum_case_parts(self, db) -> Option<(String, String)>;
pub fn callable_return(self, db) -> Option<TypeId<'db>>;
pub fn callable_parameters(self, db) -> Option<Vec<CallableParameter<'db>>>;
pub fn class_string_argument(self, db) -> Option<Option<TypeId<'db>>>;
pub fn template_bound(self, db) -> Option<TypeId<'db>>;
pub fn array_key(self, db) -> Option<TypeId<'db>>;    // shapes answer via their array form
pub fn array_value(self, db) -> Option<TypeId<'db>>;
pub fn is_list(self, db) -> bool;
pub fn is_non_empty_array(self, db) -> bool;
pub fn shape_fields(self, db) -> Option<Vec<ShapeField<'db>>>;
pub fn display(self, db) -> String;                   // display.rs, deterministic
```

Operations and judgments (free functions):

```rust
// widening.rs
pub const UNION_ARITY_CAP: usize = 32;
pub const STRUCTURAL_DEPTH_CAP: u32 = 16;
pub(crate) fn depth_of<'db>(db, of: TypeId<'db>) -> u32;         // atoms 1, composites 1 + max(children)
pub fn join<'db>(db, left: TypeId<'db>, right: TypeId<'db>) -> TypeId<'db>;
pub fn widened_literals<'db>(db, of: TypeId<'db>) -> TypeId<'db>;

// ordering.rs
pub(crate) fn structural_order<'db>(db, left: TypeId<'db>, right: TypeId<'db>) -> std::cmp::Ordering;

// judgments.rs
pub enum Proof { Holds, Fails, CannotProve }          // Copy, Eq, Hash
pub enum Nullability { NeverNull, PossiblyNull, AlwaysNull }
#[salsa::tracked]
pub fn subtype_of<'db>(db, files: AnalyzedFileSet, stubs: StubIndexInput,
                       configuration: ProjectConfiguration,
                       candidate: TypeId<'db>, target: TypeId<'db>) -> Proof;
#[salsa::tracked]
pub fn assignable_to<'db>(db, files, stubs, configuration,
                          source: TypeId<'db>, target: TypeId<'db>) -> Proof;  // delegates today
#[salsa::tracked]
pub fn nullability<'db>(db, subject: TypeId<'db>) -> Nullability;
```

The structural rank (`ordering.rs`, fixed; extending tasks append, never reorder):

```
0 Never  1 Void  2 Null  3 Bool  4 Int  5 Float  6 String  7 ClassString
8 Array  9 Shape  10 Object  11 Resource  12 Class  13 EnumCase  14 Callable
15 Template  16 KeyOf  17 ValueOf  18 Conditional
19 SelfPlaceholder  20 ParentPlaceholder  21 StaticPlaceholder
22 Intersection  23 Union  24 Mixed
```

Equal ranks compare their fields in declaration order (strings by `str` order, options `None` first, vectors lexicographically with shorter first, child types recursively).

Module map of the new crate:

```
crates/celerrate_types/
  Cargo.toml
  src/lib.rs              module doc + curated re-exports
  src/representation.rs   TypeData, TypeId, FloatBits, StringConstraint, ShapeKey, ShapeField, CallableParameter
  src/ordering.rs         structural_order
  src/construction.rs     constructors, canonicalization, interrogation methods
  src/widening.rs         depth_of, caps, join, widened_literals
  src/display.rs          display_type
  src/judgments.rs        Proof, Nullability, subtype_of, assignable_to, nullability
```

---
### Task 1: Crate scaffold and the atom lattice

**Files:**
- Create: `crates/celerrate_types/Cargo.toml`
- Create: `crates/celerrate_types/src/lib.rs`
- Create: `crates/celerrate_types/src/representation.rs`
- Create: `crates/celerrate_types/src/construction.rs`
- Test: inline `#[cfg(test)]` module in `construction.rs`

**Interfaces:**
- Consumes: `celerrate_db::testing::TestDatabase` (dev-dependency), salsa.
- Produces: `TypeId<'db>` (interned handle), the atom constructors `mixed`, `never`, `void`, `null`, `object`, `resource`, `bool`, `bool_literal`, `int`, `int_literal`, `int_range`, `float`, `float_literal`, `string`, `non_empty_string`, `numeric_string`, `literal_string_type`, `string_literal`; the interrogation methods `is_mixed`, `is_never`, `is_null`, `is_void`, `bool_literal_value`, `int_literal_value`, `int_bounds`, `float_literal_value`, `string_literal_value`; `FloatBits`. Later tasks add variants to `TypeData` and constructors beside these.

- [ ] **Step 1: Create the crate skeleton**

`crates/celerrate_types/Cargo.toml`:

```toml
[package]
name = "celerrate_types"
description = "The type lattice: interned types, canonical ordering, widening, and the typed judgments"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true

[dependencies]
salsa = { workspace = true }

[dev-dependencies]
celerrate_db = { path = "../celerrate_db" }

[lints]
workspace = true
```

`crates/celerrate_types/src/lib.rs`:

```rust
//! The type lattice of the analysis engine: every type is interned in
//! canonical form behind the opaque [`TypeId`] handle, so equality and
//! hashing are cheap and salsa's early cutoff applies to typed results.
//! The representation is never exposed as a matchable enum: consumers
//! construct through the `TypeId` constructors and interrogate through
//! its query methods (the plugin API commitment of the parent spec).

mod construction;
mod representation;

pub use representation::{FloatBits, TypeId};
```

The workspace `members = ["crates/*"]` glob picks the crate up; no other registration exists or is needed.

- [ ] **Step 2: Write the failing tests**

At the bottom of `crates/celerrate_types/src/construction.rs` (the file starts as just this test module plus a `use` line; the implementation lands in step 4):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use crate::TypeId;

    #[test]
    fn atoms_intern_to_stable_identities() {
        let db = TestDatabase::default();
        assert_eq!(TypeId::mixed(&db), TypeId::mixed(&db));
        assert_eq!(TypeId::null(&db), TypeId::null(&db));
        assert_ne!(TypeId::mixed(&db), TypeId::never(&db));
        assert_ne!(TypeId::void(&db), TypeId::null(&db));
        assert_ne!(TypeId::object(&db), TypeId::resource(&db));
    }

    #[test]
    fn interrogation_answers_the_atom_kinds() {
        let db = TestDatabase::default();
        assert!(TypeId::mixed(&db).is_mixed(&db));
        assert!(TypeId::never(&db).is_never(&db));
        assert!(TypeId::null(&db).is_null(&db));
        assert!(TypeId::void(&db).is_void(&db));
        assert!(!TypeId::null(&db).is_mixed(&db));
    }

    #[test]
    fn bool_literals_are_distinct_from_general_bool() {
        let db = TestDatabase::default();
        let general = TypeId::bool(&db);
        let true_type = TypeId::bool_literal(&db, true);
        let false_type = TypeId::bool_literal(&db, false);
        assert_ne!(general, true_type);
        assert_ne!(true_type, false_type);
        assert_eq!(true_type.bool_literal_value(&db), Some(true));
        assert_eq!(general.bool_literal_value(&db), None);
    }

    #[test]
    fn integer_literals_are_singleton_ranges() {
        let db = TestDatabase::default();
        let literal = TypeId::int_literal(&db, 42);
        let singleton = TypeId::int_range(&db, Some(42), Some(42));
        assert_eq!(literal, singleton);
        assert_eq!(literal.int_literal_value(&db), Some(42));
        assert_eq!(TypeId::int(&db).int_literal_value(&db), None);
        assert_eq!(TypeId::int(&db).int_bounds(&db), Some((None, None)));
        assert_eq!(
            TypeId::int_range(&db, Some(1), None).int_bounds(&db),
            Some((Some(1), None))
        );
    }

    #[test]
    fn an_inverted_integer_range_canonicalizes_to_never() {
        let db = TestDatabase::default();
        assert_eq!(TypeId::int_range(&db, Some(5), Some(1)), TypeId::never(&db));
    }

    #[test]
    fn float_literals_intern_by_bit_pattern() {
        let db = TestDatabase::default();
        assert_eq!(
            TypeId::float_literal(&db, 3.25),
            TypeId::float_literal(&db, 3.25)
        );
        assert_ne!(TypeId::float_literal(&db, 3.25), TypeId::float(&db));
        assert_eq!(
            TypeId::float_literal(&db, 3.25).float_literal_value(&db),
            Some(3.25)
        );
        // Every NaN canonicalizes to one interned literal.
        assert_eq!(
            TypeId::float_literal(&db, f64::NAN),
            TypeId::float_literal(&db, -f64::NAN)
        );
    }

    #[test]
    fn the_string_family_is_five_distinct_types() {
        let db = TestDatabase::default();
        let all = [
            TypeId::string(&db),
            TypeId::non_empty_string(&db),
            TypeId::numeric_string(&db),
            TypeId::literal_string_type(&db),
            TypeId::string_literal(&db, "active"),
        ];
        for (index, left) in all.iter().enumerate() {
            for right in all.iter().skip(index + 1) {
                assert_ne!(left, right);
            }
        }
        assert_eq!(
            TypeId::string_literal(&db, "active").string_literal_value(&db),
            Some("active".to_owned())
        );
        assert_eq!(TypeId::string(&db).string_literal_value(&db), None);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types`
Expected: compilation failure (`TypeId` and its constructors do not exist yet). A compile error on the test target is the failing state for a new crate.

- [ ] **Step 4: Implement the representation and the atom constructors**

`crates/celerrate_types/src/representation.rs`:

```rust
//! The lattice representation. `TypeData` stays inside this private
//! module: the public surface is the interned [`TypeId`] handle plus
//! constructors and query methods, never a matchable enum.

/// A float literal by bit pattern, so literals are `Eq`/`Hash`-safe.
/// Every NaN canonicalizes to one pattern; `0.0` and `-0.0` stay
/// distinct interned literals (their join is `float`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FloatBits(u64);

impl FloatBits {
    pub fn from_value(value: f64) -> Self {
        if value.is_nan() {
            return Self(f64::NAN.to_bits());
        }
        Self(value.to_bits())
    }

    pub fn value(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// The string subtypes the PHPStan dialect carries (spec section 3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StringConstraint {
    General,
    NonEmpty,
    Numeric,
    LiteralMarker,
    Literal(String),
}

/// The lattice. NEVER derive `Ord`/`PartialOrd` here: child handles
/// would compare by interner id, which is timing-dependent under
/// parallel fan-out; `ordering::structural_order` owns comparison.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeData {
    Mixed,
    Never,
    Void,
    Null,
    Object,
    Resource,
    Bool { literal: Option<bool> },
    Int { minimum: Option<i64>, maximum: Option<i64> },
    Float { literal: Option<FloatBits> },
    String { constraint: StringConstraint },
}

/// The opaque interned handle of one canonical type: cheap `Eq`/`Hash`
/// for early cutoff. Handle equality is structural equality because
/// every constructor canonicalizes bottom-up before interning. The id
/// never escapes the process (plan 9a serializes structurally).
#[salsa::interned(debug)]
pub struct TypeId<'db> {
    #[returns(ref)]
    pub data: TypeData,
}
```

(`TypeData` gains a `<'db>` lifetime in Task 2 when the first child-carrying variant lands; starting without it keeps this task minimal.)

`crates/celerrate_types/src/construction.rs` (above the test module from step 2):

```rust
//! Constructors and interrogation methods: the only way in and out of
//! the lattice. Every constructor canonicalizes before interning.

use crate::representation::{FloatBits, StringConstraint, TypeData, TypeId};

impl<'db> TypeId<'db> {
    fn intern(db: &'db dyn salsa::Database, data: TypeData) -> Self {
        Self::new(db, data)
    }

    pub fn mixed(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Mixed)
    }

    pub fn never(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Never)
    }

    pub fn void(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Void)
    }

    pub fn null(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Null)
    }

    pub fn object(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Object)
    }

    pub fn resource(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Resource)
    }

    pub fn bool(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Bool { literal: None })
    }

    pub fn bool_literal(db: &'db dyn salsa::Database, value: bool) -> Self {
        Self::intern(db, TypeData::Bool { literal: Some(value) })
    }

    pub fn int(db: &'db dyn salsa::Database) -> Self {
        Self::int_range(db, None, None)
    }

    pub fn int_literal(db: &'db dyn salsa::Database, value: i64) -> Self {
        Self::int_range(db, Some(value), Some(value))
    }

    /// The unified integer representation: `int` is the unbounded
    /// range, a literal is a singleton, an inverted range is `never`.
    pub fn int_range(
        db: &'db dyn salsa::Database,
        minimum: Option<i64>,
        maximum: Option<i64>,
    ) -> Self {
        if let (Some(low), Some(high)) = (minimum, maximum)
            && low > high
        {
            return Self::never(db);
        }
        Self::intern(db, TypeData::Int { minimum, maximum })
    }

    pub fn float(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Float { literal: None })
    }

    pub fn float_literal(db: &'db dyn salsa::Database, value: f64) -> Self {
        Self::intern(
            db,
            TypeData::Float { literal: Some(FloatBits::from_value(value)) },
        )
    }

    pub fn string(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::String { constraint: StringConstraint::General })
    }

    pub fn non_empty_string(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::String { constraint: StringConstraint::NonEmpty })
    }

    pub fn numeric_string(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::String { constraint: StringConstraint::Numeric })
    }

    pub fn literal_string_type(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::String { constraint: StringConstraint::LiteralMarker })
    }

    pub fn string_literal(db: &'db dyn salsa::Database, value: &str) -> Self {
        Self::intern(
            db,
            TypeData::String { constraint: StringConstraint::Literal(value.to_owned()) },
        )
    }

    pub fn is_mixed(self, db: &'db dyn salsa::Database) -> bool {
        matches!(self.data(db), TypeData::Mixed)
    }

    pub fn is_never(self, db: &'db dyn salsa::Database) -> bool {
        matches!(self.data(db), TypeData::Never)
    }

    pub fn is_null(self, db: &'db dyn salsa::Database) -> bool {
        matches!(self.data(db), TypeData::Null)
    }

    pub fn is_void(self, db: &'db dyn salsa::Database) -> bool {
        matches!(self.data(db), TypeData::Void)
    }

    pub fn bool_literal_value(self, db: &'db dyn salsa::Database) -> Option<bool> {
        match self.data(db) {
            TypeData::Bool { literal } => *literal,
            _ => None,
        }
    }

    pub fn int_literal_value(self, db: &'db dyn salsa::Database) -> Option<i64> {
        match self.data(db) {
            TypeData::Int { minimum: Some(low), maximum: Some(high) } if low == high => Some(*low),
            _ => None,
        }
    }

    pub fn int_bounds(self, db: &'db dyn salsa::Database) -> Option<(Option<i64>, Option<i64>)> {
        match self.data(db) {
            TypeData::Int { minimum, maximum } => Some((*minimum, *maximum)),
            _ => None,
        }
    }

    pub fn float_literal_value(self, db: &'db dyn salsa::Database) -> Option<f64> {
        match self.data(db) {
            TypeData::Float { literal } => literal.map(FloatBits::value),
            _ => None,
        }
    }

    pub fn string_literal_value(self, db: &'db dyn salsa::Database) -> Option<String> {
        match self.data(db) {
            TypeData::String { constraint: StringConstraint::Literal(value) } => {
                Some(value.clone())
            }
            _ => None,
        }
    }
}
```

If the interned macro rejects `TypeData` for a missing bound, apply the contingency from the scope decisions (`#[derive(salsa::Update)]`).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types`
Expected: all Task 1 tests PASS.

- [ ] **Step 6: Full verification and commit**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_types Cargo.lock
git commit -m "✨ feat(types): the celerrate_types crate with the interned atom lattice"
```

---
### Task 2: Structural ordering, unions, and intersections

**Files:**
- Create: `crates/celerrate_types/src/ordering.rs`
- Modify: `crates/celerrate_types/src/representation.rs` (lifetime + `Union`/`Intersection` variants)
- Modify: `crates/celerrate_types/src/construction.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (add `mod ordering;`)
- Test: inline modules in `ordering.rs` and `construction.rs`

**Interfaces:**
- Consumes: Task 1's `TypeData`, `TypeId`, atom constructors.
- Produces: `TypeData<'db>` (now lifetime-carrying), `structural_order(db, left, right) -> Ordering` (`pub(crate)`), `TypeId::union(db, iter)`, `TypeId::intersection(db, iter)`, `contains_null`, `without_null`, `constituents`, `intersectands`. Tasks 3 to 5 extend `rank` and `structural_order`'s match with their variants; Task 6 adds the caps at the two `// cap point` markers this task leaves.

- [ ] **Step 1: Write the failing tests**

Append to the test module in `construction.rs`:

```rust
    #[test]
    fn unions_canonicalize_independently_of_construction_order() {
        let db = TestDatabase::default();
        let forward = TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db), TypeId::null(&db)]);
        let backward = TypeId::union(&db, [TypeId::null(&db), TypeId::string(&db), TypeId::int(&db)]);
        assert_eq!(forward, backward);
    }

    #[test]
    fn unions_flatten_deduplicate_and_unwrap() {
        let db = TestDatabase::default();
        let nested = TypeId::union(
            &db,
            [TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]), TypeId::int(&db)],
        );
        let flat = TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]);
        assert_eq!(nested, flat);
        // A singleton unwraps to its only constituent.
        assert_eq!(TypeId::union(&db, [TypeId::int(&db)]), TypeId::int(&db));
        // An empty union is never.
        assert_eq!(TypeId::union(&db, std::iter::empty()), TypeId::never(&db));
    }

    #[test]
    fn union_absorption_rules_hold() {
        let db = TestDatabase::default();
        // never disappears; mixed absorbs everything.
        assert_eq!(
            TypeId::union(&db, [TypeId::int(&db), TypeId::never(&db)]),
            TypeId::int(&db)
        );
        assert_eq!(
            TypeId::union(&db, [TypeId::int(&db), TypeId::mixed(&db)]),
            TypeId::mixed(&db)
        );
        // true|false collapses to bool.
        assert_eq!(
            TypeId::union(&db, [TypeId::bool_literal(&db, true), TypeId::bool_literal(&db, false)]),
            TypeId::bool(&db)
        );
    }

    #[test]
    fn intersections_are_the_dual() {
        let db = TestDatabase::default();
        let forward = TypeId::intersection(&db, [TypeId::string(&db), TypeId::non_empty_string(&db)]);
        let backward = TypeId::intersection(&db, [TypeId::non_empty_string(&db), TypeId::string(&db)]);
        assert_eq!(forward, backward);
        assert_eq!(
            TypeId::intersection(&db, [TypeId::int(&db), TypeId::mixed(&db)]),
            TypeId::int(&db)
        );
        assert_eq!(
            TypeId::intersection(&db, [TypeId::int(&db), TypeId::never(&db)]),
            TypeId::never(&db)
        );
        assert_eq!(TypeId::intersection(&db, std::iter::empty()), TypeId::mixed(&db));
    }

    #[test]
    fn null_interrogation_walks_unions() {
        let db = TestDatabase::default();
        let nullable = TypeId::union(&db, [TypeId::int(&db), TypeId::null(&db)]);
        assert!(nullable.contains_null(&db));
        assert!(TypeId::null(&db).contains_null(&db));
        assert!(!TypeId::int(&db).contains_null(&db));
        assert_eq!(nullable.without_null(&db), TypeId::int(&db));
        assert_eq!(TypeId::null(&db).without_null(&db), TypeId::never(&db));
        assert_eq!(TypeId::int(&db).without_null(&db), TypeId::int(&db));
        assert_eq!(nullable.constituents(&db).len(), 2);
        assert_eq!(TypeId::int(&db).constituents(&db), vec![TypeId::int(&db)]);
    }
```

And a dedicated module at the bottom of `ordering.rs`:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::cmp::Ordering;

    use celerrate_db::testing::TestDatabase;

    use super::structural_order;
    use crate::TypeId;

    #[test]
    fn the_order_is_total_deterministic_and_structural() {
        let db = TestDatabase::default();
        let null = TypeId::null(&db);
        let int = TypeId::int(&db);
        let string = TypeId::string(&db);
        // Rank order: Null(2) < Int(4) < String(6).
        assert_eq!(structural_order(&db, null, int), Ordering::Less);
        assert_eq!(structural_order(&db, int, string), Ordering::Less);
        assert_eq!(structural_order(&db, string, string), Ordering::Equal);
        // Same rank compares fields: bounded ranges order by bounds, None first.
        let low = TypeId::int_range(&db, Some(1), Some(3));
        let unbounded = TypeId::int(&db);
        assert_eq!(structural_order(&db, unbounded, low), Ordering::Less);
        // String literals order by value.
        let a = TypeId::string_literal(&db, "a");
        let b = TypeId::string_literal(&db, "b");
        assert_eq!(structural_order(&db, a, b), Ordering::Less);
    }

    #[test]
    fn equal_order_means_equal_handle() {
        let db = TestDatabase::default();
        let left = TypeId::union(&db, [TypeId::int(&db), TypeId::null(&db)]);
        let right = TypeId::union(&db, [TypeId::null(&db), TypeId::int(&db)]);
        assert_eq!(structural_order(&db, left, right), Ordering::Equal);
        assert_eq!(left, right);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types`
Expected: compilation failure (`union`, `intersection`, `structural_order` missing).

- [ ] **Step 3: Add the lifetime and the two variants**

In `representation.rs`, `TypeData` becomes lifetime-carrying and gains the composites (this ripples the `<'db>` parameter through the interned struct and `construction.rs`; the atom constructors only change their `TypeData` path to `TypeData::<'db>` inference, no logic change):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeData<'db> {
    Mixed,
    Never,
    Void,
    Null,
    Object,
    Resource,
    Bool { literal: Option<bool> },
    Int { minimum: Option<i64>, maximum: Option<i64> },
    Float { literal: Option<FloatBits> },
    String { constraint: StringConstraint },
    /// Flattened, deduplicated, structurally sorted, length >= 2.
    Union { constituents: Vec<TypeId<'db>> },
    /// Flattened, deduplicated, structurally sorted, length >= 2.
    Intersection { intersectands: Vec<TypeId<'db>> },
}

#[salsa::interned(debug)]
pub struct TypeId<'db> {
    #[returns(ref)]
    pub data: TypeData<'db>,
}
```

- [ ] **Step 4: Implement the structural order**

`crates/celerrate_types/src/ordering.rs`:

```rust
//! The canonical order of the lattice: structural (by rank, name, and
//! shape), never by interner handle. Interning order is timing-dependent
//! under parallel fan-out, so a handle-based sort would make canonical
//! forms nondeterministic and break the byte-identical harness.

use std::cmp::Ordering;

use crate::representation::{StringConstraint, TypeData, TypeId};

/// The fixed rank of each variant. Extending tasks append new variants
/// at their documented rank; existing ranks never change.
fn rank(data: &TypeData<'_>) -> u8 {
    match data {
        TypeData::Never => 0,
        TypeData::Void => 1,
        TypeData::Null => 2,
        TypeData::Bool { .. } => 3,
        TypeData::Int { .. } => 4,
        TypeData::Float { .. } => 5,
        TypeData::String { .. } => 6,
        TypeData::Intersection { .. } => 22,
        TypeData::Union { .. } => 23,
        TypeData::Mixed => 24,
        TypeData::Object => 10,
        TypeData::Resource => 11,
    }
}

fn order_string_constraint(left: &StringConstraint, right: &StringConstraint) -> Ordering {
    fn constraint_rank(constraint: &StringConstraint) -> u8 {
        match constraint {
            StringConstraint::General => 0,
            StringConstraint::NonEmpty => 1,
            StringConstraint::Numeric => 2,
            StringConstraint::LiteralMarker => 3,
            StringConstraint::Literal(_) => 4,
        }
    }
    constraint_rank(left).cmp(&constraint_rank(right)).then_with(|| match (left, right) {
        (StringConstraint::Literal(a), StringConstraint::Literal(b)) => a.cmp(b),
        _ => Ordering::Equal,
    })
}

pub(crate) fn order_types<'db>(
    db: &'db dyn salsa::Database,
    left: &[TypeId<'db>],
    right: &[TypeId<'db>],
) -> Ordering {
    for (a, b) in left.iter().zip(right.iter()) {
        let ordering = structural_order(db, *a, *b);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

/// The total, deterministic, structural order over interned types.
pub(crate) fn structural_order<'db>(
    db: &'db dyn salsa::Database,
    left: TypeId<'db>,
    right: TypeId<'db>,
) -> Ordering {
    if left == right {
        return Ordering::Equal;
    }
    let left_data = left.data(db);
    let right_data = right.data(db);
    rank(left_data).cmp(&rank(right_data)).then_with(|| match (left_data, right_data) {
        (TypeData::Bool { literal: a }, TypeData::Bool { literal: b }) => a.cmp(b),
        (
            TypeData::Int { minimum: a_min, maximum: a_max },
            TypeData::Int { minimum: b_min, maximum: b_max },
        ) => (a_min, a_max).cmp(&(b_min, b_max)),
        (TypeData::Float { literal: a }, TypeData::Float { literal: b }) => a.cmp(b),
        (TypeData::String { constraint: a }, TypeData::String { constraint: b }) => {
            order_string_constraint(a, b)
        }
        (TypeData::Union { constituents: a }, TypeData::Union { constituents: b })
        | (TypeData::Intersection { intersectands: a }, TypeData::Intersection { intersectands: b }) => {
            order_types(db, a, b)
        }
        // Same rank with no fields is equal; interning made left == right
        // impossible here, so this arm is unreachable for atoms but kept
        // total for safety.
        _ => Ordering::Equal,
    })
}
```

Add `mod ordering;` to `lib.rs` (no re-export: crate-internal).

- [ ] **Step 5: Implement the composite constructors and null interrogation**

In `construction.rs`:

```rust
use std::cmp::Ordering;

use crate::ordering::structural_order;

impl<'db> TypeId<'db> {
    /// The canonical union: flatten, drop `never`, absorb into `mixed`,
    /// deduplicate, collapse the `true`/`false` pair, sort structurally,
    /// unwrap singletons. No subsumption elimination (recorded scope
    /// decision): `int|int<1,3>` keeps both constituents.
    pub fn union(
        db: &'db dyn salsa::Database,
        constituents: impl IntoIterator<Item = TypeId<'db>>,
    ) -> Self {
        let mut flat: Vec<TypeId<'db>> = Vec::new();
        for constituent in constituents {
            match constituent.data(db) {
                TypeData::Mixed => return Self::mixed(db),
                TypeData::Never => {}
                TypeData::Union { constituents: nested } => flat.extend(nested.iter().copied()),
                _ => flat.push(constituent),
            }
        }
        let true_type = Self::bool_literal(db, true);
        let false_type = Self::bool_literal(db, false);
        if flat.contains(&true_type) && flat.contains(&false_type) {
            flat.retain(|part| *part != true_type && *part != false_type);
            flat.push(Self::bool(db));
        }
        flat.sort_by(|left, right| structural_order(db, *left, *right));
        flat.dedup();
        // cap point: Task 6 collapses beyond UNION_ARITY_CAP here.
        match flat.len() {
            0 => Self::never(db),
            1 => flat.swap_remove(0),
            _ => Self::intern(db, TypeData::Union { constituents: flat }),
        }
    }

    /// The canonical intersection: the dual rules (`mixed` disappears,
    /// `never` absorbs).
    pub fn intersection(
        db: &'db dyn salsa::Database,
        intersectands: impl IntoIterator<Item = TypeId<'db>>,
    ) -> Self {
        let mut flat: Vec<TypeId<'db>> = Vec::new();
        for intersectand in intersectands {
            match intersectand.data(db) {
                TypeData::Never => return Self::never(db),
                TypeData::Mixed => {}
                TypeData::Intersection { intersectands: nested } => {
                    flat.extend(nested.iter().copied());
                }
                _ => flat.push(intersectand),
            }
        }
        flat.sort_by(|left, right| structural_order(db, *left, *right));
        flat.dedup();
        // cap point: Task 6 truncates beyond UNION_ARITY_CAP here (sorted,
        // so deterministic; a sound over-approximation).
        match flat.len() {
            0 => Self::mixed(db),
            1 => flat.swap_remove(0),
            _ => Self::intern(db, TypeData::Intersection { intersectands: flat }),
        }
    }

    pub fn contains_null(self, db: &'db dyn salsa::Database) -> bool {
        match self.data(db) {
            TypeData::Null => true,
            TypeData::Union { constituents } => {
                constituents.iter().any(|part| part.is_null(db))
            }
            _ => false,
        }
    }

    /// The type with `null` removed; `null` alone becomes `never`.
    pub fn without_null(self, db: &'db dyn salsa::Database) -> TypeId<'db> {
        match self.data(db) {
            TypeData::Null => Self::never(db),
            TypeData::Union { constituents } => Self::union(
                db,
                constituents.iter().copied().filter(|part| !part.is_null(db)),
            ),
            _ => self,
        }
    }

    /// Union constituents; any other type answers itself as a singleton.
    pub fn constituents(self, db: &'db dyn salsa::Database) -> Vec<TypeId<'db>> {
        match self.data(db) {
            TypeData::Union { constituents } => constituents.clone(),
            _ => vec![self],
        }
    }

    /// Intersection parts; any other type answers itself as a singleton.
    pub fn intersectands(self, db: &'db dyn salsa::Database) -> Vec<TypeId<'db>> {
        match self.data(db) {
            TypeData::Intersection { intersectands } => intersectands.clone(),
            _ => vec![self],
        }
    }
}
```

(The `Ordering` import serves the sort closures; remove it if the compiler flags it unused.)

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types`
Expected: all tests PASS, including Task 1's (the lifetime ripple must not change behavior).

- [ ] **Step 7: Full verification and commit**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_types
git commit -m "✨ feat(types): canonical unions and intersections under the structural order"
```

---
### Task 3: The container types

**Files:**
- Modify: `crates/celerrate_types/src/representation.rs` (`Array`, `Shape`, `ShapeKey`, `ShapeField`)
- Modify: `crates/celerrate_types/src/ordering.rs` (ranks 8 and 9)
- Modify: `crates/celerrate_types/src/construction.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (re-export `ShapeKey`, `ShapeField`)
- Test: inline module in `construction.rs`

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces: `TypeId::array`, `non_empty_array`, `list`, `non_empty_list`, `shape`, `iterable`; `ShapeKey`, `ShapeField<'db>` (public); interrogation `array_key`, `array_value`, `is_list`, `is_non_empty_array`, `shape_fields`. Task 8's judgment consumes `shape_as_array` (`pub(crate)`, the shape's general array form: key = union of key literal types, value = union of field values, `non_empty` when any field is required, `is_list` when all keys are integers `0..n` in order with optionals only as a suffix).

- [ ] **Step 1: Write the failing tests**

Append to the test module in `construction.rs`:

```rust
    #[test]
    fn arrays_carry_their_flags() {
        let db = TestDatabase::default();
        let general = TypeId::array(&db, TypeId::string(&db), TypeId::int(&db));
        let non_empty = TypeId::non_empty_array(&db, TypeId::string(&db), TypeId::int(&db));
        let list = TypeId::list(&db, TypeId::int(&db));
        assert_ne!(general, non_empty);
        assert_ne!(general, list);
        assert_eq!(general.array_key(&db), Some(TypeId::string(&db)));
        assert_eq!(general.array_value(&db), Some(TypeId::int(&db)));
        assert!(!general.is_list(&db));
        assert!(list.is_list(&db));
        assert_eq!(list.array_key(&db), Some(TypeId::int(&db)));
        assert!(non_empty.is_non_empty_array(&db));
        assert!(!general.is_non_empty_array(&db));
        assert!(TypeId::non_empty_list(&db, TypeId::int(&db)).is_list(&db));
    }

    #[test]
    fn shapes_sort_their_fields_and_keep_the_last_duplicate() {
        let db = TestDatabase::default();
        let forward = TypeId::shape(
            &db,
            vec![
                ShapeField { key: ShapeKey::String("id".to_owned()), optional: false, value: TypeId::int(&db) },
                ShapeField { key: ShapeKey::String("name".to_owned()), optional: true, value: TypeId::string(&db) },
            ],
        );
        let backward = TypeId::shape(
            &db,
            vec![
                ShapeField { key: ShapeKey::String("name".to_owned()), optional: true, value: TypeId::string(&db) },
                ShapeField { key: ShapeKey::String("id".to_owned()), optional: false, value: TypeId::int(&db) },
            ],
        );
        assert_eq!(forward, backward);
        let duplicated = TypeId::shape(
            &db,
            vec![
                ShapeField { key: ShapeKey::String("id".to_owned()), optional: false, value: TypeId::string(&db) },
                ShapeField { key: ShapeKey::String("id".to_owned()), optional: false, value: TypeId::int(&db) },
            ],
        );
        let fields = duplicated.shape_fields(&db).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields.first().unwrap().value, TypeId::int(&db));
    }

    #[test]
    fn shapes_answer_the_array_interrogation_through_their_array_form() {
        let db = TestDatabase::default();
        let shape = TypeId::shape(
            &db,
            vec![
                ShapeField { key: ShapeKey::Integer(0), optional: false, value: TypeId::int(&db) },
                ShapeField { key: ShapeKey::Integer(1), optional: true, value: TypeId::string(&db) },
            ],
        );
        assert_eq!(
            shape.array_value(&db),
            Some(TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]))
        );
        assert!(shape.is_list(&db));
        assert!(shape.is_non_empty_array(&db));
        // A gap breaks the list property.
        let gapped = TypeId::shape(
            &db,
            vec![
                ShapeField { key: ShapeKey::Integer(0), optional: false, value: TypeId::int(&db) },
                ShapeField { key: ShapeKey::Integer(2), optional: false, value: TypeId::int(&db) },
            ],
        );
        assert!(!gapped.is_list(&db));
        // An optional field before a required one breaks it too.
        let interleaved = TypeId::shape(
            &db,
            vec![
                ShapeField { key: ShapeKey::Integer(0), optional: true, value: TypeId::int(&db) },
                ShapeField { key: ShapeKey::Integer(1), optional: false, value: TypeId::int(&db) },
            ],
        );
        assert!(!interleaved.is_list(&db));
    }

    #[test]
    fn iterable_desugars_to_the_spec_union() {
        let db = TestDatabase::default();
        let iterable = TypeId::iterable(&db, TypeId::string(&db), TypeId::int(&db));
        let expected = TypeId::union(
            &db,
            [
                TypeId::array(&db, TypeId::string(&db), TypeId::int(&db)),
                TypeId::class(&db, "Traversable", vec![TypeId::string(&db), TypeId::int(&db)]),
            ],
        );
        assert_eq!(iterable, expected);
    }
```

The `iterable` test names `TypeId::class`, which lands in Task 4; keep that one test `#[ignore = "class types land in task 4"]` until Task 4 removes the attribute (record it in Task 4's steps). Add the imports `use crate::{ShapeField, ShapeKey};` to the test module.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types`
Expected: compilation failure (`ShapeKey`, `array`, `list`, `shape` missing).

- [ ] **Step 3: Implement the variants, constructors, and interrogation**

`representation.rs` additions:

```rust
/// One array-shape key. `Integer` sorts before `String`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ShapeKey {
    Integer(i64),
    String(String),
}

/// One field of an array shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapeField<'db> {
    pub key: ShapeKey,
    pub optional: bool,
    pub value: TypeId<'db>,
}
```

`TypeData` additions:

```rust
    /// `array<K, V>` and its list and non-empty refinements. A list
    /// always stores the general `int` key.
    Array { key: TypeId<'db>, value: TypeId<'db>, is_list: bool, non_empty: bool },
    /// A sealed array shape, fields sorted by key.
    Shape { fields: Vec<ShapeField<'db>> },
```

`ordering.rs`: add `TypeData::Array { .. } => 8` and `TypeData::Shape { .. } => 9` to `rank`, and the comparison arms:

```rust
        (
            TypeData::Array { key: a_key, value: a_value, is_list: a_list, non_empty: a_non_empty },
            TypeData::Array { key: b_key, value: b_value, is_list: b_list, non_empty: b_non_empty },
        ) => (a_list, a_non_empty)
            .cmp(&(b_list, b_non_empty))
            .then_with(|| structural_order(db, *a_key, *b_key))
            .then_with(|| structural_order(db, *a_value, *b_value)),
        (TypeData::Shape { fields: a }, TypeData::Shape { fields: b }) => {
            for (left_field, right_field) in a.iter().zip(b.iter()) {
                let ordering = left_field
                    .key
                    .cmp(&right_field.key)
                    .then(left_field.optional.cmp(&right_field.optional))
                    .then_with(|| structural_order(db, left_field.value, right_field.value));
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            a.len().cmp(&b.len())
        }
```

(The `Shape` arm needs a small refactor: extract the field-list comparison into a helper `fn order_shape_fields` next to `order_types` so the `return` stays inside a plain function, mirroring `order_types`.)

`construction.rs` additions:

```rust
use crate::representation::{ShapeField, ShapeKey};

impl<'db> TypeId<'db> {
    pub fn array(db: &'db dyn salsa::Database, key: TypeId<'db>, value: TypeId<'db>) -> Self {
        Self::intern(db, TypeData::Array { key, value, is_list: false, non_empty: false })
    }

    pub fn non_empty_array(
        db: &'db dyn salsa::Database,
        key: TypeId<'db>,
        value: TypeId<'db>,
    ) -> Self {
        Self::intern(db, TypeData::Array { key, value, is_list: false, non_empty: true })
    }

    pub fn list(db: &'db dyn salsa::Database, value: TypeId<'db>) -> Self {
        let key = Self::int(db);
        Self::intern(db, TypeData::Array { key, value, is_list: true, non_empty: false })
    }

    pub fn non_empty_list(db: &'db dyn salsa::Database, value: TypeId<'db>) -> Self {
        let key = Self::int(db);
        Self::intern(db, TypeData::Array { key, value, is_list: true, non_empty: true })
    }

    /// A sealed shape: duplicate keys keep the last occurrence (PHP
    /// array-literal write semantics), fields sort by key.
    pub fn shape(db: &'db dyn salsa::Database, fields: Vec<ShapeField<'db>>) -> Self {
        let mut deduplicated: Vec<ShapeField<'db>> = Vec::with_capacity(fields.len());
        for field in fields {
            if let Some(existing) =
                deduplicated.iter_mut().find(|candidate| candidate.key == field.key)
            {
                *existing = field;
            } else {
                deduplicated.push(field);
            }
        }
        deduplicated.sort_by(|left, right| left.key.cmp(&right.key));
        Self::intern(db, TypeData::Shape { fields: deduplicated })
    }

    /// `iterable<K, V>` desugared: `array<K, V>|Traversable<K, V>`
    /// (spec section 3).
    pub fn iterable(db: &'db dyn salsa::Database, key: TypeId<'db>, value: TypeId<'db>) -> Self {
        Self::union(
            db,
            [Self::array(db, key, value), Self::class(db, "Traversable", vec![key, value])],
        )
    }

    /// The general array form of a shape: the judgment's and the
    /// interrogation's shared widening.
    pub(crate) fn shape_as_array(
        db: &'db dyn salsa::Database,
        fields: &[ShapeField<'db>],
    ) -> (TypeId<'db>, TypeId<'db>, bool, bool) {
        let key = Self::union(
            db,
            fields.iter().map(|field| match &field.key {
                ShapeKey::Integer(value) => Self::int_literal(db, *value),
                ShapeKey::String(value) => Self::string_literal(db, value),
            }),
        );
        let value = Self::union(db, fields.iter().map(|field| field.value));
        let non_empty = fields.iter().any(|field| !field.optional);
        let integer_keys: Vec<i64> = fields
            .iter()
            .map(|field| match &field.key {
                ShapeKey::Integer(value) => Some(*value),
                ShapeKey::String(_) => None,
            })
            .collect::<Option<Vec<i64>>>()
            .unwrap_or_default();
        let consecutive = !integer_keys.is_empty()
            && integer_keys.iter().enumerate().all(|(index, key)| *key == index as i64);
        let optional_is_suffix = fields
            .iter()
            .position(|field| field.optional)
            .is_none_or(|first| fields.iter().skip(first).all(|field| field.optional));
        let is_list = consecutive && optional_is_suffix;
        (key, value, is_list, non_empty)
    }

    pub fn array_key(self, db: &'db dyn salsa::Database) -> Option<TypeId<'db>> {
        match self.data(db) {
            TypeData::Array { key, .. } => Some(*key),
            TypeData::Shape { fields } => Some(Self::shape_as_array(db, fields).0),
            _ => None,
        }
    }

    pub fn array_value(self, db: &'db dyn salsa::Database) -> Option<TypeId<'db>> {
        match self.data(db) {
            TypeData::Array { value, .. } => Some(*value),
            TypeData::Shape { fields } => Some(Self::shape_as_array(db, fields).1),
            _ => None,
        }
    }

    pub fn is_list(self, db: &'db dyn salsa::Database) -> bool {
        match self.data(db) {
            TypeData::Array { is_list, .. } => *is_list,
            TypeData::Shape { fields } => Self::shape_as_array(db, fields).2,
            _ => false,
        }
    }

    pub fn is_non_empty_array(self, db: &'db dyn salsa::Database) -> bool {
        match self.data(db) {
            TypeData::Array { non_empty, .. } => *non_empty,
            TypeData::Shape { fields } => Self::shape_as_array(db, fields).3,
            _ => false,
        }
    }

    pub fn shape_fields(self, db: &'db dyn salsa::Database) -> Option<Vec<ShapeField<'db>>> {
        match self.data(db) {
            TypeData::Shape { fields } => Some(fields.clone()),
            _ => None,
        }
    }
}
```

`lib.rs`: `pub use representation::{FloatBits, ShapeField, ShapeKey, TypeId};`

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types`
Expected: PASS (the `iterable` test still `#[ignore]`d).

- [ ] **Step 5: Full verification and commit**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_types
git commit -m "✨ feat(types): arrays, lists, and sealed shapes with their array form"
```

---

### Task 4: Class-likes, class-string, and the placeholders

**Files:**
- Modify: `crates/celerrate_types/Cargo.toml` (add `celerrate_semantics`)
- Modify: `crates/celerrate_types/src/representation.rs` (`ClassString`, `Class`, `EnumCase`, placeholders)
- Modify: `crates/celerrate_types/src/ordering.rs` (ranks 7, 12, 13, 19, 20, 21)
- Modify: `crates/celerrate_types/src/construction.rs`
- Test: inline module in `construction.rs`

**Interfaces:**
- Consumes: `celerrate_semantics::{SymbolSpace, folded_symbol_key}`.
- Produces: `TypeId::class` (name folded internally), `enum_case`, `class_string`, `static_placeholder`, `self_placeholder`, `parent_placeholder`; interrogation `class_name`, `class_arguments`, `enum_case_parts`, `class_string_argument`. Task 9's hierarchy rule reads `class_name` folded keys.

- [ ] **Step 1: Write the failing tests**

Append to the test module in `construction.rs`:

```rust
    #[test]
    fn class_names_fold_at_construction() {
        let db = TestDatabase::default();
        let lower = TypeId::class(&db, "app\\entity\\user", vec![]);
        let mixed_case = TypeId::class(&db, "App\\Entity\\User", vec![]);
        assert_eq!(lower, mixed_case);
        assert_eq!(lower.class_name(&db), Some("app\\entity\\user".to_owned()));
        assert_eq!(lower.class_arguments(&db), Vec::<TypeId>::new());
    }

    #[test]
    fn generic_arguments_participate_in_identity() {
        let db = TestDatabase::default();
        let of_user = TypeId::class(&db, "Collection", vec![TypeId::class(&db, "User", vec![])]);
        let of_int = TypeId::class(&db, "Collection", vec![TypeId::int(&db)]);
        let bare = TypeId::class(&db, "Collection", vec![]);
        assert_ne!(of_user, of_int);
        assert_ne!(of_user, bare);
        assert_eq!(of_user.class_arguments(&db).len(), 1);
    }

    #[test]
    fn enum_cases_fold_the_enum_and_keep_the_case_verbatim() {
        let db = TestDatabase::default();
        assert_eq!(
            TypeId::enum_case(&db, "App\\Status", "Active"),
            TypeId::enum_case(&db, "app\\status", "Active")
        );
        assert_ne!(
            TypeId::enum_case(&db, "App\\Status", "Active"),
            TypeId::enum_case(&db, "App\\Status", "ACTIVE")
        );
        assert_eq!(
            TypeId::enum_case(&db, "App\\Status", "Active").enum_case_parts(&db),
            Some(("app\\status".to_owned(), "Active".to_owned()))
        );
    }

    #[test]
    fn class_string_carries_an_optional_argument() {
        let db = TestDatabase::default();
        let bare = TypeId::class_string(&db, None);
        let of_user = TypeId::class_string(&db, Some(TypeId::class(&db, "User", vec![])));
        assert_ne!(bare, of_user);
        assert_eq!(bare.class_string_argument(&db), Some(None));
        assert_eq!(
            of_user.class_string_argument(&db),
            Some(Some(TypeId::class(&db, "User", vec![])))
        );
    }

    #[test]
    fn the_placeholders_are_three_distinct_atoms() {
        let db = TestDatabase::default();
        assert_ne!(TypeId::static_placeholder(&db), TypeId::self_placeholder(&db));
        assert_ne!(TypeId::self_placeholder(&db), TypeId::parent_placeholder(&db));
    }
```

Also remove `#[ignore]` from Task 3's `iterable_desugars_to_the_spec_union` test.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types`
Expected: compilation failure (`class`, `enum_case`, `class_string`, placeholders missing).

- [ ] **Step 3: Implement**

`Cargo.toml`: add `celerrate_semantics = { path = "../celerrate_semantics" }` to `[dependencies]`.

`TypeData` additions (ordering ranks in comments):

```rust
    /// `class-string` / `class-string<T>`: the primary template binder,
    /// never lowered to `string` (spec section 3). Rank 7.
    ClassString { argument: Option<TypeId<'db>> },
    /// A class, interface, trait, or enum type, name pre-folded,
    /// carrying its generic arguments. Rank 12.
    Class { name: String, arguments: Vec<TypeId<'db>> },
    /// One enum case: enum key folded, case name verbatim
    /// (case-sensitive, matching the member boundary). Rank 13.
    EnumCase { enum_name: String, case_name: String },
    /// The late-static-binding placeholders, symbolic until call-site
    /// substitution (plan 6). Ranks 19, 20, 21.
    SelfPlaceholder,
    ParentPlaceholder,
    StaticPlaceholder,
```

`ordering.rs` `rank` additions: `ClassString => 7`, `Class => 12`, `EnumCase => 13`, `SelfPlaceholder => 19`, `ParentPlaceholder => 20`, `StaticPlaceholder => 21`. Comparison arms:

```rust
        (TypeData::ClassString { argument: a }, TypeData::ClassString { argument: b }) => {
            match (a, b) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                (Some(left_argument), Some(right_argument)) => {
                    structural_order(db, *left_argument, *right_argument)
                }
            }
        }
        (
            TypeData::Class { name: a_name, arguments: a_arguments },
            TypeData::Class { name: b_name, arguments: b_arguments },
        ) => a_name.cmp(b_name).then_with(|| order_types(db, a_arguments, b_arguments)),
        (
            TypeData::EnumCase { enum_name: a_enum, case_name: a_case },
            TypeData::EnumCase { enum_name: b_enum, case_name: b_case },
        ) => a_enum.cmp(b_enum).then_with(|| a_case.cmp(b_case)),
```

`construction.rs` additions:

```rust
use celerrate_semantics::{SymbolSpace, folded_symbol_key};

impl<'db> TypeId<'db> {
    /// A class-like type. The name folds internally so spelling
    /// variants intern to one type; `display` therefore renders the
    /// folded key (recorded debt: plan 8 recovers the original
    /// spelling through the symbol table when rendering diagnostics).
    pub fn class(db: &'db dyn salsa::Database, name: &str, arguments: Vec<TypeId<'db>>) -> Self {
        let folded = folded_symbol_key(SymbolSpace::ClassLike, name);
        Self::intern(db, TypeData::Class { name: folded, arguments })
    }

    pub fn enum_case(db: &'db dyn salsa::Database, enum_name: &str, case_name: &str) -> Self {
        let folded = folded_symbol_key(SymbolSpace::ClassLike, enum_name);
        Self::intern(db, TypeData::EnumCase { enum_name: folded, case_name: case_name.to_owned() })
    }

    pub fn class_string(db: &'db dyn salsa::Database, argument: Option<TypeId<'db>>) -> Self {
        Self::intern(db, TypeData::ClassString { argument })
    }

    pub fn static_placeholder(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::StaticPlaceholder)
    }

    pub fn self_placeholder(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::SelfPlaceholder)
    }

    pub fn parent_placeholder(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::ParentPlaceholder)
    }

    pub fn class_name(self, db: &'db dyn salsa::Database) -> Option<String> {
        match self.data(db) {
            TypeData::Class { name, .. } => Some(name.clone()),
            _ => None,
        }
    }

    pub fn class_arguments(self, db: &'db dyn salsa::Database) -> Vec<TypeId<'db>> {
        match self.data(db) {
            TypeData::Class { arguments, .. } => arguments.clone(),
            _ => Vec::new(),
        }
    }

    pub fn enum_case_parts(self, db: &'db dyn salsa::Database) -> Option<(String, String)> {
        match self.data(db) {
            TypeData::EnumCase { enum_name, case_name } => {
                Some((enum_name.clone(), case_name.clone()))
            }
            _ => None,
        }
    }

    pub fn class_string_argument(
        self,
        db: &'db dyn salsa::Database,
    ) -> Option<Option<TypeId<'db>>> {
        match self.data(db) {
            TypeData::ClassString { argument } => Some(*argument),
            _ => None,
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types`
Expected: PASS, including the un-ignored `iterable` test.

- [ ] **Step 5: Full verification and commit**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_types Cargo.lock
git commit -m "✨ feat(types): class types, enum cases, class-string, and the placeholders"
```

---
### Task 5: Callables, templates, and the symbolic forms

**Files:**
- Modify: `crates/celerrate_types/src/representation.rs` (`Callable`, `CallableParameter`, `Template`, `KeyOf`, `ValueOf`, `Conditional`)
- Modify: `crates/celerrate_types/src/ordering.rs` (ranks 14 to 18)
- Modify: `crates/celerrate_types/src/construction.rs`
- Modify: `crates/celerrate_types/src/lib.rs` (re-export `CallableParameter`)
- Test: inline module in `construction.rs`

**Interfaces:**
- Consumes: Tasks 1 to 4.
- Produces: `TypeId::callable`, `template`, `key_of`, `value_of`, `conditional`; `CallableParameter<'db>` (public); interrogation `callable_return`, `callable_parameters`, `template_bound`. Task 8 reads `template_bound` for the bound rule and builds branch unions from `Conditional` data.

- [ ] **Step 1: Write the failing tests**

Append to the test module in `construction.rs` (import `crate::CallableParameter`):

```rust
    #[test]
    fn callables_carry_parameters_and_return() {
        let db = TestDatabase::default();
        let callable = TypeId::callable(
            &db,
            vec![
                CallableParameter {
                    parameter_type: TypeId::int(&db),
                    optional: false,
                    variadic: false,
                    by_reference: false,
                },
                CallableParameter {
                    parameter_type: TypeId::string(&db),
                    optional: true,
                    variadic: false,
                    by_reference: false,
                },
            ],
            TypeId::bool(&db),
        );
        assert_eq!(callable.callable_return(&db), Some(TypeId::bool(&db)));
        assert_eq!(callable.callable_parameters(&db).unwrap().len(), 2);
        assert_eq!(TypeId::int(&db).callable_return(&db), None);
    }

    #[test]
    fn templates_are_scoped_lattice_citizens() {
        let db = TestDatabase::default();
        let bound = TypeId::class(&db, "FormTypeInterface", vec![]);
        let one = TypeId::template(&db, "app\\form::configure", "T", bound);
        let same = TypeId::template(&db, "app\\form::configure", "T", bound);
        let other_scope = TypeId::template(&db, "app\\other::configure", "T", bound);
        assert_eq!(one, same);
        assert_ne!(one, other_scope);
        assert_eq!(one.template_bound(&db), Some(bound));
        assert_eq!(TypeId::int(&db).template_bound(&db), None);
    }

    #[test]
    fn key_of_and_value_of_evaluate_decidable_subjects() {
        let db = TestDatabase::default();
        let shape = TypeId::shape(
            &db,
            vec![
                ShapeField { key: ShapeKey::String("id".to_owned()), optional: false, value: TypeId::int(&db) },
                ShapeField { key: ShapeKey::String("name".to_owned()), optional: true, value: TypeId::string(&db) },
            ],
        );
        assert_eq!(
            TypeId::key_of(&db, shape),
            TypeId::union(
                &db,
                [TypeId::string_literal(&db, "id"), TypeId::string_literal(&db, "name")]
            )
        );
        assert_eq!(
            TypeId::value_of(&db, shape),
            TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)])
        );
        let array = TypeId::array(&db, TypeId::string(&db), TypeId::int(&db));
        assert_eq!(TypeId::key_of(&db, array), TypeId::string(&db));
        assert_eq!(TypeId::value_of(&db, array), TypeId::int(&db));
        // Undecidable subjects stay symbolic (and distinct).
        let template = TypeId::template(&db, "scope", "T", TypeId::mixed(&db));
        assert_ne!(TypeId::key_of(&db, template), TypeId::value_of(&db, template));
        assert_ne!(TypeId::key_of(&db, template), template);
    }

    #[test]
    fn conditionals_stay_symbolic_and_structural() {
        let db = TestDatabase::default();
        let subject = TypeId::template(&db, "scope", "T", TypeId::mixed(&db));
        let positive = TypeId::conditional(
            &db, subject, TypeId::int(&db), TypeId::string(&db), TypeId::bool(&db), false,
        );
        let negated = TypeId::conditional(
            &db, subject, TypeId::int(&db), TypeId::string(&db), TypeId::bool(&db), true,
        );
        assert_ne!(positive, negated);
        assert_eq!(
            positive,
            TypeId::conditional(
                &db, subject, TypeId::int(&db), TypeId::string(&db), TypeId::bool(&db), false,
            )
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types`
Expected: compilation failure.

- [ ] **Step 3: Implement**

`representation.rs` additions:

```rust
/// One parameter of a callable signature.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CallableParameter<'db> {
    pub parameter_type: TypeId<'db>,
    pub optional: bool,
    pub variadic: bool,
    pub by_reference: bool,
}
```

`TypeData` additions:

```rust
    /// A callable signature. Rank 14.
    Callable { parameters: Vec<CallableParameter<'db>>, return_type: TypeId<'db> },
    /// A template variable: a lattice citizen before any call-site
    /// substitution. The scope string discriminates same-named
    /// templates of different declarations. Rank 15.
    Template { scope: String, name: String, bound: TypeId<'db> },
    /// Symbolic `key-of<T>` (decidable subjects evaluated at
    /// construction). Rank 16.
    KeyOf { subject: TypeId<'db> },
    /// Symbolic `value-of<T>`. Rank 17.
    ValueOf { subject: TypeId<'db> },
    /// A conditional return type, evaluated at the call site (plan 6);
    /// judgments fall back to the branch union. Rank 18.
    Conditional {
        subject: TypeId<'db>,
        matches: TypeId<'db>,
        then_branch: TypeId<'db>,
        otherwise_branch: TypeId<'db>,
        negated: bool,
    },
```

`ordering.rs` `rank` additions: `Callable => 14`, `Template => 15`, `KeyOf => 16`, `ValueOf => 17`, `Conditional => 18`. Comparison arms:

```rust
        (
            TypeData::Callable { parameters: a_parameters, return_type: a_return },
            TypeData::Callable { parameters: b_parameters, return_type: b_return },
        ) => order_callable_parameters(db, a_parameters, b_parameters)
            .then_with(|| structural_order(db, *a_return, *b_return)),
        (
            TypeData::Template { scope: a_scope, name: a_name, bound: a_bound },
            TypeData::Template { scope: b_scope, name: b_name, bound: b_bound },
        ) => a_scope
            .cmp(b_scope)
            .then_with(|| a_name.cmp(b_name))
            .then_with(|| structural_order(db, *a_bound, *b_bound)),
        (TypeData::KeyOf { subject: a }, TypeData::KeyOf { subject: b })
        | (TypeData::ValueOf { subject: a }, TypeData::ValueOf { subject: b }) => {
            structural_order(db, *a, *b)
        }
        (
            TypeData::Conditional {
                subject: a_subject, matches: a_matches,
                then_branch: a_then, otherwise_branch: a_otherwise, negated: a_negated,
            },
            TypeData::Conditional {
                subject: b_subject, matches: b_matches,
                then_branch: b_then, otherwise_branch: b_otherwise, negated: b_negated,
            },
        ) => structural_order(db, *a_subject, *b_subject)
            .then_with(|| structural_order(db, *a_matches, *b_matches))
            .then_with(|| structural_order(db, *a_then, *b_then))
            .then_with(|| structural_order(db, *a_otherwise, *b_otherwise))
            .then_with(|| a_negated.cmp(b_negated)),
```

With the helper next to `order_types`:

```rust
fn order_callable_parameters<'db>(
    db: &'db dyn salsa::Database,
    left: &[CallableParameter<'db>],
    right: &[CallableParameter<'db>],
) -> Ordering {
    for (a, b) in left.iter().zip(right.iter()) {
        let ordering = (a.variadic, a.optional, a.by_reference)
            .cmp(&(b.variadic, b.optional, b.by_reference))
            .then_with(|| structural_order(db, a.parameter_type, b.parameter_type));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}
```

`construction.rs` additions:

```rust
use crate::representation::CallableParameter;

impl<'db> TypeId<'db> {
    pub fn callable(
        db: &'db dyn salsa::Database,
        parameters: Vec<CallableParameter<'db>>,
        return_type: TypeId<'db>,
    ) -> Self {
        Self::intern(db, TypeData::Callable { parameters, return_type })
    }

    /// A template variable. The scope is the declaring symbol's folded
    /// key by convention (`<class key>::<member key>` or a function
    /// key); plans 3 and 4 produce it. Callers fold; this constructor
    /// stores the scope verbatim.
    pub fn template(
        db: &'db dyn salsa::Database,
        scope: &str,
        name: &str,
        bound: TypeId<'db>,
    ) -> Self {
        Self::intern(
            db,
            TypeData::Template { scope: scope.to_owned(), name: name.to_owned(), bound },
        )
    }

    /// `key-of<subject>`: evaluates shapes (union of key literals) and
    /// arrays (the key type); anything else stays symbolic.
    pub fn key_of(db: &'db dyn salsa::Database, subject: TypeId<'db>) -> Self {
        match subject.data(db) {
            TypeData::Shape { fields } => Self::shape_as_array(db, fields).0,
            TypeData::Array { key, .. } => *key,
            _ => Self::intern(db, TypeData::KeyOf { subject }),
        }
    }

    /// `value-of<subject>`: evaluates shapes (union of field values)
    /// and arrays (the value type); enums need member facts and stay
    /// symbolic until plan 3.
    pub fn value_of(db: &'db dyn salsa::Database, subject: TypeId<'db>) -> Self {
        match subject.data(db) {
            TypeData::Shape { fields } => Self::shape_as_array(db, fields).1,
            TypeData::Array { value, .. } => *value,
            _ => Self::intern(db, TypeData::ValueOf { subject }),
        }
    }

    pub fn conditional(
        db: &'db dyn salsa::Database,
        subject: TypeId<'db>,
        matches: TypeId<'db>,
        then_branch: TypeId<'db>,
        otherwise_branch: TypeId<'db>,
        negated: bool,
    ) -> Self {
        Self::intern(
            db,
            TypeData::Conditional { subject, matches, then_branch, otherwise_branch, negated },
        )
    }

    pub fn callable_return(self, db: &'db dyn salsa::Database) -> Option<TypeId<'db>> {
        match self.data(db) {
            TypeData::Callable { return_type, .. } => Some(*return_type),
            _ => None,
        }
    }

    pub fn callable_parameters(
        self,
        db: &'db dyn salsa::Database,
    ) -> Option<Vec<CallableParameter<'db>>> {
        match self.data(db) {
            TypeData::Callable { parameters, .. } => Some(parameters.clone()),
            _ => None,
        }
    }

    pub fn template_bound(self, db: &'db dyn salsa::Database) -> Option<TypeId<'db>> {
        match self.data(db) {
            TypeData::Template { bound, .. } => Some(*bound),
            _ => None,
        }
    }
}
```

`lib.rs`: re-export `CallableParameter`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types`
Expected: PASS. The representation is now complete: every spec section 3 form has a variant or a desugaring.

- [ ] **Step 5: Full verification and commit**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_types
git commit -m "✨ feat(types): callables, scoped templates, and the symbolic forms"
```

---

### Task 6: Depth, the caps, widening, and join

**Files:**
- Create: `crates/celerrate_types/src/widening.rs`
- Modify: `crates/celerrate_types/src/construction.rs` (wire the caps at the two `// cap point` markers and into the composite constructors)
- Modify: `crates/celerrate_types/src/lib.rs` (`mod widening;` + re-export `UNION_ARITY_CAP`, `STRUCTURAL_DEPTH_CAP`, `join`, `widened_literals`)
- Test: inline module in `widening.rs`

**Interfaces:**
- Consumes: the complete representation (Tasks 1 to 5).
- Produces: `UNION_ARITY_CAP: usize = 32`, `STRUCTURAL_DEPTH_CAP: u32 = 16`, `depth_of(db, of) -> u32` (`pub(crate)`), `join(db, left, right) -> TypeId` (public), `widened_literals(db, of) -> TypeId` (public), and `pub(crate) fn capped_child(db, child) -> TypeId` used by every composite constructor. Plan 5's fixpoint discipline consumes `join` and `widened_literals`; both must be deterministic lattice operations identical regardless of a cycle's entry point (spec section 3).

- [ ] **Step 1: Write the failing tests**

`crates/celerrate_types/src/widening.rs` starts as its test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use super::{STRUCTURAL_DEPTH_CAP, UNION_ARITY_CAP, depth_of, join, widened_literals};
    use crate::TypeId;

    #[test]
    fn depth_counts_nesting() {
        let db = TestDatabase::default();
        assert_eq!(depth_of(&db, TypeId::int(&db)), 1);
        let array = TypeId::array(&db, TypeId::string(&db), TypeId::int(&db));
        assert_eq!(depth_of(&db, array), 2);
        let nested = TypeId::array(&db, TypeId::string(&db), array);
        assert_eq!(depth_of(&db, nested), 3);
    }

    #[test]
    fn an_oversized_union_collapses_to_the_join_never_a_subset() {
        let db = TestDatabase::default();
        let literals = (0..(UNION_ARITY_CAP as i64 + 8)).map(|value| TypeId::int_literal(&db, value));
        let collapsed = TypeId::union(&db, literals);
        // The join of integer literals is their range hull.
        assert_eq!(
            collapsed,
            TypeId::int_range(&db, Some(0), Some(UNION_ARITY_CAP as i64 + 7))
        );
    }

    #[test]
    fn an_oversized_mixed_kind_union_collapses_to_mixed() {
        let db = TestDatabase::default();
        let mut parts: Vec<TypeId> =
            (0..(UNION_ARITY_CAP as i64 + 8)).map(|value| TypeId::int_literal(&db, value)).collect();
        parts.push(TypeId::class(&db, "User", vec![]));
        assert_eq!(TypeId::union(&db, parts), TypeId::mixed(&db));
    }

    #[test]
    fn depth_beyond_the_cap_widens_the_deepest_children_to_mixed() {
        let db = TestDatabase::default();
        let mut current = TypeId::int(&db);
        for _ in 0..(STRUCTURAL_DEPTH_CAP + 8) {
            current = TypeId::array(&db, TypeId::string(&db), current);
        }
        assert!(depth_of(&db, current) <= STRUCTURAL_DEPTH_CAP);
    }

    #[test]
    fn the_cap_applies_to_every_growing_constructor() {
        let db = TestDatabase::default();
        let mut current = TypeId::int(&db);
        for _ in 0..(STRUCTURAL_DEPTH_CAP + 8) {
            current = TypeId::class(&db, "Collection", vec![current]);
        }
        assert!(depth_of(&db, current) <= STRUCTURAL_DEPTH_CAP);
    }

    #[test]
    fn join_is_the_deterministic_common_supertype() {
        let db = TestDatabase::default();
        assert_eq!(join(&db, TypeId::int(&db), TypeId::int(&db)), TypeId::int(&db));
        assert_eq!(
            join(&db, TypeId::int_literal(&db, 1), TypeId::int_literal(&db, 5)),
            TypeId::int_range(&db, Some(1), Some(5))
        );
        assert_eq!(
            join(&db, TypeId::bool_literal(&db, true), TypeId::bool(&db)),
            TypeId::bool(&db)
        );
        assert_eq!(
            join(&db, TypeId::string_literal(&db, "a"), TypeId::non_empty_string(&db)),
            TypeId::string(&db)
        );
        assert_eq!(join(&db, TypeId::never(&db), TypeId::int(&db)), TypeId::int(&db));
        assert_eq!(join(&db, TypeId::null(&db), TypeId::int(&db)), TypeId::mixed(&db));
        // Arrays join structurally: the list flag drops (only one side is
        // a list), keys join through the hierarchy-blind rule (int and
        // string join to mixed), values take the range hull.
        let of_int = TypeId::list(&db, TypeId::int_literal(&db, 1));
        let of_string = TypeId::array(&db, TypeId::string(&db), TypeId::int_literal(&db, 2));
        assert_eq!(
            join(&db, of_int, of_string),
            TypeId::array(
                &db,
                TypeId::mixed(&db),
                TypeId::int_range(&db, Some(1), Some(2)),
            )
        );
        // Same-name classes join argumentwise; unrelated classes join to mixed.
        assert_eq!(
            join(
                &db,
                TypeId::class(&db, "Collection", vec![TypeId::int_literal(&db, 1)]),
                TypeId::class(&db, "Collection", vec![TypeId::int_literal(&db, 2)]),
            ),
            TypeId::class(&db, "Collection", vec![TypeId::int_range(&db, Some(1), Some(2))])
        );
        assert_eq!(
            join(&db, TypeId::class(&db, "User", vec![]), TypeId::class(&db, "Order", vec![])),
            TypeId::mixed(&db)
        );
    }

    #[test]
    fn widening_generalizes_literals_recursively() {
        let db = TestDatabase::default();
        assert_eq!(widened_literals(&db, TypeId::int_literal(&db, 42)), TypeId::int(&db));
        assert_eq!(
            widened_literals(&db, TypeId::string_literal(&db, "active")),
            TypeId::string(&db)
        );
        assert_eq!(widened_literals(&db, TypeId::bool_literal(&db, true)), TypeId::bool(&db));
        assert_eq!(widened_literals(&db, TypeId::float_literal(&db, 1.5)), TypeId::float(&db));
        // Bounded ranges are not literals and stay.
        let range = TypeId::int_range(&db, Some(1), None);
        assert_eq!(widened_literals(&db, range), range);
        // Recursion into unions and arrays; class arguments stay (invariance).
        let nullable_literal = TypeId::union(&db, [TypeId::int_literal(&db, 1), TypeId::null(&db)]);
        assert_eq!(
            widened_literals(&db, nullable_literal),
            TypeId::union(&db, [TypeId::int(&db), TypeId::null(&db)])
        );
        let list = TypeId::list(&db, TypeId::int_literal(&db, 1));
        assert_eq!(widened_literals(&db, list), TypeId::list(&db, TypeId::int(&db)));
        let generic = TypeId::class(&db, "Collection", vec![TypeId::int_literal(&db, 1)]);
        assert_eq!(widened_literals(&db, generic), generic);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types`
Expected: compilation failure (`widening` module missing).

- [ ] **Step 3: Implement `widening.rs`**

```rust
//! Widening and the deterministic caps: fixpoint termination (plan 5)
//! depends on these being pure lattice operations, identical regardless
//! of a cycle's entry point (spec section 3). An arity overrun collapses
//! to the pairwise join (a common supertype, `mixed` at worst), never a
//! truncated subset, which would make the value depend on accumulation
//! order.

use crate::representation::{StringConstraint, TypeData, TypeId};

/// A union with more constituents collapses to its join.
pub const UNION_ARITY_CAP: usize = 32;

/// No canonical type nests deeper than this; construction widens the
/// children sitting at the cap to `mixed`.
pub const STRUCTURAL_DEPTH_CAP: u32 = 16;

/// Structural depth: atoms are 1, composites 1 + the deepest child.
pub(crate) fn depth_of<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> u32 {
    let children_depth = |types: &[TypeId<'db>]| {
        types.iter().map(|child| depth_of(db, *child)).max().unwrap_or(0)
    };
    match of.data(db) {
        TypeData::Mixed
        | TypeData::Never
        | TypeData::Void
        | TypeData::Null
        | TypeData::Object
        | TypeData::Resource
        | TypeData::Bool { .. }
        | TypeData::Int { .. }
        | TypeData::Float { .. }
        | TypeData::String { .. }
        | TypeData::EnumCase { .. }
        | TypeData::SelfPlaceholder
        | TypeData::ParentPlaceholder
        | TypeData::StaticPlaceholder => 1,
        TypeData::ClassString { argument } => {
            1 + argument.map(|child| depth_of(db, child)).unwrap_or(0)
        }
        TypeData::Union { constituents } => 1 + children_depth(constituents),
        TypeData::Intersection { intersectands } => 1 + children_depth(intersectands),
        TypeData::Array { key, value, .. } => 1 + children_depth(&[*key, *value]),
        TypeData::Shape { fields } => {
            1 + fields.iter().map(|field| depth_of(db, field.value)).max().unwrap_or(0)
        }
        TypeData::Class { arguments, .. } => 1 + children_depth(arguments),
        TypeData::Callable { parameters, return_type } => {
            let parameter_depth = parameters
                .iter()
                .map(|parameter| depth_of(db, parameter.parameter_type))
                .max()
                .unwrap_or(0);
            1 + parameter_depth.max(depth_of(db, *return_type))
        }
        TypeData::Template { bound, .. } => 1 + depth_of(db, *bound),
        TypeData::KeyOf { subject } | TypeData::ValueOf { subject } => 1 + depth_of(db, *subject),
        TypeData::Conditional { subject, matches, then_branch, otherwise_branch, .. } => {
            1 + children_depth(&[*subject, *matches, *then_branch, *otherwise_branch])
        }
    }
}

/// A child about to enter a composite: children already at the depth
/// cap widen to `mixed`, so every constructor's result stays at the cap
/// regardless of construction order.
pub(crate) fn capped_child<'db>(db: &'db dyn salsa::Database, child: TypeId<'db>) -> TypeId<'db> {
    if depth_of(db, child) >= STRUCTURAL_DEPTH_CAP {
        TypeId::mixed(db)
    } else {
        child
    }
}

/// The deterministic pairwise join: a common supertype, hierarchy-blind
/// in this plan (unrelated classes join to `mixed`; a hierarchy-aware
/// least upper bound can refine this later without a signature change).
pub fn join<'db>(db: &'db dyn salsa::Database, left: TypeId<'db>, right: TypeId<'db>) -> TypeId<'db> {
    if left == right {
        return left;
    }
    // Unions distribute through the join.
    if let TypeData::Union { constituents } = left.data(db) {
        return constituents.iter().fold(right, |accumulated, part| join(db, accumulated, *part));
    }
    if let TypeData::Union { constituents } = right.data(db) {
        return constituents.iter().fold(left, |accumulated, part| join(db, accumulated, *part));
    }
    match (left.data(db), right.data(db)) {
        (TypeData::Never, _) => right,
        (_, TypeData::Never) => left,
        (TypeData::Mixed, _) | (_, TypeData::Mixed) => TypeId::mixed(db),
        (TypeData::Bool { .. }, TypeData::Bool { .. }) => TypeId::bool(db),
        (
            TypeData::Int { minimum: a_min, maximum: a_max },
            TypeData::Int { minimum: b_min, maximum: b_max },
        ) => {
            let minimum = match (a_min, b_min) {
                (Some(a), Some(b)) => Some(*a.min(b)),
                _ => None,
            };
            let maximum = match (a_max, b_max) {
                (Some(a), Some(b)) => Some(*a.max(b)),
                _ => None,
            };
            TypeId::int_range(db, minimum, maximum)
        }
        (TypeData::Float { .. }, TypeData::Float { .. }) => TypeId::float(db),
        (TypeData::String { .. }, TypeData::String { .. }) => TypeId::string(db),
        (
            TypeData::Array { key: a_key, value: a_value, is_list: a_list, non_empty: a_non_empty },
            TypeData::Array { key: b_key, value: b_value, is_list: b_list, non_empty: b_non_empty },
        ) => {
            let key = join(db, *a_key, *b_key);
            let value = join(db, *a_value, *b_value);
            match (a_list && b_list, a_non_empty && b_non_empty) {
                (true, true) => TypeId::non_empty_list(db, value),
                (true, false) => TypeId::list(db, value),
                (false, true) => TypeId::non_empty_array(db, key, value),
                (false, false) => TypeId::array(db, key, value),
            }
        }
        (TypeData::Shape { fields }, _) => {
            let (key, value, is_list, non_empty) = TypeId::shape_as_array(db, fields);
            let widened = if is_list && non_empty {
                TypeId::non_empty_list(db, value)
            } else if is_list {
                TypeId::list(db, value)
            } else if non_empty {
                TypeId::non_empty_array(db, key, value)
            } else {
                TypeId::array(db, key, value)
            };
            join(db, widened, right)
        }
        (_, TypeData::Shape { .. }) => join(db, right, left),
        (
            TypeData::Class { name: a_name, arguments: a_arguments },
            TypeData::Class { name: b_name, arguments: b_arguments },
        ) if a_name == b_name => {
            if a_arguments.len() == b_arguments.len() {
                let joined = a_arguments
                    .iter()
                    .zip(b_arguments.iter())
                    .map(|(a, b)| join(db, *a, *b))
                    .collect();
                TypeId::class(db, a_name, joined)
            } else {
                TypeId::class(db, a_name, vec![])
            }
        }
        (
            TypeData::EnumCase { enum_name: a_enum, .. },
            TypeData::EnumCase { enum_name: b_enum, .. },
        ) if a_enum == b_enum => TypeId::class(db, a_enum, vec![]),
        (TypeData::EnumCase { enum_name, .. }, TypeData::Class { name, arguments })
        | (TypeData::Class { name, arguments }, TypeData::EnumCase { enum_name, .. })
            if enum_name == name && arguments.is_empty() =>
        {
            TypeId::class(db, name, vec![])
        }
        _ => TypeId::mixed(db),
    }
}

/// Literal-to-general widening, recursive through unions, intersections,
/// arrays, and shape field values. Class arguments, callables, templates,
/// and the symbolic forms keep their structure (invariance; substitution
/// is plan 6's).
pub fn widened_literals<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> TypeId<'db> {
    match of.data(db) {
        TypeData::Bool { literal: Some(_) } => TypeId::bool(db),
        TypeData::Int { minimum: Some(low), maximum: Some(high) } if low == high => TypeId::int(db),
        TypeData::Float { literal: Some(_) } => TypeId::float(db),
        TypeData::String { constraint: StringConstraint::Literal(_) } => TypeId::string(db),
        TypeData::Union { constituents } => {
            TypeId::union(db, constituents.iter().map(|part| widened_literals(db, *part)))
        }
        TypeData::Intersection { intersectands } => {
            TypeId::intersection(db, intersectands.iter().map(|part| widened_literals(db, *part)))
        }
        TypeData::Array { key, value, is_list, non_empty } => {
            let widened_key = widened_literals(db, *key);
            let widened_value = widened_literals(db, *value);
            match (is_list, non_empty) {
                (true, true) => TypeId::non_empty_list(db, widened_value),
                (true, false) => TypeId::list(db, widened_value),
                (false, true) => TypeId::non_empty_array(db, widened_key, widened_value),
                (false, false) => TypeId::array(db, widened_key, widened_value),
            }
        }
        TypeData::Shape { fields } => TypeId::shape(
            db,
            fields
                .iter()
                .map(|field| crate::ShapeField {
                    key: field.key.clone(),
                    optional: field.optional,
                    value: widened_literals(db, field.value),
                })
                .collect(),
        ),
        _ => of,
    }
}
```

- [ ] **Step 4: Wire the caps into the constructors**

In `construction.rs`:

- At the union `// cap point`: after deduplication, `if flat.len() > UNION_ARITY_CAP { return flat.iter().copied().fold(Self::never(db), |accumulated, part| crate::widening::join(db, accumulated, part)); }`
- At the intersection `// cap point`: `flat.truncate(UNION_ARITY_CAP);` (sorted, so deterministic; dropping intersectands over-approximates soundly).
- Every composite constructor passes its children through `crate::widening::capped_child` before interning: `array`/`non_empty_array`/`list`/`non_empty_list` (key and value), `shape` (field values), `class` (arguments), `enum_case` (nothing to cap), `callable` (parameter types and return), `class_string` (the argument), `template` (the bound), `key_of`/`value_of` (the subject, after the decidable evaluation), `conditional` (all four children), and `union`/`intersection` (each flattened part). Apply it mechanically: `let key = capped_child(db, key);` at the top of each constructor.

`lib.rs`: `mod widening;` and `pub use widening::{STRUCTURAL_DEPTH_CAP, UNION_ARITY_CAP, join, widened_literals};`

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types`
Expected: PASS, all previous tasks' tests included (the caps must not disturb small types).

- [ ] **Step 6: Full verification and commit**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_types
git commit -m "✨ feat(types): deterministic widening, join, and the arity and depth caps"
```

---
### Task 7: Deterministic rendering

**Files:**
- Create: `crates/celerrate_types/src/display.rs`
- Modify: `crates/celerrate_types/src/construction.rs` (the `display` method)
- Modify: `crates/celerrate_types/src/lib.rs` (`mod display;`)
- Test: inline module in `display.rs`

**Interfaces:**
- Consumes: the complete representation.
- Produces: `TypeId::display(db) -> String`, PHPStan-flavored, deterministic. Tests across the crate use it for readable assertions; plan 8's messages consume it (with the folded-name debt recorded in Task 4).

- [ ] **Step 1: Write the failing tests**

`crates/celerrate_types/src/display.rs` starts as its test module:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use crate::{CallableParameter, ShapeField, ShapeKey, TypeId};

    #[test]
    fn atoms_and_literals_render() {
        let db = TestDatabase::default();
        assert_eq!(TypeId::mixed(&db).display(&db), "mixed");
        assert_eq!(TypeId::never(&db).display(&db), "never");
        assert_eq!(TypeId::void(&db).display(&db), "void");
        assert_eq!(TypeId::null(&db).display(&db), "null");
        assert_eq!(TypeId::object(&db).display(&db), "object");
        assert_eq!(TypeId::resource(&db).display(&db), "resource");
        assert_eq!(TypeId::bool(&db).display(&db), "bool");
        assert_eq!(TypeId::bool_literal(&db, true).display(&db), "true");
        assert_eq!(TypeId::int(&db).display(&db), "int");
        assert_eq!(TypeId::int_literal(&db, 42).display(&db), "42");
        assert_eq!(TypeId::int_range(&db, Some(1), None).display(&db), "int<1, max>");
        assert_eq!(TypeId::int_range(&db, None, Some(5)).display(&db), "int<min, 5>");
        assert_eq!(TypeId::float(&db).display(&db), "float");
        assert_eq!(TypeId::float_literal(&db, 1.5).display(&db), "1.5");
        assert_eq!(TypeId::string(&db).display(&db), "string");
        assert_eq!(TypeId::non_empty_string(&db).display(&db), "non-empty-string");
        assert_eq!(TypeId::numeric_string(&db).display(&db), "numeric-string");
        assert_eq!(TypeId::literal_string_type(&db).display(&db), "literal-string");
        assert_eq!(TypeId::string_literal(&db, "active").display(&db), "'active'");
    }

    #[test]
    fn composites_render_with_null_last_in_unions() {
        let db = TestDatabase::default();
        let nullable = TypeId::union(&db, [TypeId::null(&db), TypeId::class(&db, "User", vec![])]);
        assert_eq!(nullable.display(&db), "user|null");
        assert_eq!(
            TypeId::intersection(&db, [TypeId::class(&db, "Foo", vec![]), TypeId::class(&db, "Countable", vec![])])
                .display(&db),
            "countable&foo"
        );
        assert_eq!(
            TypeId::array(&db, TypeId::string(&db), TypeId::int(&db)).display(&db),
            "array<string, int>"
        );
        assert_eq!(TypeId::list(&db, TypeId::int(&db)).display(&db), "list<int>");
        assert_eq!(
            TypeId::non_empty_array(&db, TypeId::string(&db), TypeId::int(&db)).display(&db),
            "non-empty-array<string, int>"
        );
        assert_eq!(
            TypeId::non_empty_list(&db, TypeId::int(&db)).display(&db),
            "non-empty-list<int>"
        );
        let shape = TypeId::shape(
            &db,
            vec![
                ShapeField { key: ShapeKey::String("id".to_owned()), optional: false, value: TypeId::int(&db) },
                ShapeField { key: ShapeKey::String("name".to_owned()), optional: true, value: TypeId::string(&db) },
            ],
        );
        assert_eq!(shape.display(&db), "array{id: int, name?: string}");
        assert_eq!(
            TypeId::class(&db, "Collection", vec![TypeId::class(&db, "User", vec![])]).display(&db),
            "collection<user>"
        );
        assert_eq!(TypeId::enum_case(&db, "Status", "Active").display(&db), "status::Active");
        assert_eq!(TypeId::class_string(&db, None).display(&db), "class-string");
        let callable = TypeId::callable(
            &db,
            vec![
                CallableParameter { parameter_type: TypeId::int(&db), optional: false, variadic: false, by_reference: false },
                CallableParameter { parameter_type: TypeId::string(&db), optional: true, variadic: false, by_reference: false },
                CallableParameter { parameter_type: TypeId::bool(&db), optional: false, variadic: true, by_reference: false },
            ],
            TypeId::void(&db),
        );
        assert_eq!(callable.display(&db), "callable(int, string=, bool...): void");
        let template = TypeId::template(&db, "scope", "T", TypeId::class(&db, "Foo", vec![]));
        assert_eq!(template.display(&db), "T of foo");
        assert_eq!(
            TypeId::template(&db, "scope", "T", TypeId::mixed(&db)).display(&db),
            "T"
        );
        let symbolic = TypeId::key_of(&db, template);
        assert_eq!(symbolic.display(&db), "key-of<T of foo>");
        assert_eq!(TypeId::static_placeholder(&db).display(&db), "static");
        assert_eq!(TypeId::self_placeholder(&db).display(&db), "self");
        assert_eq!(TypeId::parent_placeholder(&db).display(&db), "parent");
    }

    #[test]
    fn nested_unions_inside_intersections_are_parenthesized() {
        let db = TestDatabase::default();
        let union = TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]);
        let intersection =
            TypeId::intersection(&db, [union, TypeId::class(&db, "Countable", vec![])]);
        assert_eq!(intersection.display(&db), "countable&(int|string)");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types`
Expected: compilation failure (`display` missing).

- [ ] **Step 3: Implement `display.rs`**

```rust
//! Deterministic rendering: PHPStan-flavored spellings over the
//! canonical structure. Class names render as their folded keys in this
//! plan (recorded debt: plan 8 recovers original spellings through the
//! symbol table when rendering diagnostics).

use crate::representation::{StringConstraint, TypeData, TypeId};

pub(crate) fn display_type<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> String {
    match of.data(db) {
        TypeData::Mixed => "mixed".to_owned(),
        TypeData::Never => "never".to_owned(),
        TypeData::Void => "void".to_owned(),
        TypeData::Null => "null".to_owned(),
        TypeData::Object => "object".to_owned(),
        TypeData::Resource => "resource".to_owned(),
        TypeData::Bool { literal: None } => "bool".to_owned(),
        TypeData::Bool { literal: Some(true) } => "true".to_owned(),
        TypeData::Bool { literal: Some(false) } => "false".to_owned(),
        TypeData::Int { minimum: None, maximum: None } => "int".to_owned(),
        TypeData::Int { minimum: Some(low), maximum: Some(high) } if low == high => low.to_string(),
        TypeData::Int { minimum, maximum } => format!(
            "int<{}, {}>",
            minimum.map_or_else(|| "min".to_owned(), |bound| bound.to_string()),
            maximum.map_or_else(|| "max".to_owned(), |bound| bound.to_string()),
        ),
        TypeData::Float { literal: None } => "float".to_owned(),
        TypeData::Float { literal: Some(bits) } => format!("{}", bits.value()),
        TypeData::String { constraint: StringConstraint::General } => "string".to_owned(),
        TypeData::String { constraint: StringConstraint::NonEmpty } => "non-empty-string".to_owned(),
        TypeData::String { constraint: StringConstraint::Numeric } => "numeric-string".to_owned(),
        TypeData::String { constraint: StringConstraint::LiteralMarker } => "literal-string".to_owned(),
        TypeData::String { constraint: StringConstraint::Literal(value) } => format!("'{value}'"),
        TypeData::ClassString { argument: None } => "class-string".to_owned(),
        TypeData::ClassString { argument: Some(argument) } => {
            format!("class-string<{}>", display_type(db, *argument))
        }
        TypeData::Union { constituents } => {
            // Render null last: `User|null`, the conventional spelling.
            let (null_parts, other_parts): (Vec<_>, Vec<_>) =
                constituents.iter().partition(|part| part.is_null(db));
            let mut rendered: Vec<String> =
                other_parts.iter().map(|part| parenthesized(db, **part)).collect();
            rendered.extend(null_parts.iter().map(|part| display_type(db, **part)));
            rendered.join("|")
        }
        TypeData::Intersection { intersectands } => intersectands
            .iter()
            .map(|part| parenthesized(db, *part))
            .collect::<Vec<_>>()
            .join("&"),
        TypeData::Array { key, value, is_list, non_empty } => match (is_list, non_empty) {
            (true, true) => format!("non-empty-list<{}>", display_type(db, *value)),
            (true, false) => format!("list<{}>", display_type(db, *value)),
            (false, true) => format!(
                "non-empty-array<{}, {}>",
                display_type(db, *key),
                display_type(db, *value)
            ),
            (false, false) => {
                format!("array<{}, {}>", display_type(db, *key), display_type(db, *value))
            }
        },
        TypeData::Shape { fields } => {
            let rendered: Vec<String> = fields
                .iter()
                .map(|field| {
                    let key = match &field.key {
                        crate::ShapeKey::Integer(value) => value.to_string(),
                        crate::ShapeKey::String(value) => value.clone(),
                    };
                    let marker = if field.optional { "?" } else { "" };
                    format!("{key}{marker}: {}", display_type(db, field.value))
                })
                .collect();
            format!("array{{{}}}", rendered.join(", "))
        }
        TypeData::Class { name, arguments } => {
            if arguments.is_empty() {
                name.clone()
            } else {
                let rendered: Vec<String> =
                    arguments.iter().map(|argument| display_type(db, *argument)).collect();
                format!("{name}<{}>", rendered.join(", "))
            }
        }
        TypeData::EnumCase { enum_name, case_name } => format!("{enum_name}::{case_name}"),
        TypeData::Callable { parameters, return_type } => {
            let rendered: Vec<String> = parameters
                .iter()
                .map(|parameter| {
                    let mut text = display_type(db, parameter.parameter_type);
                    if parameter.by_reference {
                        text.push_str(" &");
                    }
                    if parameter.variadic {
                        text.push_str("...");
                    } else if parameter.optional {
                        text.push('=');
                    }
                    text
                })
                .collect();
            format!("callable({}): {}", rendered.join(", "), display_type(db, *return_type))
        }
        TypeData::Template { name, bound, .. } => {
            if bound.is_mixed(db) {
                name.clone()
            } else {
                format!("{name} of {}", display_type(db, *bound))
            }
        }
        TypeData::KeyOf { subject } => format!("key-of<{}>", display_type(db, *subject)),
        TypeData::ValueOf { subject } => format!("value-of<{}>", display_type(db, *subject)),
        TypeData::Conditional { subject, matches, then_branch, otherwise_branch, negated } => {
            let operator = if *negated { "is not" } else { "is" };
            format!(
                "({} {operator} {} ? {} : {})",
                display_type(db, *subject),
                display_type(db, *matches),
                display_type(db, *then_branch),
                display_type(db, *otherwise_branch),
            )
        }
        TypeData::SelfPlaceholder => "self".to_owned(),
        TypeData::ParentPlaceholder => "parent".to_owned(),
        TypeData::StaticPlaceholder => "static".to_owned(),
    }
}

/// Unions and intersections nested inside another compound render in
/// parentheses.
fn parenthesized<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> String {
    let rendered = display_type(db, of);
    match of.data(db) {
        TypeData::Union { .. } | TypeData::Intersection { .. } => format!("({rendered})"),
        _ => rendered,
    }
}
```

And in `construction.rs`:

```rust
impl<'db> TypeId<'db> {
    /// The deterministic, PHPStan-flavored rendering.
    pub fn display(self, db: &'db dyn salsa::Database) -> String {
        crate::display::display_type(db, self)
    }
}
```

`lib.rs`: `mod display;` (no re-export; the method is the surface).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types`
Expected: PASS.

- [ ] **Step 5: Full verification and commit**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_types
git commit -m "✨ feat(types): deterministic type rendering"
```

---
### Task 8: The three-valued judgments, structural half

**Files:**
- Create: `crates/celerrate_types/src/judgments.rs`
- Modify: `crates/celerrate_types/Cargo.toml` (add `celerrate_db`, `celerrate_project`, `celerrate_stubs` as dependencies; `celerrate_db` moves up from dev-dependencies)
- Modify: `crates/celerrate_types/src/lib.rs` (`mod judgments;` + re-export `Proof`, `Nullability`, `subtype_of`, `assignable_to`, `nullability`)
- Test: inline module in `judgments.rs`

**Interfaces:**
- Consumes: the complete representation; `celerrate_db::{AnalyzedFileSet}`, `celerrate_stubs::StubIndexInput`, `celerrate_project::ProjectConfiguration` (query parameters only in this task).
- Produces: `Proof`, `Nullability`, the tracked queries `subtype_of`, `assignable_to`, `nullability`, and the internal recursion `judge(db, context, candidate, target)` with `JudgmentContext` carrying the three inputs. `judge_class_hierarchy(db, context, candidate_name, target_name) -> Proof` answers `CannotProve` for differing names in this task; **Task 9 replaces its body** with the linearization walk without touching any caller.

**The rule list** (the authority; the implementation follows it in order). `Fails` means value-set inclusion is refuted; `CannotProve` means undecidable with available information; no consumer treats `CannotProve` as silent discard (spec section 3).

1. `candidate == target` → Holds (handle equality is structural equality).
2. Target `mixed` → Holds. 3. Candidate `never` → Holds.
4. Target `never` → Fails. 5. Candidate `mixed` → Fails.
6. Candidate union → every constituent against the target, `Proof::all`.
7. Target union → candidate against each constituent, `Proof::any` (one holds → Holds; all fail → Fails; otherwise CannotProve).
8. Candidate intersection → one intersectand holds → Holds; otherwise CannotProve (never Fails: refutation would need disjointness, and an intersection can be empty).
9. Target intersection → candidate against every intersectand, `Proof::all`.
10. Candidate template → its bound against the target: Holds → Holds (`T of Foo <: Foo` holds definitionally through the bound); otherwise CannotProve, never Fails.
11. Target template → CannotProve.
12. Candidate conditional → its branch union against the target: Holds → Holds; otherwise CannotProve. Target conditional → candidate against both branches: both Holds → Holds; otherwise CannotProve.
13. Candidate `key-of` → `int|string` against the target: Holds → Holds; otherwise CannotProve. Candidate `value-of` → CannotProve. Target `key-of`/`value-of` → CannotProve.
14. Any placeholder on either side → CannotProve (symbolic until plan 6's substitution).
15. The ground matrix: scalar inclusion (bool literals, integer range inclusion, the string-constraint table, float literals), `class-string` rules, array flag-and-covariance rules, sealed shape rules, class/enum-case rules through `judge_class_hierarchy`, callable contravariance/covariance (target `void` return accepts any return; a `by_reference` mismatch is CannotProve), `object` as supertype of class-likes, and the CannotProve islands (string/array/object/class versus callable in both directions; string literal versus `class-string`; class versus same-named enum case). Everything else, including every remaining cross-kind pair, → Fails.

- [ ] **Step 1: Move `celerrate_db` and add the input crates**

In `Cargo.toml`, `[dependencies]` gains:

```toml
celerrate_db = { path = "../celerrate_db" }
celerrate_project = { path = "../celerrate_project" }
celerrate_stubs = { path = "../celerrate_stubs" }
```

and the `[dev-dependencies]` entry for `celerrate_db` disappears (a dependency also used in tests needs no dev entry).

- [ ] **Step 2: Write the failing tests**

Bottom of `crates/celerrate_types/src/judgments.rs`:

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{StubIndex, StubIndexInput};

    use super::{Nullability, Proof, assignable_to, nullability, subtype_of};
    use crate::TypeId;

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

    fn judge<'db>(
        fixture: &'db Fixture,
        candidate: TypeId<'db>,
        target: TypeId<'db>,
    ) -> Proof {
        subtype_of(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            candidate,
            target,
        )
    }

    #[test]
    fn the_extremes_anchor_the_lattice() {
        let f = fixture(&[]);
        let db = &f.db;
        assert_eq!(judge(&f, TypeId::int(db), TypeId::mixed(db)), Proof::Holds);
        assert_eq!(judge(&f, TypeId::never(db), TypeId::int(db)), Proof::Holds);
        assert_eq!(judge(&f, TypeId::mixed(db), TypeId::int(db)), Proof::Fails);
        assert_eq!(judge(&f, TypeId::int(db), TypeId::never(db)), Proof::Fails);
        assert_eq!(judge(&f, TypeId::int(db), TypeId::int(db)), Proof::Holds);
    }

    #[test]
    fn scalar_inclusion_follows_the_matrix() {
        let f = fixture(&[]);
        let db = &f.db;
        assert_eq!(judge(&f, TypeId::bool_literal(db, true), TypeId::bool(db)), Proof::Holds);
        assert_eq!(judge(&f, TypeId::bool(db), TypeId::bool_literal(db, true)), Proof::Fails);
        assert_eq!(
            judge(&f, TypeId::int_range(db, Some(1), Some(3)), TypeId::int(db)),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, TypeId::int(db), TypeId::int_range(db, Some(1), Some(3))),
            Proof::Fails
        );
        assert_eq!(
            judge(&f, TypeId::int_literal(db, 2), TypeId::int_range(db, Some(1), Some(3))),
            Proof::Holds
        );
        assert_eq!(judge(&f, TypeId::int(db), TypeId::float(db)), Proof::Fails);
        assert_eq!(judge(&f, TypeId::float_literal(db, 1.5), TypeId::float(db)), Proof::Holds);
    }

    #[test]
    fn the_string_family_matrix_holds() {
        let f = fixture(&[]);
        let db = &f.db;
        let literal = TypeId::string_literal(db, "active");
        assert_eq!(judge(&f, literal, TypeId::string(db)), Proof::Holds);
        assert_eq!(judge(&f, literal, TypeId::non_empty_string(db)), Proof::Holds);
        assert_eq!(judge(&f, literal, TypeId::literal_string_type(db)), Proof::Holds);
        assert_eq!(judge(&f, literal, TypeId::numeric_string(db)), Proof::Fails);
        assert_eq!(
            judge(&f, TypeId::string_literal(db, "42"), TypeId::numeric_string(db)),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, TypeId::string_literal(db, ""), TypeId::non_empty_string(db)),
            Proof::Fails
        );
        assert_eq!(judge(&f, TypeId::numeric_string(db), TypeId::non_empty_string(db)), Proof::Holds);
        assert_eq!(judge(&f, TypeId::non_empty_string(db), TypeId::numeric_string(db)), Proof::Fails);
        assert_eq!(judge(&f, TypeId::string(db), TypeId::non_empty_string(db)), Proof::Fails);
        assert_eq!(judge(&f, TypeId::class_string(db, None), TypeId::string(db)), Proof::Holds);
        assert_eq!(
            judge(&f, TypeId::class_string(db, None), TypeId::non_empty_string(db)),
            Proof::Holds
        );
        assert_eq!(judge(&f, literal, TypeId::class_string(db, None)), Proof::CannotProve);
    }

    #[test]
    fn unions_and_intersections_decompose() {
        let f = fixture(&[]);
        let db = &f.db;
        let nullable_int = TypeId::union(db, [TypeId::int(db), TypeId::null(db)]);
        assert_eq!(judge(&f, TypeId::int(db), nullable_int), Proof::Holds);
        assert_eq!(judge(&f, nullable_int, TypeId::int(db)), Proof::Fails);
        assert_eq!(judge(&f, nullable_int, nullable_int), Proof::Holds);
        let counted = TypeId::intersection(
            db,
            [TypeId::class(db, "Foo", vec![]), TypeId::class(db, "Countable", vec![])],
        );
        assert_eq!(judge(&f, counted, TypeId::class(db, "Foo", vec![])), Proof::Holds);
        assert_eq!(judge(&f, TypeId::class(db, "Foo", vec![]), counted), Proof::CannotProve);
    }

    #[test]
    fn templates_judge_through_their_bounds() {
        let f = fixture(&[]);
        let db = &f.db;
        let bound = TypeId::class(db, "FormTypeInterface", vec![]);
        let template = TypeId::template(db, "scope", "T", bound);
        // T of Foo <: Foo holds definitionally through the bound.
        assert_eq!(judge(&f, template, bound), Proof::Holds);
        assert_eq!(judge(&f, template, template), Proof::Holds);
        assert_eq!(judge(&f, template, TypeId::int(db)), Proof::CannotProve);
        assert_eq!(judge(&f, TypeId::int(db), template), Proof::CannotProve);
    }

    #[test]
    fn arrays_shapes_and_callables_follow_their_rules() {
        let f = fixture(&[]);
        let db = &f.db;
        let list = TypeId::list(db, TypeId::int(db));
        let array = TypeId::array(db, TypeId::int(db), TypeId::int(db));
        assert_eq!(judge(&f, list, array), Proof::Holds);
        assert_eq!(judge(&f, array, list), Proof::Fails);
        assert_eq!(
            judge(&f, TypeId::non_empty_array(db, TypeId::int(db), TypeId::int(db)), array),
            Proof::Holds
        );
        assert_eq!(
            judge(&f, array, TypeId::non_empty_array(db, TypeId::int(db), TypeId::int(db))),
            Proof::Fails
        );
        let narrow = TypeId::shape(
            db,
            vec![crate::ShapeField {
                key: crate::ShapeKey::String("id".to_owned()),
                optional: false,
                value: TypeId::int_literal(db, 1),
            }],
        );
        let wide = TypeId::shape(
            db,
            vec![crate::ShapeField {
                key: crate::ShapeKey::String("id".to_owned()),
                optional: false,
                value: TypeId::int(db),
            }],
        );
        assert_eq!(judge(&f, narrow, wide), Proof::Holds);
        assert_eq!(judge(&f, wide, narrow), Proof::Fails);
        // A sealed shape with an extra key fails against a shape without it.
        let extra = TypeId::shape(
            db,
            vec![
                crate::ShapeField {
                    key: crate::ShapeKey::String("id".to_owned()),
                    optional: false,
                    value: TypeId::int(db),
                },
                crate::ShapeField {
                    key: crate::ShapeKey::String("extra".to_owned()),
                    optional: false,
                    value: TypeId::string(db),
                },
            ],
        );
        assert_eq!(judge(&f, extra, wide), Proof::Fails);
        // A shape is a subtype of its general array form.
        assert_eq!(judge(&f, wide, TypeId::array(db, TypeId::string(db), TypeId::int(db))), Proof::Holds);
        assert_eq!(judge(&f, TypeId::array(db, TypeId::string(db), TypeId::int(db)), wide), Proof::Fails);
        // Callables: parameters contravariant, return covariant, void target accepts all.
        let takes_int_returns_literal = TypeId::callable(
            db,
            vec![crate::CallableParameter {
                parameter_type: TypeId::int(db),
                optional: false,
                variadic: false,
                by_reference: false,
            }],
            TypeId::int_literal(db, 1),
        );
        let takes_literal_returns_int = TypeId::callable(
            db,
            vec![crate::CallableParameter {
                parameter_type: TypeId::int_literal(db, 1),
                optional: false,
                variadic: false,
                by_reference: false,
            }],
            TypeId::int(db),
        );
        assert_eq!(judge(&f, takes_int_returns_literal, takes_literal_returns_int), Proof::Holds);
        assert_eq!(judge(&f, takes_literal_returns_int, takes_int_returns_literal), Proof::Fails);
        let void_target = TypeId::callable(db, vec![], TypeId::void(db));
        let no_parameter_int = TypeId::callable(db, vec![], TypeId::int(db));
        assert_eq!(judge(&f, no_parameter_int, void_target), Proof::Holds);
    }

    #[test]
    fn class_likes_use_the_hierarchy_hook() {
        let f = fixture(&[]);
        let db = &f.db;
        let user = TypeId::class(db, "User", vec![]);
        assert_eq!(judge(&f, user, TypeId::object(db)), Proof::Holds);
        assert_eq!(judge(&f, TypeId::object(db), user), Proof::Fails);
        // Different names answer CannotProve in this task; Task 9 tightens.
        assert_eq!(judge(&f, user, TypeId::class(db, "Entity", vec![])), Proof::CannotProve);
        // Same name, differing generic arguments: invariant, CannotProve.
        let of_int = TypeId::class(db, "Collection", vec![TypeId::int(db)]);
        let of_string = TypeId::class(db, "Collection", vec![TypeId::string(db)]);
        assert_eq!(judge(&f, of_int, of_string), Proof::CannotProve);
        // An unparameterized target erases.
        assert_eq!(judge(&f, of_int, TypeId::class(db, "Collection", vec![])), Proof::Holds);
        // Enum cases sit under their enum type.
        let case = TypeId::enum_case(db, "Status", "Active");
        assert_eq!(judge(&f, case, TypeId::class(db, "Status", vec![])), Proof::Holds);
        assert_eq!(judge(&f, case, TypeId::enum_case(db, "Status", "Inactive")), Proof::Fails);
        assert_eq!(judge(&f, TypeId::class(db, "Status", vec![]), case), Proof::CannotProve);
    }

    #[test]
    fn assignability_delegates_and_nullability_answers() {
        let f = fixture(&[]);
        let db = &f.db;
        assert_eq!(
            assignable_to(&f.db, f.files, f.stubs, f.configuration, TypeId::int(db), TypeId::mixed(db)),
            Proof::Holds
        );
        assert_eq!(nullability(&f.db, TypeId::null(db)), Nullability::AlwaysNull);
        assert_eq!(nullability(&f.db, TypeId::void(db)), Nullability::AlwaysNull);
        assert_eq!(nullability(&f.db, TypeId::mixed(db)), Nullability::PossiblyNull);
        assert_eq!(nullability(&f.db, TypeId::int(db)), Nullability::NeverNull);
        assert_eq!(
            nullability(&f.db, TypeId::union(db, [TypeId::int(db), TypeId::null(db)])),
            Nullability::PossiblyNull
        );
        let nullable_bound =
            TypeId::template(db, "scope", "T", TypeId::union(db, [TypeId::int(db), TypeId::null(db)]));
        assert_eq!(nullability(&f.db, nullable_bound), Nullability::PossiblyNull);
    }
}
```

Add `celerrate_source = { path = "../celerrate_source" }` to `[dependencies]` for `FileId` (used by the fixture; it is already a transitive dependency, the declaration makes it direct).

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types`
Expected: compilation failure (`judgments` module missing).

- [ ] **Step 4: Implement `judgments.rs`**

```rust
//! The three-valued judgments (spec section 3): `Holds`, `Fails`
//! (value-set inclusion refuted), `CannotProve` (undecidable with
//! available information). Every consumer states its posture toward
//! `CannotProve`; nothing here or above silently discards it.

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::StubIndexInput;

use crate::representation::{StringConstraint, TypeData, TypeId};

/// The three-valued verdict of a typed judgment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Proof {
    Holds,
    Fails,
    CannotProve,
}

impl Proof {
    /// Conjunction: all must hold; one refutation refutes the whole.
    pub fn all(proofs: impl IntoIterator<Item = Proof>) -> Proof {
        let mut result = Proof::Holds;
        for proof in proofs {
            match proof {
                Proof::Fails => return Proof::Fails,
                Proof::CannotProve => result = Proof::CannotProve,
                Proof::Holds => {}
            }
        }
        result
    }

    /// Disjunction: one hold suffices; only unanimous refutation refutes.
    pub fn any(proofs: impl IntoIterator<Item = Proof>) -> Proof {
        let mut result = Proof::Fails;
        for proof in proofs {
            match proof {
                Proof::Holds => return Proof::Holds,
                Proof::CannotProve => result = Proof::CannotProve,
                Proof::Fails => {}
            }
        }
        result
    }
}

/// The nullability verdict of one type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Nullability {
    NeverNull,
    PossiblyNull,
    AlwaysNull,
}

/// The salsa inputs the class rule needs, carried through the recursion.
#[derive(Clone, Copy)]
struct JudgmentContext {
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

/// Is `candidate` a subtype of `target`?
#[salsa::tracked]
pub fn subtype_of<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    candidate: TypeId<'db>,
    target: TypeId<'db>,
) -> Proof {
    judge(db, JudgmentContext { files, stubs, configuration }, candidate, target)
}

/// May a `source` value be assigned where `target` is declared? Today
/// this is exactly the subtype judgment; the coercion posture (weak-mode
/// files, `Stringable`) is the argument family's and lands in plan 8
/// behind this signature.
#[salsa::tracked]
pub fn assignable_to<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    source: TypeId<'db>,
    target: TypeId<'db>,
) -> Proof {
    subtype_of(db, files, stubs, configuration, source, target)
}

/// The nullability of one type. `void` is `AlwaysNull`: reading a void
/// call's value yields `null` in PHP.
#[salsa::tracked]
pub fn nullability<'db>(db: &'db dyn salsa::Database, subject: TypeId<'db>) -> Nullability {
    match subject.data(db) {
        TypeData::Null | TypeData::Void => Nullability::AlwaysNull,
        TypeData::Mixed | TypeData::ValueOf { .. } => Nullability::PossiblyNull,
        TypeData::Union { constituents } => {
            let verdicts: Vec<Nullability> =
                constituents.iter().map(|part| nullability(db, *part)).collect();
            if verdicts.iter().all(|verdict| *verdict == Nullability::AlwaysNull) {
                Nullability::AlwaysNull
            } else if verdicts.iter().any(|verdict| *verdict != Nullability::NeverNull) {
                Nullability::PossiblyNull
            } else {
                Nullability::NeverNull
            }
        }
        TypeData::Intersection { intersectands } => {
            if intersectands.iter().any(|part| nullability(db, *part) == Nullability::NeverNull) {
                Nullability::NeverNull
            } else {
                Nullability::PossiblyNull
            }
        }
        TypeData::Template { bound, .. } => nullability(db, *bound),
        TypeData::Conditional { then_branch, otherwise_branch, .. } => {
            match (nullability(db, *then_branch), nullability(db, *otherwise_branch)) {
                (Nullability::NeverNull, Nullability::NeverNull) => Nullability::NeverNull,
                (Nullability::AlwaysNull, Nullability::AlwaysNull) => Nullability::AlwaysNull,
                _ => Nullability::PossiblyNull,
            }
        }
        _ => Nullability::NeverNull,
    }
}

/// PHP's numeric-string test for a known literal: optional surrounding
/// whitespace (PHP 8 semantics), optional sign, digits with an optional
/// fraction or exponent.
fn literal_is_numeric(value: &str) -> bool {
    let trimmed = value.trim_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    if trimmed.is_empty() {
        return false;
    }
    let unsigned = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if unsigned.is_empty() {
        return false;
    }
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (unsigned, None),
    };
    let (integer_part, fraction_part) = match mantissa.split_once('.') {
        Some((integer_part, fraction_part)) => (integer_part, Some(fraction_part)),
        None => (mantissa, None),
    };
    let integer_is_digits = !integer_part.is_empty()
        && integer_part.bytes().all(|byte| byte.is_ascii_digit());
    let fraction_is_digits = fraction_part
        .is_none_or(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    let mantissa_valid = if fraction_part.is_some() {
        (integer_part.is_empty() || integer_is_digits) && fraction_is_digits
            && !(integer_part.is_empty() && fraction_part.is_some_and(str::is_empty))
    } else {
        integer_is_digits
    };
    let exponent_valid = exponent.is_none_or(|part| {
        let unsigned_exponent = part.strip_prefix(['+', '-']).unwrap_or(part);
        !unsigned_exponent.is_empty()
            && unsigned_exponent.bytes().all(|byte| byte.is_ascii_digit())
    });
    mantissa_valid && exponent_valid
}

/// The class-versus-class hierarchy verdict for differing folded names.
/// This task answers `CannotProve` (no hierarchy consulted yet); Task 9
/// replaces this body with the `linearized_class` walk.
fn judge_class_hierarchy(
    db: &dyn salsa::Database,
    context: JudgmentContext,
    candidate_name: &str,
    target_name: &str,
) -> Proof {
    if candidate_name == target_name {
        return Proof::Holds;
    }
    let _ = (db, context);
    Proof::CannotProve
}

fn judge<'db>(
    db: &'db dyn salsa::Database,
    context: JudgmentContext,
    candidate: TypeId<'db>,
    target: TypeId<'db>,
) -> Proof {
    // Rules 1 to 5: the extremes.
    if candidate == target {
        return Proof::Holds;
    }
    if target.is_mixed(db) {
        return Proof::Holds;
    }
    if candidate.is_never(db) {
        return Proof::Holds;
    }
    if target.is_never(db) {
        return Proof::Fails;
    }
    if candidate.is_mixed(db) {
        return Proof::Fails;
    }
    // Rules 6 to 9: decomposition.
    if let TypeData::Union { constituents } = candidate.data(db) {
        return Proof::all(constituents.iter().map(|part| judge(db, context, *part, target)));
    }
    if let TypeData::Union { constituents } = target.data(db) {
        return Proof::any(constituents.iter().map(|part| judge(db, context, candidate, *part)));
    }
    if let TypeData::Intersection { intersectands } = candidate.data(db) {
        return match Proof::any(
            intersectands.iter().map(|part| judge(db, context, *part, target)),
        ) {
            Proof::Holds => Proof::Holds,
            _ => Proof::CannotProve,
        };
    }
    if let TypeData::Intersection { intersectands } = target.data(db) {
        return Proof::all(intersectands.iter().map(|part| judge(db, context, candidate, *part)));
    }
    // Rules 10 and 11: templates.
    if let TypeData::Template { bound, .. } = candidate.data(db) {
        return match judge(db, context, *bound, target) {
            Proof::Holds => Proof::Holds,
            _ => Proof::CannotProve,
        };
    }
    if matches!(target.data(db), TypeData::Template { .. }) {
        return Proof::CannotProve;
    }
    // Rule 12: conditionals through their branch unions.
    if let TypeData::Conditional { then_branch, otherwise_branch, .. } = candidate.data(db) {
        let fallback = TypeId::union(db, [*then_branch, *otherwise_branch]);
        return match judge(db, context, fallback, target) {
            Proof::Holds => Proof::Holds,
            _ => Proof::CannotProve,
        };
    }
    if let TypeData::Conditional { then_branch, otherwise_branch, .. } = target.data(db) {
        let both = Proof::all([
            judge(db, context, candidate, *then_branch),
            judge(db, context, candidate, *otherwise_branch),
        ]);
        return match both {
            Proof::Holds => Proof::Holds,
            _ => Proof::CannotProve,
        };
    }
    // Rule 13: the symbolic key-of and value-of.
    if matches!(candidate.data(db), TypeData::KeyOf { .. }) {
        let keys = TypeId::union(db, [TypeId::int(db), TypeId::string(db)]);
        return match judge(db, context, keys, target) {
            Proof::Holds => Proof::Holds,
            _ => Proof::CannotProve,
        };
    }
    if matches!(candidate.data(db), TypeData::ValueOf { .. })
        || matches!(target.data(db), TypeData::KeyOf { .. } | TypeData::ValueOf { .. })
    {
        return Proof::CannotProve;
    }
    // Rule 14: the placeholders.
    if matches!(
        candidate.data(db),
        TypeData::SelfPlaceholder | TypeData::ParentPlaceholder | TypeData::StaticPlaceholder
    ) || matches!(
        target.data(db),
        TypeData::SelfPlaceholder | TypeData::ParentPlaceholder | TypeData::StaticPlaceholder
    ) {
        return Proof::CannotProve;
    }
    // Rule 15: the ground matrix.
    judge_ground(db, context, candidate, target)
}
```

The ground matrix, in the same file (complete; the fallthrough is `Fails`):

```rust
fn judge_ground<'db>(
    db: &'db dyn salsa::Database,
    context: JudgmentContext,
    candidate: TypeId<'db>,
    target: TypeId<'db>,
) -> Proof {
    match (candidate.data(db), target.data(db)) {
        // Booleans: a literal sits under the general type.
        (TypeData::Bool { literal: Some(_) }, TypeData::Bool { literal: None }) => Proof::Holds,
        (TypeData::Bool { .. }, TypeData::Bool { .. }) => Proof::Fails,
        // Integers: range inclusion.
        (
            TypeData::Int { minimum: a_min, maximum: a_max },
            TypeData::Int { minimum: b_min, maximum: b_max },
        ) => {
            let low_included = match (a_min, b_min) {
                (_, None) => true,
                (None, Some(_)) => false,
                (Some(a), Some(b)) => a >= b,
            };
            let high_included = match (a_max, b_max) {
                (_, None) => true,
                (None, Some(_)) => false,
                (Some(a), Some(b)) => a <= b,
            };
            if low_included && high_included { Proof::Holds } else { Proof::Fails }
        }
        // Floats: a literal sits under the general type.
        (TypeData::Float { literal: Some(_) }, TypeData::Float { literal: None }) => Proof::Holds,
        (TypeData::Float { .. }, TypeData::Float { .. }) => Proof::Fails,
        // The string-constraint table.
        (TypeData::String { constraint: a }, TypeData::String { constraint: b }) => {
            let holds = match (a, b) {
                (_, StringConstraint::General) => true,
                (StringConstraint::Literal(value), StringConstraint::NonEmpty) => !value.is_empty(),
                (StringConstraint::Literal(value), StringConstraint::Numeric) => {
                    literal_is_numeric(value)
                }
                (StringConstraint::Literal(_), StringConstraint::LiteralMarker) => true,
                (StringConstraint::Numeric, StringConstraint::NonEmpty) => true,
                _ => false,
            };
            if holds { Proof::Holds } else { Proof::Fails }
        }
        // class-string.
        (TypeData::ClassString { .. }, TypeData::ClassString { argument: None }) => Proof::Holds,
        (
            TypeData::ClassString { argument: Some(a) },
            TypeData::ClassString { argument: Some(b) },
        ) => judge(db, context, *a, *b),
        (TypeData::ClassString { .. }, TypeData::ClassString { .. }) => Proof::Fails,
        (
            TypeData::ClassString { .. },
            TypeData::String { constraint: StringConstraint::General | StringConstraint::NonEmpty },
        ) => Proof::Holds,
        (TypeData::ClassString { .. }, TypeData::String { .. }) => Proof::Fails,
        (
            TypeData::String { constraint: StringConstraint::Literal(_) },
            TypeData::ClassString { .. },
        ) => Proof::CannotProve,
        // Arrays: flags gate, then key and value covariance.
        (
            TypeData::Array { key: a_key, value: a_value, is_list: a_list, non_empty: a_non_empty },
            TypeData::Array { key: b_key, value: b_value, is_list: b_list, non_empty: b_non_empty },
        ) => {
            if (*b_list && !*a_list) || (*b_non_empty && !*a_non_empty) {
                return Proof::Fails;
            }
            Proof::all([
                judge(db, context, *a_key, *b_key),
                judge(db, context, *a_value, *b_value),
            ])
        }
        // Shapes: sealed, width-strict, optionality-aware.
        (TypeData::Shape { fields: a }, TypeData::Shape { fields: b }) => {
            if a.iter().any(|field| !b.iter().any(|other| other.key == field.key)) {
                return Proof::Fails;
            }
            Proof::all(b.iter().map(|target_field| {
                match a.iter().find(|field| field.key == target_field.key) {
                    Some(candidate_field) => {
                        let value =
                            judge(db, context, candidate_field.value, target_field.value);
                        if !target_field.optional && candidate_field.optional {
                            Proof::all([value, Proof::CannotProve])
                        } else {
                            value
                        }
                    }
                    None if target_field.optional => Proof::Holds,
                    None => Proof::Fails,
                }
            }))
        }
        (TypeData::Shape { fields }, TypeData::Array { .. }) => {
            let (key, value, is_list, non_empty) = TypeId::shape_as_array(db, fields);
            let widened = match (is_list, non_empty) {
                (true, true) => TypeId::non_empty_list(db, value),
                (true, false) => TypeId::list(db, value),
                (false, true) => TypeId::non_empty_array(db, key, value),
                (false, false) => TypeId::array(db, key, value),
            };
            judge(db, context, widened, target)
        }
        (TypeData::Array { .. }, TypeData::Shape { .. }) => Proof::Fails,
        // Class-likes.
        (TypeData::Class { .. } | TypeData::EnumCase { .. }, TypeData::Object) => Proof::Holds,
        (TypeData::Object, TypeData::Class { .. } | TypeData::EnumCase { .. }) => Proof::Fails,
        (
            TypeData::Class { name: a_name, arguments: a_arguments },
            TypeData::Class { name: b_name, arguments: b_arguments },
        ) => {
            if a_name == b_name {
                if b_arguments.is_empty() || a_arguments == b_arguments {
                    Proof::Holds
                } else {
                    // Invariant arguments; variance is out of scope.
                    Proof::CannotProve
                }
            } else {
                match judge_class_hierarchy(db, context, a_name, b_name) {
                    Proof::Holds if b_arguments.is_empty() => Proof::Holds,
                    Proof::Holds => Proof::CannotProve,
                    verdict => verdict,
                }
            }
        }
        (TypeData::EnumCase { enum_name, .. }, TypeData::Class { name, arguments }) => {
            if arguments.is_empty() {
                judge_class_hierarchy(db, context, enum_name, name)
            } else {
                Proof::CannotProve
            }
        }
        (TypeData::EnumCase { .. }, TypeData::EnumCase { .. }) => Proof::Fails,
        (TypeData::Class { name, .. }, TypeData::EnumCase { enum_name, .. }) => {
            if name == enum_name { Proof::CannotProve } else { Proof::Fails }
        }
        // Callables: contravariant parameters, covariant return.
        (
            TypeData::Callable { parameters: a_parameters, return_type: a_return },
            TypeData::Callable { parameters: b_parameters, return_type: b_return },
        ) => judge_callable(db, context, a_parameters, *a_return, b_parameters, *b_return),
        // The CannotProve islands: invokable objects and callable
        // strings and arrays keep these pairs undecidable.
        (TypeData::Callable { .. }, TypeData::Object)
        | (TypeData::Object | TypeData::Class { .. }, TypeData::Callable { .. })
        | (TypeData::String { .. } | TypeData::ClassString { .. }, TypeData::Callable { .. })
        | (TypeData::Array { .. } | TypeData::Shape { .. }, TypeData::Callable { .. }) => {
            Proof::CannotProve
        }
        // Everything else is a refuted cross-kind pair.
        _ => Proof::Fails,
    }
}

fn judge_callable<'db>(
    db: &'db dyn salsa::Database,
    context: JudgmentContext,
    candidate_parameters: &[crate::CallableParameter<'db>],
    candidate_return: TypeId<'db>,
    target_parameters: &[crate::CallableParameter<'db>],
    target_return: TypeId<'db>,
) -> Proof {
    // A void target accepts any return; otherwise the return is covariant.
    let return_proof = if target_return.is_void(db) {
        Proof::Holds
    } else {
        judge(db, context, candidate_return, target_return)
    };
    let mut proofs = vec![return_proof];
    let candidate_variadic = candidate_parameters.last().filter(|parameter| parameter.variadic);
    for (index, target_parameter) in target_parameters.iter().enumerate() {
        let candidate_parameter = candidate_parameters
            .get(index)
            .filter(|parameter| !parameter.variadic)
            .or(candidate_variadic);
        match candidate_parameter {
            Some(parameter) => {
                if parameter.by_reference != target_parameter.by_reference {
                    proofs.push(Proof::CannotProve);
                } else {
                    // Contravariant: the target's argument flows into the
                    // candidate's parameter.
                    proofs.push(judge(
                        db,
                        context,
                        target_parameter.parameter_type,
                        parameter.parameter_type,
                    ));
                }
            }
            // The target may pass an argument the candidate cannot take.
            None => proofs.push(Proof::Fails),
        }
    }
    // Candidate parameters beyond the target's arity must be optional.
    let required_beyond = candidate_parameters
        .iter()
        .skip(target_parameters.len())
        .any(|parameter| !parameter.optional && !parameter.variadic);
    if required_beyond {
        proofs.push(Proof::Fails);
    }
    Proof::all(proofs)
}
```

`lib.rs`: `mod judgments;` and `pub use judgments::{Nullability, Proof, assignable_to, nullability, subtype_of};`

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types`
Expected: PASS.

- [ ] **Step 6: Full verification and commit**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_types Cargo.lock
git commit -m "✨ feat(types): the three-valued subtype, assignability, and nullability judgments"
```

---
### Task 9: The hierarchy-aware class rule

**Files:**
- Modify: `crates/celerrate_types/src/judgments.rs` (replace `judge_class_hierarchy`'s body)
- Create: `crates/celerrate_types/tests/invalidation_scope.rs`
- Test: inline module in `judgments.rs` + the new integration test

**Interfaces:**
- Consumes: `celerrate_semantics::{ClassQuery, linearized_class}` (already a dependency since Task 4).
- Produces: the final `judge_class_hierarchy`: found in the walked ancestry → Holds; not found behind a stub boundary, an unresolved edge, or a broken cycle → CannotProve; not found in a fully resolved hierarchy → Fails. No signature changes anywhere.

**The rule, exactly.** `linearized_class` (plan 1a) walks a source class-like's full ancestry iteratively and returns its edges (`AncestorEdge { resolved: Option<String>, .. }`, walk order), the stub-only ancestor keys (`stub_ancestors`), and a `cyclic` flag; it returns `None` when the queried key is not a source class-like. The verdict for `candidate_name <: target_name` (both pre-folded, names differing):

1. `linearized_class(candidate)` is `None` → **CannotProve** (a stub or unknown class; the stub blob carries no hierarchy in this sub-project, recorded as a limitation the stub signature payload of plan 3 does not lift either: it carries signatures, not ancestry).
2. `target_name` appears among the resolved ancestry edges or the stub ancestors → **Holds**.
3. Otherwise, if `cyclic` or any edge has `resolved: None` and its written name is not accounted for by `stub_ancestors` → **CannotProve** (the walk hit a boundary it cannot see past: a stub ancestor's own ancestors are invisible). Simplification, recorded: any `resolved: None` edge (stub or unresolved alike) triggers CannotProve, because stub edges also carry `resolved: None` and their ancestry is equally invisible.
4. Otherwise → **Fails** (the hierarchy is fully resolved and the target is not in it).

- [ ] **Step 1: Write the failing tests**

Append to the test module in `judgments.rs` (the fixture helper from Task 8 already builds source files):

```rust
    #[test]
    fn a_resolved_hierarchy_proves_and_refutes() {
        let f = fixture(&[
            "<?php class Entity {} interface Timestamped {}",
            "<?php class User extends Entity implements Timestamped {}",
            "<?php class Order {}",
        ]);
        let db = &f.db;
        let user = TypeId::class(db, "User", vec![]);
        assert_eq!(judge(&f, user, TypeId::class(db, "Entity", vec![])), Proof::Holds);
        assert_eq!(judge(&f, user, TypeId::class(db, "Timestamped", vec![])), Proof::Holds);
        assert_eq!(judge(&f, TypeId::class(db, "Entity", vec![]), user), Proof::Fails);
        assert_eq!(judge(&f, user, TypeId::class(db, "Order", vec![])), Proof::Fails);
    }

    #[test]
    fn grandparents_count_and_generic_targets_stay_invariant() {
        let f = fixture(&[
            "<?php class A {} class B extends A {} class C extends B {}",
        ]);
        let db = &f.db;
        let c = TypeId::class(db, "C", vec![]);
        assert_eq!(judge(&f, c, TypeId::class(db, "A", vec![])), Proof::Holds);
        // A parameterized target cannot be proven through erasure.
        assert_eq!(
            judge(&f, c, TypeId::class(db, "A", vec![TypeId::int(db)])),
            Proof::CannotProve
        );
    }

    #[test]
    fn boundaries_answer_cannot_prove() {
        let f = fixture(&[
            // Extends a class that exists nowhere in the file set.
            "<?php class Repository extends ServiceEntityRepository {}",
            // A genuine cycle, broken by linearization.
            "<?php class Ouro extends Boros {} class Boros extends Ouro {}",
        ]);
        let db = &f.db;
        let repository = TypeId::class(db, "Repository", vec![]);
        assert_eq!(
            judge(&f, repository, TypeId::class(db, "ObjectRepository", vec![])),
            Proof::CannotProve
        );
        let ouro = TypeId::class(db, "Ouro", vec![]);
        assert_eq!(judge(&f, ouro, TypeId::class(db, "Unrelated", vec![])), Proof::CannotProve);
        // An unknown candidate class is undecidable too.
        assert_eq!(
            judge(&f, TypeId::class(db, "Ghost", vec![]), TypeId::class(db, "Entity", vec![])),
            Proof::CannotProve
        );
    }

    #[test]
    fn enum_cases_inherit_through_their_enum_hierarchy() {
        let f = fixture(&[
            "<?php interface HasLabel {} enum Status implements HasLabel { case Active; }",
        ]);
        let db = &f.db;
        let case = TypeId::enum_case(db, "Status", "Active");
        assert_eq!(judge(&f, case, TypeId::class(db, "HasLabel", vec![])), Proof::Holds);
    }
```

`crates/celerrate_types/tests/invalidation_scope.rs` (the crate's first incremental test; the pattern comes from `celerrate_semantics/tests/invalidation_scope.rs`):

```rust
//! The typed judgments must ride the member boundary's early cutoff: a
//! method-body edit backdates the member tree, so a memoized subtype
//! verdict that consulted the hierarchy does not recompute.

#![allow(clippy::unwrap_used)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_source::FileId;
use celerrate_stubs::{StubIndex, StubIndexInput};
use celerrate_types::{Proof, TypeId, subtype_of};
use salsa::Setter;

#[test]
fn a_body_edit_does_not_recompute_a_hierarchy_verdict() {
    let db = TestDatabase::default();
    let parent = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php class Entity { public function id(): int { return 1; } }".to_vec(),
    );
    let child = SourceFile::new(&db, FileId::new(1), b"<?php class User extends Entity {}".to_vec());
    let files = AnalyzedFileSet::new(&db, vec![parent, child]);
    let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);

    let user = TypeId::class(&db, "User", vec![]);
    let entity = TypeId::class(&db, "Entity", vec![]);
    assert_eq!(subtype_of(&db, files, stubs, configuration, user, entity), Proof::Holds);

    db.take_executed();
    parent.set_bytes(&mut db.clone()).to(
        b"<?php class Entity { public function id(): int { return 2; } }".to_vec(),
    );
    let user = TypeId::class(&db, "User", vec![]);
    let entity = TypeId::class(&db, "Entity", vec![]);
    assert_eq!(subtype_of(&db, files, stubs, configuration, user, entity), Proof::Holds);
    let executed = db.take_executed();
    assert!(
        !executed.iter().any(|query| query.contains("subtype_of")),
        "a body edit must backdate below the judgment, ran: {executed:?}"
    );
}
```

(If `set_bytes` needs the database by value rather than through a clone, follow the exact mutation idiom used in `celerrate_semantics/tests/invalidation_scope.rs`; copy its `Setter` usage verbatim; the assertion is the contract, not the mutation ceremony. Re-interning `user`/`entity` after the edit is deliberate: interning is idempotent and returns the same handles.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_types`
Expected: the four new unit tests FAIL on `CannotProve`-versus-`Holds`/`Fails` assertions (the Task 8 stub answers CannotProve for every differing name); the integration test may already pass once compilation succeeds; that is fine, it pins the contract.

- [ ] **Step 3: Implement the walk**

Replace `judge_class_hierarchy`'s body in `judgments.rs`:

```rust
use celerrate_semantics::{ClassQuery, linearized_class};

/// The class-versus-class hierarchy verdict for differing folded names:
/// found in the walked ancestry proves; a stub boundary, an unresolved
/// edge, or a broken cycle leaves the answer undecidable; a fully
/// resolved hierarchy without the target refutes.
fn judge_class_hierarchy(
    db: &dyn salsa::Database,
    context: JudgmentContext,
    candidate_name: &str,
    target_name: &str,
) -> Proof {
    if candidate_name == target_name {
        return Proof::Holds;
    }
    let class = ClassQuery::new(db, candidate_name.to_owned());
    let Some(linearized) =
        linearized_class(db, context.files, context.stubs, context.configuration, class)
    else {
        // A stub or unknown class: the stub blob carries no hierarchy.
        return Proof::CannotProve;
    };
    let found = linearized
        .ancestry
        .iter()
        .any(|edge| edge.resolved.as_deref() == Some(target_name))
        || linearized.stub_ancestors.iter().any(|key| key == target_name);
    if found {
        return Proof::Holds;
    }
    let opaque_boundary =
        linearized.cyclic || linearized.ancestry.iter().any(|edge| edge.resolved.is_none());
    if opaque_boundary { Proof::CannotProve } else { Proof::Fails }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_types`
Expected: PASS. If the stub-boundary test trips on `stub_ancestors` naming (a stub ancestor is both in `stub_ancestors` and an unresolved edge), the rule still answers correctly: found wins before the boundary check.

- [ ] **Step 5: Full verification and commit**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
git add crates/celerrate_types
git commit -m "✨ feat(types): the hierarchy-aware class subtype rule over linearization"
```

---

### Task 10: The Celerrate norm draft

**Files:**
- Create: `.claude/superpowers/specs/2026-07-14-celerrate-norm-draft.md`

**Interfaces:**
- Consumes: the finished lattice (its constructor list is the draft's completeness checklist).
- Produces: the internal norm draft; plan 7's stub-curation overlay is its first consumer (spec section 7). Internal only: no public documentation, no migration tooling, no stability promise; it freezes in v1.x.

- [ ] **Step 1: Write the draft**

The document covers every lattice constructor with a norm spelling, its PHPStan equivalent, and the divergence rationale where the norm departs. Structure (write real content under each heading; the table below seeds the mapping and must be completed for **every** `TypeId` constructor of this plan):

```markdown
# The Celerrate Norm (Internal Draft)

Date: 2026-07-14
Status: Internal draft. No public documentation, no migration tooling,
no stability promise. Freezes in v1.x, informed by real-world feedback.
Parent spec: `.claude/superpowers/specs/2026-07-14-type-engine-design.md`
(sections 3 and 7).

## 1. Purpose and posture

The norm is the annotation syntax Celerrate will natively own, designed
against the lattice so both stay honest (the parent spec's timing
decision). It reads as PHPStan where PHPStan is already clean, and
departs only where the departure buys regularity. First consumer: the
"Celerrate refinements" stub overlay (plan 7).

## 2. Design rules

1. One spelling per lattice constructor; no synonyms.
2. Every norm expression lowers losslessly to the lattice; nothing in
   the norm exists that the lattice cannot represent.
3. Ranges use `..` (`int<1..>`, `int<..5>`, `int<1..5>`); `min`/`max`
   keywords do not exist in the norm.
4. Nullable shorthand `?T` is `T|null`, usable anywhere a type appears.
5. Shapes drop the `array` prefix: `{id: int, name?: string}`.
6. Lists are `list<T>`; `T[]` does not exist in the norm.

## 3. The mapping table

| Lattice constructor | Norm | PHPStan equivalent | Divergence |
| --- | --- | --- | --- |
| `mixed` / `never` / `void` / `null` | same | same | none |
| `bool`, `true`, `false` | same | same | none |
| `int`, `42`, `int<1..>`, `int<1..5>` | `..` ranges | `int<1, max>` | rule 3 |
| `float`, float literals | same | same | none |
| `string`, `non-empty-string`, `numeric-string`, `literal-string`, `'active'` | same | same | none |
| `class-string`, `class-string<T>` | same | same | none |
| `array<K, V>`, `non-empty-array<K, V>` | same | same | none |
| `list<T>`, `non-empty-list<T>` | same | same | rule 6 |
| shapes | `{id: int, name?: string}` | `array{id: int, name?: string}` | rule 5 |
| `iterable<K, V>` | same (desugars) | same | none |
| class and enum types, generic arguments | `User`, `Collection<User>` | same | none |
| enum cases | `Status::Active` | same | none |
| callables | `callable(int, string=, bool...): void` | same | none |
| templates | `@template T of Foo` | same | none |
| `key-of<T>` / `value-of<T>` | same | same | none |
| conditionals | `(T is int ? A : B)` | same | none |
| `static` / `self` / `parent` | same | same | none |
| nullable | `?T` | `T|null` also accepted by PHPStan | rule 4 |
| `resource` | same | same | none |

## 4. The tag set (sketch, revised by curation)

`@param`, `@return`, `@var`, `@template`, `@extends`, `@implements`,
`@use`: the standard names, unprefixed; the norm is recognized by
context (a Celerrate-flavored expression parses under the norm grammar
first when the bridge gains it, a later sub-project's concern).

## 5. Open questions for curation (plan 7)

- Whether the refinements overlay wants a compact multi-signature form
  for per-version stub deltas.
- Whether `?T` inside unions needs parenthesization rules.
- Intersection spelling in shapes' field types.
```

Fill section 4 and 5 honestly from the state of the lattice at writing time; the open questions list must contain at least the items above.

- [ ] **Step 2: Verify the completeness claim**

Cross-check the mapping table against `construction.rs`: every `pub fn` constructor appears in some row. Add any missed row.

- [ ] **Step 3: Commit**

```bash
git add .claude/superpowers/specs/2026-07-14-celerrate-norm-draft.md
git commit -m "📝 docs(types): the internal Celerrate norm draft against the lattice"
```

---

### Task 11: Documentation, export audit, and closure

**Files:**
- Modify: `crates/celerrate_types/src/lib.rs` (final module documentation)
- Modify: `crates/celerrate_types/src/construction.rs`, `representation.rs`, `judgments.rs`, `widening.rs` (doc comments only where a decision is otherwise invisible)
- Test: the full workspace suite

- [ ] **Step 1: Finalize the crate documentation**

`lib.rs` module documentation states, in prose: the canonical-form invariant (handle equality is structural equality), the two determinism invariants (structural ordering, no process-escaping ids and therefore no serde), the `Fails`/`CannotProve` semantics, the caps and their collapse rules, and the folded-name rendering debt. Verify the final re-export list is exactly:

```rust
pub use judgments::{Nullability, Proof, assignable_to, nullability, subtype_of};
pub use representation::{CallableParameter, FloatBits, ShapeField, ShapeKey, TypeId};
pub use widening::{STRUCTURAL_DEPTH_CAP, UNION_ARITY_CAP, join, widened_literals};
```

Nothing else leaves the crate: not `TypeData`, not `StringConstraint`, not `structural_order`, not `depth_of`, not `display_type` (the method is the surface).

- [ ] **Step 2: Audit the judgment posture statements**

Confirm `judgments.rs` documents, on `Proof`, the consumer contract: `CannotProve` is never a silent discard; each family (plan 8) states its posture. Confirm `assignable_to` documents that the coercion posture is plan 8's.

- [ ] **Step 3: Full verification**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

Expected: all green. `cargo deny` is unaffected (no new external dependencies were added by this plan).

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_types
git commit -m "📝 docs(types): pin the lattice invariants and the judgment contract"
```

---

## Self-review checklist (run after writing, fixed inline)

- Spec section 3 coverage: every enumerated lattice form has a variant or a desugaring (`iterable`); the two determinism invariants have tests (order-independent union construction; no serde anywhere); widening, the arity collapse-to-join, and the depth cap are Tasks 6; the three-valued judgment and every consumer-posture statement are Tasks 8, 9, and 11; source precedence, declared-type inheritance, and the stub payload are **plan 3**, deliberately absent here.
- Spec section 7: the norm draft is Task 10, written against the finished lattice, internal-only.
- Type consistency: `TypeId<'db>`, `TypeData<'db>`, `Proof`, `Nullability`, `judge`, `judge_class_hierarchy`, `shape_as_array`, `capped_child`, and the constructor names are used with the same spellings across all tasks.
- The judgment queries take `(db, files, stubs, configuration, left, right)` in Task 8 already, so Task 9 changes no signature.







