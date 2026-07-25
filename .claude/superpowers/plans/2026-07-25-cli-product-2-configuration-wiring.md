# CLI Product Part 2: Configuration Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the parsed `celerrate.toml` actually govern the engine: include/exclude and the `php` override drive discovery and the walk, the active rule set and the severity remap are computed at the composition root, force-activation exists with its first consumer (the explain-page harness), the configuration digest joins the persistent cache header, and `--watch` reloads and reports the configuration.

**Architecture:** Configuration enters the engine as data consumed by each layer (spec `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`, section 2). `celerrate_project` gains a dependency on `celerrate_config` and derives walk roots, excluded roots, and the version range from the configuration. The composition root (`celerrate_cli`) loads the file first, threads the model into discovery, computes the active set and the severity remap, and keys the persistent cache on a digest of the normalized `[rules]` and `[severity]` sections. Nothing configuration-shaped ever enters a salsa query: the remap applies in the per-file composition where suppression filtering lives, and the digest lives in the pack header.

**Tech Stack:** Rust workspace, salsa inputs at the composition root, blake3 for the digest, postcard packs, the existing in-process integration-test harness (`celerrate_cli::run`).

## Global Constraints

- Zero panic, mechanically enforced: clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic` workspace-wide; `unsafe_code` forbidden. Test modules may locally `#[allow]` these lints (every existing test module shows the idiom).
- TDD: failing test, minimal implementation, refactor. No production code without a test that demanded it.
- Strict layering: `celerrate_config` sits above `celerrate_diagnostics` and below `celerrate_project`. A crate depends only on crates below it.
- Determinism: no wall-clock, randomness, or environment reads inside queries. Configuration is read at startup (and on `--watch` reload), never inside a query.
- Error resilience: no user input may ever crash the tool.
- Everything written in English, full words, no abbreviated names (standard acronyms fine).
- Commits: gitmoji + Conventional Commits (e.g. `✨ feat(project): ...`). No Claude attribution anywhere. Use the repository-configured git identity; never override it.
- Zero-config parity is a closure gate: without a `celerrate.toml`, behavior stays byte-identical (the corpus snapshot must not move; Task 8 proves it).
- The mixed-rate baseline cannot move (no type work happens here).

## Context for the implementer

Read these before starting; every task below assumes them:

- Spec: `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md` sections 2 and 3 (this plan implements section 10, item 2).
- `crates/celerrate_config/src/model.rs`: the parsed `Configuration` (`php: Option<Spanned<(u8, u8)>>`, `include`/`exclude: Vec<Spanned<String>>`, `rules: Vec<RuleEntry>` with `enabled: Option<Spanned<bool>>`, `severity: Vec<SeverityEntry>` with `severity: Option<Spanned<Severity>>`). `Configuration` derives `Clone` and `Default`; an empty file parses to the default.
- `crates/celerrate_cli/src/configuration.rs`: `load(root, &mut Vfs) -> Option<LoadedConfiguration>` (part 1). `LoadedConfiguration` carries `file`, `text`, `configuration`, `diagnostics`. The `known_sets` helper builds `KnownSets` from `celerrate_rules::core_rules()` metadata plus `celerrate_diagnostics::REGISTRY`.
- `crates/celerrate_project/src/discovery.rs`: `discover(root)`, `discover_from_sources(root, manifest, installed)`, `ProjectDiscovery { root, vendor_root, php_version_range, project_walk_roots, vendor_walk_roots, notices }`, `resolve_version_range`.
- `crates/celerrate_vfs/src/walk.rs`: `enumerate_php_files(roots: &[PathBuf]) -> Walk`.
- `crates/celerrate_cli/src/session.rs`: `Session::start` (the startup order this plan restructures), `inputs()` (range-gates the cache snapshot), `absorb`, `rediscover`, `is_project_manifest`.
- `crates/celerrate_cli/src/plugins.rs`: `register_core_rules(database)` sets `RuleRegistration.active = (tier == Tier::Default)`; `plugin_set_digest` is the model for digest encoding (length-prefixed fields, count-prefixed sections).
- `crates/celerrate_cli/src/cache/pack.rs`: `PackHeader { schema, binary, stub_blob, plugins, php_minimum, php_maximum }`, `PackHeader::current(range, plugins)`, `CACHE_SCHEMA_VERSION = 7`. Sub-project 4 reserved one header field for an active-set digest; this plan adds it.
- `crates/celerrate_cli/src/analysis.rs`: `AnalysisInputs`, `persistable_diagnostics`, `typed_portion` (both apply the suppression filter; the severity remap lands beside it), `composed_diagnostics`.
- `crates/celerrate_cli/src/cache/mod.rs`: `persist_timed` builds the header (line ~129) and computes `header_moved` from the range alone.
- `crates/celerrate_cli/tests/cache_equivalence.rs`: the served-equals-recomputed harness pattern (in-process `run`, then `Session::start` + `lookup_verdict`).
- `crates/celerrate_cli/tests/explain_pages.rs`: the executable-example harness and the tier guard test this plan replaces.
- `crates/celerrate_cli/src/watch.rs`: `completed_cycle` (renders each picture), `Watch::build` (registers `composer.json`/`composer.lock` non-recursively), `iteration` (the graceful-exit `Outcome::of` arm).

Diagnostic fixtures used by tests below (all verified against the current engine):

- CEL0018 (unknown class): `<?php new Missing();`
- CEL0021 (symbol introduced after the minimum): `json_validate('{}')` under `"require": {"php": ">=8.1"}` (introduced in 8.3).
- CEL0034 (possibly-null dereference, rule `null-dereference`): a `?User $maybe` parameter and `$maybe->save()` (see `typed_answers_replay_equal` in `tests/cache_equivalence.rs` for the exact fixture).

The rich renderer titles diagnostics through `annotate-snippets` levels (`error`/`warning` plus the identifier). When a test below asserts on a severity word in the report, first run the fixture once and copy the exact rendered line shape; assert on the real literal, never on a guessed one.

---

### Task 1: `celerrate_project` consumes the configuration

**Files:**
- Modify: `crates/celerrate_project/Cargo.toml`
- Modify: `crates/celerrate_project/src/discovery.rs`
- Modify: `crates/celerrate_project/src/lib.rs` (no export change needed; `ProjectDiscovery` and `discover` are already exported)
- Modify: `crates/celerrate_cli/src/session.rs:134` and `:470` (call sites, temporary default)
- Modify: `crates/celerrate_project/tests/discovery_end_to_end.rs:23` and `:127` (call sites)

