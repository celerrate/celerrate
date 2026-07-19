# Plugin-Seal Negative Proof and Derived Plugin-Crate Set — Design

Date: 2026-07-19
Status: Approved (issue #67)

## Problem

The plugin-boundary seal of #61 (PR #65) rests on two mechanisms with no
guard against regression:

1. **Visibility with no compile-fail proof.** The seal is `pub(crate)`
   on `AnnotationSite::new`, `AnnotationSite::database()`,
   `TypeContext::new`, and `InvocationSite::new`, plus the absence of
   any `salsa` re-export from `celerrate_plugin`. Nothing pins any of
   it: a refactor flipping a `pub(crate)` back to `pub`, or a new
   re-export of `salsa` from the facade, would compile, pass every
   test, and silently reopen the boundary.
2. **A hardcoded plugin-crate list.** `PLUGIN_CRATES` in
   `xtask/src/dependency_shape.rs:8` names the two known plugin crates.
   A future plugin crate added to the workspace but not to the list
   silently escapes both dependency rules. The existing "not found"
   guard protects against renames of the known two, not against
   omissions of new ones.

## Design

### 1. A `trybuild` compile-fail suite in `celerrate_plugin`

`trybuild` (MIT OR Apache-2.0, passes the `deny.toml` license
allow-list) becomes `celerrate_plugin`'s first dev-dependency, with a
`tests/seal.rs` harness over one file per case. Every case takes the
facade-only consumer's point of view — `use celerrate_plugin::...`
paths only — and pins its compiler error in a committed `.stderr` file
(stable because the toolchain is pinned by `rust-toolchain.toml`):

- `site.database()` on an `AnnotationSite` does not resolve (private
  method).
- `TypeContext::new(...)` is not nameable.
- `InvocationSite::new(...)` is not nameable.
- `AnnotationSite::new(...)` is not nameable.
- `use celerrate_plugin::salsa;` does not exist (no re-export).

The proof property: any future edit that reopens the boundary makes the
corresponding case compile, which fails the suite. The suite is the
negative complement of the positive API tests — together they pin both
"the facade suffices" and "the facade is all there is".

The compile-fail cases obtain their `AnnotationSite`/`InvocationSite`
values as unreachable-by-construction typed bindings (for example
behind `fn take(site: AnnotationSite)` signatures) so each case fails
on exactly the sealed item, not on value construction.

### 2. The plugin-crate set is derived, not listed

`dependency_shape` stops hardcoding `PLUGIN_CRATES` and derives the
governed set from the workspace metadata it already parses
(`cargo metadata`): **a plugin crate is any workspace member with a
non-dev dependency on `celerrate_plugin`, excluding the composition
roots.** The composition-root allowlist (`celerrate_cli` today, the LSP
binary later) is the one remaining literal, and it fails loud: a crate
that depends on the facade and is not allowlisted is checked as a
plugin, so a new plugin crate is governed by construction the moment it
depends on the facade — which is the one thing a plugin cannot avoid
doing.

Two sanity guards replace the old "not found" guard:

- The derived set must still contain the two known first-party crates
  (`celerrate_phpdoc_bridge`, `celerrate_stdlib_provider`): protects
  against renames and against derivation bugs alike.
- The derived set must be non-empty.

The existing rules are unchanged: no workspace dependency beyond
`celerrate_plugin`, no direct `salsa`, dev-dependencies exempt. The
synthetic-metadata unit tests extend to cover derivation: a new member
depending on the facade enters the governed set with no xtask edit; a
member with no facade dependency stays out; the allowlist excludes the
composition root.

## Testing

- The trybuild suite is itself the test (runs under
  `cargo test --workspace` in CI on the pinned toolchain).
- `dependency_shape` unit tests extend as above; `cargo xtask
  dependency-shape` in CI is unchanged.
- No production code changes, so corpus gates trivially hold; run them
  anyway per plan-verification policy.

## Out of scope

- Widening or narrowing the sealed surface itself: this proves the
  #61/#65 seal as it stands.
- The digest defects of the plugin registry (issue #60, its own spec).
- WASM-boundary enforcement: the WASM host does not exist yet.
