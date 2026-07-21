//! Inline suppressions, end to end: the four written forms extinguish
//! every diagnostic family on their scope, the report and the exit
//! code count the same post-filter set, and nothing leaks across
//! files (design sections 4 and 5).

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

#[test]
fn a_trailing_ignore_line_extinguishes_the_finding() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore-line\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
    assert!(!text.contains("CEL0018"), "{text}");
}

#[test]
fn a_hash_comment_carries_the_directive_too() {
    let root = project(&[("a.php", "<?php\nnew MissingOne(); # @phpstan-ignore-line\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn ignore_next_line_targets_the_line_below_and_only_it() {
    let root = project(&[(
        "a.php",
        "<?php\n// @phpstan-ignore-next-line\nnew MissingOne();\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn the_bare_identifier_form_covers_both_of_its_placements() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore class.notFound\n// @phpstan-ignore class.notFound\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn psalm_suppress_on_a_declaration_docblock_covers_its_whole_span() {
    let root = project(&[(
        "a.php",
        "<?php\n/** @psalm-suppress UndefinedClass */\nclass Service\n{\n    public function boot(): void\n    {\n        new MissingOne();\n    }\n}\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn suppression_extinguishes_the_syntax_family_too() {
    // Design section 5: suppression is family-agnostic — exempting the
    // existing families would re-report exactly what it forbids.
    let root = project(&[("a.php", "<?php\n$x = ; // @phpstan-ignore-line\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn a_directive_never_leaks_into_another_file() {
    let root = project(&[
        ("a.php", "<?php\n// @phpstan-ignore-next-line\n"),
        ("b.php", "<?php\nnew MissingOne();\n"),
    ]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("MissingOne"), "{text}");
}

#[test]
fn an_unrelated_line_still_reports_beside_a_suppressed_one() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore-line\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn a_suppression_extinguishes_a_typed_diagnostic() {
    // Design section 5's family-agnostic rule extends to the typed
    // families too: a suppression covers whatever family reports on its
    // scope, not just the two that predate them.
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        (
            "src/Service.php",
            r#"<?php
namespace App;

class User { public function save(): void {} }

function run(?User $user): void
{
    /** @phpstan-ignore-next-line */
    $user->save();
}
"#,
        ),
    ]);
    let (outcome, output) = check(root.path());
    assert_eq!(
        outcome,
        Outcome::Clean,
        "suppression is family-agnostic: {output}"
    );
}

#[test]
fn issue_58_suppressing_one_code_keeps_the_co_located_other_reported() {
    // The acceptance test of the #58 triage: two diagnostics on one
    // line (CEL0018 and CEL0019), a directive naming only the class
    // identifier. Before identifier-level correspondence this
    // suppressed both.
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); // @phpstan-ignore class.notFound\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("CEL0018"), "{text}");
    assert!(text.contains("CEL0019"), "{text}");
}

#[test]
fn a_fully_mapped_identifier_list_suppresses_the_union() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); // @phpstan-ignore class.notFound, function.notFound\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn any_unmapped_identifier_falls_back_to_the_whole_scope() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); // @phpstan-ignore class.notFound, some.unknownIdentifier\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn psalm_suppress_all_is_scope_wide() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); /* @psalm-suppress all */\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn correspondence_lookup_is_exact_case() {
    // The properly cased identifier narrows (CEL0019 survives); the
    // miscased one is unmapped and widens to the whole scope. Both
    // honor the user's suppression; only the exact-case form is
    // precise.
    let narrowed = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); /* @psalm-suppress UndefinedClass */\n",
    )]);
    let (outcome, text) = check(narrowed.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0019"), "{text}");

    let widened = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); /* @psalm-suppress undefinedclass */\n",
    )]);
    let (outcome, text) = check(widened.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn a_trailing_native_directive_suppresses_exactly_its_codes_on_its_line() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); // @celerrate-ignore CEL0018\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("CEL0018"), "{text}");
    assert!(text.contains("CEL0019"), "{text}");
}

