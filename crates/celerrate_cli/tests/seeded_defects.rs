//! The seeded-defect recall suite: the gate a
//! silent engine cannot pass. One known defect per identifier; each
//! MUST be reported through the full product pipeline. These
//! fixtures are the family's substance contract; never weaken an
//! assertion to unblock a refactor.

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

const MANIFEST: &str = r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;

fn seeded(identifier: &str, source: &str) {
    let root = project(&[("composer.json", MANIFEST), ("src/Seed.php", source)]);
    let (outcome, output) = check(root.path());
    assert_eq!(
        outcome,
        Outcome::DiagnosticsReported,
        "{identifier} must be reported:\n{output}",
    );
    assert!(
        output.contains(identifier),
        "{identifier} must appear in the report:\n{output}",
    );
}

#[test]
fn cel0024_a_gated_construct_below_the_minimum_is_reported() {
    seeded(
        "CEL0024",
        r#"<?php
namespace App;
readonly class Point {}
"#,
    );
}

#[test]
fn cel0030_a_known_unknown_method_is_reported() {
    seeded(
        "CEL0030",
        r#"<?php
namespace App;
class User { public function save(): void {} }
function f(User $u): void { $u->svae(); }
"#,
    );
}

#[test]
fn cel0031_a_known_unknown_property_is_reported() {
    seeded(
        "CEL0031",
        r#"<?php
namespace App;
class User { public string $name = ''; }
function f(User $u): void { $x = $u->nmae; }
"#,
    );
}

#[test]
fn cel0032_a_known_unknown_class_constant_is_reported() {
    seeded(
        "CEL0032",
        r#"<?php
namespace App;
class Config { public const LIMIT = 10; }
function f(): int { return Config::LIMTI; }
"#,
    );
}

#[test]
fn cel0033_a_known_unknown_enum_case_is_reported() {
    seeded(
        "CEL0033",
        r#"<?php
namespace App;
enum Status { case Active; }
function f(): Status { return Status::Draft; }
"#,
    );
}

#[test]
fn cel0034_a_known_null_dereference_is_reported() {
    seeded(
        "CEL0034",
        r#"<?php
namespace App;
class User { public function save(): void {} }
function f(?User $u): void { $u->save(); }
"#,
    );
}

#[test]
fn cel0035_a_known_wrong_argument_is_reported() {
    seeded(
        "CEL0035",
        r#"<?php
declare(strict_types=1);
namespace App;
class Plain {}
function takes(int $n): void {}
function f(Plain $p): void { takes($p); }
"#,
    );
}

#[test]
fn cel0036_a_known_missing_argument_is_reported() {
    seeded(
        "CEL0036",
        r#"<?php
namespace App;
function pair(int $a, int $b): void {}
function f(): void { pair(1); }
"#,
    );
}

#[test]
fn cel0037_a_known_excess_argument_is_reported() {
    seeded(
        "CEL0037",
        r#"<?php
namespace App;
function single(int $a): void {}
function f(): void { single(1, 2); }
"#,
    );
}

#[test]
fn cel0038_a_known_unknown_named_argument_is_reported() {
    seeded(
        "CEL0038",
        r#"<?php
namespace App;
function single(int $a): void {}
function f(): void { single(b: 1); }
"#,
    );
}

#[test]
fn cel0018_a_known_unknown_class_is_reported() {
    seeded(
        "CEL0018",
        r#"<?php
namespace App;
function f(): void { $x = new MissingService(); }
"#,
    );
}

#[test]
fn cel0019_a_known_unknown_function_is_reported() {
    seeded(
        "CEL0019",
        r#"<?php
namespace App;
function f(): void { missing_helper(); }
"#,
    );
}

#[test]
fn cel0020_a_known_unknown_constant_is_reported() {
    seeded(
        "CEL0020",
        r#"<?php
namespace App;
function f(): int { return MISSING_LIMIT; }
"#,
    );
}

#[test]
fn cel0021_a_known_gated_symbol_is_reported() {
    // `json_validate` was introduced in PHP 8.3 and the shipped stub
    // blob carries that window; the manifest's `^8.1` puts the range
    // minimum below it.
    seeded(
        "CEL0021",
        r#"<?php
namespace App;
function f(): bool { return \json_validate('{}'); }
"#,
    );
}

#[test]
fn cel0023_a_known_deprecated_symbol_is_reported() {
    // Deprecated since PHP 8.2 in the shipped stub blob; the range
    // maximum under `^8.1` is past it. A warning still exits 1
    // (`a_warning_alone_still_exits_one` in check.rs pins that), so
    // the shared harness applies unchanged.
    seeded(
        "CEL0023",
        r#"<?php
namespace App;
function f(): void { \utf8_encode('x'); }
"#,
    );
}

// CEL0022 (symbol removed within the supported range) has no
// product-level fixture, deliberately: the shipped stub blob carries
// no symbol whose removal falls inside the supported window 8.1 to
// 8.5 (a removal at or below the minimum drops the symbol from the
// table entirely and reports CEL0019 instead). Its recall fixture
// drives the full framework path with a synthetic stub instead:
// `cel0022_a_removed_symbol_is_reported_through_the_phase` in
// `crates/celerrate_rules/src/rules/symbol_version_gating.rs`. It
// moves here the day a real removal enters the supported window.
