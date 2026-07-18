# Type Engine 9a — Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The typed-artifact persistent cache — the hardest cache problem
of the sub-project, on which the flagship warm criterion rests. The two
artifact classes the design fixed (per-body **inferred signatures**,
persisted in a structural serialization and revalidated recursively and
memoized; full **expression type tables**, never persisted) go live; the
typed verdict families (`CEL0030`–`CEL0038`) join `StoredVerdict` with
their own revalidation records so a warm run serves them without
re-running inference; the member tree joins the packs (the debt the
`queries.rs::member_tree` rustdoc records — the "No artifact-cache
consultation yet" sentence there and in `body.rs`; a rustdoc debt,
not a spec item); the pack header gains the plugin-set
key the plan-4a `PluginIdentity` rustdoc promised; the equivalence net
and the adversarial suites extend over every new artifact (or the net
silently stops guarding); the watch persist economics are measured with
the recorded alternative; and the inherited second revalidation mirror
(`resolution_records`/`reference_diagnostics`) disappears into a single
constructive walk, the same closure the first mirror already received.
Design source: `.claude/superpowers/specs/2026-07-14-type-engine-design.md`,
sections 9 (the typed-artifact cache design, the two artifact classes,
the fallback lever, watch economics, the inherited debts, and the "or
the net silently stops guarding" clause the harness extension
answers), 3 (the two
determinism invariants: structural canonical order, `TypeId` never
escapes the process), 4 (the plugin-set key), 6 (engine invariants: no
provisional value served or persisted), and 11 item 12 (this plan).

**Architecture:** Three new persistence surfaces, all keyed under one
bumped pack header (schema 4 → 5, plus a `plugins` digest field):
`member_trees.bin` (the member boundary, seeded through a new
`ArtifactCache::member_tree` method so warm revalidation never parses),
`inferred_signatures.bin` (per-signature records: content hash of the
defining file, the return as a `StoredType`, and the reduced-answer
records the computation itself collected), and typed fields on
`StoredVerdict` in the existing `diagnostics.bin`. Revalidation follows
the shipped reduced-answer pattern, extended with two new record
classes: **class-surface digests** (one blake3 digest over a class's
whole lookup surface — existence, flags, resolved signatures, ancestry,
magic, virtual members — so any fact `lookup_member` or a judgment
could have consulted flips it) and **inferred-return records** (callee
key plus expected return). The recursive-and-memoized revalidation the
design demands is salsa itself: `inferred_function_return` and
`inferred_method_return` consult a `celerrate_types`-owned
`TypedArtifactCache` extension point first and validate by reading live
salsa facts, so each signature validates once per run and an inferred
edge whose callee still answers the recorded return holds even when the
callee's file changed — early cutoff across the process boundary.
Records are **constructive**: the inference walker and the check
walkers record what they consult as they consult it; no third mirror is
born, and the second one dies (`reference_outcomes`, one walk producing
findings and answers together).

**Tech Stack:** Rust (edition 2024, toolchain 1.94), salsa 0.27.2,
blake3, postcard + serde (the shipped pack encoding), the plan-5/6
inference engine (`inferred_body_types`, `inferred_function_return`,
`inferred_method_return`, `InferredBody`, `FIXPOINT_ITERATION_BUDGET`),
the plan-8 check stack (`typed_file_verdicts`, `typed_diagnostics`,
`TypedFileResult`, `analysis.rs::persistable_diagnostics` /
`typed_portion`), the plan-4a plugin identities (`PluginIdentity`), the
shipped cache (`CACHE_SCHEMA_VERSION`, `PackHeader`, `StoredVerdict`,
`lookup_verdict`, `CacheSnapshot`, `SnapshotCache`, `CacheStatistics`).

## Global Constraints

- **Zero panic, mechanically enforced**: workspace lints deny
  `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`;
  `unsafe_code` is forbidden. Test modules may locally `#[allow]`.
  No indexing: `.get()`, `.first()`, iterators, `.split_once()`.
- **TDD**: failing test → minimal implementation → refactor. No
  production code without a test that demanded it.
- **`TypeId` never hits disk** (design section 3): persistence uses the
  structural serialization only; a `TypeId` is a process-local interner
  handle and never appears in any `Stored*` type or pack payload.
- **Canonical ordering is structural, never by interner handle**
  (design section 3): every persisted collection is sorted by
  structural content before encoding, so pack bytes are identical
  across processes and thread counts.
- **No provisional value served or persisted** (design section 6): a
  cancelled or mid-fixpoint iterate never reaches a pack; persist runs
  only on a completed analysis outcome.
