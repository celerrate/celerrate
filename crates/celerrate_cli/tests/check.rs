//! The product, end to end. `run` takes its arguments and its output as
//! values, so these drive the whole thing in process: no spawning, no
//! timing flakiness, and the rendering pinned exactly.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::{ColorMode, Outcome, run};

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

fn check(root: &Path) -> (Outcome, String) {
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()],
        &mut output,
        ColorMode::Plain,
    );
    (outcome, String::from_utf8(output).unwrap())
}

/// Rewrites the location token of every finding line to forward slashes.
///
/// The report prints paths in the platform's native spelling: on Windows
/// the walk hands back `src\Kernel.php`, and that spelling is the right
/// one to show a Windows user. A committed snapshot can pin only one
/// spelling, so the location token, and only that token, is normalized
/// before comparing. The rest of the line stays untouched: PHP namespace
/// separators are backslashes too, and `App\Missing` in a message is not
/// a path.
fn normalize_location_separators(text: &str) -> String {
    text.lines()
        .map(|line| match line.split_once(' ') {
            Some((location, rest)) if location.contains(':') => {
                format!("{} {rest}", location.replace('\\', "/"))
            }
            _ => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_clean_project_reports_nothing_and_exits_zero() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        ("src/Kernel.php", "<?php\nnamespace App;\nclass Kernel {}\n"),
    ]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean);
    insta::assert_snapshot!("clean", text);
}