**Interfaces:**
- Consumes: `celerrate_config::Configuration` (part 1's model, unchanged).
- Produces (later tasks rely on these exact signatures):
  - `pub fn discover(root: &Path, configuration: &celerrate_config::Configuration) -> ProjectDiscovery`
  - `pub fn discover_from_sources(root: &Path, manifest_source: FileSource, installed_source: FileSource, configuration: &celerrate_config::Configuration) -> ProjectDiscovery`
  - `ProjectDiscovery` gains `pub excluded_roots: Vec<PathBuf>` (normalized, root-joined, deduplicated, in file order)
  - `pub fn ProjectDiscovery::is_excluded(&self, path: &Path) -> bool` (prefix match against `excluded_roots`)

Semantics being implemented (spec section 3):

- `php = "8.2"` is the first stage of the version detection precedence: it collapses the range to `PhpVersionRange::point(PhpVersion::new(major, minor).clamped_to_supported())`, wins over `config.platform.php` and `require.php`, and emits no notice. When the override is present, the manifest detection stages are skipped entirely, so their notices (`InvalidPhpVersionConstraint`, `PhpVersionFallback`) do not fire; a missing-manifest notice still does (it is about the manifest, not the version).
- `include` (non-empty) replaces the Composer-derived `project_walk_roots` with the normalized, root-joined paths in declaration order, deduplicated. An empty `include` array behaves like an absent one. Include paths are lexical, exactly like autoload walk roots: nothing stats them. Vendor walk roots are untouched (dependency symbols must keep resolving).
- `exclude` populates `excluded_roots` the same way (normalized, root-joined, deduplicated). Enforcement (walk pruning) is Tasks 2 and 3; this task only derives the data and the `is_excluded` predicate.

- [ ] **Step 1: Add the dependency**

In `crates/celerrate_project/Cargo.toml`, add to `[dependencies]`:

```toml
celerrate_config = { path = "../celerrate_config" }
```

and to `[dev-dependencies]` (tests construct `Spanned` values, whose `range` is a `celerrate_source::TextRange`):

```toml
celerrate_source = { path = "../celerrate_source" }
```

- [ ] **Step 2: Write the failing tests**

In the `tests` module of `crates/celerrate_project/src/discovery.rs`, first add a helper and update every existing call to `discover_from_sources` to pass `&Configuration::default()` as the new fourth argument (this is mechanical; the parity claim is that no existing assertion changes). Then add:

```rust
use celerrate_config::{Configuration, Spanned};

fn spanned<T>(value: T) -> Spanned<T> {
    Spanned {
        value,
        range: celerrate_source::TextRange::new(
            celerrate_source::TextSize::from(0),
            celerrate_source::TextSize::from(0),
        ),
    }
}

#[test]
fn the_php_override_collapses_the_range_and_wins_over_the_manifest() {
    let configuration = Configuration {
        php: Some(spanned((8, 2))),
        ..Configuration::default()
    };
    let discovery = discover_from_sources(
        Path::new(ROOT),
        FileSource::from(
            r#"{
                "require": { "php": "^8.1" },
                "config": { "platform": { "php": "8.4.0" } }
            }"#,
        ),
        FileSource::Absent,
        &configuration,
    );
    assert_eq!(
        discovery.php_version_range,
        PhpVersionRange::point(PhpVersion::new(8, 2)),
    );
    assert_eq!(discovery.notices, Vec::<ProjectNotice>::new());
}

#[test]
fn the_php_override_applies_without_a_manifest_and_is_clamped() {
    let configuration = Configuration {
        php: Some(spanned((7, 0))),
        ..Configuration::default()
    };
    let discovery =
        discover_from_sources(Path::new(ROOT), FileSource::Absent, FileSource::Absent, &configuration);
    assert_eq!(
        discovery.php_version_range,
        PhpVersionRange::point(PhpVersion::new(8, 1)),
        "an unsupported override is clamped, exactly like config.platform.php",
    );
    assert_eq!(
        discovery.notices,
        vec![ProjectNotice::MissingComposerManifest],
        "the manifest notice stays; no version notice fires when the override decides",
    );
}

#[test]
fn the_override_skips_the_detection_stages_and_their_notices() {
    let configuration = Configuration {
        php: Some(spanned((8, 3))),
        ..Configuration::default()
    };
    let discovery = discover_from_sources(
        Path::new(ROOT),
        FileSource::from(r#"{ "require": { "php": "7.4.*" } }"#),
        FileSource::Absent,
        &configuration,
    );
    assert_eq!(
        discovery.php_version_range,
        PhpVersionRange::point(PhpVersion::new(8, 3)),
    );
    assert_eq!(
        discovery.notices,
        Vec::<ProjectNotice>::new(),
        "the user pinned the version; the unused manifest constraint is not reported",
    );
}

#[test]
fn include_replaces_the_autoload_walk_roots_in_declaration_order() {
    let configuration = Configuration {
        include: vec![spanned("app".to_owned()), spanned("scripts".to_owned())],
        ..Configuration::default()
    };
    let discovery = discover_from_sources(
        Path::new(ROOT),
        FileSource::from(
            r#"{ "require": { "php": "^8.1" }, "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        ),
        FileSource::Absent,
        &configuration,
    );
    assert_eq!(
        discovery.project_walk_roots,
        vec![PathBuf::from("/project/app"), PathBuf::from("/project/scripts")],
        "include replaces the Composer-derived roots",
    );
}

#[test]
fn an_empty_include_behaves_like_an_absent_one() {
    let configuration = Configuration::default();
    let with_empty = discover_from_sources(
        Path::new(ROOT),
        FileSource::from(r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#),
        FileSource::Absent,
        &configuration,
    );
    assert_eq!(with_empty.project_walk_roots, vec![PathBuf::from("/project/src")]);
}

#[test]
fn exclude_populates_the_excluded_roots_and_the_predicate() {
    let configuration = Configuration {
        exclude: vec![spanned("src/Generated".to_owned())],
        ..Configuration::default()
    };
    let discovery = discover_from_sources(
        Path::new(ROOT),
        FileSource::from(r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#),
        FileSource::Absent,
        &configuration,
    );
    assert_eq!(
        discovery.excluded_roots,
        vec![PathBuf::from("/project/src/Generated")],
    );
    assert!(discovery.is_excluded(Path::new("/project/src/Generated/Machine.php")));
    assert!(!discovery.is_excluded(Path::new("/project/src/Kernel.php")));
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package celerrate_project`
Expected: compile errors first (the new fourth argument and the missing field/method), then, once signatures exist with stub behavior, assertion failures.

- [ ] **Step 4: Implement**

In `crates/celerrate_project/src/discovery.rs`:

```rust
use celerrate_config::Configuration;
```

`ProjectDiscovery` gains the field and predicate:

```rust
pub struct ProjectDiscovery {
    pub root: PathBuf,
    pub vendor_root: PathBuf,
    pub php_version_range: PhpVersionRange,
    pub project_walk_roots: Vec<PathBuf>,
    pub vendor_walk_roots: Vec<PathBuf>,
    /// The configuration's `exclude` entries, normalized against the
    /// root: subtracted from the walk (spec section 3). Empty without a
    /// configuration file.
    pub excluded_roots: Vec<PathBuf>,
    pub notices: Vec<ProjectNotice>,
}

impl ProjectDiscovery {
    /// Whether `path` falls under an excluded root. Lexical prefix
    /// matching over normalized paths, like `classify`.
    pub fn is_excluded(&self, path: &Path) -> bool {
        self.excluded_roots.iter().any(|root| path.starts_with(root))
    }
}
```

`discover` passes the configuration through:

```rust
pub fn discover(root: &Path, configuration: &Configuration) -> ProjectDiscovery {
    // ... unchanged body ...
    discover_from_sources(root, manifest, installed, configuration)
}
```

`discover_from_sources` gains the parameter and consumes it. The version range becomes (replacing the current `match &manifest` for the range):

```rust
// The parent's detection precedence regains its first stage: the
// `celerrate.toml` override. It collapses the range to a clamped
// point and decides alone; the manifest stages, and the notices
// that report their failures, only speak when no override exists,
// because a constraint the resolution never used is not news.
let php_version_range = match &configuration.php {
    Some(overridden) => {
        let (major, minor) = overridden.value;
        PhpVersionRange::point(PhpVersion::new(major, minor).clamped_to_supported())
    }
    None => match &manifest {
        None => PhpVersionRange::fallback(),
        Some(manifest) => resolve_version_range(manifest, &mut notices),
    },
};
```

The walk roots (replacing the current `project_walk_roots` derivation):

```rust
// A non-empty `include` replaces the Composer-derived roots
// (spec section 3: "default: Composer autoload roots"). The paths
// are lexical, exactly like autoload walk roots: nothing stats
// them, so a declared-but-absent directory is ordinary.
let project_walk_roots = if configuration.include.is_empty() {
    manifest
        .as_ref()
        .map(|manifest| manifest.autoload.walk_roots(root))
        .filter(|walk_roots| !walk_roots.is_empty())
        .unwrap_or_else(|| vec![normalize_path(root, root)])
} else {
    normalized_configuration_paths(&configuration.include, root)
};
let excluded_roots = normalized_configuration_paths(&configuration.exclude, root);
```

with the shared helper:

```rust
/// Root-joined, normalized, deduplicated, in declaration order: the
/// shape both `include` and `exclude` share.
fn normalized_configuration_paths(
    paths: &[celerrate_config::Spanned<String>],
    root: &Path,
) -> Vec<PathBuf> {
    let mut normalized = Vec::new();
    for path in paths {
        let candidate = normalize_path(Path::new(&path.value), root);
        if !normalized.contains(&candidate) {
            normalized.push(candidate);
        }
    }
    normalized
}
```

Add `excluded_roots` to the struct literal at the end of `discover_from_sources`.

Update the two call sites in `crates/celerrate_cli/src/session.rs` (lines 134 and 470) to pass `&celerrate_config::Configuration::default()` for now, with a `// Task 3 threads the loaded configuration here.` comment, and the two calls in `crates/celerrate_project/tests/discovery_end_to_end.rs` the same way (those stay on the default permanently: they test zero-config discovery).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package celerrate_project && cargo test --package celerrate_cli`
Expected: PASS. Every pre-existing `celerrate_project` test passing unchanged with `&Configuration::default()` is the zero-config parity proof at this layer.

- [ ] **Step 6: Commit**

```bash
git add crates/celerrate_project crates/celerrate_cli/src/session.rs
git commit -m "✨ feat(project): derive walk roots and the version range from the configuration"
```

---

### Task 2: Walk exclusion in `celerrate_vfs`

**Files:**
- Modify: `crates/celerrate_vfs/src/walk.rs`
- Modify: `crates/celerrate_cli/src/session.rs` (both `enumerate_php_files` call sites)
- Modify: any other `enumerate_php_files` caller the compiler names (`crates/celerrate_project/tests/discovery_end_to_end.rs` uses it)

**Interfaces:**
- Produces: `pub fn enumerate_php_files(roots: &[PathBuf], excluded: &[PathBuf]) -> Walk`. A root or directory under an excluded prefix is pruned before it is read: an excluded `src/Generated` holding ten thousand files costs nothing.

- [ ] **Step 1: Write the failing tests**

In the existing `tests` module of `crates/celerrate_vfs/src/walk.rs` (create one if the module keeps its tests elsewhere; follow the file's current layout), add:

```rust
#[test]
fn an_excluded_directory_is_pruned_from_the_walk() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src/Generated")).unwrap();
    std::fs::write(root.path().join("src/Kept.php"), "<?php").unwrap();
    std::fs::write(root.path().join("src/Generated/Machine.php"), "<?php").unwrap();

    let walk = enumerate_php_files(
        &[root.path().join("src")],
        &[root.path().join("src/Generated")],
    );
    assert_eq!(walk.files, vec![root.path().join("src/Kept.php")]);
}

#[test]
fn an_excluded_root_yields_nothing_at_all() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/File.php"), "<?php").unwrap();

    let walk = enumerate_php_files(&[root.path().join("src")], &[root.path().join("src")]);
    assert!(walk.files.is_empty());
}