- **Cache misses are silent**: a corrupt, stale, absurd, or
  half-written entry is discarded and recomputed — never a diagnostic,
  never a panic (the shipped cache's contract, extended verbatim).
- **Determinism**: no wall clock, no randomness, no environment reads
  inside queries. Wall-clock reads are legal only in the orchestration
  layer (`persist` timing, statistics rendering).
- **Counters never live inside queries**: statistics increment at the
  orchestration layer or inside the composition-root-owned trait
  implementations (`SnapshotCache`), the shipped precedent.
- **Strict layering**: `celerrate_types` gains three external
  dependencies (`serde` derive, `postcard`, `blake3` — all already in
  the workspace) and zero new inter-crate edges. The typed cache trait
  is owned by `celerrate_types` and implemented in `celerrate_cli`,
  the `ArtifactCache` precedent. `cargo xtask dependency-shape` stays
  green.
- **Everything in English, full words** (standard acronyms fine).
- **Commits**: gitmoji + Conventional Commits, repository-configured
  identity, no AI attribution of any kind.
- Local gate for every task: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`.

## Fixed decisions (the header the tasks implement)

1. **The two artifact classes, verbatim from the design.** Per-body
   **inferred signatures** (returns, principally) persist: small,
   structural serialization, keyed by the defining file's content hash
   plus their own recorded reduced answers, revalidated recursively and
   memoized. Full **expression type tables** never persist: large,
   presumed a net loss, recomputed on demand from cached inferred
   signatures. Nothing in this plan writes an expression type table to
   disk, and a test pins that the packs contain no such payload class.
2. **`StoredType` is the structural serialization**, in
   `crates/celerrate_types/src/stored.rs`: a self-contained recursive
   mirror of `TypeData` (every variant covered — the symbolic forms
   `Template`, `Conditional`, `KeyOf`, `ValueOf` and the three
   placeholders survive into persisted returns, so the serialization
   cannot assume ground types) with `serde` derives.
   `StoredType::of(db, TypeId) -> StoredType` matches `TypeData`
   inside the owning crate; `to_type_id(&self, db) -> Option<TypeId>`
   re-interns through the public constructors, which canonicalize
   bottom-up — a decoded value that violates a canonical invariant
   (a one-armed union, an unsorted shape) re-canonicalizes instead of
   panicking or leaking a non-canonical handle. The round-trip law
   `StoredType::of(db, t).to_type_id(db) == Some(t)` holds for every
   constructible type within `STORED_DEPTH_LIMIT` and is
   property-tested over the constructor
   surface. Depth is guarded at the decode boundary, never assumed:
   `STRUCTURAL_DEPTH_CAP` is enforced in the widening join path
   (`widening.rs`), not by the constructors — a constructor can build
   an arbitrarily deep value (this plan's own depth test does), while
   decoding and re-interning recurse once per nesting level before
   any constructor runs, and a forged over-deep value would overflow
   the stack (an abort no `catch_unwind` contains, so a breach of the
   never-panics contract). `pub const STORED_DEPTH_LIMIT: usize`
   (comfortably above `STRUCTURAL_DEPTH_CAP`) therefore bounds both
   `Deserialize` (a depth-counting implementation, so decoding itself
   rejects an over-deep payload as a decode error) and `to_type_id`,
   which answers `None` past the limit: a silent cache miss for every
   caller, never a panic and never a served fallback type.
3. **Two new reduced-answer record classes — this plan's realization
   of design section 9's "signature-hash records" pattern.** The spec
   names the pattern ("signature-hash records, the same reduced-answer
   pattern"), not the classes: the two names below and the count of
   two are this plan's choices. **Class-surface digests**:
   `class_surface_digest(db, files, stubs, configuration, class:
   ClassQuery) -> Option<[u8; 32]>` in `celerrate_types` — blake3 over
   the postcard encoding of a canonical projection of the class's
   entire lookup surface: linearized members (kind, folded key, owner,
   flags, the **resolved** declared signature as `StoredType`s with
   their `Trust`), virtual members **with their full payload** (the
   resolved signature through `declared_member_signature`'s
   `MemberResolution::Virtual` arm, so a `@method` or `@property`
   type edit flips the digest, never existence alone), the
   class-like's own `DeclarationKind` and class-level flags
   (abstract, final, readonly), ancestry keys, `stub_ancestors`,
   `cyclic`, `has_opaque_edge`, `MagicMarkers`. Any change to
   existence, a signature, an annotation, ancestry, magic, a virtual
   member's existence or type, or the class-like's own kind or flags
   anywhere under the class flips the digest; `None` for a key
   that is not a source class-like (stub surfaces are pinned by the
   header's `stub_blob` field, and a key that later gains a source
   definition flips `None` → `Some`). **Function-signature digests**:
   `function_signature_digest(db, files, stubs, configuration, query:
   FunctionQuery) -> Option<[u8; 32]>` — the same digest over the
   resolved `DeclaredSignature` of a Function-space callee. Digests
   are computed by tracked queries, so both the persist side and the
   validation side are memoized per run.
4. **Records are constructive — a third mirror is never born.** The
   walkers that consume the facts produce the records at the
   consultation sites where `edge_counts` already increments:
   `InferredBody` gains `dependencies: TypedDependencies<'db>`
   (consulted class keys, consulted Function-space keys, inferred
   edges as `(key, TypeId)` pairs), filled by the flow walker; each
   recorded inferred-edge return is the **raw callee-query answer**,
   captured before `member_boundary_type` substitutes placeholders
   or threaded generic arguments, so it compares equal to the live
   query the decision-9 validator demands (recording the substituted
   value would mismatch forever, a silent loss of the cutoff); the
   plan-8 check walkers record every receiver, scoped-subject, and
   signature-owner class key they consult through their `CheckContext`;
   `typed_file_verdicts` aggregates both into
   `TypedFileResult.dependencies: FileDependencies` (lifetime-free:
   inferred-edge types converted to `StoredType` at aggregation).
   The persist path reads records; it never re-derives them.
5. **Provider edges carry no record class.** A dynamic type provider's
   answer is a pure function of its `Invocation` (own-body facts,
   covered by the defining file's content hash) and its
   `PluginIdentity` (covered by the header's plugin-set digest,
   decision 7). The constraint is documented on the
   `DynamicTypeProvider` trait: a future provider that reads
   cross-file state must extend the record vocabulary before it ships.
   Free-constant reads carry no record either: the member boundary
   gives free constants no typed payload, so inference types them
   `mixed` and no cross-file dependency exists — the same rustdoc
   records that a future free-constant type source extends the
   vocabulary symmetrically.
6. **The member tree joins the cache; the body IR does not.** New pack
   `member_trees.bin` (`StoredMemberTree`, the `StoredItemTree` mirror
   pattern member-deep), new trait method `ArtifactCache::member_tree(
   &self, file: FileId, content: ContentHash) -> Option<MemberTree>`
   with a default `None` body (test doubles unaffected), consulted by
   `queries.rs::member_tree` exactly as `item_tree` consults its hook
   — this closes the `queries.rs::member_tree` rustdoc debt (the
   "No artifact-cache consultation yet" sentence), and it is
   load-bearing:
   warm class-surface digests demand member trees, and without the
   pack a warm run would reparse every consulted file. The matching
   `body.rs::body_ir` sentence closes with a recorded decision
   instead: the
   body IR is never read on the warm serve path (content hashes stand
   in for body identity) and every recompute path has the parse
   anyway, so `body_ir` gets no consultation point — its rustdoc says
   so and names this plan.
7. **Pack format 5, and the header gains the plugin-set key.**
   `CACHE_SCHEMA_VERSION` 4 → 5, history line appended: "5 = typed
   artifacts: member-tree pack, inferred-signature pack, typed verdict
   fields, plugin-set header digest." `PackHeader` gains
   `pub plugins: [u8; 32]`: blake3 over the postcard encoding of the
   sorted `Vec<(String, String, String)>` of every registered plugin's
   `PluginIdentity` (name, version, configuration) — assembled at the
   composition root from the same registrations it sets on the
   registries, the fact the `PluginIdentity` rustdoc promised this
   plan would read. `PackHeader::current` gains the digest parameter;
   a version bump, a reconfiguration, or an added or removed plugin
   discards every pack wholesale, exactly like a stub-blob change.
8. **The inferred-signature pack.** `inferred_signatures.bin` carries
   `Vec<(StoredSignatureKey, StoredInferredSignature)>`, sorted by
   key. `StoredSignatureKey` is `Function { key: String }` or
   `Method { class_key: String, member_key: String }` — the same keys
   as `FunctionQuery` and `MethodQuery`. `StoredInferredSignature`
   carries the defining file's `ContentHash`, the return as
   `StoredType`, and the records: class-surface dependencies,
   function-signature dependencies, inferred edges. Persist enumerates
   each analyzed file's member tree (free functions plus
   `MemberKind::Method` members with resolvable owners) and reads the
   already-computed inference results — it never runs inference the
   analysis did not. Two recorded exclusions, both conservative:
   **trait bodies persist no entries** (their memo key includes the
   using-class context, which the trait's own file cannot enumerate)
   and **anonymous-class methods persist no entries** (`BodyOwner`
   answers no stable class key); both fall back to recomputation.
9. **The recursive memoized revalidation is the seeded query layer.**
   `celerrate_types/src/cache.rs` owns the extension point, the
   `ArtifactCache` pattern transposed:

   ```rust
   pub trait TypedArtifactCache: Send + Sync {
       fn inferred_signature(&self, key: &StoredSignatureKey)
           -> Option<StoredInferredSignature>;
   }
   #[derive(Clone)]
   pub struct TypedCacheHandle(pub Arc<dyn TypedArtifactCache>);
   #[salsa::input(singleton)]
   pub struct TypedCacheInput {
       #[returns(ref)]
       pub cache: TypedCacheHandle,
   }
   ```

   `inferred_function_return` and `inferred_method_return` consult it
   first: resolve the key to its defining file, compare the recorded
   content hash, re-check every class and function digest, and for
   each inferred edge **demand the live callee query** and compare
   against the re-interned expected return. All hold → serve the
   re-interned stored return; any miss → compute exactly as today.
   Salsa memoizes per key, so each signature validates once per run
   ("constructive-trace style"), and every fact the serve path reads
   is a salsa read, so the served result carries real dependencies
   and invalidates correctly on later in-process edits. An inferred
   edge whose callee's file changed but whose recomputed return equals
   the record still validates — early cutoff across the process
   boundary, the design's whole point. **Cyclic clusters always
   recompute, by rule**: a live callee answer equal to the
   cycle-provisional value (`never`) is treated as a validation
   **mismatch**, never a match, even when the record expects `never`.
   A mid-cycle provisional iterate can therefore never validate a
   record, and the join ascent can never absorb a stale served
   return (a served-then-invalidated value joined into an iterate
   would widen the warm fixpoint past the fresh one); a legitimately
   never-returning callee simply recomputes (rare, sound,
   deterministic, and recorded as a stance).
10. **Typed verdicts join `StoredVerdict`, independently droppable.**
    `StoredVerdict` gains `pub typed: Option<StoredTypedVerdict>`
    (`None` = not persisted); `StoredTypedVerdict` carries the typed
    diagnostics (post-suppression, the schema-4 convention) and the
    file-level records (class digests, function digests, inferred
    edges). The serve path is layered: the untyped records validate
    first (as today — a miss discards the whole entry); then, on an
    untyped hit, the typed portion serves only if it is present and
    every typed record validates (digest compares, plus one live
    demand per inferred edge — microseconds when the callee's own
    entry validates); otherwise `analyze_one` composes
    `typed_portion(inputs, file)` fresh for that file only. A partial
    hit (untyped served, typed recomputed) is a first-class outcome
    with its own counter.
11. **The fallback lever is one line** (design section 9): 
    `pub(crate) const PERSIST_TYPED_ARTIFACTS: bool = true;` in
    `cache/mod.rs` gates the typed fields and the signature pack at
    persist time. Flipping it to `false` drops typed verdicts from the
    cache, warm runs re-infer, and the release decision escalates —
    the honest number, never a silent one. A test runs the persist
    path under both values.
12. **The second revalidation mirror dies.** One tracked query,
    `reference_outcomes(db, file, files, stubs, configuration) ->
    ReferenceOutcomes { diagnostics: Vec<Diagnostic>, records:
    Vec<ResolutionRecord> }`, walks `collect_references` once and
    produces findings and answers from the same `resolve_name` call.
    `reference_diagnostics` and `resolution_records` become thin
    tracked projections (public API unchanged, both still backdate
    independently). Drift between the walks is now structurally
    impossible — the same closure `composed_diagnostics` gave the
    first mirror.
13. **`CELERRATE_CACHE_STATS` grows the serve-side counters.**
    `CacheStatistics` gains `member_tree_hits`, `member_tree_misses`
    (incremented in `SnapshotCache::member_tree`, the `item_tree`
    precedent), `signatures_found`, `signatures_absent` (incremented
    in the `TypedArtifactCache` implementation — presence only; the
    validation outcome lives in the query layer where counters are
    forbidden), and `typed_served`, `typed_recomputed` (incremented in
    `analyze_one` at the partial-hit fork). The post-9a `render()`
    format is five semicolon clauses, in this order: **trees** (the
    shipped item-tree pair, joined by `member_tree_hits` /
    `member_tree_misses`); **verdicts** (unchanged); **typed**
    (`typed_served` / `typed_recomputed` — the clause plan 8's
    decision 14 contributes, inserted before the persist clause);
    **signatures** (`signatures_found` / `signatures_absent` — this
    plan's clause); **persist** (the shipped written/skipped/failed
    clause, extended with the duration figure). `persist` additionally
    records its own wall-clock duration in `persist_milliseconds:
    AtomicU64` (orchestration-side, legal), rendered as a `{n}ms`
    figure folded into the existing persist clause — never a separate
    clause: the named instrument decision 15 and plan 9b's measurement
    procedure read.
14. **The harness extension is a gate, not an afterthought** (design:
    "or the net silently stops guarding"). The equivalence net
    (`cache_equivalence.rs`) gains typed fixtures asserting
    served-typed == recomputed-typed; the consistency harness
    (`cache_consistency.rs`) gains the new edit classes (a body edit
    that changes an inferred return and must flip a caller's typed
    verdict in another file; a signature edit; a docblock annotation
    edit; a virtual-member type edit, `@method`/`@property`). One
    recorded scope stance: the spec's default-value edit class belongs
    to harness 2's in-process invalidation scope (spec section 10) and
    is already pinned by
    `celerrate_types/tests/invalidation_scope.rs`, so it stays
    deliberately outside the cross-process consistency suite. The
    adversarial suite (`cache_seeding.rs`) gains
    hand-written v5 packs (absurd `StoredType` nesting, stale class
    digests, mismatched content hashes, duplicate signature keys, a
    flipped plugin digest) — every one discards silently, never
    panics, never changes the rendering; and a no-provisional pin
    asserts a cancelled watch cycle persists nothing (the persist call
    sits after `absorb_outcome`, so cancellation restarts the cycle
    before persist — the test makes that ordering a contract).
15. **Watch persist economics: measured, with the recorded
    alternative implemented only if the number demands it** (the
    plan-1a `source_symbol_table` pattern). With the packs fattened by
    the two new artifact classes, the closing task measures per-cycle
    persist duration (`persist_milliseconds` over scripted watch
    cycles on the corpus). Acceptance: per-cycle persist stays under
    10% of the median warm cycle. If it does, the number is recorded
    in the closing memo and per-cycle persist stays (audit finding I6
    is preserved: a crash loses at most one cycle). If it does not,
    the recorded alternative lands in the same task: persist runs on
    idle — `completed_cycle` skips `cache::persist` when
    `wait_for_a_burst` already has a pending burst, and a final
    persist runs on exit — with the crash-window regression stated in
    the memo.
16. **Public API deltas, complete list.** `celerrate_types` exports
    `StoredType`, `STORED_DEPTH_LIMIT`, `StoredSignatureKey`,
    `StoredInferredSignature`,
    `StoredClassDependency`, `StoredFunctionDependency`,
    `StoredInferredEdge`, `TypedArtifactCache`, `TypedCacheHandle`,
    `TypedCacheInput`, `class_surface_digest`,
    `function_signature_digest`, and the new fields on `InferredBody`
    and `TypedFileResult`. `celerrate_semantics` exports
    `reference_outcomes`, `ReferenceOutcomes`, and the
    `ArtifactCache::member_tree` default method. `celerrate_cli` keeps
    everything `pub(crate)` except what tests already reach through
    `celerrate_cli::cache::{pack, stored, snapshot, verdict}`.

## File structure

Created:

- `crates/celerrate_types/src/stored.rs` — `StoredType` (the
  structural serialization), `StoredSignatureKey`,
  `StoredInferredSignature`, `StoredClassDependency`,
  `StoredFunctionDependency`, `StoredInferredEdge`, the digest
  helpers (`signature_digest`, `surface_digest`).
- `crates/celerrate_types/src/cache.rs` — `TypedArtifactCache`,
  `TypedCacheHandle`, `TypedCacheInput` (the extension point).
- `crates/celerrate_types/src/records.rs` — `TypedDependencies<'db>`,
  `FileDependencies`, `class_surface_digest`,
  `function_signature_digest`.

Modified:

- `crates/celerrate_types/Cargo.toml` — `serde` (derive), `postcard`,
  `blake3` workspace dependencies.
- `crates/celerrate_types/src/lib.rs` — modules `stored`, `cache`,
  `records`; the decision-16 re-exports.
- `crates/celerrate_types/src/inference.rs` — `InferredBody` gains
  `dependencies`; the flow-walker seams record; the two return
  queries gain the consult-validate-serve head.
- `crates/celerrate_types/src/flow.rs` — the recording seams beside
  the `edge_counts` increments.
- `crates/celerrate_types/src/checks/mod.rs` — `TypedFileResult`
  gains `dependencies: FileDependencies`; aggregation.
- `crates/celerrate_types/src/checks/receivers.rs`,
  `checks/members.rs`, `checks/nullability.rs`,
  `checks/arguments.rs` — consulted-class recording through
  `CheckContext`.
- `crates/celerrate_types/src/dynamic_type_provider.rs` — the
  decision-5 constraint rustdoc.
- `crates/celerrate_semantics/src/reference_checks.rs` —
  `reference_outcomes`, `ReferenceOutcomes`; `reference_diagnostics`
  becomes a projection.
- `crates/celerrate_semantics/src/revalidation.rs` —
  `resolution_records` becomes a projection.
- `crates/celerrate_semantics/src/cache.rs` —
  `ArtifactCache::member_tree` (default `None`).
- `crates/celerrate_semantics/src/queries.rs` — `member_tree`
  consults the cache; the no-consultation-yet sentence closes.
- `crates/celerrate_semantics/src/body.rs` — the `body_ir` rustdoc
  records the no-consultation decision.
- `crates/celerrate_semantics/src/lib.rs` — the new exports.
- `crates/celerrate_cli/src/cache/pack.rs` — schema 5, the `plugins`
  header field, the history line.
- `crates/celerrate_cli/src/cache/stored.rs` — `StoredMemberTree`
  and its member mirrors; `StoredVerdict.typed`;
  `StoredTypedVerdict`.
- `crates/celerrate_cli/src/cache/snapshot.rs` — the two new packs,
  the two new snapshot maps, `SnapshotCache::member_tree`, the
  `TypedArtifactCache` implementation.
- `crates/celerrate_cli/src/cache/verdict.rs` — the layered typed
  validation in `lookup_verdict`.
- `crates/celerrate_cli/src/cache/mod.rs` — signature-entry
  collection, `PERSIST_TYPED_ARTIFACTS`, the new pack writes,
  persist timing.
- `crates/celerrate_cli/src/cache/statistics.rs` — the decision-13
  counters and render clause.
- `crates/celerrate_cli/src/analysis.rs` — the partial-hit fork in
  `analyze_one`.
- `crates/celerrate_cli/src/plugins.rs` — `plugin_set_digest()`.
- `crates/celerrate_cli/src/session.rs` — header construction with
  the plugin digest; `TypedCacheInput` registration.
- `crates/celerrate_cli/src/watch.rs` — decision 15's outcome (only
  if the measurement demands it).
- `crates/celerrate_cli/tests/cache_equivalence.rs`,
  `tests/cache_consistency.rs`, `tests/cache_seeding.rs` — the
  decision-14 extensions.
- `crates/celerrate_types/tests/invalidation_scope.rs` — the
  cross-boundary cutoff pins.

Task order is strict: 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11 →
12. Task 1 is the serialization everything else encodes with; 2 the
digest queries; 3 the constructive records; 4 the mirror closure
(independent, but it reshapes `revalidation.rs` before task 9 reads
it); 5 the member-tree pack; 6 the format bump; 7 the signature pack;
8 the seeded query layer; 9 the typed verdict serve; 10 the harness
extension; 11 the watch economics; 12 the closure pins and the debt
ledger. Do not parallelize: 5–9 all touch the cache seams, and 3, 8
both touch `inference.rs`.

---

### Task 1: `StoredType` — the structural serialization

**Files:**
- Create: `crates/celerrate_types/src/stored.rs`
- Modify: `crates/celerrate_types/Cargo.toml`
- Modify: `crates/celerrate_types/src/lib.rs`
- Test: inline `#[cfg(test)] mod tests` in `stored.rs`

**Interfaces:**
- Consumes: `TypeId<'db>` / `TypeData<'db>` and the constructor
  surface of `crates/celerrate_types/src/construction.rs` (`union`,
  `intersection`, `array`, `shape`, `class`, `enum_case`,
  `class_string`, `callable`, `template`, `key_of`, `value_of`,
  `conditional`, `static_placeholder`, `self_placeholder`,
  `parent_placeholder`, the scalar constructors), `FloatBits`,
  `StringConstraint`, `ShapeKey`, `ShapeField`, `CallableParameter`
  from `representation.rs`.
- Produces: `pub enum StoredType` with
  `pub fn of(db: &dyn salsa::Database, of: TypeId<'_>) -> StoredType`
  and
  `pub fn to_type_id<'db>(&self, db: &'db dyn salsa::Database) -> Option<TypeId<'db>>`
  (`None` past `pub const STORED_DEPTH_LIMIT: usize`, the decision-2
  decode guard),
  plus the leaf mirrors `StoredStringConstraint`, `StoredShapeKey`,
  `StoredShapeField`, `StoredCallableParameter`. Every later task
  encodes types exclusively through this pair.

- [ ] **Step 1: Add the dependencies**

In `crates/celerrate_types/Cargo.toml`, under `[dependencies]`:

```toml
blake3 = { workspace = true }
postcard = { workspace = true }
serde = { workspace = true }
```

(All three already exist at the workspace root — `blake3` and
`postcard` are used by `celerrate_db`/`celerrate_cli`/
`celerrate_stubs`; check `Cargo.toml` at the root and add any missing
`[workspace.dependencies]` entry with the version the other crates
already use, never a second version.)

- [ ] **Step 2: Write the failing round-trip tests**

Create `crates/celerrate_types/src/stored.rs` with the test module
first (the module skeleton may contain only the types' declarations to
let the tests compile — TDD on shape, then behavior):

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::TypeId;

    fn database() -> impl salsa::Database {
        salsa::DatabaseImpl::new()
    }

    #[test]
    fn every_ground_shape_round_trips() {
        let db = database();
        let samples = vec![
            TypeId::mixed(&db),
            TypeId::never(&db),
            TypeId::void(&db),
            TypeId::null(&db),
            TypeId::object(&db),
            TypeId::resource(&db),
            TypeId::bool(&db),
            TypeId::bool_literal(&db, true),
            TypeId::int(&db),
            TypeId::int_literal(&db, 42),
            TypeId::int_range(&db, Some(1), None),
            TypeId::float(&db),
            TypeId::float_literal(&db, 1.5),
            TypeId::string(&db),
            TypeId::non_empty_string(&db),
            TypeId::numeric_string(&db),
            TypeId::literal_string_type(&db),
            TypeId::string_literal(&db, "active"),
            TypeId::class(&db, "app\\user", vec![]),
            TypeId::enum_case(&db, "app\\status", "Active"),
            TypeId::class_string(&db, None),
        ];
        for sample in samples {
            let stored = StoredType::of(&db, sample);
            assert_eq!(stored.to_type_id(&db), Some(sample));
        }
    }

    #[test]
    fn every_composite_shape_round_trips() {
        let db = database();
        let user = TypeId::class(&db, "app\\user", vec![]);
        let nullable = TypeId::union(&db, [user, TypeId::null(&db)]);
        let samples = vec![
            nullable,
            TypeId::intersection(&db, [user, TypeId::class(&db, "countable", vec![])]),
            TypeId::array(&db, TypeId::string(&db), nullable),
            TypeId::list(&db, user),
            TypeId::non_empty_array(&db, TypeId::int(&db), user),
            TypeId::shape(&db, vec![
                ShapeField { key: ShapeKey::String("id".to_owned()), optional: false, value: TypeId::int(&db) },
                ShapeField { key: ShapeKey::Integer(0), optional: true, value: nullable },
            ]),
            TypeId::class(&db, "collection", vec![user]),
            TypeId::class_string(&db, Some(user)),
            TypeId::callable(&db, vec![CallableParameter {
                parameter_type: user, optional: false, variadic: false, by_reference: false,
            }], nullable),
        ];
        for sample in samples {
            let stored = StoredType::of(&db, sample);
            assert_eq!(stored.to_type_id(&db), Some(sample));
        }
    }

    #[test]
    fn every_symbolic_shape_round_trips() {
        // Symbolic forms survive into persisted inferred returns
        // (plan 6): the serialization cannot assume ground types.
        let db = database();
        let bound = TypeId::class(&db, "app\\entity", vec![]);
        let template = TypeId::template(&db, "app\\repo::find", "T", bound);
        let samples = vec![
            template,
            TypeId::key_of(&db, TypeId::array(&db, TypeId::string(&db), bound)),
            TypeId::value_of(&db, TypeId::array(&db, TypeId::string(&db), bound)),
            TypeId::conditional(&db, template, bound, TypeId::null(&db), bound, false),
            TypeId::static_placeholder(&db),
            TypeId::self_placeholder(&db),
            TypeId::parent_placeholder(&db),
        ];
        for sample in samples {
            let stored = StoredType::of(&db, sample);
            assert_eq!(stored.to_type_id(&db), Some(sample));
        }
    }

    #[test]
    fn a_forged_non_canonical_value_re_canonicalizes_instead_of_panicking() {
        // A hand-written pack can carry anything: a one-armed union,
        // a duplicated constituent. Re-interning goes through the
        // constructors, which canonicalize — never a panic, never a
        // non-canonical handle.
        let db = database();
        let one_armed = StoredType::Union { constituents: vec![StoredType::Int { minimum: None, maximum: None }] };
        assert_eq!(one_armed.to_type_id(&db), Some(TypeId::int(&db)));
        let duplicated = StoredType::Union { constituents: vec![
            StoredType::Null,
            StoredType::Null,
            StoredType::Int { minimum: None, maximum: None },
        ]};
        assert_eq!(
            duplicated.to_type_id(&db),
            Some(TypeId::union(&db, [TypeId::null(&db), TypeId::int(&db)]))
        );
        let empty = StoredType::Union { constituents: vec![] };
        // An empty union has no constructible meaning: the defined
        // degenerate answer is `never` (the union identity).
        assert_eq!(empty.to_type_id(&db), Some(TypeId::never(&db)));
    }

    #[test]
    fn an_over_deep_value_is_a_silent_miss_never_an_overflow() {
        // Nesting past STORED_DEPTH_LIMIT is forged with an iterative
        // fold (the test itself never recurses): to_type_id answers
        // None, never a panic, never a stack overflow. The
        // Deserialize half of the guard is pinned at the byte level
        // in task 10's adversarial suite.
        let db = database();
        let mut deep = StoredType::Null;
        for _ in 0..=STORED_DEPTH_LIMIT {
            deep = StoredType::KeyOf { subject: Box::new(deep) };
        }
        assert_eq!(deep.to_type_id(&db), None);
    }

    #[test]
    fn the_encoding_is_deterministic() {
        let db = database();
        let user = TypeId::class(&db, "app\\user", vec![]);
        let value = TypeId::union(&db, [user, TypeId::null(&db), TypeId::int(&db)]);
        let first = postcard::to_allocvec(&StoredType::of(&db, value)).unwrap();
        let second = postcard::to_allocvec(&StoredType::of(&db, value)).unwrap();
        assert_eq!(first, second);
    }
}
```

Adjust constructor call shapes to the real `construction.rs` surface
(the names are pinned by plan 2; argument order may differ — follow
the code, not this sketch, and keep the covered-variant set complete).

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p celerrate_types stored -- --nocapture`
Expected: FAIL — `StoredType` not defined.

