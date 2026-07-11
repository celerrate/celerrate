//! Anti-false-positive smoke: realistic PHP over the real embedded
//! stub index must produce zero semantic diagnostics. A false positive
//! here is a priority bug (parent spec, section 6); the full pinned
//! Symfony corpus enters CI in part 8.

#![allow(clippy::unwrap_used)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::semantic_diagnostics;
use celerrate_source::FileId;
use celerrate_stubs::{StubIndexInput, embedded_stub_index};

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
    for file in handles {
        let diagnostics = semantic_diagnostics(&db, file, files, stubs, configuration);
        assert_eq!(diagnostics, &vec![], "file {:?}", file.file_id(&db));
    }
}
