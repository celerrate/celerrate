//! Anti-false-positive smoke: realistic PHP over the real embedded
//! stub index must produce zero semantic diagnostics. A false positive
//! here is a priority bug (parent spec, section 6); the full pinned
//! Symfony corpus enters CI in part 8.

#![allow(clippy::unwrap_used)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_rules::{
    CORE_IDENTITY_NAME, RuleRegistration, RuleRegistry, Tier, core_rules,
    semantic_phase_diagnostics,
};
use celerrate_semantics::PluginIdentity;
use celerrate_source::FileId;
use celerrate_stubs::{StubIndexInput, embedded_stub_index};

/// The core-rule registration the composition root performs, repeated
/// here: an integration test cannot reach the crate's `#[cfg(test)]`
/// `rules::test_support` harness, so this file owns its own copy of the
/// four lines `register_core_rules` composes.
fn register_core_rules(db: &TestDatabase) {
    let identity = PluginIdentity {
        name: CORE_IDENTITY_NAME.to_owned(),
        version: "test".to_owned(),
        configuration: String::new(),
    };
    let registrations = core_rules()
        .into_iter()
        .map(|(metadata, implementation)| RuleRegistration {
            identity: identity.clone(),
            active: metadata.tier == Tier::Default,
            metadata,
            implementation,
        })
        .collect();
    let _ = RuleRegistry::builder(registrations)
        .durability(salsa::Durability::HIGH)
        .new(db);
}

const REALISTIC_SOURCES: &[&str] = &[
    // Conditional declaration, calls, constants, magic constants.
    "<?php
     if (!function_exists('app_helper')) { function app_helper(): string { return __DIR__; } }
     $path = app_helper() . PHP_EOL;
     $length = strlen($path);
     $items = array_map(strtoupper(...), ['a', 'b']);",
    // Class-likes: inheritance, traits, attributes, enums, match.
    "<?php
     namespace App;
     use ArrayAccess;
     #[\\Attribute]
     class Marker {}
     interface Repository extends ArrayAccess {}
     trait Timestamps { public ?\\DateTimeImmutable $updatedAt = null; }
     enum Suit: string {
         case Hearts = 'H';
         public function color(): string {
             return match ($this) { self::Hearts => 'red', default => 'black' };
         }
     }
     final class User implements \\Stringable {
         use Timestamps;
         public function __construct(private readonly string $name) {}
         public function __toString(): string { return $this->name; }
     }",
    // Types, catch, instanceof, scoped access, closures.
    "<?php
     namespace App;
     function load(int|string $id, ?\\Throwable $previous = null): iterable {
         try {
             $when = new \\DateTimeImmutable('now');
         } catch (\\Exception $error) {
             throw new \\RuntimeException($error->getMessage(), 0, $previous);
         }
         $mapper = fn (mixed $value): bool => $value instanceof \\Countable;
         yield from array_filter([$when, $id], $mapper);
     }",
    // Strict types, group/function/const imports, an interface
    // constant, and a trait adaptation block.
    "<?php
     declare(strict_types=1);
     namespace Lib;
     class A {}
     class B {}
     trait TraitA { public function f(): void {} }
     trait TraitB { public function f(): void {} }
     function helper(): string { return 'lib'; }
     const VERSION = '1.0';
     interface HasVersion { const VERSION = '2.0'; }
     #[\\Attribute]
     class Marker {
         public function __construct(public string $value) {}
     }
     namespace App;
     use Lib\\{A, B};
     use function Lib\\helper;
     use const Lib\\VERSION;
     #[\\Lib\\Marker(A::class)]
     class Combined {
         use \\Lib\\TraitA, \\Lib\\TraitB {
             \\Lib\\TraitA::f insteadof \\Lib\\TraitB;
         }
         public function run(): string {
             $a = new A();
             $ok = $a instanceof B;
             return helper() . VERSION . ($ok ? '1' : '0');
         }
     }",
];