#[test]
fn a_project_with_findings_renders_notices_diagnostics_and_a_summary() {
    // Zero configuration: no manifest, no PHP version. Both fall back,
    // both say so, and neither blocks.
    let root = project(&[(
        "src/Kernel.php",
        "<?php\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    insta::assert_snapshot!("findings", normalize_location_separators(&text));
}

/// A typo'd path that exits 0 is the one thing a CI-facing checker must
/// never do: the build goes green over a project nothing ever looked at.
/// It used to fall through to zero-configuration discovery, announce that
/// it was analyzing a directory it had never been handed, analyze nothing,
/// and succeed.
#[test]
fn a_root_that_does_not_exist_is_a_usage_error_that_names_it() {
    let mut output = Vec::new();
    let outcome = run(
        vec![
            "celerrate".into(),
            "check".into(),
            "/nonexistent/path/xyz".into(),
        ],
        &mut output,
        ColorMode::Plain,
    );
    let text = String::from_utf8(output).unwrap();

    assert_eq!(outcome, Outcome::UsageError, "{text}");
    assert!(
        text.contains("/nonexistent/path/xyz"),
        "the message names the path the user gave: {text}",
    );
    assert!(
        !text.contains("CEL0025"),
        "and it does not announce a fallback it never took: {text}",
    );
}

#[test]
fn a_root_that_is_a_file_rather_than_a_directory_is_a_usage_error() {
    let root = project(&[("a.php", "<?php echo 1;")]);
    let file = root.path().join("a.php");
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "check".into(), file.as_os_str().into()],
        &mut output,
        ColorMode::Plain,
    );
    let text = String::from_utf8(output).unwrap();

    assert_eq!(outcome, Outcome::UsageError, "{text}");
    assert!(text.contains("a.php"), "{text}");
}

/// Permissions are the only portable way to make a directory unreadable,
/// and only Unix has them in the form this needs.
#[cfg(unix)]
#[test]
fn a_subdirectory_that_cannot_be_read_is_reported_and_the_rest_still_analyzed() {
    // A root that cannot be read is a usage error: nothing can be
    // analyzed. A subdirectory that cannot be read is not, because the
    // rest of the project still can. But it must not be skipped in
    // silence either: that was a green build over a project only half
    // read. So the run reports it, analyzes everything it could reach,
    // and exits 2, because it did not analyze the whole project.
    use std::os::unix::fs::PermissionsExt as _;

    let root = project(&[
        ("src/Kernel.php", "<?php\nclass Kernel {}\n"),
        ("src/locked/Hidden.php", "<?php\nclass Hidden {}\n"),
    ]);
    let locked = root.path().join("src/locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let (outcome, text) = check(root.path());

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(outcome, Outcome::InternalError, "{text}");
    assert!(
        text.contains("src/locked") && text.contains("could not be read"),
        "the directory it could not look inside is named, with the reason: {text}",
    );
    assert!(
        !text.contains("Please report it"),
        "a permissions problem is the environment's, not a bug in Celerrate: {text}",
    );
}

#[cfg(unix)]
#[test]
fn a_root_that_cannot_be_read_is_a_usage_error() {
    // `is_dir` succeeds on a directory whose contents cannot be listed:
    // the stat goes through the parent. So the walk yielded nothing, the
    // run reported nothing, and it exited 0. A green build over a project
    // nothing looked at is the one failure a checker must never have.
    use std::os::unix::fs::PermissionsExt as _;

    let root = project(&[("src/Kernel.php", "<?php class Kernel {}")]);
    let unreadable = root.path().join("src");
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();

    let mut output = Vec::new();
    let outcome = run(
        vec![
            "celerrate".into(),
            "check".into(),
            unreadable.as_os_str().into(),
        ],
        &mut output,
        ColorMode::Plain,
    );
    let text = String::from_utf8(output).unwrap();

    // Restore before asserting, so a failure does not leave the temporary
    // directory undeletable.
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(outcome, Outcome::UsageError, "{text}");
    assert!(
        text.contains("src"),
        "the message names the path the user gave: {text}",
    );
    assert!(
        !text.contains("0 diagnostics"),
        "it must not report a clean run over a directory it never read: {text}",
    );
}

#[test]
fn notices_alone_are_not_a_failure() {
    // Every notice announces a fallback already taken. Zero-configuration
    // must never block, so notices never touch the exit code.
    let root = project(&[("src/Kernel.php", "<?php\nnamespace App;\nclass Kernel {}\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean);
    assert!(
        text.contains("CEL0025"),
        "the missing manifest is announced"
    );
    assert!(text.contains("0 diagnostics"));
}

#[test]
fn a_notice_announces_itself_as_a_notice() {
    // One screen must not use one word two ways. A notice is counted as a
    // notice in the summary and never touches the exit code, so calling it
    // a warning on the line above contradicts both: a warning diagnostic
    // exits 1, and this does not.
    let root = project(&[("src/Kernel.php", "<?php\nnamespace App;\nclass Kernel {}\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean);
    assert!(
        text.contains("notice CEL0025: "),
        "the notice names itself: {text}",
    );
    assert!(
        !text.contains("warning"),
        "nothing that leaves the exit code at zero calls itself a warning: {text}",
    );
}

/// A real Composer project has thousands of third-party files, and a
/// report dominated by their findings is no report at all: they are not
/// the user's code, not the user's to fix, and failing the build on them
/// is failing it on someone else's work.
///
/// They must still be analyzed, though. Their symbols are exactly what
/// makes `use Acme\Thing;` resolve, which is what the `Kernel` here
/// proves: drop the vendor files from the analyzed set and it reports an
/// unknown class instead, so this test cannot pass by silencing vendor
/// the lazy way.
///
/// And the count that reaches the exit code must be the count that was
/// printed. A vendor finding that exits 1 over an empty report is worse
/// than either half of the bug.
#[test]
fn a_finding_in_vendor_is_analyzed_but_never_reported() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        (
            "vendor/composer/installed.json",
            r#"{"packages": [{"name": "acme/lib", "install-path": "../acme/lib",
               "autoload": {"psr-4": {"Acme\\": "src/"}}}]}"#,
        ),
        (
            "vendor/acme/lib/src/Broken.php",
            "<?php\nnamespace Acme;\nclass Broken extends TotallyMissing {}\n",
        ),
        (
            "vendor/acme/lib/src/Thing.php",
            "<?php\nnamespace Acme;\nclass Thing {}\n",
        ),
        (
            "src/Kernel.php",
            "<?php\nnamespace App;\nuse Acme\\Thing;\nclass Kernel extends Thing {}\n",
        ),
    ]);
    let (outcome, text) = check(root.path());

    assert!(
        !text.contains("TotallyMissing"),
        "a third-party finding is not the user's to fix: {text}",
    );
    assert!(
        !text.contains("vendor"),
        "nothing from vendor reaches the report at all: {text}",
    );
    assert!(text.contains("0 diagnostics"), "{text}");
    assert_eq!(
        outcome,
        Outcome::Clean,
        "the count the exit code is derived from is the count that was printed: {text}",
    );
}

#[test]
fn a_warning_alone_still_exits_one() {
    // 1 means "any diagnostic reported", warning or error alike.
    // `utf8_encode` is deprecated since PHP 8.2 (a warning, CEL0023), and
    // the shipped stub blob carries that deprecation: `^8.1` admits the
    // supported range [8.1, 8.5], whose maximum (8.5) is past 8.2, so the
    // deprecation always applies here.
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        (
            "src/Legacy.php",
            "<?php\nnamespace App;\nfunction f(): void { \\utf8_encode('x'); }\n",
        ),
    ]);
    let (outcome, text) = check(root.path());
    assert!(
        text.contains("CEL0023"),
        "the deprecation warning fires: {text}"
    );
    assert_eq!(outcome, Outcome::DiagnosticsReported);
}