#[test]
fn no_exclusions_is_the_walk_as_before() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/File.php"), "<?php").unwrap();

    let walk = enumerate_php_files(&[root.path().join("src")], &[]);
    assert_eq!(walk.files, vec![root.path().join("src/File.php")]);
}
```

If `walk.rs` has no test module yet, add one with the standard test-lint allows and `tempfile` as a dev-dependency of `celerrate_vfs` (check `Cargo.toml`; add it if absent).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_vfs`
Expected: compile error (wrong arity) first, then failures.

- [ ] **Step 3: Implement**

In `crates/celerrate_vfs/src/walk.rs`:

```rust
/// Whether `path` falls under one of the excluded prefixes.
fn is_excluded(path: &Path, excluded: &[PathBuf]) -> bool {
    excluded.iter().any(|root| path.starts_with(root))
}

pub fn enumerate_php_files(roots: &[PathBuf], excluded: &[PathBuf]) -> Walk {
    let mut files = BTreeSet::new();
    let mut unreadable = BTreeSet::new();
    let mut visited_directories = BTreeSet::new();
    for root in roots {
        if is_excluded(root, excluded) {
            continue;
        }
        if root.is_file() {
            files.insert(root.clone());
        } else if root.is_dir() {
            walk_directory(root, excluded, &mut files, &mut unreadable, &mut visited_directories);
        }
    }
    Walk {
        files: files.into_iter().collect(),
        unreadable_directories: unreadable.into_iter().collect(),
    }
}
```

`walk_directory` gains `excluded: &[PathBuf]` and prunes in its entry loop, before recursing or inserting:

```rust
for entry in entries.flatten() {
    let path = entry.path();
    if is_excluded(&path, excluded) {
        continue;
    }
    if path.is_dir() {
        walk_directory(&path, excluded, files, unreadable, visited);
    } else if has_php_extension(&path) && path.is_file() {
        files.insert(path);
    }
}
```

Update the doc comment: pruning happens before the directory is read, so an excluded tree is never opened. Update every caller: in `crates/celerrate_cli/src/session.rs`, both sites become `enumerate_php_files(&walk_roots, &session.discovery.excluded_roots)` (in `start`, the discovery is on the local before the session is built; adapt to the local binding) and `enumerate_php_files(&self.discovery.walk_roots(), &self.discovery.excluded_roots)` in `rediscover`. Other callers the compiler names pass `&[]`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_vfs && cargo test --package celerrate_cli`
Expected: PASS (`excluded_roots` is still always empty in the CLI, so nothing changes behavior there yet).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_vfs crates/celerrate_cli crates/celerrate_project
git commit -m "✨ feat(vfs): prune excluded roots from the walk before reading them"
```

---

### Task 3: The composition root wires the configuration into discovery and the walk

**Files:**
- Modify: `crates/celerrate_cli/src/session.rs` (`Session::start` restructure, `absorb`, `rediscover`, a `configuration_model` helper)
- Modify: `crates/celerrate_cli/src/configuration.rs` (module doc: the part 1 boundary paragraph shrinks)
- Test: `crates/celerrate_cli/tests/configuration.rs`

**Interfaces:**
- Consumes: Task 1's `discover(root, &Configuration)`, Task 2's `enumerate_php_files(roots, excluded)`.
- Produces: `Session::start` loads `celerrate.toml` before discovery; `Session` gains `pub(crate) fn configuration_model(&self) -> celerrate_config::Configuration` (the parsed model, or the default when no file). Tasks 4 to 7 rely on the new startup order: configuration load, then discovery, then registration, then cache load, then walk.

- [ ] **Step 1: Write the failing integration tests**

Append to `crates/celerrate_cli/tests/configuration.rs` (reusing its `check`/`write_files`/`run_check` helpers and `MANIFEST`/`CLEAN_SOURCE` constants):

```rust
#[test]
fn include_widens_the_analysis_to_directories_composer_does_not_declare() {
    let files: &[(&str, &str)] = &[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        ("scripts/tool.php", "<?php\nnew MissingFromScripts();\n"),
    ];
    let (outcome, report) = check(files);
    assert!(
        matches!(outcome, Outcome::Clean),
        "without include, scripts/ is not walked: {report}",
    );

    let mut with_include = files.to_vec();
    with_include.push(("celerrate.toml", "[project]\ninclude = [\"src\", \"scripts\"]\n"));
    let (outcome, report) = check(&with_include);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");
    assert!(report.contains("CEL0018"), "{report}");
    assert!(report.contains("MissingFromScripts"), "{report}");
}

#[test]
fn exclude_subtracts_a_directory_from_the_analysis() {
    let files: &[(&str, &str)] = &[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        ("src/Generated/Machine.php", "<?php\nnew MissingFromGenerated();\n"),
    ];
    let (outcome, report) = check(files);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");

    let mut with_exclude = files.to_vec();
    with_exclude.push(("celerrate.toml", "[project]\nexclude = [\"src/Generated\"]\n"));
    let (outcome, report) = check(&with_exclude);
    assert!(
        matches!(outcome, Outcome::Clean),
        "the excluded directory no longer speaks: {report}",
    );
}

#[test]
fn the_php_override_collapses_the_range_and_gates_availability() {
    let files: &[(&str, &str)] = &[
        ("composer.json", r#"{"require": {"php": ">=8.1"}}"#),
        ("a.php", "<?php json_validate('{}');\n"),
    ];
    let (outcome, report) = check(files);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");
    assert!(report.contains("CEL0021"), "{report}");

    let mut with_override = files.to_vec();
    with_override.push(("celerrate.toml", "[project]\nphp = \"8.3\"\n"));
    let (outcome, report) = check(&with_override);
    assert!(
        matches!(outcome, Outcome::Clean),
        "at a fixed 8.3 the symbol exists: {report}",
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli --test configuration`
Expected: the three new tests FAIL (the file is loaded after discovery and never consumed); the part 1 tests still pass.

- [ ] **Step 3: Restructure `Session::start`**

In `crates/celerrate_cli/src/session.rs`, reorder the top of `start` so the configuration is loaded first and threaded through:

```rust
pub fn start(root: &Path) -> Self {
    // The same normalized form discovery would produce: paths are
    // interned and relativized against this exact value.
    let root = celerrate_vfs::normalize_path(root, root);
    let mut internal_errors = Vec::new();
    let database = AnalysisDatabase::default();
    let mut vfs = Vfs::default();
    // Loaded before discovery, deliberately: include/exclude and the
    // `php` override are discovery inputs now (spec section 2).
    // `celerrate.toml` therefore takes the first file identifier;
    // nothing reads the interning order, and rendering resolves
    // display paths through the VFS by identity.
    let loaded_configuration = crate::configuration::load(&root, &mut vfs);
    let configuration_model = loaded_configuration
        .as_ref()
        .map(|loaded| loaded.configuration.clone())
        .unwrap_or_default();
    let discovery = discover(&root, &configuration_model);
    // ... the rest of the existing body, unchanged ...
```

Then, in the existing body: keep the stub index, `ProjectConfiguration`, file set, statistics, cache directory, plugin registration, core-rule registration, digest, and cache load exactly where they are; build the session struct with `vfs` (moved in) and `loaded_configuration` (moved in, no post-construction assignment anymore); delete the old post-walk `configuration::load` call and its comment. The walk call becomes:

```rust
let walk = enumerate_php_files(
    &session.discovery.walk_roots(),
    &session.discovery.excluded_roots,
);
session.load(&walk);
session
```

Add the model helper (used by `rediscover` now and Tasks 4 to 7 later):

```rust
/// The parsed configuration model, or the default when the project
/// has no file: what every consumer of configuration data reads.
pub(crate) fn configuration_model(&self) -> celerrate_config::Configuration {
    self.loaded_configuration
        .as_ref()
        .map(|loaded| loaded.configuration.clone())
        .unwrap_or_default()
}
```

`rediscover` threads the model (the reload itself is Task 7):

```rust
fn rediscover(&mut self) {
    let root = self.discovery.root.clone();
    let discovery = discover(&root, &self.configuration_model());
    // ... unchanged: range comparison, walk, load ...
}
```

`absorb` ignores excluded paths (a change under `src/Generated` must not re-enter the analyzed set):

```rust
for path in changed {
    if !is_php(path) || self.discovery.is_excluded(path) {
        continue;
    }
    // ... unchanged ...
}
```

