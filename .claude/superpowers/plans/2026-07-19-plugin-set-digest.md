# Plugin-Set Digest Integrity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The plugin-set cache key derives from the post-admission registration record, with no duplicate descriptor list and no fallible encoding arm (issue #60), per `.claude/superpowers/specs/2026-07-19-plugin-set-digest-design.md`.

**Architecture:** On branch `fix-60-plugin-set-digest`: `RegisteredPlugins` grows an `admitted: Vec<PluginIdentity>` record; `plugin_set_digest` becomes a function of `&RegisteredPlugins` hashing admitted identities and excluded names directly into `blake3::Hasher` (length-prefixed, no postcard step); `Session::start` registers first, digests the result. The pack-header gate (`PackHeader.plugins`, wholesale discard on mismatch) is untouched.

**Tech Stack:** Rust 1.94, blake3 1.x (already a dependency), salsa 0.27 (untouched).

## Global Constraints

- Zero panic lints at deny; `unsafe_code` forbidden; test modules may locally `#[allow]`.
- TDD: failing test before implementation.
- Determinism: the digest is a pure function of the registration record; sorting before hashing keeps registration order out of the key.
- Commits: gitmoji + Conventional Commits.
- The digest **value** changes with this fix (post-admission content); that discards local caches once, which is correct and expected. The corpus snapshot does not embed the digest: corpus gates must show zero delta.

---

### Task 1: `RegisteredPlugins` records the admitted identities

**Files:**
- Modify: `crates/celerrate_cli/src/plugins.rs:14-105` (`RegisteredPlugins`, `register_plugins`)
- Test: same file's `#[cfg(test)]` module (starts line 170)

**Interfaces:**
- Consumes: existing registration arms.
- Produces: `pub struct RegisteredPlugins { pub admitted: Vec<PluginIdentity>, pub excluded: Vec<ExcludedPlugin> }` — `admitted` in registration order, containing exactly the identities whose registrations entered a salsa registry (dynamic providers post-claim-admission). Task 2 digests this.

- [ ] **Step 1: Write the failing tests**

In the test module (mirror its existing database/registration helpers):

```rust
#[test]
fn registration_records_the_admitted_identities_in_order() {
    let database = AnalysisDatabase::default();
    let plugins = register_plugins(&database);
    assert_eq!(
        plugins
            .admitted
            .iter()
            .map(|identity| identity.name.as_str())
            .collect::<Vec<_>>(),
        vec!["phpdoc-bridge", "stdlib-provider"],
    );
    assert!(plugins.excluded.is_empty());
}
```

Use the two descriptors' actual `name` values — read them from
`celerrate_phpdoc_bridge::descriptor().identity.name` and the stdlib
equivalent in the test rather than trusting the strings above. Also
extend the existing claim-conflict test (the one driving
`admit_dynamic_providers` through a rebuilt set) to assert the
conflict-excluded provider's identity is **absent** from `admitted`.
`admit_dynamic_providers` is unit-tested directly today; if no test
drives `register_plugins`-level exclusion, assert at the
`admit_dynamic_providers` + composition level: survivors in, excluded
out.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli registration_records`
Expected: FAIL to compile — no field `admitted`.

- [ ] **Step 3: Implement the record**

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegisteredPlugins {
    /// The identities whose registrations actually entered a salsa
    /// registry, in registration order — dynamic providers counted
    /// after claim admission. This is the effective set the plugin-set
    /// digest keys the cache on (issue #60).
    pub admitted: Vec<PluginIdentity>,
    pub excluded: Vec<ExcludedPlugin>,
}
```

In `register_plugins`: bind each descriptor once
(`let descriptor = celerrate_stdlib_provider::descriptor();` — collapsing
the current triple call at lines 71, 74, 79), push the identity into a
local `admitted` on the `Ok` arms. For the bridge (a non-dynamic
registration), push at admission. For the stdlib provider (dynamic),
push **after** `admit_dynamic_providers` from the surviving
registrations' identities, so a claim-excluded provider never lands in
`admitted`:

```rust
    let (dynamic_providers, rebuild_exclusions) = admit_dynamic_providers(dynamic_providers);
    excluded.extend(rebuild_exclusions);
    admitted.extend(
        dynamic_providers
            .iter()
            .map(|registration| registration.identity.clone()),
    );
```

