//! The seeded-defect recall suite (design section 9): the gate a
//! silent engine cannot pass. One known defect per identifier; each
//! MUST be reported through the full product pipeline. These
//! fixtures are the family's substance contract; never weaken an
//! assertion to unblock a refactor.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::{Outcome, run};

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
