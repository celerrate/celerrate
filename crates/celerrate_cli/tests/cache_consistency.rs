//! The cross-process extension of the incremental correctness harness:
//! edit sequences replayed over a project on disk, with every
//! cache-seeded run asserted byte-for-byte identical to a from-scratch
//! run over the same state. Nothing survives between runs except
//! `.celerrate/cache/`, which is exactly the boundary under test.

#![allow(clippy::unwrap_used)]
#![allow(clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::{ColorMode, run};

fn run_check(root: &Path) -> String {
    let mut output = Vec::new();
    let _ = run(
        vec![
            "celerrate".into(),
            "check".into(),
            root.as_os_str().to_owned(),
        ],
        &mut output,
        ColorMode::Plain,
    );
    String::from_utf8(output).unwrap()
}

/// The rendering is root-relative, but notices and internal errors may
/// name absolute paths: normalize both roots to one marker before
/// comparing.
fn normalized(output: &str, root: &Path) -> String {
    output.replace(&root.display().to_string(), "<root>")
}

/// Copies the project, excluding the cache: the from-scratch twin.
fn copy_without_cache(source: &Path, destination: &Path) {
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == ".celerrate" {
            continue;
        }
        let target = destination.join(&name);
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_without_cache(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// One step of an edit sequence.
enum Step {
    Write(&'static str, &'static str),
    Delete(&'static str),
}

/// Replays the steps over one cached project directory; after the
/// initial state and after every step, the cached run must render what
/// a from-scratch run over a cache-free copy renders.
fn assert_cached_matches_fresh(initial: &[(&str, &str)], steps: &[Step]) {
    let cached = tempfile::tempdir().unwrap();
    for (path, contents) in initial {
        let path = cached.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    let assert_state_matches = |label: &str| {
        let cached_output = run_check(cached.path());
        let fresh = tempfile::tempdir().unwrap();
        copy_without_cache(cached.path(), fresh.path());
        let fresh_output = run_check(fresh.path());
        assert_eq!(
            normalized(&cached_output, cached.path()),
            normalized(&fresh_output, fresh.path()),
            "cached and from-scratch renderings diverged {label}",
        );
    };

    // The first run both checks the cold state and writes the cache;
    // the second checks the warm no-change state.
    assert_state_matches("on the cold state");
    assert_state_matches("on the warm unchanged state");

    for (index, step) in steps.iter().enumerate() {
        match step {
            Step::Write(path, contents) => {
                let path = cached.path().join(path);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(path, contents).unwrap();
            }
            Step::Delete(path) => {
                std::fs::remove_file(cached.path().join(path)).unwrap();
            }
        }
        assert_state_matches(&format!("after step {index}"));
    }
}

#[test]
fn body_and_comment_edits_replay_consistently() {
    assert_cached_matches_fresh(
        &[
            (
                "src/Service.php",
                "<?php class Service { public function run() { return 1; } }",
            ),
            ("src/User.php", "<?php class User {}"),
        ],
        &[
            Step::Write(
                "src/Service.php",
                "<?php class Service { public function run() { return 2; } }",
            ),
            Step::Write(
                "src/Service.php",
                "<?php /* documented */ class Service { public function run() { return 2; } }",
            ),
        ],
    );
}

/// The stale-verdict trap in both directions: a cached unknown-symbol
/// diagnostic must die when a defining file appears, and come back
/// when it goes.
#[test]
fn a_definition_appearing_and_vanishing_replays_consistently() {
    assert_cached_matches_fresh(
        &[("src/Consumer.php", "<?php new Missing();")],
        &[
            Step::Write("src/Definer.php", "<?php class Missing {}"),
            Step::Delete("src/Definer.php"),
        ],
    );
}

/// The exact case the architecture audit named: a `define()` call the
/// item traversal cannot see, inside a method body, is the whole reason
/// `define()`-detected names now ride on the `ItemTree` as a separate,
/// range-free list rather than through a per-file query with no
/// persisted artifact. This replays it appearing, being edited (the
/// define's value changes, its name does not, so the item tree's
/// `defines` list is unchanged and the table must backdate), and
/// vanishing again, across process restarts.
#[test]
fn a_body_level_define_appearing_being_edited_and_vanishing_replays_consistently() {
    assert_cached_matches_fresh(
        &[("src/Consumer.php", "<?php echo APP_ROOT;")],
        &[
            Step::Write(
                "src/Definer.php",
                "<?php function boot() { define('APP_ROOT', 1); } boot();",
            ),
            Step::Write(
                "src/Definer.php",
                "<?php function boot() { define('APP_ROOT', 2); } boot();",
            ),
            Step::Delete("src/Definer.php"),
        ],
    );
}

/// A signature-level edit in one file must be seen by the cached
/// verdicts of another: renaming the declared class flips its
/// consumers' resolution.
#[test]
fn a_rename_in_another_file_replays_consistently() {
    assert_cached_matches_fresh(
        &[
            ("src/Consumer.php", "<?php new Widget();"),
            ("src/Widget.php", "<?php class Widget {}"),
        ],
        &[
            Step::Write("src/Widget.php", "<?php class Renamed {}"),
            Step::Write("src/Widget.php", "<?php class Widget {}"),
        ],
    );
}

/// Composer projects: a vendor file's symbols resolve from the cache
/// like from source, and vendor diagnostics stay unreported.
///
/// `ProjectDiscovery` only walks a vendor package that `installed.json`
/// actually declares (name, `install-path`, autoload): an empty package
/// list, as the task brief's fixture had it, walks nothing, so the
/// vendor file would neither define `Helper` nor be parsed at all,
/// defeating the fixture's intent. This mirrors the package shape
/// `crates/celerrate_project/tests/discovery_end_to_end.rs` and
/// `crates/celerrate_cli/tests/check.rs` use for an installed
/// dependency, with `install-path` chosen so the walk root lands on
/// exactly `vendor/lib/src`, keeping the brief's file path unchanged.
#[test]
fn a_composer_project_replays_consistently() {
    assert_cached_matches_fresh(
        &[
            (
                "composer.json",
                r#"{"require": {"php": "^8.2"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
            ),
            (
                "vendor/lib/src/Helper.php",
                "<?php namespace Lib; class Helper { public function broken( }",
            ),
            (
                "vendor/composer/installed.json",
                r#"{"packages": [{"name": "acme/lib", "install-path": "../lib",
                   "autoload": {"psr-4": {"Lib\\": "src/"}}}]}"#,
            ),
            (
                "src/App.php",
                "<?php namespace App; use Lib\\Helper; new Helper();",
            ),
        ],
        &[Step::Write(
            "src/App.php",
            "<?php namespace App; use Lib\\Helper; new Helper(); new Gone();",
        )],
    );
}

/// Builds `files` in a fresh, cache-free directory and renders one
/// `check` pass. Used only as an independent sanity check that a
/// fixture's "before" and "after" states genuinely produce different
/// typed diagnostics — `assert_cached_matches_fresh` below is what
/// proves cache correctness; this is what proves the fixture is not
/// vacuous.
fn fresh_render(files: &[(&str, &str)]) -> String {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let path = root.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    run_check(root.path())
}

// ---------------------------------------------------------------------
// The consistency harness extended over the new typed
// edit classes. Every test below drives the SAME cross-process harness
// (`assert_cached_matches_fresh`) the untyped edit classes above already
// use, so a cache-seeded warm run must render exactly what a from-scratch
// run over the same on-disk state renders, at every step — including the
// step where the typed verdict actually flips.
//
// Scope note: the default-value edit class (a parameter's default
// changing the DECLARED signature, e.g. `= null` -> `= 1`) belongs to
// the in-process invalidation-scope suite and is already pinned by
// `celerrate_types/tests/invalidation_scope.rs` (`a_default_value_edit_
// changes_the_declared_signature` and its neighbors); it is
// deliberately left OUT of this cross-process suite to avoid duplicating
// that coverage under a slower harness.
// ---------------------------------------------------------------------

/// An inferred-return change in one file flips a typed verdict in
/// another: `helper()` carries no declared or annotated return, so
/// `caller.php`'s dereference of its result depends on the INFERRED
/// tier. Flipping `helper()`'s body from `return 1;` to `return null;`
/// must flip the caller's CEL0034 (a possibly-null dereference) on the
/// warm run exactly as it does fresh.
#[test]
fn an_inferred_return_change_replays_consistently() {
    let helper_before = "<?php function helper() { return 1; }";
    let helper_after = "<?php function helper() { return null; }";
    let caller = "<?php class Widget { public function inspect(): void {} } \
         function useIt() { $w = helper(); $w->inspect(); }";

    assert!(
        !fresh_render(&[("helper.php", helper_before), ("caller.php", caller)]).contains("CEL0034"),
        "sanity: helper() returning 1 must not trip the nullability check",
    );
    assert!(
        fresh_render(&[("helper.php", helper_after), ("caller.php", caller)]).contains("CEL0034"),
        "sanity: helper() returning null must trip the nullability check",
    );

    assert_cached_matches_fresh(
        &[("helper.php", helper_before), ("caller.php", caller)],
        &[Step::Write("helper.php", helper_after)],
    );
}

/// A signature edit in one file flips an argument-type verdict in
/// another: `takes`'s parameter type moves from `int` to `string`,
/// which a caller passing an `int` literal under `strict_types` no
/// longer satisfies — CEL0035 on the warm run exactly as on fresh.
#[test]
fn a_signature_edit_replays_consistently() {
    let takes_before = "<?php function takes(int $n): void {}";
    let takes_after = "<?php function takes(string $n): void {}";
    let caller = "<?php declare(strict_types=1); function f() { takes(1); }";

    assert!(
        !fresh_render(&[("takes.php", takes_before), ("caller.php", caller)]).contains("CEL0035"),
        "sanity: an int argument against an int parameter must not trip the check",
    );
    assert!(
        fresh_render(&[("takes.php", takes_after), ("caller.php", caller)]).contains("CEL0035"),
        "sanity: an int argument against a string parameter must trip the check",
    );

    assert_cached_matches_fresh(
        &[("takes.php", takes_before), ("caller.php", caller)],
        &[Step::Write("takes.php", takes_after)],
    );
}

/// A docblock annotation appearing on a callee that previously carried
/// none: `findWidget()` gains an `@return ?Widget` docblock, moving its
/// callers from the inferred tier to the declared tier. The caller's
/// typed verdict must follow on the warm run exactly as on fresh.
///
/// This test used to FAIL and was `#[ignore]`d rather than adjusted
/// (report a genuine bug as a stop signal, never weaken the test that
/// found it). The root cause traced to `crates/celerrate_types/src/
/// flow.rs`: `function_call_result` (the free-function call site)
/// recorded a `StoredFunctionDependency` (`dependencies.functions.
/// insert(key)`) ONLY when `declared_present(signature)` already held
/// at persist time; when it did not (a `Trust::NativeOnly` signature
/// with a `mixed` value type, exactly an undocumented, unannotated
/// function), only the INFERRED edge was recorded (`dependencies.
/// inferred_functions.push((key, raw))`), carrying no fact about
/// whether the callee has a declared signature at all. Revalidation
/// only re-checks what was recorded, and `inferred_function_return`
/// never consults a docblock's `@return`, so a warm run kept serving
/// the stale pre-docblock verdict even after the docblock appeared.
/// `method_call_result_for_keys` and `projected_callable_of_function`
/// shared the identical pattern for methods and first-class callables.
///
/// The fix records the callee's declared-signature-guarding dependency
/// (a `functions`/`classes` entry, keyed on `function_signature_digest`/
/// `class_surface_digest`) at each of those three sites IN ADDITION to
/// the inferred edge, so a later presence flip is visible to
/// revalidation while an unchanged callee's digest still matches and
/// the warm verdict still serves.
#[test]
fn a_docblock_annotation_edit_replays_consistently() {
    let callee_before = "<?php namespace App; \
         class Widget { public function inspect(): void {} } \
         function findWidget() { return new Widget(); }";
    let callee_after = "<?php namespace App; \
         class Widget { public function inspect(): void {} } \
         /** @return ?Widget */ \
         function findWidget() { return new Widget(); }";
    let caller =
        "<?php namespace App; function useIt(): void { $w = findWidget(); $w->inspect(); }";

    assert!(
        !fresh_render(&[("callee.php", callee_before), ("caller.php", caller)]).contains("CEL0034"),
        "sanity: an undocumented findWidget() must not trip the nullability check",
    );
    assert!(
        fresh_render(&[("callee.php", callee_after), ("caller.php", caller)]).contains("CEL0034"),
        "sanity: the nullable @return must trip the caller's nullability check",
    );

    assert_cached_matches_fresh(
        &[("callee.php", callee_before), ("caller.php", caller)],
        &[Step::Write("callee.php", callee_after)],
    );
}

/// A class member addition in one file clears an unknown-method
/// verdict in another: `User` gains the `save` method `Consumer.php`
/// calls, exercising the class-surface digest path end to end.
#[test]
fn a_class_member_addition_replays_consistently() {
    let user_before = "<?php namespace App; class User {}";
    let user_after = "<?php namespace App; class User { public function save(): void {} }";
    let consumer = "<?php namespace App; function f(User $u): void { $u->save(); }";

    assert!(
        fresh_render(&[("user.php", user_before), ("consumer.php", consumer)]).contains("CEL0030"),
        "sanity: a missing save() must trip the unknown-method check",
    );
    assert!(
        !fresh_render(&[("user.php", user_after), ("consumer.php", consumer)]).contains("CEL0030"),
        "sanity: an added save() must clear the unknown-method check",
    );

    assert_cached_matches_fresh(
        &[("user.php", user_before), ("consumer.php", consumer)],
        &[Step::Write("user.php", user_after)],
    );
}

/// A virtual member's type edit in one file changes what a dependent in
/// another file sees: `Repository`'s class docblock `@method User
/// find()` becomes `@method Order find()`, and `Order` has no `save`
/// method, so the caller's chained `->save()` flips from silent to
/// CEL0030, the digest's virtual-member payload followed end to end,
/// warm exactly as fresh.
#[test]
fn a_virtual_member_type_edit_replays_consistently() {
    let repository_before = "<?php namespace App; /** @method User find() */ class Repository {}";
    let repository_after = "<?php namespace App; /** @method Order find() */ class Repository {}";
    let classes = "<?php namespace App; \
         class User { public function save(): void {} } \
         class Order {}";
    let consumer = "<?php namespace App; function f(Repository $r): void { $r->find()->save(); }";

    assert!(
        !fresh_render(&[
            ("classes.php", classes),
            ("repository.php", repository_before),
            ("consumer.php", consumer),
        ])
        .contains("CEL0030"),
        "sanity: find() returning User must not trip the unknown-method check",
    );
    assert!(
        fresh_render(&[
            ("classes.php", classes),
            ("repository.php", repository_after),
            ("consumer.php", consumer),
        ])
        .contains("CEL0030"),
        "sanity: find() returning Order must trip the unknown-method check",
    );

    assert_cached_matches_fresh(
        &[
            ("classes.php", classes),
            ("repository.php", repository_before),
            ("consumer.php", consumer),
        ],
        &[Step::Write("repository.php", repository_after)],
    );
}

/// Every corruption mode of a pack on disk regenerates silently: the
/// run's rendering never changes.
#[test]
fn corrupted_packs_never_change_the_rendering() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.php"), "<?php new Missing();").unwrap();
    let baseline = normalized(&run_check(root.path()), root.path());

    let cache = root.path().join(".celerrate/cache");
    for pack in ["item_trees.bin", "diagnostics.bin"] {
        let path = cache.join(pack);
        let original = std::fs::read(&path).unwrap();

        // Truncated.
        std::fs::write(&path, &original[..original.len() / 2]).unwrap();
        assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);

        // Garbage.
        std::fs::write(&path, b"not a pack at all").unwrap();
        assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);

        // A flipped byte deep in the payload.
        let mut flipped = std::fs::read(&path).unwrap();
        if let Some(last) = flipped.last_mut() {
            *last ^= 0xFF;
        }
        std::fs::write(&path, &flipped).unwrap();
        assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);
    }

    // After all that abuse the packs are healthy again: one more
    // clean run.
    assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);
}