Also update the `loaded_configuration` field doc (session.rs lines ~120-124): the configuration is now consumed; only the `--watch` reload note remains (and Task 7 removes that too). Update the `configuration.rs` module doc's "Part 1 boundary" paragraph: include/exclude and `php` are consumed as of this task; the active set, the remap, and the digest still name their tasks.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_cli`
Expected: PASS, including every pre-existing session, watch, cache, and snapshot test (the reorder must not change zero-config behavior; if a snapshot moves, stop and understand why before touching it).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cli): wire include, exclude, and the php override through the session"
```

---

### Task 4: The configuration digest joins the persistent cache header

**Files:**
- Modify: `crates/celerrate_cli/src/configuration.rs` (the digest function)
- Modify: `crates/celerrate_cli/src/cache/pack.rs` (`PackHeader`, `CACHE_SCHEMA_VERSION`)
- Modify: `crates/celerrate_cli/src/session.rs` (two new fields, `start`, `inputs`)
- Modify: `crates/celerrate_cli/src/cache/mod.rs` (`persist_timed` header and `header_moved`)
- Modify: every `PackHeader::current` call site the compiler names (`crates/celerrate_cli/tests/cache_seeding.rs` has ~28, `src/cache/pack.rs` tests, `src/cache/mod.rs` tests, `src/watch.rs` tests)
- Test: `crates/celerrate_cli/tests/cache_configuration.rs` (new)

**Interfaces:**
- Consumes: `celerrate_config::Configuration`, `blake3` (already a dependency), Task 3's startup order (the model exists before the cache loads).
- Produces:
  - `pub fn configuration_digest(configuration: &celerrate_config::Configuration) -> [u8; 32]` in `celerrate_cli::configuration`
  - `PackHeader` gains `pub configuration: [u8; 32]`; `PackHeader::current(range: PhpVersionRange, plugins: [u8; 32], configuration: [u8; 32]) -> Self`
  - `CACHE_SCHEMA_VERSION = 8`
  - `Session` gains `pub configuration_digest: [u8; 32]` and `pub cache_loaded_configuration_digest: [u8; 32]`

Digest semantics (spec section 2): blake3 over the **normalized** `[rules]` and `[severity]` sections, not only the active set, so future rule options join the cache key by construction. Normalization means: entries sorted, every text field length-prefixed, every section count-prefixed (the `plugin_set_digest` encoding discipline). `include`/`exclude`/`php` stay out: the PHP range is already a header field, and file membership is per-entry, not per-pack.

- [ ] **Step 1: Write the failing unit tests**

In the `tests` module of `crates/celerrate_cli/src/configuration.rs`:

```rust
use super::configuration_digest;

/// Parses a `celerrate.toml` text into its model, for digest tests.
fn model_of(text: &str) -> celerrate_config::Configuration {
    let (configuration, _) = celerrate_config::parse(celerrate_source::FileId::new(0), text);
    configuration
}

#[test]
fn the_digest_ignores_span_and_order_but_not_content() {
    let ordered = model_of("[rules.a-rule]\nenabled = true\n\n[rules.b-rule]\nenabled = false\n");
    let reversed = model_of("[rules.b-rule]\nenabled = false\n\n[rules.a-rule]\nenabled = true\n");
    assert_eq!(
        configuration_digest(&ordered),
        configuration_digest(&reversed),
        "normalization sorts the entries",
    );
    let changed = model_of("[rules.a-rule]\nenabled = false\n\n[rules.b-rule]\nenabled = false\n");
    assert_ne!(configuration_digest(&ordered), configuration_digest(&changed));
}

#[test]
fn a_severity_entry_moves_the_digest() {
    let without = model_of("");
    let with = model_of("[severity]\n\"CEL0034\" = \"warning\"\n");
    assert_ne!(configuration_digest(&without), configuration_digest(&with));
}

#[test]
fn no_file_and_an_empty_file_share_the_digest() {
    assert_eq!(
        configuration_digest(&celerrate_config::Configuration::default()),
        configuration_digest(&model_of("")),
    );
}

#[test]
fn the_project_table_does_not_move_the_digest() {
    // include/exclude change file membership (per-entry keys) and the
    // php override moves the header's own range fields: neither
    // belongs in this digest, per the spec's normalized-sections rule.
    let without = model_of("");
    let with = model_of("[project]\nphp = \"8.2\"\ninclude = [\"src\"]\n");
    assert_eq!(configuration_digest(&without), configuration_digest(&with));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli --lib configuration`
Expected: compile error (`configuration_digest` does not exist).

- [ ] **Step 3: Implement the digest**

In `crates/celerrate_cli/src/configuration.rs`:

```rust
/// blake3 over the normalized `[rules]` and `[severity]` sections: the
/// active-set-and-severity cache key the sub-project 4 spec reserved a
/// header field for (CLI product spec section 2). The whole sections
/// are digested, not the derived active set, so future rule options
/// join the key with no header change. Normalization: entries sorted,
/// text length-prefixed, sections count-prefixed, spans dropped.
pub fn configuration_digest(configuration: &celerrate_config::Configuration) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    let mut rules: Vec<(&str, u8)> = configuration
        .rules
        .iter()
        .map(|rule| {
            let enabled = match &rule.enabled {
                None => 0u8,
                Some(enabled) if !enabled.value => 1,
                Some(_) => 2,
            };
            (rule.name.value.as_str(), enabled)
        })
        .collect();
    rules.sort_unstable();
    hash_count(&mut hasher, rules.len());
    for (name, enabled) in rules {
        hash_text(&mut hasher, name);
        hasher.update(&[enabled]);
    }
    let mut severity: Vec<(&str, u8)> = configuration
        .severity
        .iter()
        .map(|entry| {
            let level = match &entry.severity {
                None => 0u8,
                Some(severity) if severity.value == celerrate_diagnostics::Severity::Warning => 1,
                Some(_) => 2,
            };
            (entry.identifier.value.as_str(), level)
        })
        .collect();
    severity.sort_unstable();
    hash_count(&mut hasher, severity.len());
    for (identifier, level) in severity {
        hash_text(&mut hasher, identifier);
        hasher.update(&[level]);
    }
    *hasher.finalize().as_bytes()
}

fn hash_count(hasher: &mut blake3::Hasher, count: usize) {
    hasher.update(&u64::try_from(count).unwrap_or(u64::MAX).to_le_bytes());
}

fn hash_text(hasher: &mut blake3::Hasher, text: &str) {
    hash_count(hasher, text.len());
    hasher.update(text.as_bytes());
}
```

Run: `cargo test --package celerrate_cli --lib configuration`
Expected: PASS.

- [ ] **Step 4: Put the digest in the header**

In `crates/celerrate_cli/src/cache/pack.rs`:

- `PackHeader` gains, after `plugins`:

```rust
/// blake3 over the normalized `[rules]` and `[severity]` sections of
/// `celerrate.toml` (`configuration::configuration_digest`): the
/// field sub-project 4 reserved. A configuration change discards the
/// pack wholesale, which is what keeps a warm run under a new
/// configuration honest.
pub configuration: [u8; 32],
```

- `current` gains the parameter:

```rust
pub fn current(range: PhpVersionRange, plugins: [u8; 32], configuration: [u8; 32]) -> Self {
    Self {
        schema: CACHE_SCHEMA_VERSION,
        binary: super::identity::binary_identity().to_owned(),
        stub_blob: *blake3::hash(celerrate_stubs::EMBEDDED_STUB_BLOB).as_bytes(),
        plugins,
        configuration,
        php_minimum: (range.minimum.major, range.minimum.minor),
        php_maximum: (range.maximum.major, range.maximum.minor),
    }
}
```

- `CACHE_SCHEMA_VERSION` becomes `8`, with the doc entry:

```rust
/// 8: the header gains `configuration`, the active-set-and-severity
/// digest the diagnostics-and-fixes spec reserved a field for (CLI
/// product spec, section 2).
```

In `crates/celerrate_cli/src/session.rs`:

- `Session` gains the two fields (documented on the `cache_loaded_range` model):

```rust
/// The configuration digest packs are keyed on this session
/// (`configuration::configuration_digest` over the loaded model).
pub configuration_digest: [u8; 32],
/// The digest the snapshot was loaded under: `--watch` can move the
/// live digest at runtime (a `celerrate.toml` edit), and verdicts
/// persisted under the old configuration must not be served past it.
pub cache_loaded_configuration_digest: [u8; 32],
```

- `start` computes it right after `configuration_model` exists and before the cache loads:

```rust
let configuration_digest = crate::configuration::configuration_digest(&configuration_model);
```

uses it in the load: `&PackHeader::current(cache_loaded_range, plugin_set_digest, configuration_digest)`, and initializes both fields with it in the struct literal.

- `inputs()` gates on it too:

```rust
cache: if current_range == self.cache_loaded_range
    && self.configuration_digest == self.cache_loaded_configuration_digest
{
    self.cache.clone()
} else {
    Arc::new(CacheSnapshot::default())
},
```

In `crates/celerrate_cli/src/cache/mod.rs`, `persist_timed`:

```rust
let header = PackHeader::current(
    current_range,
    session.plugin_set_digest,
    session.configuration_digest,
);
```

and the staleness comparison (keep the existing comment, extend it):

```rust
let header_moved = current_range != session.cache_loaded_range
    || session.configuration_digest != session.cache_loaded_configuration_digest;
```

Sweep every other `PackHeader::current` call site the compiler names (the pack tests, `cache/mod.rs` tests, `watch.rs` tests, and `tests/cache_seeding.rs`): where a session exists, pass `session.configuration_digest`; in header-only tests, pass `crate::configuration::configuration_digest(&celerrate_config::Configuration::default())` (or the tests' own helper wrapping it). In `pack.rs`, extend `a_header_mismatch_discards_the_whole_pack` with a flipped-configuration arm:

```rust
let mut other_configuration = header();
other_configuration.configuration[0] ^= 0xFF;
assert!(
    decode::<Vec<(u32, String)>>(&bytes, &other_configuration).is_none(),
    "the configuration field is load-bearing",
);
```

Run: `cargo test --package celerrate_cli`
Expected: PASS.

- [ ] **Step 5: Write the failing warm/cold integration tests**

Create `crates/celerrate_cli/tests/cache_configuration.rs`:

```rust
//! Warm/cold extended to configuration (closure gate 2): the same
//! `celerrate.toml` gives the warm path byte for byte; a change to its
//! `[rules]` or `[severity]` sections invalidates through the header
//! digest, never through luck.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::path::Path;