- [ ] **Step 4: Implement `StoredType`**

The mirror is mechanical: one variant per `TypeData` variant,
`TypeId` fields become `Box<StoredType>` / `Vec<StoredType>`:

```rust
//! The structural serialization of the type lattice: `TypeId` is a
//! process-local interner handle and never hits disk (design section
//! 3); a persisted type is this self-contained mirror, re-interned
//! through the public constructors on the way back in — which
//! canonicalize, so a forged or stale value re-canonicalizes instead
//! of panicking.

use serde::{Deserialize, Serialize};

use crate::representation::{
    CallableParameter, FloatBits, ShapeField, ShapeKey, StringConstraint, TypeData,
};
use crate::TypeId;

// `Deserialize` is a manual, depth-counting implementation
// (decision 2's decode guard); everything else derives.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub enum StoredType {
    Mixed,
    Never,
    Void,
    Null,
    Object,
    Resource,
    Bool { literal: Option<bool> },
    Int { minimum: Option<i64>, maximum: Option<i64> },
    Float { literal: Option<u64> },
    String { constraint: StoredStringConstraint },
    Union { constituents: Vec<StoredType> },
    Intersection { intersectands: Vec<StoredType> },
    Array { key: Box<StoredType>, value: Box<StoredType>, is_list: bool, non_empty: bool },
    Shape { fields: Vec<StoredShapeField> },
    ClassString { argument: Option<Box<StoredType>> },
    Class { name: String, arguments: Vec<StoredType> },
    EnumCase { enum_name: String, case_name: String },
    Callable { parameters: Vec<StoredCallableParameter>, return_type: Box<StoredType> },
    Template { scope: String, name: String, bound: Box<StoredType> },
    KeyOf { subject: Box<StoredType> },
    ValueOf { subject: Box<StoredType> },
    Conditional {
        subject: Box<StoredType>,
        matches: Box<StoredType>,
        then_branch: Box<StoredType>,
        otherwise_branch: Box<StoredType>,
        negated: bool,
    },
    SelfPlaceholder,
    ParentPlaceholder,
    StaticPlaceholder,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StoredStringConstraint {
    General,
    NonEmpty,
    Numeric,
    LiteralMarker,
    Literal(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoredShapeField {
    pub key: StoredShapeKey,
    pub optional: bool,
    pub value: StoredType,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StoredShapeKey {
    Integer(i64),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoredCallableParameter {
    pub parameter_type: StoredType,
    pub optional: bool,
    pub variadic: bool,
    pub by_reference: bool,
}
```

