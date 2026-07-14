//! The type lattice of the analysis engine: every type is interned in
//! canonical form behind the opaque [`TypeId`] handle, so equality and
//! hashing are cheap and salsa's early cutoff applies to typed results.
//! The representation is never exposed as a matchable enum: consumers
//! construct through the `TypeId` constructors and interrogate through
//! its query methods (the plugin API commitment of the parent spec).
//!
//! # Invariants
//!
//! **Canonical form.** Handle equality is structural equality: every
//! constructor canonicalizes its arguments bottom-up before interning,
//! so two types built from the same values along different paths (a
//! union assembled forwards or backwards, a nested union flattened, a
//! duplicate shape key overwritten) intern to the same [`TypeId`]. There
//! is no separate equality check to run against the represented value;
//! `==` on two handles answers the lattice question directly.
//!
//! **Determinism.** Two invariants keep results reproducible under
//! salsa's parallel fan-out. First, the canonical ordering used to sort
//! union constituents, intersection members, shape fields, and callable
//! parameters is structural (rank, then name, then shape); it is never
//! derived from the interner's handle order, because interning order is
//! timing-dependent across threads and a handle-based sort would make
//! canonical forms nondeterministic. Second, [`TypeId`] never escapes
//! the process: it is a salsa interned id, valid only for the database
//! that produced it, and this crate carries no `serde` implementation
//! for it or for the private representation it wraps. Cross-process
//! persistence is a later plan's structural serialization, built from
//! the query methods, not from the handle.
//!
//! **Three-valued judgments.** [`Proof::Holds`] and [`Proof::Fails`] are
//! both decisions: `Fails` means value-set inclusion is refuted, not
//! merely unproven. [`Proof::CannotProve`] means the judgment is
//! undecidable with the information available (a stub boundary, an
//! unresolved hierarchy edge, an invariant generic argument); every
//! consumer states its posture toward it and never silently discards it
//! in favor of `Fails` or `Holds`.
//!
//! **Deterministic caps.** [`UNION_ARITY_CAP`] (32) bounds union and
//! intersection width. A union that would exceed it collapses to the
//! pairwise join of its constituents (a common supertype, never a
//! truncated subset, which would make the result depend on
//! accumulation order); an intersection that would exceed it keeps the
//! first 32 members after the structural sort, which stays deterministic
//! because the sort is. [`STRUCTURAL_DEPTH_CAP`] (16) bounds nesting
//! depth: a child about to enter a composite constructor widens to
//! `mixed` once it is already at the cap, applied before the
//! union or intersection absorption and deduplication rules run, so the
//! capped result is independent of construction order.
//!
//! **Rendering debt.** `TypeId::display` renders class and enum names as
//! their folded keys (the case-insensitive, backslash-normalized
//! symbol-table key), not their originally written spelling. Recovering
//! the original spelling requires the symbol table and is deferred to
//! plan 8, which renders diagnostics.
//!
//! **Declared types.** [`declared_member_signature`] and
//! [`declared_function_signature`] are the per-member and per-function
//! queries that resolve one written signature into the lattice at its
//! declaring site (methods, properties, class constants, enum cases;
//! free functions the symmetric source-side case). Every value and
//! parameter carries a [`Trust`] verdict tracing how it was obtained:
//! `NativeOnly` when no annotation exists, `Refined` when an annotation
//! is a proven subtype of the native declaration, `RefinedUnproven`
//! when the judgment is `CannotProve` (trusted and traced — template
//! types, principally), `RejectedAnnotation` when the annotation
//! contradicts the native declaration and the native declaration wins;
//! never a crash, never a silent widening, never a silently dropped
//! annotation. The annotation layer itself is a seam:
//! [`member_annotations`] answers `MemberAnnotations::default()` until
//! plan 4a's bridge fills it through the type-syntax registry — at
//! which point source precedence, the trust rule, and the
//! nearest-ancestor inheritance walk (already wired and unit-tested
//! against injected readers) change nothing else. Stub signatures
//! resolve under the `[min, max]` configured-version range rule: a
//! return or value type is the union of its per-version forms (the
//! least restrictive reading of a call's result); a parameter type
//! takes the most restrictive per-version form that is a proven subtype
//! of every other, or falls silent (`parameter_type: None`) when no
//! such form exists — the design's degenerate empty-intersection guard,
//! silencing the check rather than fabricating an uninhabited
//! intersection. Bare `callable` lowers to `mixed`: no top-of-callables
//! form exists in the lattice, so `mixed` is a documented sound
//! widening (silence, never a false positive); a first-class
//! bare-callable form is recorded debt, revisited when plan 8 measures
//! the silence.

mod construction;
mod declared;
mod display;
mod dynamic_type_provider;
mod judgments;
mod ordering;
mod representation;
mod type_syntax;
mod widening;
mod written;

pub use declared::{
    DeclaredParameter, DeclaredSignature, FunctionQuery, MemberAnnotations, Trust,
    declared_function_signature, declared_member_signature, member_annotations,
};
pub use dynamic_type_provider::{
    ClaimConflict, DynamicTypeProvider, DynamicTypeProviderRegistration,
    DynamicTypeProviderRegistry, Invocation, SymbolClaim, validate_claims,
};
pub use judgments::{Nullability, Proof, assignable_to, nullability, subtype_of};
pub use representation::{CallableParameter, FloatBits, ShapeField, ShapeKey, TypeId};
pub use type_syntax::{
    AnnotationSite, ParsedAnnotations, TypeSyntax, TypeSyntaxRegistration, TypeSyntaxRegistry,
};
pub use widening::{STRUCTURAL_DEPTH_CAP, UNION_ARITY_CAP, join, widened_literals};
