# Plugin-Set Digest Integrity — Design

Date: 2026-07-19
Status: Approved (issue #60)

## Problem

The plugin-set cache key (`plugin_set_digest`,
`crates/celerrate_cli/src/plugins.rs:129`) has three defects, all of the
silent-wrong-cache-hit class:

1. **Two sources of truth.** It hashes a hardcoded literal list of the
   two descriptor identities, duplicated from — not shared with —
   `register_plugins` (`plugins.rs:38`). Its rustdoc claims it "collects
   from the same descriptor list `register_plugins` above registers",
   which is not structurally true. Adding a plugin to one site and
   forgetting the other leaves the cache key unchanged across a real
   plugin-set change.
2. **Pre-admission digest.** The function takes no arguments and runs
   before (and independently of) admission: a plugin excluded by the
   API-version gate (`plugins.rs:25`) or by a dynamic-provider claim
   conflict (`plugins.rs:109`) still counts in the digest. The key
   describes a plugin set that is not the one actually active; two runs
   whose effective sets differ can collide on one key. The pack gate is
   wholesale (`PackHeader` equality, `cache/pack.rs:111`), so a
   collision serves every cached verdict, not one.
3. **Constant on encode failure.** `digest_identities`
   (`plugins.rs:152`) maps a postcard-encode failure to `[0u8; 32]`; if
   it ever fired, all plugin sets would collide on one digest.

Harmless while the plugin set is two first-party crates compiled into
the binary (the `binary` field of `PackHeader` already covers them), but
the failure mode arms itself the moment plugins become configurable or
runtime-excludable (framework providers, sub-project 6). Cache keys must
be structurally derived, not maintained by convention.

## Design

### 1. One descriptor list

A single private function, `plugin_descriptors() -> Vec<PluginDescriptor>`
(or equivalent), becomes the only place the first-party descriptor set
is written down. `register_plugins` iterates it; nothing else calls the
per-crate `descriptor()` functions directly (today `register_plugins`
calls `celerrate_stdlib_provider::descriptor()` three separate times —
that collapses too). Adding a plugin is one edit, and the digest follows
by construction because of the next point.

### 2. Digest the post-admission effective set

`RegisteredPlugins` grows a record of the admitted identities alongside
the exclusions it already carries:

- `admitted: Vec<PluginIdentity>` — the identities whose registrations
  actually entered the salsa registries, in registration order.
- `excluded: Vec<ExcludedPlugin>` — unchanged (name + reason, already
  rendered as the degraded-run report at `lib.rs:230`).

`plugin_set_digest` becomes a function of `&RegisteredPlugins`: it
digests the admitted identities plus the excluded plugin **names** (not
the reason prose — wording is free to change; the `binary` header field
already keys the code that produces it). Sorting before encoding keeps
the existing order-independence property. Two runs whose effective sets
differ — one admits what the other excluded — now produce different
keys by construction.

Sequencing in `Session` construction moves accordingly: register first,
digest the result (today the digest is computed at `session.rs:136`
before registration at `session.rs:161`; the two lines reorder so the
digest consumes registration's output). The ~25 `plugin_set_digest()`
call sites in `cache_seeding.rs` update mechanically to build their
digest from a `RegisteredPlugins` value.

### 3. Encode failure is an internal error

`digest_identities` returns `Result<[u8; 32], ...>`; the postcard-error
arm propagates through `Session` construction as an internal error (the
CLI's existing internal-error surface, exit 2) instead of a constant
key. A cache key that cannot be computed is a bug to surface, never a
value to guess. Zero-panic rule respected: `Result`, not `panic`.

## Testing

- Single-source test: the descriptor list `register_plugins` registers
  and the list the digest consumes are the same value (structural,
  not two lists compared by hand).
- Post-admission tests: a simulated exclusion (API-version mismatch, and
  separately a claim conflict) changes the digest relative to the
  all-admitted run.
- Order-independence and identity-sensitivity tests carry over.
- The encode-failure arm is exercised or, if unreachable by
  construction with the concrete types, documented as such where the
  `Result` is introduced.
- Cache-seeding integration suite passes with the new signature.
- Corpus gates: zero delta (`cargo xtask corpus`,
  `cargo xtask mixed-rate`). The digest value itself may change
  (admitted set now digested post-admission); the corpus snapshot does
  not read the digest, but the local cache invalidates once — expected
  and harmless.

## Out of scope

- Configurable or runtime-excludable plugins (sub-project 6): this fix
  makes the key correct for when they arrive, it does not add them.
- Any change to admission semantics: what is admitted and excluded
  stays exactly as it is.