Return `RegisteredPlugins { admitted, excluded }`.

- [ ] **Step 4: Run the crate suite**

Run: `cargo test -p celerrate_cli plugins`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli/src/plugins.rs
git commit -m "✨ feat(cli): the registration record carries the admitted identities (#60)"
```

---

### Task 2: The digest derives from the record, hashing directly

**Files:**
- Modify: `crates/celerrate_cli/src/plugins.rs:129-168` (`plugin_set_digest`, `digest_identities`)
- Test: same file's test module

**Interfaces:**
- Consumes: Task 1's `RegisteredPlugins`.
- Produces: `pub fn plugin_set_digest(plugins: &RegisteredPlugins) -> [u8; 32]`. The zero-argument function and `digest_identities`'s postcard step are gone.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn an_exclusion_changes_the_digest() {
    let admitted = RegisteredPlugins {
        admitted: vec![identity("bridge", "1.0"), identity("provider", "1.0")],
        excluded: Vec::new(),
    };
    let degraded = RegisteredPlugins {
        admitted: vec![identity("bridge", "1.0")],
        excluded: vec![ExcludedPlugin {
            name: "provider".to_owned(),
            reason: "claim conflict".to_owned(),
        }],
    };
    assert_ne!(plugin_set_digest(&admitted), plugin_set_digest(&degraded));
}

#[test]
fn the_exclusion_reason_wording_does_not_key_the_cache() {
    let one = RegisteredPlugins {
        admitted: Vec::new(),
        excluded: vec![ExcludedPlugin {
            name: "provider".to_owned(),
            reason: "old wording".to_owned(),
        }],
    };
    let other = RegisteredPlugins {
        excluded: vec![ExcludedPlugin {
            name: "provider".to_owned(),
            reason: "new wording".to_owned(),
        }],
        ..one.clone()
    };
    assert_eq!(plugin_set_digest(&one), plugin_set_digest(&other));
}

#[test]
fn adjacent_fields_do_not_collide() {
    // Length prefixes: ("ab","c","") and ("a","bc","") must differ.
    let one = RegisteredPlugins {
        admitted: vec![raw_identity("ab", "c", "")],
        excluded: Vec::new(),
    };
    let other = RegisteredPlugins {
        admitted: vec![raw_identity("a", "bc", "")],
        excluded: Vec::new(),
    };
    assert_ne!(plugin_set_digest(&one), plugin_set_digest(&other));
}
```

With small local helpers `identity(name, version)` /
`raw_identity(name, version, configuration)` building `PluginIdentity`
values. Keep (and port to the new signature) the existing
order-independence and identity-sensitivity tests: order-independence
asserts two records whose `admitted` vectors are permutations digest
equally.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p celerrate_cli digest`
Expected: FAIL to compile — `plugin_set_digest` takes no arguments today.

- [ ] **Step 3: Implement**

Replace both functions (lines 129-168) with:

```rust
/// The plugin-set cache key (plan 4a decision 1, corrected by issue
/// #60): a blake3 digest of the **post-admission** effective set — the
/// admitted identities' `(name, version, configuration)` triples plus
/// the excluded plugin names. Derived from `register_plugins`' output,
/// so there is no second descriptor list to forget; sorted before
/// hashing, so registration order does not key the cache. Fields are
/// length-prefixed and sections count-prefixed straight into the
/// hasher: no serialization step, no failure arm.
pub fn plugin_set_digest(plugins: &RegisteredPlugins) -> [u8; 32] {
    let mut triples: Vec<(&str, &str, &str)> = plugins
        .admitted
        .iter()
        .map(|identity| {
            (
                identity.name.as_str(),
                identity.version.as_str(),
                identity.configuration.as_str(),
            )
        })
        .collect();
    triples.sort_unstable();
    let mut excluded: Vec<&str> = plugins
        .excluded
        .iter()
        .map(|plugin| plugin.name.as_str())
        .collect();
    excluded.sort_unstable();

    let mut hasher = blake3::Hasher::new();
    update_count(&mut hasher, triples.len());
    for (name, version, configuration) in triples {
        update_field(&mut hasher, name);
        update_field(&mut hasher, version);
        update_field(&mut hasher, configuration);
    }
    update_count(&mut hasher, excluded.len());
    for name in excluded {
        update_field(&mut hasher, name);
    }
    *hasher.finalize().as_bytes()
}