#[test]
fn realistic_sources_produce_no_diagnostics() {
    let db = TestDatabase::default();
    let handles: Vec<SourceFile> = REALISTIC_SOURCES
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let files = AnalyzedFileSet::new(&db, handles.clone());
    let stubs = StubIndexInput::builder(embedded_stub_index().unwrap())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    register_core_rules(&db);
    for file in handles {
        let diagnostics = semantic_phase_diagnostics(&db, file, files, stubs, configuration);
        assert_eq!(diagnostics, &vec![], "file {:?}", file.file_id(&db));
    }
}

/// One source, checked alone against the real embedded stub index: zero
/// semantic diagnostics is the only acceptable outcome.
fn assert_no_diagnostics(source: &str) {
    let db = TestDatabase::default();
    let file = SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
    let files = AnalyzedFileSet::new(&db, vec![file]);
    let stubs = StubIndexInput::builder(embedded_stub_index().unwrap())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    register_core_rules(&db);
    let diagnostics = semantic_phase_diagnostics(&db, file, files, stubs, configuration);
    assert_eq!(diagnostics, &vec![], "source: {source}");
}

#[test]
fn a_define_in_a_method_body_is_not_an_unknown_constant() {
    // The case that motivated the design: bootstrap code calling
    // `define()` from a static method is not exotic, and an unseen
    // `define()` is a false positive.
    assert_no_diagnostics(
        r"<?php
        class Bootstrap {
            public static function boot(): void {
                define('APP_ROOT', __DIR__);
            }
        }
        echo APP_ROOT;
        ",
    );
}

#[test]
fn a_define_inside_a_namespace_declares_globally() {
    assert_no_diagnostics(
        r"<?php
        namespace App;
        define('APP_ROOT', __DIR__);
        echo APP_ROOT;
        echo \APP_ROOT;
        ",
    );
}

#[test]
fn a_qualified_define_literal_resolves_where_it_says() {
    assert_no_diagnostics(
        r"<?php
        namespace App;
        define('Vendor\Product\LIMIT', 10);
        echo \Vendor\Product\LIMIT;
        ",
    );
}

/// A double-quoted `define()` is at least as common as a single-quoted
/// one in real PHP, and the parser builds a different node for it: a
/// `Literal` only wraps a single-quoted string, while a double-quoted
/// one is an `InterpolatedString`. Reading only the first left every
/// double-quoted constant unindexed, and an unseen `define()` is a false
/// positive.
#[test]
fn a_double_quoted_define_is_not_an_unknown_constant() {
    assert_no_diagnostics(
        r#"<?php
        define("APP_ROOT", 1);
        define('OTHER_ROOT', 2);
        echo APP_ROOT;
        echo OTHER_ROOT;
        "#,
    );
}

#[test]
fn a_double_quoted_define_in_a_method_body_is_not_an_unknown_constant() {
    assert_no_diagnostics(
        r#"<?php
        class Bootstrap {
            public static function boot(): void {
                define("APP_ROOT", __DIR__);
            }
        }
        echo APP_ROOT;
        "#,
    );
}

/// The escapes are where the two quotings part company: `\\` is one
/// backslash in double quotes, and every other backslash PHP does not
/// read as an escape stays exactly where it is.
#[test]
fn a_qualified_double_quoted_define_literal_resolves_where_it_says() {
    assert_no_diagnostics(
        r#"<?php
        namespace App;
        define("Vendor\\Product\\LIMIT", 10);
        define("Other\Product\BOUND", 20);
        echo \Vendor\Product\LIMIT;
        echo \Other\Product\BOUND;
        "#,
    );
}

/// `\u` is an escape only before `{`, and `\x` only before a hexadecimal
/// digit. PHP reads every other `\u` and `\x` literally, so a namespace
/// segment that happens to start with one names exactly what it looks
/// like. A lowercase segment is unconventional and perfectly legal, and
/// refusing to index the name would be an unknown-constant diagnostic at
/// every use site.
#[test]
fn a_double_quoted_define_whose_segment_starts_like_a_byte_escape_still_resolves() {
    assert_no_diagnostics(
        r#"<?php
        namespace App;
        define("Acme\utils\VERSION", 1);
        define("Foo\xml\NS", 2);
        echo \Acme\utils\VERSION;
        echo \Foo\xml\NS;
        "#,
    );
}