`of` matches `type_id.data(db)` exhaustively (this module lives in the
owning crate, where `TypeData` is matchable; the no-matchable-enum
commitment binds plugins, not the crate itself). `to_type_id` calls
the constructor for each variant; the composite arms recurse; the
`Union`/`Intersection` arms feed the collected constituents to
`TypeId::union` / `TypeId::intersection` (which canonicalize: flatten,
dedup, sort, collapse arity 1, answer the identity on arity 0 —
`never` for a union). `Float` stores the `FloatBits` bit pattern
(`u64`) and re-interns through the bits-preserving constructor so NaN
canonicalization and the `0.0`/`-0.0` distinction survive. No arm may
panic; there is no `unwrap` anywhere in the module. `to_type_id`
threads a depth counter and answers `None` past `STORED_DEPTH_LIMIT`
(the decision-2 guard), and `StoredType`'s `Deserialize` is a manual
implementation carrying the same depth budget, because postcard
recurses once per nesting level and the guard must run before the
stack does.

- [ ] **Step 5: Declare the module and run the tests**

In `crates/celerrate_types/src/lib.rs`: `pub mod stored;` and re-export
`pub use stored::StoredType;`.

Run: `cargo test -p celerrate_types stored`
Expected: PASS.

- [ ] **Step 6: Full gate and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): the structural type serialization"
```

### Task 2: The digest queries

**Files:**
- Create: `crates/celerrate_types/src/records.rs` (the digest queries;
  the dependency records arrive in task 3)
- Modify: `crates/celerrate_types/src/stored.rs` (the digest helpers)
- Modify: `crates/celerrate_types/src/lib.rs`
- Test: inline in `records.rs`

**Interfaces:**
- Consumes: `linearized_class`, `LinearizedClass`, `ClassQuery`,
  `MemberQuery`, `folded_member_key` from `celerrate_semantics`;
  `declared_member_signature`, `declared_function_signature`,
  `DeclaredSignature`, `DeclaredParameter`, `Trust`, `FunctionQuery`
  from `declared.rs`; `StoredType` from task 1.
- Produces:
  `pub fn class_surface_digest<'db>(db, files: AnalyzedFileSet, stubs: StubIndexInput, configuration: ProjectConfiguration, class: ClassQuery<'db>) -> Option<[u8; 32]>`
  (tracked) and
  `pub fn function_signature_digest<'db>(db, files, stubs, configuration, query: FunctionQuery<'db>) -> Option<[u8; 32]>`
  (tracked), plus `stored.rs::StoredSignature`
  (`{ parameters: Vec<StoredParameter>, value_type: StoredType, value_trust: StoredTrust, by_reference: bool }`
  with `StoredParameter { name, parameter_type: Option<StoredType>, trust: StoredTrust, optional, variadic, by_reference }`
  and `StoredTrust { NativeOnly, Refined, RefinedUnproven, RejectedAnnotation }`)
  — tasks 7–9 compare these digests; task 7 persists `StoredSignature`
  digests indirectly through them.

- [ ] **Step 1: Write the failing digest-sensitivity tests**

In `records.rs`, a fixture helper in the style of
`crates/celerrate_types/src/declared.rs::tests` (one in-memory project,
`AnalyzedFileSet` + `StubIndexInput` + `ProjectConfiguration` built the
way that module's tests build them — copy the existing helper shape,
do not invent a new one). Tests:

```rust
#[test]
fn the_digest_is_stable_across_identical_projects() { /* same sources
    twice, two databases: identical digests — process-independence */ }

#[test]
fn adding_a_member_flips_the_digest() { /* class with one method vs
    the same class plus a property: digests differ */ }

#[test]
fn editing_a_signature_flips_the_digest() { /* `function f(): int` vs
    `function f(): string` on a member: digests differ */ }

#[test]
fn editing_an_annotation_flips_the_digest() { /* same native
    signature, a docblock `@return string` appears: digests differ —
    the digest is over RESOLVED signatures, so annotation-layer
    changes count */ }

#[test]
fn an_ancestry_change_flips_the_digest() { /* `class B {}` vs
    `class B extends A {}` (A defining a member): B's digest differs
    — the linearized table folds ancestors in */ }

#[test]
fn a_magic_marker_flips_the_digest() { /* adding `__get` to the class:
    digests differ */ }

#[test]
fn editing_a_virtual_member_type_flips_the_digest() { /* the class
    docblock's `@method User find()` becomes `@method Order find()`:
    digests differ (the payload participates, never existence alone) */ }

#[test]
fn a_declaration_kind_or_class_flag_change_flips_the_digest() { /* the
    same member surface declared `class` then `interface`, and `class`
    then `final class`: digests differ */ }

#[test]
fn a_body_edit_does_not_flip_the_digest() { /* editing a method BODY
    (same signature): digests equal — the member boundary's whole
    point, and the warm path's economics */ }

#[test]
fn a_non_source_key_answers_none() { /* `class_surface_digest` of
    "datetime" (a stub) and of an unknown key: None */ }

#[test]
fn a_function_signature_digest_flips_on_signature_edits_only() {
    /* same trio for a free function: stable, flips on signature or
    annotation edit, survives a body edit, None when unresolved */ }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_types records`
Expected: FAIL — queries not defined.

- [ ] **Step 3: Implement the digests**

In `stored.rs`, the signature mirror and one helper:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StoredTrust { NativeOnly, Refined, RefinedUnproven, RejectedAnnotation }

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoredParameter {
    pub name: String,
    /// `None` mirrors the empty-intersection stub guard verbatim: it
    /// is a judgment-visible fact, so it participates in the digest.
    pub parameter_type: Option<StoredType>,
    pub trust: StoredTrust,
    pub optional: bool,
    pub variadic: bool,
    pub by_reference: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoredSignature {
    pub parameters: Vec<StoredParameter>,
    pub value_type: StoredType,
    pub value_trust: StoredTrust,
    pub by_reference: bool,
}

/// blake3 over the postcard encoding; `None` when encoding fails
/// (never observed for these plain shapes, but the zero-panic rule
/// forbids assuming it).
pub(crate) fn digest_of<T: Serialize>(value: &T) -> Option<[u8; 32]> {
    let bytes = postcard::to_allocvec(value).ok()?;
    Some(*blake3::hash(&bytes).as_bytes())
}
```

In `records.rs`, the class surface projection and the two tracked
queries:

```rust
/// The canonical projection of one class's whole lookup surface:
/// everything `lookup_member`, `member_existence`, a judgment's
/// ancestry walk, or declared-signature resolution could consult.
/// One digest compare revalidates all of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SurfaceProjection {
    kind: u8,                  // the class-like's own DeclarationKind discriminant
    flags: (bool, bool, bool), // abstract, final, readonly
    members: Vec<SurfaceMember>,
    virtual_members: Vec<SurfaceVirtualMember>,
    ancestry: Vec<String>,                  // ancestor folded keys, walk order
    stub_ancestors: Vec<String>,
    cyclic: bool,
    has_opaque_edge: bool,
    magic: (bool, bool, bool, bool, bool),  // the five MagicMarkers, field order
}

/// One annotation-declared member, payload included: a `@method` or
/// `@property` type edit must flip the digest, never existence alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SurfaceVirtualMember {
    kind: u8,                // VirtualMemberKind discriminant
    key: String,             // folded member key
    owner: String,           // declaring class-like folded key
    signature: Option<StoredSignature>, // resolved through the Virtual arm
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SurfaceMember {
    kind: u8,                // MemberKind discriminant, stable order
    key: String,             // folded member key
    owner: String,           // declaring class folded key
    is_static: bool,
    visibility: u8,
    signature: Option<StoredSignature>, // resolved through declared_member_signature
}

#[salsa::tracked]
pub fn class_surface_digest<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    class: ClassQuery<'db>,
) -> Option<[u8; 32]> {
    let linearized = linearized_class(db, files, stubs, configuration, class).as_ref()?;
    // Build SurfaceProjection: for each linearized member, resolve its
    // declared signature through declared_member_signature (a
    // per-(class, member) query — memoized, and the annotation layer
    // participates); StoredSignature::of the result. Virtual members
    // resolve through the same query's Virtual arm and contribute
    // their resolved signature beside (kind, key, owner). The
    // class-like's own kind and flags come from the declaration
    // entry the linearization resolves first. Sort orders are the
    // linearized table's own (already deterministic).
    crate::stored::digest_of(&projection)
}
```

`function_signature_digest` resolves through
`declared_function_signature` and digests the `StoredSignature` alone
(`None` when the signature is `None` — an unresolved callee is a
recordable answer, compared as `None` at validation). Conversions
`StoredSignature::of(db, &DeclaredSignature) -> StoredSignature` and
`StoredTrust::of(Trust)` live in `stored.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p celerrate_types records`
Expected: PASS.