#[test]
fn a_native_directive_alone_on_its_line_targets_the_next_line() {
    let root = project(&[(
        "a.php",
        "<?php\n// @celerrate-ignore CEL0018\nnew MissingOne();\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn a_native_docblock_directive_covers_the_annotated_declaration() {
    let root = project(&[(
        "a.php",
        "<?php\n/** @celerrate-ignore CEL0018 */\nclass Service {\n    public function boot() { new MissingOne(); }\n}\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn a_native_reason_trailer_is_honored() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @celerrate-ignore CEL0018 (legacy fixture class)\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn an_unknown_native_identifier_suppresses_nothing() {
    // The typo does not widen: CEL0018 stays reported. Its CEL0041
    // warning arrives with the reporting phase (a later task).
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @celerrate-ignore CEL9999\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0018"), "{text}");
}

#[test]
fn co_located_native_and_foreign_directives_union() {
    // Two separate comments on one line: the native identifier list is
    // comma-separated and runs to the end of its line, so the foreign
    // directive must live in its own comment to keep both parses clean.
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); absent_function(); /* @celerrate-ignore CEL0018 */ // @phpstan-ignore function.notFound\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

// The per-code semantic-evidence gate (design section 8): the
// correspondence-table gate (`suppression_correspondence.rs`) only
// proves the table and the vendored catalogues agree on which
// identifiers exist, never that a mapped `Codes` entry names the
// *right* Celerrate code. A wrong entry under-suppresses invisibly -
// the corpus snapshot is pinned at zero diagnostics, so it cannot
// catch it. For every distinct CEL code named in a `Codes` entry,
// provoke that code in a small fixture and prove that one
// representative identifier mapped to it actually suppresses it. Any
// failure here is a wrong table entry, not a wrong test: fix
// `correspondence.rs`, not the fixture.
//
// Each arm is self-proving: the bare fixture (the directive absent)
// must first report the intended code, so an arm can never turn green
// because a rule refactor or a stub change stopped provoking it in the
// first place; only then is the directive applied and the outcome
// asserted clean.
//
// CEL0021 and CEL0022 also appear in `Codes` entries, always bundled
// with CEL0018/CEL0019/CEL0020 under the same identifiers exercised
// below (`class.notFound`/`UndefinedClass`,
// `function.notFound`/`UndefinedFunction`,
// `constant.notFound`/`UndefinedConstant`). They get no arm of their
// own here: CEL0021 needs the same version-gated-symbol fixture as
// `cel0021_a_known_gated_symbol_is_reported` in `seeded_defects.rs`
// to provoke on its own, and CEL0022 has no product-level fixture at
// all today (see the comment beside that same test). Their presence
// in the bundled three-code sets is exercised indirectly: the
// CEL0018/CEL0019/CEL0020 arms below suppress the very same union,
// so a table entry that dropped CEL0021 or CEL0022 from the bundle
// would not be caught here, only by the two fixtures named above.

const PER_CODE_MANIFEST: &str =
    r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;

/// One per-code fixture, the directive-bearing comment stripped so the
/// helper can prove both halves of the round trip: the bare source
/// alone provokes `code`, and the source with `identifier`'s directive
/// appended suppresses it.
struct PerCodeSeed {
    code: &'static str,
    identifier: &'static str,
    /// The fixture without any suppression directive, ending in a
    /// single trailing newline.
    source: &'static str,
}

/// Runs one seed through both halves of the round trip: first the bare
/// fixture, which must still report `seed.code` (proving the fixture
/// actually provokes it); then the same fixture with `seed.identifier`'s
/// directive appended on the same line via `comment`, which must fully
/// suppress it.
fn assert_round_trips(seed: &PerCodeSeed, comment: impl Fn(&str) -> String) {
    let bare_root = project(&[
        ("composer.json", PER_CODE_MANIFEST),
        ("src/Seed.php", seed.source),
    ]);
    let (bare_outcome, bare_text) = check(bare_root.path());
    assert_eq!(
        bare_outcome,
        Outcome::DiagnosticsReported,
        "{} must be provoked by the bare fixture (no directive present):\n{bare_text}",
        seed.code,
    );
    assert!(
        bare_text.contains(seed.code),
        "{} must be provoked by the bare fixture (no directive present):\n{bare_text}",
        seed.code,
    );

    let directed_source = format!(
        "{}{}\n",
        seed.source.trim_end_matches('\n'),
        comment(seed.identifier)
    );
    let directed_root = project(&[
        ("composer.json", PER_CODE_MANIFEST),
        ("src/Seed.php", directed_source.as_str()),
    ]);
    let (directed_outcome, directed_text) = check(directed_root.path());
    assert_eq!(
        directed_outcome,
        Outcome::Clean,
        "{} via `{}` must be fully suppressed:\n{directed_text}",
        seed.code,
        seed.identifier,
    );
}

fn phpstan_ignore_comment(identifier: &str) -> String {
    format!(" // @phpstan-ignore {identifier}")
}

fn psalm_suppress_comment(identifier: &str) -> String {
    format!(" /* @psalm-suppress {identifier} */")
}

#[test]
fn every_mapped_phpstan_code_is_actually_suppressed_by_its_identifier() {
    const SEEDS: &[PerCodeSeed] = &[
        PerCodeSeed {
            code: "CEL0018",
            identifier: "class.notFound",
            source: "<?php\nnamespace App;\nfunction f(): void { $x = new MissingService(); }\n",
        },
        PerCodeSeed {
            code: "CEL0019",
            identifier: "function.notFound",
            source: "<?php\nnamespace App;\nfunction f(): void { missing_helper(); }\n",
        },
        PerCodeSeed {
            code: "CEL0020",
            identifier: "constant.notFound",
            source: "<?php\nnamespace App;\nfunction f(): int { return MISSING_LIMIT; }\n",
        },
        PerCodeSeed {
            code: "CEL0023",
            identifier: "function.deprecated",
            source: "<?php\nnamespace App;\nfunction f(): void { \\utf8_encode('x'); }\n",
        },
        PerCodeSeed {
            code: "CEL0030",
            identifier: "method.notFound",
            source: "<?php\nnamespace App;\nclass User { public function save(): void {} }\nfunction f(User $u): void { $u->svae(); }\n",
        },
        PerCodeSeed {
            code: "CEL0031",
            identifier: "property.notFound",
            source: "<?php\nnamespace App;\nclass User { public string $name = ''; }\nfunction f(User $u): void { $x = $u->nmae; }\n",
        },
        PerCodeSeed {
            code: "CEL0032",
            identifier: "classConstant.notFound",
            source: "<?php\nnamespace App;\nclass Config { public const LIMIT = 10; }\nfunction f(): int { return Config::LIMTI; }\n",
        },
        PerCodeSeed {
            code: "CEL0034",
            identifier: "method.nonObject",
            source: "<?php\nnamespace App;\nclass User { public function save(): void {} }\nfunction f(?User $u): void { $u->save(); }\n",
        },
        PerCodeSeed {
            code: "CEL0035",
            identifier: "argument.type",
            source: "<?php\ndeclare(strict_types=1);\nnamespace App;\nclass Plain {}\nfunction takes(int $n): void {}\nfunction f(Plain $p): void { takes($p); }\n",
        },
        PerCodeSeed {
            code: "CEL0036",
            identifier: "arguments.count",
            source: "<?php\nnamespace App;\nfunction pair(int $a, int $b): void {}\nfunction f(): void { pair(1); }\n",
        },
        PerCodeSeed {
            code: "CEL0037",
            identifier: "arguments.count",
            source: "<?php\nnamespace App;\nfunction single(int $a): void {}\nfunction f(): void { single(1, 2); }\n",
        },
        PerCodeSeed {
            code: "CEL0038",
            identifier: "argument.unknown",
            source: "<?php\nnamespace App;\nfunction single(int $a): void {}\nfunction f(): void { single(a: 1, b: 2); }\n",
        },
    ];
    for seed in SEEDS {
        assert_round_trips(seed, phpstan_ignore_comment);
    }
}

#[test]
fn every_mapped_psalm_code_is_actually_suppressed_by_its_identifier() {
    const SEEDS: &[PerCodeSeed] = &[
        PerCodeSeed {
            code: "CEL0018",
            identifier: "UndefinedClass",
            source: "<?php\nnamespace App;\nfunction f(): void { $x = new MissingService(); }\n",
        },
        PerCodeSeed {
            code: "CEL0019",
            identifier: "UndefinedFunction",
            source: "<?php\nnamespace App;\nfunction f(): void { missing_helper(); }\n",
        },
        PerCodeSeed {
            code: "CEL0020",
            identifier: "UndefinedConstant",
            source: "<?php\nnamespace App;\nfunction f(): int { return MISSING_LIMIT; }\n",
        },
        PerCodeSeed {
            code: "CEL0023",
            identifier: "DeprecatedFunction",
            source: "<?php\nnamespace App;\nfunction f(): void { \\utf8_encode('x'); }\n",
        },
        PerCodeSeed {
            code: "CEL0030",
            identifier: "UndefinedMethod",
            source: "<?php\nnamespace App;\nclass User { public function save(): void {} }\nfunction f(User $u): void { $u->svae(); }\n",
        },
        PerCodeSeed {
            code: "CEL0031",
            identifier: "UndefinedPropertyFetch",
            source: "<?php\nnamespace App;\nclass User { public string $name = ''; }\nfunction f(User $u): void { $x = $u->nmae; }\n",
        },
        PerCodeSeed {
            code: "CEL0032",
            identifier: "UndefinedConstant",
            source: "<?php\nnamespace App;\nclass Config { public const LIMIT = 10; }\nfunction f(): int { return Config::LIMTI; }\n",
        },
        PerCodeSeed {
            code: "CEL0034",
            identifier: "PossiblyNullReference",
            source: "<?php\nnamespace App;\nclass User { public function save(): void {} }\nfunction f(?User $u): void { $u->save(); }\n",
        },
        PerCodeSeed {
            code: "CEL0035",
            identifier: "InvalidArgument",
            source: "<?php\ndeclare(strict_types=1);\nnamespace App;\nclass Plain {}\nfunction takes(int $n): void {}\nfunction f(Plain $p): void { takes($p); }\n",
        },
        PerCodeSeed {
            code: "CEL0036",
            identifier: "TooFewArguments",
            source: "<?php\nnamespace App;\nfunction pair(int $a, int $b): void {}\nfunction f(): void { pair(1); }\n",
        },
        PerCodeSeed {
            code: "CEL0037",
            identifier: "TooManyArguments",
            source: "<?php\nnamespace App;\nfunction single(int $a): void {}\nfunction f(): void { single(1, 2); }\n",
        },
        PerCodeSeed {
            code: "CEL0038",
            identifier: "InvalidNamedArgument",
            source: "<?php\nnamespace App;\nfunction single(int $a): void {}\nfunction f(): void { single(a: 1, b: 2); }\n",
        },
    ];
    for seed in SEEDS {
        assert_round_trips(seed, psalm_suppress_comment);
    }
}