/// The invariant that makes two concurrent `celerrate check` processes
/// safe today, named and pinned: both packs' entries
/// are independently content-keyed and revalidated, so packs from two
/// different generations of the project may be mixed freely — a stale
/// pack beside a fresh one must render exactly what a fresh run
/// renders. A future pack keyed over the *set* of tree hashes would
/// break exactly this; this test is the tripwire that says so.
#[test]
fn packs_from_different_generations_mix_safely() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.php"), "<?php new Missing();").unwrap();
    let _ = run_check(root.path());
    let cache = root.path().join(".celerrate/cache");
    let stale_verdicts = std::fs::read(cache.join("diagnostics.bin")).unwrap();
    let stale_trees = std::fs::read(cache.join("item_trees.bin")).unwrap();

    // Second generation: a defining file appears, and a full run
    // refreshes both packs and the expected rendering.
    std::fs::write(root.path().join("b.php"), "<?php class Missing {}").unwrap();
    let baseline = normalized(&run_check(root.path()), root.path());

    // Stale verdicts beside fresh trees: the stale entry's recorded
    // `Unknown` answer no longer holds, revalidation discards it.
    std::fs::write(cache.join("diagnostics.bin"), &stale_verdicts).unwrap();
    assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);

    // Stale trees beside fresh verdicts: the new file's tree is simply
    // absent from the stale pack, a miss, recomputed.
    std::fs::write(cache.join("item_trees.bin"), &stale_trees).unwrap();
    assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);
}