use celerrate_cli::cache::verdict::{VerdictLookup, lookup_verdict};
use celerrate_cli::session::Session;
use celerrate_cli::{ColorMode, run};

const MANIFEST: &str = r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;
/// A file with one stable diagnostic (CEL0018), so verdicts are
/// non-trivial and the report is not empty.
const SOURCE: &str = "<?php\nnamespace App;\n\nnew \\MissingDependency();\n";

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let path = root.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    root
}

fn run_check(root: &Path) -> String {
    let mut output = Vec::new();
    let _ = run(
        vec!["celerrate".into(), "check".into(), root.as_os_str().to_owned()],
        &mut output,
        ColorMode::Plain,
    );
    String::from_utf8(output).unwrap()
}

/// Whether every analyzed file's verdict is served from the packs when
/// a fresh session opens over `root` now.
fn all_verdicts_hit(root: &Path) -> bool {
    let session = Session::start(root);
    let inputs = session.inputs();
    session
        .sources
        .values()
        .all(|&file| matches!(lookup_verdict(&inputs, file), VerdictLookup::Hit { .. }))
}

#[test]
fn the_same_configuration_file_serves_the_warm_path_byte_for_byte() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", SOURCE),
        ("celerrate.toml", "[rules.null-dereference]\nenabled = true\n"),
    ]);
    let cold = run_check(root.path());
    assert!(all_verdicts_hit(root.path()), "the first run persisted");
    let warm = run_check(root.path());
    assert_eq!(cold, warm, "the warm report is byte-identical");
}

#[test]
fn a_rules_section_change_discards_the_packs() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", SOURCE),
        ("celerrate.toml", "[rules.null-dereference]\nenabled = true\n"),
    ]);
    let _ = run_check(root.path());
    assert!(all_verdicts_hit(root.path()));

    std::fs::write(root.path().join("celerrate.toml"), "").unwrap();
    assert!(
        !all_verdicts_hit(root.path()),
        "a different digest must not serve the old packs",
    );
}