/// Builds the relative spelling of `target` as seen from the current
/// directory, purely lexically: enough `..` to climb out of every
/// normal component of the current directory, then the target without
/// its root. Unix-only, like its caller: on Windows the two paths can
/// sit on different drives, where no relative spelling exists.
#[cfg(unix)]
fn relative_from_current_directory(target: &Path) -> std::path::PathBuf {
    let current = std::env::current_dir().unwrap();
    let mut relative = std::path::PathBuf::new();
    for component in current.components() {
        if matches!(component, std::path::Component::Normal(_)) {
            relative.push("..");
        }
    }
    relative.push(target.strip_prefix("/").unwrap());
    relative
}

/// A relative project root analyzes exactly like its absolute
/// spelling. It used to analyze nothing: discovery's zero-configuration
/// fallback self-joined the relative root (`project` became
/// `project/project`, `.` the empty path), the walk found no such
/// directory, and the run exited 0 under an untrue notice - a green
/// build over a project nothing looked at, from the exact command the
/// README's quick start names. The lexical absolutization itself is
/// covered on every platform by the `normalize_path` tests; this pins
/// the command line actually applying it.
#[cfg(unix)]
#[test]
fn a_relative_root_analyzes_like_its_absolute_spelling() {
    let root = project(&[("index.php", "<?php\nmissing_function();\n")]);
    let (absolute_outcome, absolute_text) = check(root.path());
    let (relative_outcome, relative_text) = check(&relative_from_current_directory(root.path()));
    assert!(
        relative_text.contains("CEL0019"),
        "the unknown function is reported through the relative spelling: {relative_text}",
    );
    assert_eq!(relative_outcome, Outcome::DiagnosticsReported);
    assert_eq!(relative_outcome, absolute_outcome);
    assert_eq!(
        relative_text, absolute_text,
        "both spellings name the same project, so they print the same report",
    );
}

/// The typed families (CEL0030-CEL0038) render through `check` exactly
/// like the untyped ones: same command, same report, no separate flag.
#[test]
fn the_typed_families_render_through_check() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        (
            "src/Service.php",
            r#"<?php
declare(strict_types=1);
namespace App;

class User
{
    public function save(): void
    {
    }
}

class Service
{
    public function run(?User $user): void
    {
        $user->save();
        $user?->svae();
    }
}
"#,
        ),
    ]);
    let (outcome, output) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    let output = normalize_location_separators(&output);
    assert!(
        output.contains("CEL0034"),
        "the null dereference renders: {output}"
    );
    assert!(
        output.contains("CEL0030"),
        "the unknown method renders: {output}"
    );
    assert!(
        output.contains("accessing `save` on a possibly null `App\\User|null`"),
        "{output}",
    );
}

/// The pack serves only the untyped verdict (decision 13): the typed
/// families are recomputed fresh on every path, cold or warm. A warm
/// run must still report the same typed diagnostics as the cold run
/// that produced the cache.
#[test]
fn a_warm_run_reports_the_same_typed_diagnostics() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        (
            "src/Service.php",
            r#"<?php
declare(strict_types=1);
namespace App;

class User
{
    public function save(): void
    {
    }
}

class Service
{
    public function run(?User $user): void
    {
        $user->save();
        $user?->svae();
    }
}
"#,
        ),
    ]);
    let (_, cold) = check(root.path());
    let (_, warm) = check(root.path());
    assert_eq!(
        normalize_location_separators(&cold),
        normalize_location_separators(&warm),
        "the pack serves untyped verdicts; typed recompute must agree",
    );
}

/// The presentation-time did-you-mean surfaces in the plain report: a
/// `help:` line under the diagnostic that owns the suggestion. This is
/// the minimal pre-part-7 rendering; the rich renderer replaces it.
#[test]
fn a_near_typo_renders_a_help_line_under_its_diagnostic() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        (
            "src/User.php",
            "<?php\nnamespace App;\nclass User { public function save(): void {} }\n",
        ),
        (
            "src/Caller.php",
            "<?php\nnamespace App;\nfunction persist(User $user): void { $user->svae(); }\n",
        ),
    ]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    insta::assert_snapshot!("help_line", normalize_location_separators(&text));
}