fn update_count(hasher: &mut blake3::Hasher, count: usize) {
    hasher.update(&(count as u64).to_le_bytes());
}

fn update_field(hasher: &mut blake3::Hasher, field: &str) {
    hasher.update(&(field.len() as u64).to_le_bytes());
    hasher.update(field.as_bytes());
}
```

Drop the `postcard` import if plugins.rs no longer uses it (check the
file; `postcard` stays a crate dependency for the pack format).

- [ ] **Step 4: Run the crate's unit suite**

Run: `cargo test -p celerrate_cli --lib`
Expected: unit tests PASS; integration tests still broken until Task 3
migrates them (expected at this step).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli/src/plugins.rs
git commit -m "🐛 fix(cli): the plugin-set digest keys the post-admission effective set (#60)"
```

---

### Task 3: `Session::start` registers first, then digests; call sites migrate

**Files:**
- Modify: `crates/celerrate_cli/src/session.rs:130-165` (the start ordering)
- Modify: `crates/celerrate_cli/src/cache/pack.rs:147,206` (test call sites)
- Modify: `crates/celerrate_cli/tests/cache_seeding.rs` (~25 call sites)

**Interfaces:**
- Consumes: Tasks 1-2.
- Produces: `Session` fields unchanged in name and type; construction order becomes registries-then-cache.

- [ ] **Step 1: Reorder `Session::start`**

Move the `register_plugins(&database)` call (currently `session.rs:161`)
up to just before the digest computation (currently `session.rs:136`),
and derive the digest from it:

```rust
        // Registration happens before the cache loads: the registries
        // are salsa singletons set once per database, and the
        // plugin-set digest keys the packs on the post-admission
        // record (issue #60), so it must exist first. Computed once
        // and threaded through: load and persist must key packs on
        // the same digest, never recompute it independently.
        let plugins = register_plugins(&database);
        let plugin_set_digest = plugin_set_digest(&plugins);
        let cache = Arc::new(CacheSnapshot::load(
            &cache_directory,
            &PackHeader::current(cache_loaded_range, plugin_set_digest),
        ));
```

The `ArtifactCacheInput`/`TypedCacheInput` registrations
(`session.rs:141-156`) stay where they are (still before any query
runs); the later `let plugins = register_plugins(&database);` line is
removed and the `Self { .. }` literal takes the moved binding.

- [ ] **Step 2: Migrate the remaining call sites**

`cargo check -p celerrate_cli --all-targets` lists them. Rule per site:
a test that registers plugins digests the record it got; a test
exercising a plugin-free database digests
`&RegisteredPlugins::default()` (the empty effective set). No test may
hand-build a digest another way.

- [ ] **Step 3: Run the full crate suite**

Run: `cargo test -p celerrate_cli`
Expected: PASS, including `cache_seeding.rs` end to end.

- [ ] **Step 4: Commit**

```bash
git add crates/celerrate_cli
git commit -m "♻️ refactor(cli): sessions digest the registration record they made (#60)"
```

---

### Task 4: Verification and PR

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

Expected: zero delta (the digest keys the local cache, not the
analysis; a one-time local cache discard is expected and harmless).

- [ ] **Step 3: Changelog and PR**

Unreleased entry: the plugin-set cache key now describes the
post-admission plugin set and has a single source of truth (#60).

```bash
git add CHANGELOG.md
git commit -m "📝 docs(changelog): record the plugin-set digest integrity fix (#60)"
git push -u origin fix-60-plugin-set-digest
gh pr create --title "🐛 fix(cli): plugin-set digest keys the post-admission effective set (#60)" --body "Implements .claude/superpowers/specs/2026-07-19-plugin-set-digest-design.md: the registration record carries admitted identities, the digest derives from it (length-prefixed direct hashing, no fallible encode arm), and sessions register before they load the cache. Closes #60."
```