- [ ] **Step 5: Full gate and commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): class-surface and function-signature digests"
```

### Task 3: Constructive dependency records

**Files:**
- Modify: `crates/celerrate_types/src/records.rs`
  (`TypedDependencies`, `FileDependencies`)
- Modify: `crates/celerrate_types/src/inference.rs` (`InferredBody`
  gains `dependencies`)
- Modify: `crates/celerrate_types/src/flow.rs` (the recording seams)
- Modify: `crates/celerrate_types/src/checks/mod.rs`
  (`TypedFileResult.dependencies`, aggregation)
- Modify: `crates/celerrate_types/src/checks/receivers.rs`,
  `members.rs`, `nullability.rs`, `arguments.rs` (record consulted
  classes through `CheckContext`)
- Modify: `crates/celerrate_types/src/dynamic_type_provider.rs`
  (the decision-5 rustdoc)
- Test: inline in `inference.rs` tests + `checks/mod.rs` tests

**Interfaces:**
- Consumes: the flow walker's existing `edge_counts` increment sites
  (`function_call_result`, `method_call_result_for_keys`), plan 8's
  `CheckContext` and walkers, `StoredType` (task 1).
- Produces:

  ```rust
  // records.rs
  #[derive(Debug, Clone, PartialEq, Eq, Default, salsa::Update)]
  pub struct TypedDependencies<'db> {
      /// Folded keys of every class whose surface was consulted.
      pub classes: BTreeSet<String>,
      /// Function-space keys whose declared signature was consulted.
      pub functions: BTreeSet<String>,
      /// (function key, consumed inferred return).
      pub inferred_functions: Vec<(String, TypeId<'db>)>,
      /// ((defining class key, member key), consumed inferred return).
      pub inferred_methods: Vec<((String, String), TypeId<'db>)>,
  }

  // records.rs — lifetime-free, aggregated per file
  #[derive(Debug, Clone, PartialEq, Eq, Default)]
  pub struct FileDependencies {
      pub classes: BTreeSet<String>,
      pub functions: BTreeSet<String>,
      pub inferred_functions: Vec<(String, StoredType)>,
      pub inferred_methods: Vec<((String, String), StoredType)>,
  }
  ```

  `InferredBody` gains `pub dependencies: TypedDependencies<'db>`;
  `TypedFileResult` gains `pub dependencies: FileDependencies`.
  Tasks 7 and 9 read these verbatim.

- [ ] **Step 1: Write the failing recording tests**

In `inference.rs::tests`, beside the existing edge-count tests (the
same fixtures serve — `the_edge_count_instrument_counts_each_tier_once`
is the model):

```rust
#[test]
fn the_walker_records_each_dependency_it_consults() {
    // One body calling: an annotated free function (declared tier),
    // an unannotated free function (inferred tier), a method on a
    // class (class surface + inferred method tier).
    // Assert: dependencies.functions contains the declared callee's
    // key; dependencies.inferred_functions contains (key, the callee's
    // inferred return); dependencies.classes contains the receiver's
    // folded key; inferred_methods contains the (class, member) pair.
}

#[test]
fn recording_preserves_the_eq_cutoff() {
    // Two textually different but inference-identical bodies produce
    // equal InferredBody values (dependencies included) — the
    // backdating contract survives the new field.
}

#[test]
fn recorded_inferred_edges_carry_the_raw_pre_substitution_return() {
    // A callee method whose unannotated body returns `static`: the
    // recorded edge carries the placeholder-carrying raw query
    // answer, not the call site's substituted class type, so it
    // equals what task 8's validator demands live (decision 4).
}
```

In `checks/mod.rs::tests`:

```rust
#[test]
fn the_checks_record_the_receivers_they_consult() {
    // A file whose body dereferences `$user->name` (User defined in
    // another file): typed_file_verdicts(...).dependencies.classes
    // contains "app\\user" even when no diagnostic fires — absence
    // is a verdict too, and its revalidation needs the record.
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_types dependencies -- --nocapture` (and
the checks test) — Expected: FAIL, fields not defined.

- [ ] **Step 3: Implement the recording**

`TypedDependencies`/`FileDependencies` as specified. In `flow.rs`, at
each existing `edge_counts` increment site, push the identity beside
the count (the walker already holds the key it consulted — that is
what makes this constructive):

- declared free-function edge → `dependencies.functions.insert(key)`;
- inferred free-function edge →
  `dependencies.inferred_functions.push((key, returned))`;
- inferred method edge →
  `dependencies.inferred_methods.push(((class_key, member_key), returned))`;
- every `lookup_member` / `linearized_class` consultation the walker
  performs (receiver resolution, iteration protocol, property types,
  the body owner's own class) → `dependencies.classes.insert(key)`.

In both inferred-edge bullets, `returned` is the **raw callee-query
answer**, captured before `member_boundary_type` substitutes
placeholders or threaded generic arguments (decision 4): recording
the substituted value would mismatch the task-8 validator's live
demand forever, a silent loss of the cross-process cutoff no
rendering test would catch.

Sort-and-dedup the two `Vec`s once at walk end (deterministic order).
In the plan-8 walkers, thread `&mut BTreeSet<String>` through
`CheckContext` and insert at every `member_existence`,
`resolved_call_signature`, and coercion-mode `lookup_member`
consultation; `typed_file_verdicts` unions the bodies'
`TypedDependencies` (converting `TypeId` → `StoredType`) with the
checks' set into `TypedFileResult.dependencies`.

In `dynamic_type_provider.rs`, extend the trait rustdoc: "A provider's
answer must be a pure function of its `Invocation` and its
`PluginIdentity`: the persistent cache records no per-answer
dependency (plan 9a decision 5), so a provider that reads cross-file
state would silently break warm revalidation — extend the record
vocabulary in `celerrate_types::records` before shipping one."

- [ ] **Step 4: Run the tests, then the invalidation-scope suite**

Run: `cargo test -p celerrate_types`
Expected: PASS, including the pre-existing plan-5/6 suites (the new
field must not disturb any backdating pin).

- [ ] **Step 5: Full gate and commit**

```bash
git add crates/celerrate_types
git commit -m "✨ feat(types): constructive typed-dependency records"
```
### Task 4: The second revalidation mirror dies

**Files:**
- Modify: `crates/celerrate_semantics/src/reference_checks.rs`
- Modify: `crates/celerrate_semantics/src/revalidation.rs`
- Modify: `crates/celerrate_semantics/src/lib.rs`
- Test: existing suites in both files, plus one new drift pin

**Interfaces:**
- Consumes: `collect_references`, `resolve_name`, `answer_of`,
  `UseTables`, `SymbolSources` — the two walks' shared vocabulary.
- Produces:

  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct ReferenceOutcomes {
      pub diagnostics: Vec<Diagnostic>,
      pub records: Vec<ResolutionRecord>,
  }

  #[salsa::tracked(returns(ref))]
  pub fn reference_outcomes(
      db: &dyn salsa::Database,
      file: SourceFile,
      files: AnalyzedFileSet,
      stubs: StubIndexInput,
      configuration: ProjectConfiguration,
  ) -> ReferenceOutcomes
  ```

  `reference_diagnostics` and `resolution_records` keep their exact
  public signatures and become projections (`.diagnostics.clone()` /
  `.records.clone()` over `reference_outcomes`) — no caller anywhere
  changes.

- [ ] **Step 1: Write the failing drift pin**

In `revalidation.rs::tests`:

```rust
#[test]
fn findings_and_answers_come_from_one_walk() {
    // A file with one unknown class, one stub reference, one source
    // reference. Assert reference_outcomes' records length equals
    // resolution_records' (the projection is total), and that every
    // diagnostic in reference_outcomes corresponds to a record whose
    // answer explains it: Unknown → an unknown-symbol id,
    // Stub with a violating window → a gating id. The correspondence
    // is the property the two hand-maintained walks could never
    // guarantee.
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_semantics reference_outcomes`
Expected: FAIL — not defined.

- [ ] **Step 3: Implement the single walk**

Move the body of `reference_diagnostics` into `reference_outcomes` and
extend its single loop: for each reference, `resolve_name` once, then
*both* push the record (`ResolutionRecord { written, space, namespace,
answer: answer_of(resolution) }`) *and* run the existing
diagnostic match on the same resolution value. Sort diagnostics as
today; records keep walk order (the shipped `resolution_records`
convention — check its tests and keep whichever order they pin).
Rewrite the two old queries as projections. Delete the duplicated
traversal from `revalidation.rs` (the module keeps the types,
`answer_of`, and its tests). Update both module docs: the mirror
language ("the same traversal … reduced to answers") is replaced by
the constructive statement ("one walk produces findings and answers;
drift is structurally impossible — the `composed_diagnostics`
closure, applied to the second mirror, plan 9a").

- [ ] **Step 4: Run the full semantics and CLI suites**

Run: `cargo test -p celerrate_semantics && cargo test -p celerrate_cli`
Expected: PASS — every existing revalidation, equivalence, and
consistency test unchanged (the refactor is observationally neutral).

- [ ] **Step 5: Full gate and commit**

```bash
git add crates/celerrate_semantics
git commit -m "♻️ refactor(semantics): findings and answers from one reference walk"
```

### Task 5: The member-tree pack

**Files:**
- Modify: `crates/celerrate_semantics/src/cache.rs`
  (`ArtifactCache::member_tree`, default `None`)
- Modify: `crates/celerrate_semantics/src/queries.rs` (`member_tree`
  consults; the no-consultation-yet sentence closes)
- Modify: `crates/celerrate_semantics/src/body.rs` (the `body_ir`
  rustdoc records the no-consultation decision, decision 6)
- Modify: `crates/celerrate_cli/src/cache/stored.rs`
  (`StoredMemberTree` and mirrors)
- Modify: `crates/celerrate_cli/src/cache/snapshot.rs`
  (`MEMBER_TREES_PACK`, the snapshot map, `SnapshotCache::member_tree`)
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (collection and the
  third `write_when_changed`)
- Modify: `crates/celerrate_cli/src/cache/statistics.rs`
  (`member_tree_hits` / `member_tree_misses`)
- Test: `crates/celerrate_cli/tests/cache_seeding.rs` + inline
  `stored.rs` round-trip tests

**Interfaces:**
- Consumes: `MemberTree`, `ClassMembers`, `Member`, `MemberSignature`,
  `ParameterSignature`, `MemberFlags`, `Visibility`, `TraitUse`,
  `TraitAdaptation`, `FreeFunction`, `MemberKind`, `DeclarationKind`,
  `AstId` from `celerrate_semantics::members`; the `StoredItemTree`
  mirror pattern (`of` / `to_*` with the file identity stamped back).
- Produces: `StoredMemberTree::of(&MemberTree) -> StoredMemberTree`
  and `to_member_tree(&self, file: FileId) -> MemberTree`;
  `pub const MEMBER_TREES_PACK: &str = "member_trees.bin";`
  `CacheSnapshot.member_trees: HashMap<ContentHash, StoredMemberTree>`.
  Task 8's warm digest path depends on this seeding existing.

- [ ] **Step 1: Write the failing mirror round-trip test**

In `stored.rs::tests`:

```rust
#[test]
fn a_member_tree_round_trips_onto_another_file_identity() {
    // Build a MemberTree by parsing a fixture with: a class with one
    // method (typed parameters, a default, by-reference, a docblock),
    // one readonly promoted-constructor property, a trait use with an
    // `as` adaptation, an `#[AllowDynamicProperties]` attribute, an
    // enum with a case, a free function with a docblock, an anonymous
    // class. StoredMemberTree::of(...).to_member_tree(other_file)
    // equals the original with every AstId's file swapped.
}
```

And the seeding integration test in `cache_seeding.rs` (the
`a_matching_pack_seeds_the_item_tree_query` model):

```rust
#[test]
fn a_matching_pack_seeds_the_member_tree_query() { /* write a
    member-trees pack whose entry deliberately differs from the true
    projection (the probe convention), run, assert the query answered
    the planted value — proof of consultation. */ }

#[test]
fn a_member_tree_with_an_absurd_ast_index_never_panics() { /* the
    adversarial convention, transposed. */ }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli member_tree`
Expected: FAIL — types and pack not defined.

- [ ] **Step 3: Implement the mirror, the trait method, the pack**

`StoredMemberTree` mirrors `MemberTree` field-for-field with `AstId`
reduced to `ast_index: u32` (the `StoredDeclaration` convention) and
everything else carried verbatim (`String`s, flags as plain bools,
`Visibility` as a stored enum). In `celerrate_semantics/src/cache.rs`:

```rust
pub trait ArtifactCache: Send + Sync {
    fn item_tree(&self, file: FileId, content: ContentHash) -> Option<ItemTree>;
    /// The member projection, same contract as `item_tree`. Default
    /// `None`: an implementation that has no member pack simply
    /// misses (plan 9a).
    fn member_tree(&self, _file: FileId, _content: ContentHash) -> Option<MemberTree> {
        None
    }
}
```

`queries.rs::member_tree` consults `ArtifactCacheInput::try_get(db)`
before projecting, exactly as `item_tree` does, and its
no-consultation-yet rustdoc sentence is deleted. `body.rs::body_ir`'s
matching sentence becomes: "No artifact-cache
consultation, decided (plan 9a decision 6): the warm serve path never
reads a body IR — content hashes stand in for body identity — and
every recompute path has the parse anyway."

`snapshot.rs`: the third constant, the third map in `CacheSnapshot`
(loaded via the existing generic `load_pack`), and
`SnapshotCache::member_tree` counting `member_tree_hits`/`misses`.
`mod.rs::collect_entries` collects `(content_hash, StoredMemberTree)`
per analyzed file beside the item trees (vendor files included — the
item-tree convention); `persist` writes the third pack through
`write_when_changed`.

- [ ] **Step 4: Run the suites**

Run: `cargo test -p celerrate_cli && cargo test -p celerrate_semantics`
Expected: PASS.

- [ ] **Step 5: Full gate and commit**

```bash
git add crates/celerrate_semantics crates/celerrate_cli
git commit -m "✨ feat(cache): the member tree joins the persistent packs"
```

### Task 6: Pack format 5 and the plugin-set header key

**Files:**
- Modify: `crates/celerrate_cli/src/cache/pack.rs`
- Modify: `crates/celerrate_cli/src/plugins.rs` (`plugin_set_digest`)
- Modify: `crates/celerrate_cli/src/session.rs` (header construction)
- Test: inline `pack.rs::tests` + `cache_seeding.rs`

**Interfaces:**
- Consumes: `PluginIdentity` (name, version, configuration) from the
  descriptors the composition root already registers (the bridge's and
  the stdlib provider's); `PackHeader`, `CACHE_SCHEMA_VERSION`.
- Produces: `PackHeader` gains `pub plugins: [u8; 32]`;
  `PackHeader::current(range: PhpVersionRange, plugins: [u8; 32])`;
  `plugins.rs::plugin_set_digest() -> [u8; 32]` — blake3 over the
  postcard encoding of the sorted
  `Vec<(String, String, String)>` of registered identities. Every
  later task builds headers through this pair.

- [ ] **Step 1: Write the failing header tests**

In `pack.rs::tests`, extend `a_header_mismatch_discards_the_whole_pack`
with the new case:

```rust
// ...existing cases...
let mut other_plugins = expected.clone();
other_plugins.plugins[0] ^= 0xFF; // the plugin-set field is load-bearing
assert!(decode::<Vec<u8>>(&bytes, &other_plugins).is_none());
```

And in `plugins.rs::tests`:

```rust
#[test]
fn the_plugin_set_digest_is_order_independent_and_identity_sensitive() {
    // Two identity lists with the same members in different orders
    // digest equal (the digest sorts); changing any name, version, or
    // configuration string digests different.
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli pack plugins`
Expected: FAIL — field and function not defined.

- [ ] **Step 3: Implement**

`CACHE_SCHEMA_VERSION` 4 → 5; append the doc-comment history line:
"5 = typed artifacts: member-tree pack, inferred-signature pack, typed
verdict fields, plugin-set header digest (plan 9a)." Add the field
after `stub_blob`:

```rust
pub struct PackHeader {
    pub schema: u32,
    pub binary: String,
    pub stub_blob: [u8; 32],
    /// blake3 over the sorted registered plugin identities
    /// (name, version, configuration): the plugin-set cache key the
    /// `PluginIdentity` rustdoc promised (plan 4a decision 1).
    pub plugins: [u8; 32],
    pub php_minimum: (u8, u8),
    pub php_maximum: (u8, u8),
}
```

`plugin_set_digest` collects the identities from the same descriptor
list `plugins.rs` registers (bridge first, stdlib provider second —
but the digest sorts, so registration order does not key the cache),
maps each to its `(name, version, configuration)` triple, sorts,
postcard-encodes, blake3-hashes; an encoding failure answers
`[0u8; 32]` with the degenerate case noted (a constant digest still
discards nothing wrongly — it merely never varies, and postcard on
`Vec<(String, String, String)>` cannot fail in practice).
`session.rs` computes it once at startup and passes it to every
`PackHeader::current` call site (load and persist share the value
through `Session`).

- [ ] **Step 4: Run the suites**

Run: `cargo test -p celerrate_cli`
Expected: PASS — note the schema bump makes every pre-existing pack
invalid, which the generation-mixing test
(`packs_from_different_generations_mix_safely`) already proves safe.

- [ ] **Step 5: Full gate and commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cache): pack format 5 with the plugin-set header key"
```
### Task 7: The inferred-signature pack

**Files:**
- Modify: `crates/celerrate_types/src/stored.rs`
  (`StoredSignatureKey`, `StoredInferredSignature`, the record types)
- Modify: `crates/celerrate_cli/src/cache/snapshot.rs`
  (`INFERRED_SIGNATURES_PACK`, the fourth map)
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (collection,
  `PERSIST_TYPED_ARTIFACTS`, the fourth `write_when_changed`)
- Test: `crates/celerrate_cli/tests/cache_seeding.rs`

**Interfaces:**
- Consumes: `member_tree` (body enumeration and key derivation; the
  crate-private `BodyOwner` is deliberately not consumed, so no
  visibility widens), `inferred_body_types` (already computed by the
  analysis — persist
  reads, never re-demands what did not run), `content_hash`,
  `class_surface_digest`, `function_signature_digest`,
  `FileDependencies` (task 3), `StoredType` (task 1).
- Produces (in `celerrate_types::stored`, so task 8's in-query
  validation can read them below the CLI):

  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
  pub enum StoredSignatureKey {
      Function { key: String },
      Method { class_key: String, member_key: String },
  }

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct StoredClassDependency { pub key: String, pub digest: Option<[u8; 32]> }

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct StoredFunctionDependency { pub key: String, pub digest: Option<[u8; 32]> }

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct StoredInferredEdge { pub callee: StoredSignatureKey, pub return_type: StoredType }

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct StoredInferredSignature {
      /// The defining file at persist time: body identity by proxy.
      pub content: ContentHash,
      pub return_type: StoredType,
      pub classes: Vec<StoredClassDependency>,
      pub functions: Vec<StoredFunctionDependency>,
      pub inferred: Vec<StoredInferredEdge>,
  }
  ```

  `pub const INFERRED_SIGNATURES_PACK: &str = "inferred_signatures.bin";`
  and `CacheSnapshot.signatures: HashMap<StoredSignatureKey, StoredInferredSignature>`.
  (`ContentHash` is `[u8; 32]` — `celerrate_types` already sees
  `celerrate_db`, no new edge.)

- [ ] **Step 1: Write the failing persist-shape test**

In `cache_seeding.rs`:

```rust
#[test]
fn persist_writes_an_inferred_signature_entry_per_eligible_body() {
    // Project: a.php has an annotated function (declared return) and
    // an unannotated function; b.php has a class with a method and a
    // trait whose method a class uses; c.php has an anonymous class.
    // After run_check: the signature pack contains entries for BOTH
    // free functions and the class method (an annotated body still
    // has an inferred return — the artifact is unconditional; the
    // *edges into it* are what declared returns cut); NO entry for
    // the trait's own body under the trait key; NO entry for the
    // anonymous-class method. Each entry's content equals the
    // defining file's blake3; the records name the digests and edges
    // the fixtures make inevitable.
}

#[test]
fn the_signature_pack_is_sorted_and_deterministic() {
    // Two identical runs in fresh directories produce byte-identical
    // inferred_signatures.bin.
}

#[test]
fn the_persist_lever_drops_the_typed_artifacts() {
    // With PERSIST_TYPED_ARTIFACTS = false the signature pack is not
    // written and StoredVerdict.typed is None (task 9 asserts the
    // field half; write the pack half now, #[ignore] until the const
    // exists if needed — prefer landing the const in this task).
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli inferred_signature`
Expected: FAIL.

- [ ] **Step 3: Implement collection**

In `cache/mod.rs`, beside `collect_entries`, a
`collect_signature_entries(inputs, outcome) -> Vec<(StoredSignatureKey,
StoredInferredSignature)>` guarded by
`pub(crate) const PERSIST_TYPED_ARTIFACTS: bool = true;` and the
`analysis::isolated` panic guard (skip `outcome.panicked` files, the
existing convention). Per analyzed file: `member_tree` enumerates free
functions and `MemberKind::Method` members, and the persist keys
derive from those entries themselves (`Function` from the free
function's folded key; `Method` from the enclosing class-like's
folded key plus `folded_member_key`), so the crate-private
`BodyOwner` is never consumed and no visibility changes (skip
class-likes without a stable folded key, the anonymous-class
exclusion; skip `DeclarationKind::Trait` class-likes, decision 8's
trait exclusion);
`inferred_body_types` gives return and dependencies; the digest
queries stamp each recorded class and function key; `StoredType::of`
converts the return and the inferred edges. Sort by key; on duplicate
keys (two definitions of one function name across files) keep the
first in sorted-key order — deterministic, and the pathological
duplicate-definition case is already an unknown-symbol diagnostic
upstream. `persist` writes the fourth pack; `snapshot.rs` loads it
(a generic `load_pack` already handles any key type that
deserializes — adjust its signature bound from `ContentHash` keys to
`K: DeserializeOwned + Eq + Hash`).

- [ ] **Step 4: Run the suites**

Run: `cargo test -p celerrate_cli`
Expected: PASS.

- [ ] **Step 5: Full gate and commit**

```bash
git add crates/celerrate_types crates/celerrate_cli
git commit -m "✨ feat(cache): the inferred-signature pack"
```

### Task 8: The typed cache extension point and the seeded query layer

**Files:**
- Create: `crates/celerrate_types/src/cache.rs`
- Modify: `crates/celerrate_types/src/inference.rs` (the
  consult-validate-serve head on both return queries)
- Modify: `crates/celerrate_types/src/lib.rs`
- Modify: `crates/celerrate_cli/src/cache/snapshot.rs`
  (`SnapshotCache` implements `TypedArtifactCache`)
- Modify: `crates/celerrate_cli/src/session.rs` (`TypedCacheInput`
  registration at HIGH durability, beside `ArtifactCacheInput`)
- Modify: `crates/celerrate_cli/src/cache/statistics.rs`
  (`signatures_found` / `signatures_absent`)
- Test: `crates/celerrate_types/tests/invalidation_scope.rs` (unit
  seams) + `crates/celerrate_cli/tests/cache_seeding.rs` (end to end)

**Interfaces:**
- Consumes: `StoredSignatureKey`, `StoredInferredSignature` (task 7),
  the digest queries (task 2), `content_hash`,
  `inferred_function_return` / `inferred_method_return` (plans 5/6).
- Produces: the decision-9 trait, handle, and singleton input,
  verbatim:

  ```rust
  pub trait TypedArtifactCache: Send + Sync {
      fn inferred_signature(&self, key: &StoredSignatureKey)
          -> Option<StoredInferredSignature>;
  }
  #[derive(Clone)]
  pub struct TypedCacheHandle(pub Arc<dyn TypedArtifactCache>);
  #[salsa::input(singleton)]
  pub struct TypedCacheInput {
      #[returns(ref)]
      pub cache: TypedCacheHandle,
  }
  ```

  Plus `pub(crate) fn validated_stored_return<'db>(db, files, stubs,
  configuration, key: &StoredSignatureKey) -> Option<TypeId<'db>>` in
  `inference.rs` — the shared consult-validate head both return
  queries call first, and the primitive task 9's verdict validation
  reuses through the live queries.

- [ ] **Step 1: Write the failing end-to-end serve tests**

In `cache_seeding.rs`:

```rust
#[test]
fn a_warm_run_serves_inferred_returns_without_reinference() {
    // Cold run over caller.php (calls helper() from helper.php,
    // unannotated) — persist. Edit caller.php (a body change that
    // keeps its own diagnostics recomputable). Warm run: rendering
    // byte-identical to a fresh copy, and signatures_found > 0 under
    // CELERRATE_CACHE_STATS (the callee's return served, not
    // re-inferred — the instrument is the observable).
}

#[test]
fn an_edited_callee_with_an_unchanged_return_still_validates_the_caller() {
    // helper.php's body edited but still returning int: warm run of
    // caller.php's verdict serves (task 9 asserts the verdict half;
    // here assert the signature layer: the recomputed helper return
    // equals the record, and caller.php needed no reanalysis marker
    // in the fresh-copy comparison). Early cutoff across the process
    // boundary — the design's flagship property.
}

#[test]
fn a_changed_class_surface_invalidates_the_dependent_signature() {
    // A method added to a class the cached signature consulted:
    // the entry misses (digest flip) and the run recomputes — and
    // renders identically to fresh.
}

#[test]
fn a_cyclic_cluster_recomputes_and_stays_deterministic() {
    // Two mutually recursive functions, cached, warm run: served or
    // recomputed, the rendering equals fresh (the recorded stance:
    // cycles fall through to the fixpoint).
}

#[test]
fn a_never_returning_cycle_participant_never_validates_warm() {
    // A cached mutually recursive cluster containing a participant
    // whose fixpoint is `never` (it always throws). Warm run:
    // rendering byte-equal to fresh, across entry points and thread
    // counts (decision 9's provisional-mismatch rule keeps a
    // mid-cycle iterate from validating a record and keeps the join
    // ascent free of stale served returns).
}

#[test]
fn a_static_returning_callee_serves_warm() {
    // helper.php's unannotated method returns `static` (a
    // placeholder-carrying inferred return). Edit the caller's file;
    // warm run: the callee's signature entry validates and serves
    // (signatures_found > 0), proving the task-3 records carry the
    // raw pre-substitution return the live demand answers.
}
```

In `invalidation_scope.rs`, the unit seam (a hand-built
`TypedArtifactCache` test double planting a probe record whose return
deliberately differs from what computation would answer — the
`cache_seeding` probe convention transposed):

```rust
#[test]
fn a_valid_record_is_served_and_a_stale_content_hash_is_not() { ... }

#[test]
fn a_stale_inferred_edge_falls_through_to_computation() { ... }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_types cache && cargo test -p celerrate_cli warm`
Expected: FAIL.

- [ ] **Step 3: Implement the consult-validate-serve head**

`celerrate_types/src/cache.rs` exactly as the interface block. In
`inference.rs`:

```rust
/// The recursive memoized revalidation of one persisted signature
/// (plan 9a decision 9). Every read is a salsa read, so a served
/// return carries real dependencies; salsa memoizes the enclosing
/// return query per key, so each signature validates once per run.
pub(crate) fn validated_stored_return<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    key: &StoredSignatureKey,
) -> Option<TypeId<'db>> {
    let handle = TypedCacheInput::try_get(db)?;
    let record = handle.cache(db).0.inferred_signature(key)?;
    let file = defining_file(db, files, key)?; // the computing path's own resolution, factored
    if content_hash(db, file) != record.content {
        return None;
    }
    for class in &record.classes {
        let current = class_surface_digest(
            db, files, stubs, configuration,
            ClassQuery::new(db, class.key.clone()),
        );
        if current != class.digest {
            return None;
        }
    }
    for function in &record.functions {
        let current = function_signature_digest(
            db, files, stubs, configuration,
            FunctionQuery::new(db, function.key.clone()),
        );
        if current != function.digest {
            return None;
        }
    }
    for edge in &record.inferred {
        let live = match &edge.callee {
            StoredSignatureKey::Function { key } => inferred_function_return(
                db, files, stubs, configuration, FunctionQuery::new(db, key.clone()),
            ),
            StoredSignatureKey::Method { class_key, member_key } => inferred_method_return(
                db, files, stubs, configuration,
                MethodQuery::new(db, class_key.clone(), member_key.clone()),
            ),
        };
        if live.is_never(db) {
            // The cycle-provisional value: a mismatch by rule
            // (decision 9), even when the record expects `never`.
            // A mid-cycle iterate can never validate a record, and
            // the join ascent never absorbs a stale served return.
            return None;
        }
        if Some(live) != edge.return_type.to_type_id(db) {
            return None;
        }
    }
    record.return_type.to_type_id(db)
}
```

Both return queries call it first (building their own
`StoredSignatureKey`) and fall through to today's body on `None`.
`defining_file` factors the key→file resolution the computing paths
already perform (Function-space lookup for functions; the member
lookup's owning file for methods) — one resolution, shared, its salsa
dependency recorded on both paths. A live `never` answer is a
mismatch by rule (decision 9): the demand on a callee inside a true
cycle receives the provisional iterate and falls through, and a
legitimately never-returning callee recomputes; no new cycle
machinery.
`SnapshotCache` implements the trait over the fourth map, counting
`signatures_found`/`signatures_absent` on presence; `session.rs`
registers `TypedCacheInput` beside `ArtifactCacheInput` at HIGH
durability.

- [ ] **Step 4: Run the suites, then the byte-identity harness**

Run: `cargo test --workspace`
Expected: PASS, including the fixpoint determinism fixtures across
thread counts — the serve path must not perturb them.

- [ ] **Step 5: Full gate and commit**

```bash
git add crates/celerrate_types crates/celerrate_cli
git commit -m "✨ feat(cache): inferred returns served through recursive memoized revalidation"
```

### Task 9: Typed verdicts persist and serve

**Files:**
- Modify: `crates/celerrate_cli/src/cache/stored.rs`
  (`StoredVerdict.typed`, `StoredTypedVerdict`)
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (`composed_verdict`
  gains the typed half)
- Modify: `crates/celerrate_cli/src/cache/verdict.rs` (the layered
  validation)
- Modify: `crates/celerrate_cli/src/analysis.rs` (the partial-hit
  fork in `analyze_one`)
- Modify: `crates/celerrate_cli/src/cache/statistics.rs`
  (`typed_served` / `typed_recomputed`, the render clause)
- Test: `crates/celerrate_cli/tests/cache_seeding.rs`

**Interfaces:**
- Consumes: `typed_diagnostics`, `typed_file_verdicts`,
  `TypedFileResult.dependencies` (task 3), `persistable_diagnostics`
  / `typed_portion` (plan 8), the digest queries, the live return
  queries (task 8's validation reuse), `StoredDiagnostic`.
- Produces:

  ```rust
  pub struct StoredVerdict {
      pub diagnostics: Vec<StoredDiagnostic>,   // untyped, unchanged
      pub records: Vec<StoredRecord>,           // unchanged
      pub typed: Option<StoredTypedVerdict>,    // None = lever pulled or ineligible
  }

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct StoredTypedVerdict {
      pub diagnostics: Vec<StoredDiagnostic>,   // post-suppression, schema-4 convention
      pub classes: Vec<StoredClassDependency>,
      pub functions: Vec<StoredFunctionDependency>,
      pub inferred: Vec<StoredInferredEdge>,
  }
  ```

  `VerdictLookup` gains the partial outcome:
  `Hit { verdict: &StoredVerdict, typed: TypedOutcome }` with
  `enum TypedOutcome { Served, Recompute }` (shape may fold into the
  existing enum — keep `Discarded`/`Absent` untouched).

- [ ] **Step 1: Write the failing serve tests**

In `cache_seeding.rs`:

```rust
#[test]
fn a_warm_run_serves_typed_verdicts_without_inference() {
    // caller.php dereferences a possibly-null return from helper.php
    // (CEL0034 fires). Cold run, persist. Warm run with NO edits:
    // rendering byte-identical, typed_served > 0, and — the substance
    // assertion — typed_bodies == 0 in the statistics line (no
    // inference ran for served files).
}

#[test]
fn a_stale_typed_record_recomputes_only_the_typed_portion() {
    // Edit helper.php so its inferred return changes (int → string):
    // caller.php's typed portion recomputes (typed_recomputed > 0)
    // while its untyped portion still serves; rendering equals fresh.
}

#[test]
fn the_lever_persists_untyped_only_verdicts() {
    // Under PERSIST_TYPED_ARTIFACTS = false: StoredVerdict.typed is
    // None on disk; a warm run re-infers (typed_bodies > 0) and
    // renders identically. (Compile-time const: test via a #[cfg] or
    // by asserting the shape both ways around a temporary flip —
    // implement as a unit test of composed_verdict's two branches
    // with the const parameterized into a fn for testability.)
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli typed`
Expected: FAIL.

- [ ] **Step 3: Implement**

`composed_verdict` becomes: untyped half exactly as today
(`persistable_diagnostics` + `resolution_records` via
`reference_outcomes`' projection); typed half (when the lever is set):
`typed_portion(inputs, file)` diagnostics as `StoredDiagnostic`s plus
the records from `typed_file_verdicts(...).dependencies`, digests
stamped through the task-2 queries, inferred edges carried as
recorded. `lookup_verdict` validates in layers: untyped records first
(any miss → `Discarded`, whole entry, as today); then the typed
portion — present, every class and function digest re-checked, every
inferred edge compared against the **live** return queries (which
serve through task 8's path when their own entries validate —
microseconds warm) → `TypedOutcome::Served`; anything else →
`TypedOutcome::Recompute`. `analyze_one` on a hit composes: served
untyped diagnostics + (served typed diagnostics | fresh
`typed_portion`), sorted — the exact union `composed_diagnostics`
produces, so the equivalence harness keeps one truth. Counters at the
fork. The suppression note: both halves are stored post-suppression
(schema 4's convention), and suppression directives are own-file
facts under the content-hash key — a comment edit changes the hash,
so a stale suppression can never survive into a served verdict; state
this in `stored.rs`'s module doc.

- [ ] **Step 4: Run the suites**

Run: `cargo test -p celerrate_cli`
Expected: PASS.

- [ ] **Step 5: Full gate and commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cache): typed verdicts persist with recursive revalidation records"
```
### Task 10: The harness extension

**Files:**
- Modify: `crates/celerrate_cli/tests/cache_equivalence.rs`
- Modify: `crates/celerrate_cli/tests/cache_consistency.rs`
- Modify: `crates/celerrate_cli/tests/cache_seeding.rs`
- Modify: `crates/celerrate_cli/src/watch.rs` tests (the
  no-provisional pin)

**Interfaces:**
- Consumes: `served_equals_recomputed`, `assert_cached_matches_fresh`,
  `Step`, the pack-writing helpers (`write_item_trees_pack` /
  `write_diagnostics_pack` — add `write_signatures_pack` and
  `write_member_trees_pack` in the same style), everything tasks 5–9
  produced.
- Produces: the extended nets later plans (9b, 9c) trust.

- [ ] **Step 1: The equivalence net over the typed families**

```rust
#[test]
fn typed_answers_replay_equal() {
    // A fixture firing CEL0030 (unknown method cross-file), CEL0034
    // (possibly-null dereference through an inferred return), CEL0035
    // (argument mismatch against an annotated signature). Assert
    // served_equals_recomputed's identifier set contains all three —
    // the served union and the recomputed union are byte-equal per
    // file, the net's existing contract, now over typed content.
}
```

Run — this should PASS immediately if tasks 8–9 are sound; a failure
here is a bug in them, not in the test. Treat a failure as a stop
signal, not a test to adjust.

- [ ] **Step 2: The consistency harness over the new edit classes**

New tests in `cache_consistency.rs`, all through
`assert_cached_matches_fresh`:

```rust
#[test]
fn an_inferred_return_change_replays_consistently() {
    // Step::Write flips helper()'s body from `return 1;` to
    // `return null;` — the caller's CEL0034 appears on the warm run
    // exactly as on fresh.
}

#[test]
fn a_signature_edit_replays_consistently() { /* parameter type edit
    in another file flips CEL0035 on the caller */ }

#[test]
fn a_docblock_annotation_edit_replays_consistently() { /* an
    `@return` appears on the callee; the caller's typed verdicts
    follow */ }

#[test]
fn a_class_member_addition_replays_consistently() { /* adding the
    missing method clears CEL0030 warm — the class-surface digest
    path end to end */ }

#[test]
fn a_virtual_member_type_edit_replays_consistently() { /* the class
    docblock's `@method User find()` becomes `@method Order find()`
    in another file: the dependent's typed verdicts follow on the
    warm run exactly as on fresh (the digest's virtual-member
    payload, decision 3, end to end) */ }
```

- [ ] **Step 3: The adversarial suite over the new artifacts**

In `cache_seeding.rs`, hand-written v5 packs through the new helpers:

```rust
#[test]
fn an_absurd_stored_type_never_panics() { /* a signature entry whose
    return nests past STORED_DEPTH_LIMIT, forged at the byte level
    (a repeated variant prefix built iteratively, so the test itself
    never recurses), and one with an empty union: the depth-counting
    Deserialize rejects the former as a silent miss (decision 2's
    guard, exercised for real), the run completes, rendering equals
    a no-cache run. */ }

#[test]
fn a_stale_class_digest_discards_the_typed_portion_only() { /* plant
    a verdict whose typed records carry a wrong digest and whose
    untyped records are honest: untyped serves (verdicts_served),
    typed recomputes (typed_recomputed), rendering equals fresh. */ }

#[test]
fn a_signature_entry_with_a_wrong_content_hash_is_ignored() { ... }

#[test]
fn duplicate_signature_keys_never_panic() { ... }

#[test]
fn a_plugin_digest_mismatch_ignores_every_pack() { /* write packs
    under a header whose plugins field differs: cold-run statistics
    (all misses/absent), rendering unchanged. */ }

#[test]
fn the_packs_carry_no_expression_type_tables() { /* decision 1's pin:
    decode inferred_signatures.bin and assert the entry type is
    StoredInferredSignature — a compile-time truth made an explicit
    regression witness by asserting the pack's decoded size stays
    proportional to signature count, not expression count: a fixture
    with one 100-expression body and one 3-expression body produces
    entries whose encodings differ by less than the expression-count
    ratio would predict. Keep the assertion coarse (a factor bound),
    not a byte snapshot. */ }
```

- [ ] **Step 4: The no-provisional pin**

In `watch.rs` tests, beside `a_cycle_rewrites_the_packs_with_its_results`:

```rust
#[test]
fn a_cancelled_cycle_persists_nothing() {
    // Drive one completed cycle (packs exist), then an edit burst that
    // lands mid-analysis so the cycle restarts (the existing
    // cancellation path). Assert the pack mtimes/bytes are unchanged
    // between the cancellation and the next COMPLETED cycle's persist:
    // persist sits after absorb_outcome, and this test makes that
    // ordering a contract (design section 6: no provisional value
    // served or persisted).
}
```

- [ ] **Step 5: Run everything, full gate, commit**

Run: `cargo test --workspace -- --test-threads=1` once (the
consistency suites are I/O heavy), then the normal gate.

```bash
git add crates/celerrate_cli
git commit -m "✅ test(cache): the equivalence, consistency, and adversarial nets over typed artifacts"
```

### Task 11: Watch persist economics

**Files:**
- Modify: `crates/celerrate_cli/src/cache/statistics.rs`
  (`persist_milliseconds`)
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (the timing wrap)
- Modify: `crates/celerrate_cli/src/watch.rs` (only if the
  measurement demands the alternative)
- Test: inline statistics test; the measurement itself is a recorded
  procedure, not a CI test

**Interfaces:**
- Consumes: `persist`, `completed_cycle`, `wait_for_a_burst`,
  `CacheStatistics::render`.
- Produces: the decision-15 outcome, recorded in the closing memo
  (the `## Closing memo` section task 12 appends to this plan file).

- [ ] **Step 1: The instrument**

Failing test in `statistics.rs::tests`: `render`'s existing persist
clause carries the `{}ms` figure when `persist_milliseconds > 0`
(decision 13: folded into that clause, never a separate one).
Implement:
`persist` wraps its body in an `std::time::Instant` pair
(orchestration layer — wall clock is legal here and only here) and
adds the elapsed milliseconds. Run, PASS, commit:

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cache): persist duration joins the statistics line"
```

- [ ] **Step 2: The measurement (recorded procedure)**

On the corpus checkout (`xtask/corpus.pin`, symfony/demo):

```bash
CELERRATE_CACHE_STATS=1 cargo run --release -- check <corpus>   # cold, persist once
CELERRATE_CACHE_STATS=1 cargo run --release -- check <corpus> --watch   # scripted: 10 single-line body edits, one per cycle
```

Record per-cycle `persist ms` against the median warm cycle time.
Acceptance (decision 15): persist ≤ 10% of the median warm cycle.

- [ ] **Step 3: Keep or fall back — either way, record it**

If the number passes: write it into the closing memo (task 12) and
change nothing — per-cycle persist keeps audit finding I6's
crash-window property.

If it fails: implement the recorded alternative in `completed_cycle` —
skip `cache::persist` when the watcher already holds a pending burst
(a peek on the burst channel before persisting), and add a final
persist on the loop's exit path (`watch`'s `Outcome` return), with a
test (`a_skipped_persist_lands_on_the_next_quiet_cycle`) and the
crash-window regression stated in the memo. Commit whichever branch
ran:

```bash
git commit -m "⚡️ perf(watch): persist on quiet cycles only"   # only if the fallback landed
```

### Task 12: Closure — the ledger, the pins, the memo

**Files:**
- Modify: rustdoc seams named below
- Modify: `xtask/corpus-snapshot.txt` (re-bless only if the cache
  work surfaced corpus changes — it must not; a diff here is a bug)
- Modify: this plan file
  (`.claude/superpowers/plans/2026-07-16-type-engine-9a-cache.md`) —
  the closing memo appended under a `## Closing memo` heading, the
  precedent plans 8 and 9b set (plan 8 appends its `## Corpus triage
  memo` to its plan file, plan 9b its `## Measurement memo` and
  `## Closing memo` to its own)

**Interfaces:** produces the `## Closing memo` section plan 9c's
prerequisites and task 1 read — this task writes down what the code
now owes.

- [ ] **Step 1: Verify the debts this plan set out to close are closed**

- `crates/celerrate_semantics/src/queries.rs` — the `member_tree`
  no-consultation-yet rustdoc sentence is gone (task 5).
- `crates/celerrate_semantics/src/body.rs` — the `body_ir` rustdoc
  records the no-consultation decision (task 5).
- `crates/celerrate_semantics/src/plugin.rs` — the `PluginIdentity`
  rustdoc's "plan 9a" promise now points at
  `plugins.rs::plugin_set_digest` as a fulfilled fact (update the
  sentence tense).
- `crates/celerrate_types/src/representation.rs` — the `TypeId`
  rustdoc's "(plan 9a serializes structurally)" becomes a pointer to
  `stored.rs`.
- `crates/celerrate_types/src/inference.rs` — the
  `InterproceduralEdgeCounts` rustdoc's plan-8/9a note updates: the
  orchestration consumer exists (plan 8), and the serve-side counters
  exist (this plan).
- The second mirror: `revalidation.rs`'s module doc names the
  constructive closure (task 4).

- [ ] **Step 2: Write the new ledger (rustdoc at the seams, one line each)**

- `checks/mod.rs` / `records.rs`: trait bodies and anonymous-class
  methods persist no inferred-signature entries; both recompute warm
  (decision 8's recorded exclusions).
- `inference.rs::validated_stored_return`: a live `never` answer
  never validates a record, the provisional-mismatch rule, so cyclic
  clusters always recompute (decision 9's recorded stance).
- `stored.rs::StoredInferredSignature`: keyed by the defining file's
  content hash, a recorded coarsening of the design's per-body
  keying; plan 9b's numbers own the revisit (the self-review note
  made a rustdoc fact).
- `dynamic_type_provider.rs`: the purity constraint is load-bearing
  for warm revalidation (decision 5 — landed in task 3, verify the
  sentence names this plan).
- `cache/mod.rs::PERSIST_TYPED_ARTIFACTS`: the fallback lever and
  what pulling it means (the warm number converges toward
  cold-with-inference; the release decision escalates — design
  section 9, plan 9c's call).
- `statistics.rs`: the LRU capacity decision remains plan 9b's (the
  peak-memory measurement owns it); nothing in this plan set `lru`.
- `watch.rs`: the task-11 outcome, whichever branch ran.

- [ ] **Step 3: Append the closing memo to this plan file**

Append a `## Closing memo` section to
`.claude/superpowers/plans/2026-07-16-type-engine-9a-cache.md`
carrying at least: the `PERSIST_TYPED_ARTIFACTS` stance (decision 11
— still `true`, or flipped and why), the watch-persist outcome (task
11: which branch ran), and the measured 10%-rule number (per-cycle
persist against the median warm cycle). The commit message and the
rustdoc anchors of steps 1–2 may restate the highlights, but the
plan-file section is the deliverable plan 9c reads.

- [ ] **Step 4: The final full gate**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all && cargo deny check && cargo xtask corpus && cargo xtask dependency-shape`
Expected: all green; `corpus-snapshot.txt` byte-identical (the cache
never changes what is reported, only how fast).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "📝 docs(cache): the typed-artifact closure ledger"
```

## Self-review notes (performed at authoring time)

- **Spec coverage.** Design section 9's typed-cache paragraphs map:
  two artifact classes → decisions 1, 8 and tasks 7–8; structural
  serialization → decision 2, task 1; recursive memoized revalidation
  → decision 9, task 8; signature-hash records → decision 3, task 2;
  the fallback lever → decision 11, tasks 7/9; watch economics →
  decision 15, task 11; equivalence/adversarial extension → decision
  14, task 10; the second mirror → decision 12, task 4; the
  plugin-set key → decision 7, task 6; schema bumps → decision 7.
  Section 11 item 12's clause list is covered term by term. The
  residual instrumentation (design section 9, "typed counters") is
  decision 13. Peak memory and the benchmark protocol are plan 9b's
  and deliberately absent here.
- **Known simplification, stated**: the design keys per-body
  signatures "by body content"; this plan keys by the defining
  *file's* content hash — coarser (any edit in a file invalidates its
  bodies' persisted signatures) but sound, consistent with every
  existing pack key, and cheap; the early-cutoff loss is bounded
  because a recomputed body whose return matches its callers' records
  still validates them (task 8's cross-boundary cutoff). If plan 9b's
  numbers show the coarseness matters, a per-body hash is a pack-only
  change behind the same record shape.
- **Type-name consistency**: `StoredSignatureKey` /
  `StoredInferredSignature` / `StoredClassDependency` /
  `StoredFunctionDependency` / `StoredInferredEdge` are defined once
  (task 7's interface block, in `celerrate_types::stored`) and used
  with those exact names in tasks 8–10; `TypedDependencies` /
  `FileDependencies` defined in task 3, consumed in 7 and 9;
  `validated_stored_return` defined in task 8, named in 9.
- **Plans 7 and 8 are prerequisites**: this plan consumes
  `typed_file_verdicts`, `typed_portion`, `persistable_diagnostics`,
  `CheckContext`, and the stdlib provider's registration — do not
  start it before plans 7 and 8 have merged.