#[test]
fn a_severity_section_change_discards_the_packs() {
    let root = project(&[("composer.json", MANIFEST), ("src/Example.php", SOURCE)]);
    let _ = run_check(root.path());
    assert!(all_verdicts_hit(root.path()));

    std::fs::write(
        root.path().join("celerrate.toml"),
        "[severity]\n\"CEL0018\" = \"warning\"\n",
    )
    .unwrap();
    assert!(!all_verdicts_hit(root.path()));
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo test --package celerrate_cli --test cache_configuration`
Expected: PASS already (the digest work above is what they pin). If any fails, the wiring above is wrong; fix it, not the test.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cache): key the packs on the configuration digest"
```

---

### Task 5: The active set: disabling, force-activation, and the guard-test update

**Files:**
- Modify: `crates/celerrate_cli/src/plugins.rs` (`register_core_rules`, new `core_registrations` and `rule_is_active`)
- Modify: `crates/celerrate_cli/src/configuration.rs` (`rule_overrides`)
- Modify: `crates/celerrate_cli/src/session.rs` (thread the overrides into registration)
- Modify: `crates/celerrate_cli/tests/explain_pages.rs` (the harness forces activation; the tier guard test is replaced)
- Test: `crates/celerrate_cli/tests/configuration.rs`, `crates/celerrate_cli/tests/cache_configuration.rs`

**Interfaces:**
- Consumes: Task 3's startup order (the loaded configuration exists before registration), Task 4's digest (an active-set change invalidates the packs).
- Produces:
  - `pub fn rule_overrides(loaded: Option<&LoadedConfiguration>) -> BTreeMap<String, bool>` in `celerrate_cli::configuration` (every `[rules.<name>]` table with an `enabled` key; unknown names ride along inert, CEL0046 already reported them)
  - `pub fn rule_is_active(tier: celerrate_rules::Tier, override_enabled: Option<bool>) -> bool` in `celerrate_cli::plugins`
  - `pub fn core_registrations(overrides: &BTreeMap<String, bool>) -> Vec<celerrate_rules::RuleRegistration>` in `celerrate_cli::plugins` (Task 7 reuses it for the `--watch` reload)
  - `pub fn register_core_rules(database: &AnalysisDatabase, overrides: &BTreeMap<String, bool>)`

- [ ] **Step 1: Write the failing unit tests**

In the `tests` module of `crates/celerrate_cli/src/plugins.rs`:

```rust
#[test]
fn the_active_computation_is_the_specs_formula() {
    use celerrate_rules::Tier;
    // (`Default`-tier rules minus disabled) union (nursery enabled).
    assert!(rule_is_active(Tier::Default, None));
    assert!(!rule_is_active(Tier::Default, Some(false)));
    assert!(rule_is_active(Tier::Default, Some(true)), "a valid no-op");
    assert!(!rule_is_active(Tier::Nursery, None));
    assert!(rule_is_active(Tier::Nursery, Some(true)), "force-activation");
    assert!(!rule_is_active(Tier::Nursery, Some(false)), "a valid no-op");
}

#[test]
fn an_override_reaches_the_registration_it_names_and_no_other() {
    let mut overrides = std::collections::BTreeMap::new();
    overrides.insert("null-dereference".to_owned(), false);
    let registrations = core_registrations(&overrides);
    for registration in &registrations {
        let expected = registration.metadata.name != "null-dereference";
        assert_eq!(registration.active, expected, "{}", registration.metadata.name);
    }
}
```

In the `tests` module of `crates/celerrate_cli/src/configuration.rs`:

```rust
#[test]
fn rule_overrides_carry_every_enabled_key_and_nothing_else() {
    let root = root(&[(
        "celerrate.toml",
        "[rules.null-dereference]\nenabled = false\n\n[rules.unknown-members]\n",
    )]);
    let mut vfs = celerrate_vfs::Vfs::default();
    let loaded = load(root.path(), &mut vfs);
    let overrides = rule_overrides(loaded.as_ref());
    assert_eq!(overrides.len(), 1, "a table without `enabled` is a no-op");
    assert_eq!(overrides.get("null-dereference"), Some(&false));
}
```

And the integration tests, appended to `crates/celerrate_cli/tests/configuration.rs` (the CEL0034 fixture is the one `tests/cache_equivalence.rs` uses in `typed_answers_replay_equal`):

```rust
const NULLABLE_MANIFEST: &str =
    r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;
const NULLABLE_SOURCE: &str = "<?php\nnamespace App;\n\nclass User { public function save(): void {} }\n\nclass Consumer\n{\n    public function run(?User $maybe): void\n    {\n        $maybe->save();\n    }\n}\n";

#[test]
fn disabling_a_default_rule_removes_its_diagnostics() {
    let files: &[(&str, &str)] = &[
        ("composer.json", NULLABLE_MANIFEST),
        ("src/Consumer.php", NULLABLE_SOURCE),
    ];
    let (outcome, report) = check(files);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");
    assert!(report.contains("CEL0034"), "{report}");

    let mut disabled = files.to_vec();
    disabled.push(("celerrate.toml", "[rules.null-dereference]\nenabled = false\n"));
    let (outcome, report) = check(&disabled);
    assert!(matches!(outcome, Outcome::Clean), "{report}");
}

#[test]
fn enabling_a_default_rule_is_a_silent_no_op() {
    let (outcome, report) = check(&[
        ("composer.json", NULLABLE_MANIFEST),
        ("src/Consumer.php", NULLABLE_SOURCE),
        ("celerrate.toml", "[rules.null-dereference]\nenabled = true\n"),
    ]);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");
    assert!(report.contains("CEL0034"), "{report}");
}
```

And the warm-path interaction, appended to `crates/celerrate_cli/tests/cache_configuration.rs`:

```rust
#[test]
fn a_rule_disabled_after_a_warm_run_stops_speaking() {
    // The first run persists verdicts carrying CEL0034; disabling the
    // rule moves the digest, so the second run must not serve them.
    let root = project(&[
        ("composer.json", MANIFEST),
        (
            "src/Consumer.php",
            "<?php\nnamespace App;\n\nclass User { public function save(): void {} }\n\nclass Consumer\n{\n    public function run(?User $maybe): void\n    {\n        $maybe->save();\n    }\n}\n",
        ),
    ]);
    let first = run_check(root.path());
    assert!(first.contains("CEL0034"), "{first}");

    std::fs::write(
        root.path().join("celerrate.toml"),
        "[rules.null-dereference]\nenabled = false\n",
    )
    .unwrap();
    let second = run_check(root.path());
    assert!(
        !second.contains("CEL0034"),
        "a stale pack must not resurrect a disabled rule: {second}",
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli`
Expected: compile errors (`rule_is_active`, `core_registrations`, `rule_overrides` missing; `register_core_rules` arity), then failures.

- [ ] **Step 3: Implement**

In `crates/celerrate_cli/src/configuration.rs`:

```rust
/// The `[rules]` activation overrides: every table that sets `enabled`,
/// by rule name. Unknown names ride along inert (no registration
/// matches them; CEL0046 already reported the typo), and a table
/// without `enabled` configures nothing, both per spec section 3.
pub fn rule_overrides(loaded: Option<&LoadedConfiguration>) -> BTreeMap<String, bool> {
    let Some(loaded) = loaded else {
        return BTreeMap::new();
    };
    loaded
        .configuration
        .rules
        .iter()
        .filter_map(|rule| {
            rule.enabled
                .as_ref()
                .map(|enabled| (rule.name.value.clone(), enabled.value))
        })
        .collect()
}
```

In `crates/celerrate_cli/src/plugins.rs` (add `use std::collections::BTreeMap;`):

```rust
/// The spec's active-set formula (section 2): (`Default`-tier rules
/// minus disabled) union (nursery rules enabled). An override on the
/// tier's own default is a valid no-op, so promotions and demotions
/// never break existing configurations.
pub fn rule_is_active(tier: celerrate_rules::Tier, override_enabled: Option<bool>) -> bool {
    override_enabled.unwrap_or(tier == celerrate_rules::Tier::Default)
}

/// The core registrations under the configured active set. Split from
/// `register_core_rules` so the `--watch` reload can rebuild the same
/// list for the registry setter.
pub fn core_registrations(overrides: &BTreeMap<String, bool>) -> Vec<celerrate_rules::RuleRegistration> {
    let identity = core_identity();
    celerrate_rules::core_rules()
        .into_iter()
        .map(|(metadata, implementation)| celerrate_rules::RuleRegistration {
            identity: identity.clone(),
            active: rule_is_active(metadata.tier, overrides.get(metadata.name.as_str()).copied()),
            metadata,
            implementation,
        })
        .collect()
}

pub fn register_core_rules(database: &AnalysisDatabase, overrides: &BTreeMap<String, bool>) {
    let _ = celerrate_rules::RuleRegistry::builder(core_registrations(overrides))
        .durability(salsa::Durability::HIGH)
        .new(database);
}
```

(Keep the existing rustdoc on `register_core_rules`; extend it with one sentence about the overrides.)

In `crates/celerrate_cli/src/session.rs`, `start`:

```rust
register_core_rules(
    &database,
    &crate::configuration::rule_overrides(loaded_configuration.as_ref()),
);
```

Update every other `register_core_rules` caller the compiler names (the `plugins.rs` tests and any composition-root test helper) to pass `&std::collections::BTreeMap::new()`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_cli`
Expected: PASS, except `explain_pages::every_core_rule_is_default_tier_so_the_default_active_set_covers_all_pages`, which is untouched so far and still passes; the next step replaces it.

- [ ] **Step 5: The harness forces activation, and the guard test is replaced**

In `crates/celerrate_cli/tests/explain_pages.rs`:

Add the generator and wire it into `report_for`:

```rust
/// Forces every core rule active through the product's own channel
/// (`[rules.<name>] enabled = true`): a valid no-op for `Default`-tier
/// rules today, and the force-activation the spec requires the day the
/// first nursery rule lands, with no harness change. Skipped when the
/// example declares its own `celerrate.toml`.
fn force_activation_configuration() -> String {
    let mut text = String::new();
    for (metadata, _) in celerrate_rules::core_rules() {
        text.push_str("[rules.");
        text.push_str(&metadata.name);
        text.push_str("]\nenabled = true\n\n");
    }
    text
}
```

In `report_for`, after writing the fixture files:

```rust
let files = fixture_files(example);
let declares_configuration = files.iter().any(|(path, _)| path == "celerrate.toml");
for (path, contents) in files {
    // ... unchanged write loop ...
}
if !declares_configuration {
    std::fs::write(
        root.path().join("celerrate.toml"),
        force_activation_configuration(),
    )
    .unwrap();
}
```

Delete `every_core_rule_is_default_tier_so_the_default_active_set_covers_all_pages` and its doc comment, and add in its place:

```rust
/// The spec's harness requirement (section 2): explain pages stay
/// executable for every tier, because the harness forces each rule
/// active through the same `[rules]` mechanism a user would. This
/// pins that the generated configuration names every core rule and is
/// itself silent, so no page's report can be polluted by it.
#[test]
fn the_forced_activation_names_every_core_rule_and_stays_silent() {
    let configuration = force_activation_configuration();
    for (metadata, _) in celerrate_rules::core_rules() {
        assert!(
            configuration.contains(&format!("[rules.{}]", metadata.name)),
            "rule `{}` is missing from the forced activation",
            metadata.name,
        );
    }
    let report = report_for("<?php\n\nfunction example(): void {}\n");
    assert!(
        !report.contains("CEL004"),
        "the forced activation must not report configuration diagnostics:\n{report}",
    );
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --package celerrate_cli --test explain_pages -- --nocapture`
Expected: PASS. `every_written_page_example_is_honest` now runs every markerless example under the forced-activation file; if any page's fixed example suddenly fires something, the injected file is wrong (most likely a config diagnostic), not the page.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cli): compute the active rule set from the configuration"
```

---

### Task 6: The severity remap in the per-file composition

**Files:**
- Modify: `crates/celerrate_cli/src/configuration.rs` (`severity_remap`, module doc finalized)
- Modify: `crates/celerrate_cli/src/session.rs` (new field, `inputs`)
- Modify: `crates/celerrate_cli/src/analysis.rs` (`AnalysisInputs`, the remap application)
- Test: `crates/celerrate_cli/tests/configuration.rs`, `crates/celerrate_cli/tests/cache_equivalence.rs`, `crates/celerrate_cli/tests/cache_configuration.rs`

**Interfaces:**
- Consumes: Task 4's digest (a remap change invalidates the packs), `celerrate_diagnostics::Severity`, `Diagnostic.severity` (a public field).
- Produces:
  - `pub fn severity_remap(loaded: Option<&LoadedConfiguration>) -> BTreeMap<String, Severity>` in `celerrate_cli::configuration` (keys are identifier strings like `"CEL0034"`; only remappable identifiers survive, resilience and unknown entries were already reported and must not apply)
  - `Session` and `AnalysisInputs` gain `pub severity_remap: Arc<BTreeMap<String, Severity>>`

Placement (spec section 2, load-bearing): the remap applies in the per-file composition **before persistence**, at the same place suppression filtering lives (`persistable_diagnostics` and `typed_portion`), so "the persisted verdict equals the printed report" holds by construction. On a warm hit the stored verdict already carries remapped severities, and the header digest guarantees it was written under the same remap.

- [ ] **Step 1: Write the failing tests**

Unit test in `crates/celerrate_cli/src/configuration.rs`:

```rust
#[test]
fn the_remap_keeps_remappable_entries_and_drops_the_reported_ones() {
    let root = root(&[(
        "celerrate.toml",
        "[severity]\n\"CEL0034\" = \"warning\"\n\"CEL0026\" = \"warning\"\n\"CEL9999\" = \"warning\"\n",
    )]);
    let mut vfs = celerrate_vfs::Vfs::default();
    let loaded = load(root.path(), &mut vfs);
    let remap = severity_remap(loaded.as_ref());
    assert_eq!(remap.len(), 1, "resilience and unknown entries never apply");
    assert_eq!(
        remap.get("CEL0034"),
        Some(&celerrate_diagnostics::Severity::Warning),
    );
}
```

Integration tests appended to `crates/celerrate_cli/tests/configuration.rs`. Before writing the assertions, run the CEL0018 fixture once without a remap and copy the exact severity-bearing line the renderer produces (an `annotate-snippets` title carrying `error` and the identifier); use that literal shape in both assertions:

```rust
#[test]
fn a_severity_remap_changes_the_printed_severity_but_not_the_exit_code() {
    let files: &[(&str, &str)] = &[
        ("composer.json", MANIFEST),
        ("src/Example.php", "<?php\nnamespace App;\n\nnew \\MissingDependency();\n"),
    ];
    let (outcome, report) = check(files);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");
    // Copy the real rendered title here after one run, for example:
    // assert!(report.contains("error[CEL0018]"), "{report}");

    let mut remapped = files.to_vec();
    remapped.push(("celerrate.toml", "[severity]\n\"CEL0018\" = \"warning\"\n"));
    let (outcome, report) = check(&remapped);
    assert!(
        matches!(outcome, Outcome::DiagnosticsReported),
        "a warning still exits 1: {report}",
    );
    // The warning form of the same title, again the real literal:
    // assert!(report.contains("warning[CEL0018]"), "{report}");
    // assert!(!report.contains("error[CEL0018]"), "{report}");
}
```

Warm-path change test appended to `crates/celerrate_cli/tests/cache_configuration.rs`:

```rust
#[test]
fn a_remap_changed_after_a_warm_run_reprints_with_the_new_severity() {
    let root = project(&[("composer.json", MANIFEST), ("src/Example.php", SOURCE)]);
    let first = run_check(root.path());

    std::fs::write(
        root.path().join("celerrate.toml"),
        "[severity]\n\"CEL0018\" = \"warning\"\n",
    )
    .unwrap();
    let second = run_check(root.path());
    assert_ne!(first, second, "the digest forced the cold path");
    // The severity literals, once copied from the real renderer:
    // assert!(second.contains("warning[CEL0018]"), "{second}");
}
```

And the served-equals-recomputed extension in `crates/celerrate_cli/tests/cache_equivalence.rs` (the harness compares full `Diagnostic` values, severity included, so one remapped fixture makes it cover the remap):

```rust
/// The severity remap rides the persisted verdicts: a warm run must
/// serve the remapped severity, equal to what recomputation under the
/// same remap produces (spec section 2's "persisted verdict equals the
/// printed report").
#[test]
fn remapped_severities_replay_equal() {
    let identifiers = served_equals_recomputed(&[
        ("celerrate.toml", "[severity]\n\"CEL0018\" = \"warning\"\n"),
        (
            "a.php",
            "<?php class Known {} new Known(); new Missing();",
        ),
    ]);
    assert!(identifiers.contains("CEL0018"), "{identifiers:?}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli`
Expected: compile error on `severity_remap`, then assertion failures (the severity does not change).

- [ ] **Step 3: Implement**

In `crates/celerrate_cli/src/configuration.rs`:

```rust
/// The `[severity]` remap that actually applies: entries naming a
/// remappable identifier, keyed by identifier text. Resilience and
/// unknown entries were already reported (CEL0048/CEL0049) and must
/// not half-apply; an entry whose value failed to parse carries no
/// severity to apply.
pub fn severity_remap(loaded: Option<&LoadedConfiguration>) -> BTreeMap<String, Severity> {
    let Some(loaded) = loaded else {
        return BTreeMap::new();
    };
    let metadata: Vec<RuleMetadata> = celerrate_rules::core_rules()
        .into_iter()
        .map(|(metadata, _)| metadata)
        .collect();
    let known = known_sets(&metadata);
    loaded
        .configuration
        .severity
        .iter()
        .filter(|entry| {
            known
                .remappable_identifiers
                .contains(entry.identifier.value.as_str())
        })
        .filter_map(|entry| {
            entry
                .severity
                .as_ref()
                .map(|severity| (entry.identifier.value.clone(), severity.value))
        })
        .collect()
}
```

In `crates/celerrate_cli/src/session.rs`: the field

```rust
/// The `[severity]` remap the per-file composition applies
/// (`configuration::severity_remap`): identifier text to severity.
/// Empty without a file; shared with every `AnalysisInputs` clone.
pub severity_remap: Arc<BTreeMap<String, Severity>>,
```

computed in `start` after `loaded_configuration`:

```rust
let severity_remap = Arc::new(crate::configuration::severity_remap(
    loaded_configuration.as_ref(),
));
```

and passed in `inputs()`: `severity_remap: self.severity_remap.clone(),`.

In `crates/celerrate_cli/src/analysis.rs`: the field on `AnalysisInputs`

```rust
/// The `[severity]` remap, applied by the per-file composers below,
/// never inside a query: `persistable_diagnostics` and `typed_portion`
/// remap right where they filter suppressions, so the exit-code count,
/// the printed report, and the persisted verdict carry the same
/// severities by construction (spec section 2).
pub severity_remap: Arc<std::collections::BTreeMap<String, celerrate_diagnostics::Severity>>,
```

the helper:

```rust
/// Applies the `[severity]` remap in place. Only remappable
/// identifiers are in the map (`configuration::severity_remap`), so
/// resilience diagnostics cannot be touched here by construction.
fn apply_severity_remap(
    remap: &std::collections::BTreeMap<String, celerrate_diagnostics::Severity>,
    diagnostics: &mut [Diagnostic],
) {
    if remap.is_empty() {
        return;
    }
    for diagnostic in diagnostics {
        if let Some(&severity) = remap.get(diagnostic.id.as_str()) {
            diagnostic.severity = severity;
        }
    }
}
```

and the two call sites, in `persistable_diagnostics` and `typed_portion`, right before each builds its `FilteredPortion`:

```rust
apply_severity_remap(&inputs.severity_remap, &mut diagnostics);
```

Fill in the real severity literals in the tests from Step 1 (run one fixture, copy the rendered title shape). Finalize the `configuration.rs` module doc: the part 1 boundary paragraph is gone; the module now loads, validates, and derives (`rule_overrides`, `severity_remap`, `configuration_digest`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package celerrate_cli`
Expected: PASS, including `cache_equivalence` (the harness recomputes through `composed_diagnostics`, which now remaps on both paths).

- [ ] **Step 5: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(cli): apply the severity remap in the per-file composition"
```

---

### Task 7: `--watch` reloads and reports the configuration

**Files:**
- Modify: `crates/celerrate_cli/src/session.rs` (`is_project_manifest`, `rediscover`, a `refresh_configuration` helper)
- Modify: `crates/celerrate_cli/src/watch.rs` (`Watch::build` registers `celerrate.toml`; `completed_cycle` presents configuration diagnostics; `iteration`'s exit arm counts them)
- Modify: `crates/celerrate_cli/src/configuration.rs` (`merge_diagnostics`, `diagnostic_count`)
- Modify: `crates/celerrate_cli/src/lib.rs` (the check path reuses `merge_diagnostics`)
- Test: session tests in `crates/celerrate_cli/src/session.rs`, plus a watch test following the module's injected-event pattern

**Interfaces:**
- Consumes: Tasks 3 to 6 (the model helper, `core_registrations`, `severity_remap`, `configuration_digest`, the digest gating in `inputs()` and `persist_timed`).
- Produces:
  - `pub fn merge_diagnostics(session: &Session, outcome: &mut AnalysisOutcome) -> usize` and `pub fn diagnostic_count(session: &Session) -> usize` in `celerrate_cli::configuration`
  - `Session::rediscover` reloads `celerrate.toml` (so a save of it, `composer.json`, or `composer.lock` reconfigures the session)

- [ ] **Step 1: Write the failing session tests**

In the `tests` module of `crates/celerrate_cli/src/session.rs`:

```rust
#[test]
fn absorbing_a_configuration_change_reconfigures_the_session() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        ("src/Kernel.php", "<?php namespace App; class Kernel {}"),
    ]);
    let mut session = Session::start(root.path());
    let original_digest = session.configuration_digest;

    let configuration = root.path().join("celerrate.toml");
    std::fs::write(
        &configuration,
        "[project]\nphp = \"8.3\"\n\n[rules.null-dereference]\nenabled = false\n",
    )
    .unwrap();
    session.absorb(&[configuration]);

    assert_eq!(
        session.configuration.php_version_range(&session.database),
        celerrate_project::PhpVersionRange::point(PhpVersion::new(8, 3)),
        "the php override reached the salsa input",
    );
    assert_eq!(
        session.configuration_digest, original_digest,
        "the [project] table does not move the digest",
    );
    let registry = celerrate_rules::RuleRegistry::try_get(&session.database)
        .expect("the registry is set at startup");
    let null_dereference = registry
        .registrations(&session.database)
        .iter()
        .find(|registration| registration.metadata.name == "null-dereference")
        .expect("the rule is registered");
    assert!(!null_dereference.active, "the disable reached the registry");
}

#[test]
fn absorbing_a_rules_change_moves_the_digest_and_gates_the_cache() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        ("src/Kernel.php", "<?php namespace App; class Kernel {}"),
    ]);
    let mut session = Session::start(root.path());
    let original_digest = session.configuration_digest;

    let configuration = root.path().join("celerrate.toml");
    std::fs::write(&configuration, "[rules.null-dereference]\nenabled = false\n").unwrap();
    session.absorb(&[configuration]);

    assert_ne!(session.configuration_digest, original_digest);
    assert_eq!(
        session.cache_loaded_configuration_digest, original_digest,
        "the loaded snapshot keeps its own digest, so inputs() serves nothing stale",
    );
}

#[test]
fn a_deleted_configuration_file_returns_the_session_to_defaults() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        ("celerrate.toml", "[rules.null-dereference]\nenabled = false\n"),
        ("src/Kernel.php", "<?php namespace App; class Kernel {}"),
    ]);
    let mut session = Session::start(root.path());
    assert!(session.loaded_configuration.is_some());

    let configuration = root.path().join("celerrate.toml");
    std::fs::remove_file(&configuration).unwrap();
    session.absorb(&[configuration]);

    assert!(session.loaded_configuration.is_none());
    let registry = celerrate_rules::RuleRegistry::try_get(&session.database).unwrap();
    assert!(
        registry
            .registrations(&session.database)
            .iter()
            .all(|registration| registration.active),
        "every Default-tier rule is active again",
    );
}
```

(Adjust `try_get`/`registrations` accessor spellings to the real salsa-generated API in `celerrate_rules::registry`; the intent is fixed, the accessor name follows the crate.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package celerrate_cli --lib session`
Expected: FAIL (a `celerrate.toml` change is not a manifest event; nothing reconfigures).

- [ ] **Step 3: Implement the reload**

In `crates/celerrate_cli/src/session.rs`:

```rust
fn is_project_manifest(&self, path: &Path) -> bool {
    let root = &self.discovery.root;
    path == root.join("composer.json")
        || path == root.join("composer.lock")
        || path == root.join("celerrate.toml")
}
```

`rediscover` reloads the configuration first and refreshes everything derived from it:

```rust
/// A changed manifest, lockfile, or `celerrate.toml` re-runs discovery
/// under the freshly reloaded configuration and rebuilds everything
/// derived from it: the walk, the version range, the active set, the
/// severity remap, and the cache digest. Configuration changes are
/// rare; whole invalidation is the accepted cost (spec, rejected
/// approach C).
fn rediscover(&mut self) {
    let root = self.discovery.root.clone();
    self.loaded_configuration = crate::configuration::load(&root, &mut self.vfs);
    let model = self.configuration_model();
    let discovery = discover(&root, &model);
    if discovery.php_version_range != self.discovery.php_version_range {
        self.configuration
            .set_php_version_range(&mut self.database)
            .to(discovery.php_version_range);
    }
    self.discovery = discovery;
    self.refresh_configuration(&model);
    let walk = enumerate_php_files(&self.discovery.walk_roots(), &self.discovery.excluded_roots);
    self.load(&walk);
}

/// Refreshes the registration-time consumers of the configuration:
/// the active set (through the registry setter, only when it actually
/// moved, because setting a HIGH-durability input invalidates the
/// world), the severity remap, and the digest packs are keyed on.
fn refresh_configuration(&mut self, model: &celerrate_config::Configuration) {
    let overrides = crate::configuration::rule_overrides(self.loaded_configuration.as_ref());
    let desired = crate::plugins::core_registrations(&overrides);
    let desired_shape: Vec<(String, bool)> = desired
        .iter()
        .map(|registration| (registration.metadata.name.clone(), registration.active))
        .collect();
    let current_shape: Option<Vec<(String, bool)>> =
        celerrate_rules::RuleRegistry::try_get(&self.database).map(|registry| {
            registry
                .registrations(&self.database)
                .iter()
                .map(|registration| (registration.metadata.name.clone(), registration.active))
                .collect()
        });
    if current_shape.as_deref() != Some(desired_shape.as_slice())
        && let Some(registry) = celerrate_rules::RuleRegistry::try_get(&self.database)
    {
        registry.set_registrations(&mut self.database).to(desired);
    }
    self.severity_remap = Arc::new(crate::configuration::severity_remap(
        self.loaded_configuration.as_ref(),
    ));
    self.configuration_digest = crate::configuration::configuration_digest(model);
}
```

(`Session::start` may also route its registration through `refresh_configuration`-shaped code, but the builder-versus-setter split makes that awkward; keeping `start` on `register_core_rules` and the reload on the setter is fine, the shared truth is `core_registrations`.)

Run: `cargo test --package celerrate_cli --lib session`
Expected: the three tests PASS.

- [ ] **Step 4: The watch observes the file and reports its diagnostics**

In `crates/celerrate_cli/src/watch.rs`, `Watch::build`, the manifest registration loop becomes:

```rust
for manifest in ["composer.json", "composer.lock", "celerrate.toml"] {
```

(The surrounding comment already explains `Declared::ByNobody` and the recorded-refusal retry, which now also covers a `celerrate.toml` created mid-session.)

In `crates/celerrate_cli/src/configuration.rs`, the presentation helpers:

```rust
/// Merges the configuration diagnostics into a presentation outcome
/// and answers how many were merged. Presentation and exit-code input
/// only, never cache input: callers keep the analysis outcome pure and
/// merge into a copy.
pub fn merge_diagnostics(
    session: &crate::session::Session,
    outcome: &mut crate::analysis::AnalysisOutcome,
) -> usize {
    let Some(loaded) = &session.loaded_configuration else {
        return 0;
    };
    outcome.diagnostics.extend(loaded.diagnostics.iter().cloned());
    outcome.diagnostics.sort();
    loaded.diagnostics.len()
}

/// The configuration diagnostics' contribution to the exit code.
pub fn diagnostic_count(session: &crate::session::Session) -> usize {
    session
        .loaded_configuration
        .as_ref()
        .map(|loaded| loaded.diagnostics.len())
        .unwrap_or(0)
}
```

In `crates/celerrate_cli/src/lib.rs`, replace the inline `configuration_diagnostics` match in the check path with:

```rust
let configuration_diagnostics = configuration::merge_diagnostics(&session, &mut presented);
```

(keep the existing comment about presentation versus cache input; the `Outcome::of` line is unchanged).

In `crates/celerrate_cli/src/watch.rs`, `completed_cycle` renders a presented copy:

```rust
// Configuration diagnostics are part of every picture, exactly as in
// a single check: merged into a presentation copy, never into the
// outcome the persisted verdicts read.
let mut presented = outcome.clone();
let _ = crate::configuration::merge_diagnostics(session, &mut presented);
if render::render_cycle(
    output,
    session,
    &presented,
    reanalyzed,
    started.elapsed(),
    color,
    height,
)
.is_err()
{
    return Err(Outcome::InternalError);
}
```

and `iteration`'s graceful-exit arm counts them:

```rust
return ControlFlow::Break(Outcome::of(
    outcome.diagnostics.len() + crate::configuration::diagnostic_count(session),
    session.internal_errors.len(),
));
```

- [ ] **Step 5: The watch integration test**

Following the injected-event pattern of the existing `watch.rs` tests (a held sender cell driving `iteration`, see `a_walk_root_that_was_missing_and_now_exists_is_watched_and_mapped` and its helpers), add one end-to-end test:

```rust
/// The part 2 wiring promise: a `celerrate.toml` saved mid-watch
/// reconfigures the very next cycle. The project fires CEL0034; a
/// saved configuration disabling `null-dereference` must make the next
/// picture clean, and deleting it must bring the diagnostic back.
#[test]
fn a_configuration_saved_mid_watch_reconfigures_the_next_cycle() {
    // Fixture: composer.json (psr-4 App -> src/), src/Consumer.php with
    // the ?User dereference fixture (CEL0034).
    // Drive: start session, run one cycle picture (assert CEL0034 in
    // the rendered output), write celerrate.toml disabling the rule,
    // inject the Changed event for it, run the next iteration, assert
    // the new picture does not contain CEL0034 and does not contain
    // any CEL004x configuration diagnostic.
    // Use the module's own test helpers for driving `iteration` with a
    // held sender; assert on the rendered output buffer.
}
```

Write it against the real helpers in the module (they exist; mirror the closest existing test's structure rather than inventing a new driver).

- [ ] **Step 6: Run the full package suite**

Run: `cargo test --package celerrate_cli`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/celerrate_cli
git commit -m "✨ feat(watch): reload celerrate.toml and report its diagnostics each cycle"
```

---

### Task 8: Verification: gates, corpus parity, closure evidence

**Files:**
- None to create; this task runs the gates and fixes only what they surface.

**Interfaces:**
- Consumes: everything above.
- Produces: the part 2 closure evidence (spec closure gates 1 and 2, and the untouched invariants).

- [ ] **Step 1: The mechanical suite**

Run, in order, each expected clean:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

- [ ] **Step 2: The xtask gates**

```bash
cargo xtask dependency-shape
cargo xtask emission-scan
```

Expected: both PASS. `celerrate_project` gaining `celerrate_config` does not enter dependency-shape's governed set (it does not depend on the plugin facade); if the gate names anything, stop and read its message rather than editing the gate.

- [ ] **Step 3: Zero-config parity on the corpus (closure gate 1) and the type baseline**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: the corpus snapshot byte-identical to the committed one (the corpus has no `celerrate.toml`; this run is the parity proof for the whole wiring), and `mixed-rate` unchanged (no type work happened, it cannot move by construction). Any delta is a stop-and-investigate, never a re-bless.

- [ ] **Step 4: Sweep the stale part-boundary comments**

Grep for the part 1 boundary markers and confirm none survive incorrectly:

```bash
rg -n "part 2|Part 2|part 1|Part 1" crates/celerrate_cli/src crates/celerrate_config/src
```

Expected: no comment still claims the configuration "is not yet consumed" or that `--watch` "does not reload it yet". Fix any stragglers.

- [ ] **Step 5: Commit (only if something was fixed)**

```bash
git commit -m "🔧 chore(cli): settle the configuration wiring after the workspace gates"
```

---

## Self-review notes (already applied)

- Spec coverage, section 2's wiring list: include/exclude and the `php` override into `celerrate_project` (Tasks 1 to 3), the active set and the severity remap at the composition root (Tasks 5 and 6), force-activation with the `explain_pages.rs` guard-test update (Task 5), the digest in the reserved cache-header field (Task 4), warm/cold extended (Tasks 4 to 6 tests), `--watch` reload and reporting parity (Task 7, closing the comment part 1 left in `session.rs`).
- Ordering is deliberate: the digest (Task 4) lands **before** the active set (Task 5) and the remap (Task 6), because both change what verdicts mean, and a pack written under one configuration must never be served under another, even between adjacent commits.
- The digest covers the whole normalized `[rules]` and `[severity]` sections, not the derived active set, per the spec's future-options argument; `[project]` stays out (the range is already a header field, file membership is per-entry), pinned by `the_project_table_does_not_move_the_digest`.
- Type consistency across tasks: `discover(root, &Configuration)` (Task 1) is what Task 3 and Task 7 call; `core_registrations`/`rule_is_active`/`register_core_rules(db, overrides)` (Task 5) are what Task 7's setter path reuses; `configuration_digest` (Task 4) is what `Session::start`, `persist_timed`, and `refresh_configuration` all call; `severity_remap` returns `BTreeMap<String, Severity>` everywhere it appears.
- Known judgment calls, stated: the `php` override suppresses the manifest detection notices (the constraint the resolution never used is not news); `celerrate.toml` is interned first (the old "intern after the walk" comment is replaced with the reasoning); the severity literals in two Task 6 tests are copied from the real renderer output on first run rather than guessed in this plan.
- Placeholder scan: the only deliberately deferred literals are those two renderer severity strings and the Task 7 watch test body, each with explicit instructions to derive them from the running code and existing helpers, not to skip them.
