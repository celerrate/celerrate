# Flow Module Extraction — Design (issue #39)

Date: 2026-07-19
Status: Approved (brainstorming output)
Issue: #39 — flow.rs is ~3,300 lines: extract the cohesive clusters
Follows: PR #35 (type engine 6), whose reviews identified the clusters.

## 1. Problem

`crates/celerrate_types/src/flow.rs` has grown to ~3,911 lines (the issue
said ~3,300; two subsequent fixes added more). It carries the entire
inference walker in a single file: the flow `Environment`,
`PendingAssertion`, the `Walker` struct, one ~3,400-line `impl Walker`
block, and four free helper functions. The issue names three cohesive
extraction candidates (the iteration protocol, class-generic delivery,
the member boundary cluster) and one constraint to preserve:
`member_boundary_type` is the single funnel every member read, method
call, callable projection, and `new` result passes through — a property
verified by hand during the PR #35 whole-branch review by auditing all
seven direct `substitution::substitute` call sites outside it.

## 2. Decisions

Three decisions were made during brainstorming:

1. **Full decomposition**, not just the three clusters the issue names:
   `flow.rs` becomes a `flow/` directory with one submodule per cohesive
   cluster. The file drops from ~3,911 to ~800 lines in `mod.rs` and the
   issue is settled durably.
2. **Mechanical moves only.** The statement/expression walk — including
   the ~875-line `expression_value` match — moves whole into
   `flow/walk.rs` without carving the match into per-family methods.
   Every move is a translation verifiable with `git diff --color-moved`;
   this is a pure refactor with no behaviour change, and the diff must
   prove it.
3. **Guard test + rustdoc for the funnel invariant.** The hand-verified
   funnel property becomes mechanically held (same spirit as issue #67's
   negative-proof guard), not a prose promise.

## 3. Constraints

- No behaviour change. No new diagnostics, no changed inference results.
- No public-surface change: `crate::flow` keeps exporting `walk_body`,
  `FlowContext`, and `resolved_function_key` identically. Consumers
  (`inference.rs`, `checks/arguments.rs`) do not change their imports.
- `lib.rs` does not change (`mod flow;` resolves the directory).
- The `Walker` struct stays defined in `flow`; clusters become
  `impl Walker` blocks in submodules of the same crate — the mechanism
  Rust permits natively. This differs from the `narrowing.rs` /
  `operators.rs` precedent, which extracted pure functions; here the
  cohesive units are methods on the walker.

## 4. Target module map

```
flow/mod.rs           Environment, PendingAssertion, Walker, FlowContext,
                      walk_body, context helpers (db, record, recorded,
                      class_type_of_written, subject_type, this_type,
                      current_static_type, receiver_parts,
                      scoped_subject), submodule declarations,
                      re-exports                                  ~800 l.
flow/walk.rs          statements, statement, looped, expression,
                      expression_value — moved whole             ~1,250 l.
flow/boundary.rs      member_value_type, method_signatures,
                      declared_present, member_boundary_type (THE
                      funnel), member_owner, scope_keyword_class,
                      parent_class_key, parent_class_key_of        ~280 l.
flow/calls.rs         typed_arguments, kill_property_bindings,
                      apply_by_reference, solver_pairs,
                      solved_call_result, resolved_function_key,
                      provider_return, provider_by_reference,
                      apply_provider_by_reference,
                      function_call_result,
                      method_call_result_for_keys_with_provider,
                      method_call_result_with_provider,
                      method_call_result_for_keys,
                      apply_call_assertions                        ~700 l.
flow/instantiation.rs constructor_solved_class, inline_variables,
                      bind_inline_variables (the class-generic
                      delivery cluster from the issue)             ~170 l.
flow/iteration.rs     iteration_types, implements_iteration_protocol,
                      class_iteration_types                        ~210 l.
flow/branching.rs     branch_environments, split_on_subject,
                      narrowed_to, removed_type, instanceof_target,
                      type_check_facts                             ~320 l.
flow/callables.rs     nested_returns, closure_type,
                      seed_written_parameters, projected_callable,
                      projected_callable_of_function,
                      projected_callable_of_method,
                      projected_callable_of_keys                   ~260 l.
flow/assignment.rs    string_parts, array_literal, assignment,
                      assign_target, and the four free helpers
                      (widen_if_literal, compound_base,
                      updated_array, shape_join)                   ~350 l.
```

The three clusters the issue names map to `iteration.rs`,
`instantiation.rs`, and `boundary.rs`. The issue noted the boundary
cluster "sits naturally beside `substitution.rs`" (top level); it stays
a `flow/` submodule instead, because it is a cluster of `Walker`
methods, not a module of pure functions — its place is under `flow/`.

The four free helpers move to `assignment.rs` with their callers;
`widen_if_literal`, also called from `walk.rs` (`expression_value`'s
`??` coalesce arm), becomes `pub(super)`.

`resolved_function_key` exists in two forms: the `pub(crate)` free
function (the one `checks/arguments.rs` imports as
`crate::flow::resolved_function_key`) and a private method wrapper on
`Walker`. Both move to `calls.rs`; `mod.rs` re-exports the free
function (`pub(crate) use calls::resolved_function_key;`) so the
consumer's import path does not change.

Line counts are approximate (imports and rustdoc shift them); the
partition boundaries are the design, not the counts.

## 5. Visibility mechanics

Methods called across submodules go from private to `pub(super)` —
visible within `flow/` only, invisible to the rest of the crate. Nothing
becomes `pub(crate)` that was not already. Each submodule imports what
it needs; `mod.rs` declares the submodules and keeps the existing
re-exports.

## 6. The funnel guard test

A test in `celerrate_types` (a `tests/` integration test or a dedicated
`#[cfg(test)]` module) that:

1. reads the crate's sources under `CARGO_MANIFEST_DIR`,
2. counts every textual occurrence of `substitution::substitute` per
   file,
3. compares against an explicit allowlist: the definition site, the
   funnel (`flow/boundary.rs`, `member_boundary_type`), and the seven
   legitimate call sites outside the funnel that the PR #35 review
   audited (the exact list is pinned when the test is written, from the
   code as it stands),
4. fails with a message stating the invariant and pointing at
   `flow/boundary.rs`'s rustdoc when an unlisted site appears.

The rustdoc of `flow/boundary.rs` states the property: every member
read, method call, callable projection, and `new` result passes through
`member_boundary_type`; substitution outside it must be justified and
added to the guard's allowlist deliberately.

## 7. Verification

- `flow.rs` has no inline tests; all coverage lives in the test crates
  and snapshots. No test moves.
- Proof of no regression: `cargo test --workspace`, clippy
  `--all-targets -- -D warnings`, `cargo fmt --all`, `cargo deny check`,
  and a `git diff --color-moved` reading confirming each block is a
  translation.
- One commit per extracted cluster, on a worktree branch, PR to `main`
  referencing #39.

## 8. Out of scope

- Splitting `expression_value`'s match into per-family methods.
- `inference.rs` (~4,500 lines) — a separate issue if wanted.
- Any behaviour, diagnostic, or public-API change.
- Relocating `substitution.rs` or reworking its visibility (the
  "module visibility" enforcement option was considered and rejected:
  the seven legitimate out-of-funnel call sites live in other modules
  of the crate, so restricting visibility would force a remodel beyond
  this issue's scope).
