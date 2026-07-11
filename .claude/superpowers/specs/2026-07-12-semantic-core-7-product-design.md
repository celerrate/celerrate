# Semantic Core Part 7: The Preview Product (Design)

Date: 2026-07-12
Status: Approved
Parent spec: `.claude/superpowers/specs/2026-07-11-semantic-core-design.md`
(sections 2, 7, 10)
Predecessor plan: `.claude/superpowers/plans/2026-07-11-semantic-core-6-checks.md`

## 1. Goal and scope

Ship `celerrate_cli`: the composition root, the `celerrate check`
command, `--watch`, panic isolation, and the internal-error report.
This is the part that first renders a diagnostic to a human being,
which is why it carries more than a binary. Three promises the engine
has been making internally become externally visible here, and each one
breaks on contact with real PHP unless it is closed first. They are
release blockers, not follow-ups, and they land before the CLI prints
anything.

The umbrella design delegates part-level decisions to a part design only
when a part justifies one. This part justified one: the CLI is the first
consumer that sees every producing crate at once, and looking at all of
them together surfaced a defect none of them could see alone.

In scope:

- The identifier registry and the renumbering it forces (section 2).
- `define()` constants in the symbol index (section 3).
- A total `SyntaxKind::describe()` (section 4).
- `celerrate_cli`: the concrete salsa database, the startup sequence,
  the parallel analysis loop, panic isolation, rendering, exit codes
  (section 5).
- `--watch` (section 6).

Out of scope, deliberately: color and terminal styling, `celerrate.toml`,
JSON, SARIF, and GitHub output formats, baseline, and the persistent
artifact cache, which is part 8. The text format is the one the umbrella
design fixes, and it is documented as temporary.

## 2. The identifier registry

**The defect.** Part 2 allocated `CEL0018` through `CEL0022` to the
Composer discovery notices (`crates/celerrate_project/src/notice.rs`).
Part 6 independently allocated `CEL0018` through `CEL0023` to the
semantic checks (`crates/celerrate_semantics/src/reference_checks.rs`).
`CEL0018` therefore means both "no `composer.json`" and "unknown class".
Both crates carry a test asserting their own numbering is stable, and
both tests pass, because neither crate can see the other: there is no
central registry and no workspace-wide uniqueness check. The umbrella
design's section 7 enumerates only the semantics side, which is how the
collision survived review.

Nothing is broken yet, for the same reason the other two blockers are
not yet false positives: nothing has rendered to a user. But identifiers
are the one thing the design declares un-renumberable after publication,
because suppressions and tooling are scripted against them, and part 7
is the publication.

**The resolution.** Semantics keeps `CEL0018` through `CEL0024` exactly
as the umbrella design already writes them. The five project notices
move to `CEL0025` through `CEL0029`. The full allocation after this part:

| Identifiers | Family | Owner |
| --- | --- | --- |
| `CEL0001` | source too large | `celerrate_db` |
| `CEL0002`-`CEL0006` | lexer | `celerrate_syntax` |
| `CEL0007`-`CEL0017` | parser | `celerrate_syntax` |
| `CEL0018`-`CEL0020` | unknown class, function, constant | `celerrate_semantics` |
| `CEL0021`-`CEL0023` | symbol gating | `celerrate_semantics` |
| `CEL0024` | syntax construct gating | `celerrate_semantics` |
| `CEL0025`-`CEL0029` | Composer discovery notices | `celerrate_project` |

**The mechanism that prevents a recurrence.** `celerrate_diagnostics`
owns the registry: one table naming every allocated identifier and the
family it belongs to, readable by a human as the canonical list. Each
producing crate exposes the identifiers it allocates, declared beside
the constants it already has, so the list cannot drift from the
constants without the crate's own stability test noticing.

The uniqueness test lives in `celerrate_cli`. This is not a convenience:
in a strict dependency DAG, the composition root is the only place that
can observe every producer at once, which is precisely why the layers
below could not catch this themselves. The test asserts that every
identifier is allocated exactly once across all producers, and that the
union of what the producers allocate equals the registry table. A
seventh crate cannot repeat part 2's mistake without failing it.

## 3. `define()` constants

**The gap.** The symbol index carries `const` declarations only, so
`define('APP_ROOT', __DIR__)` followed by `echo APP_ROOT;` reports an
unknown constant. `define()` is pervasive in real PHP, and this is the
first part that shows a diagnostic to anyone, so the anti-false-positive
promise would break on the first legacy file the preview ever sees.

**Why the obvious placement is wrong.** The natural move is to teach the
item traversal to recognize `define()` where it already walks. That
traversal deliberately never descends into a member list, so a
`define()` called from a method body stays unindexed:

    class Bootstrap {
        public static function boot(): void {
            define('APP_ROOT', __DIR__);
        }
    }
    echo APP_ROOT;   // reported unknown

An unseen `define()` is a false positive, the exact direction the policy
forbids, and bootstrap code that calls `define()` from a static method is
not exotic. Making the `ItemTree` see into method bodies would close the
hole, but at the cost of the two invariants part 4 was built to
guarantee: adding a `define()` inside a body would renumber the `AstId`s
of every later item in the file, and a body edit could change the
`ItemTree`, so the early cutoff would stop firing for that file.

**The design.** A new per-file query, `defined_constants(file)`, walks
the whole tree, method bodies included, and returns the constant names
introduced by `define()` with their spans. `source_symbol_table` is
built from `item_tree` plus `defined_constants`. The `ItemTree` is not
touched, so both invariants survive intact, and the new query is an
independent early-cutoff unit in its own right: editing a body that
contains no `define()` call produces an identical result, which salsa
backdates.

**The PHP realities it must honor.** Only a literal string first argument
is indexed; anything dynamic (`define($name, ...)`) stays out of scope,
under the same stance that already excludes `new $class`. The name is
taken literally, so unlike `const`, a `define()` inside a namespace block
declares a constant in the **global** namespace, unless the literal is
itself qualified (`define('Foo\Bar', ...)`). The terminal segment stays
case-sensitive, matching the constant folding rule part 5 fixed. The
callee is matched case-insensitively in both its bare and root-qualified
spellings, since function names are case-insensitive: `define`,
`\define`, and `DEFINE` are the same function.

## 4. `SyntaxKind::describe()`

The parser's `Expected(kind)` family formats through `Debug`, so a user
would read `expected OpenBrace` rather than ``expected `{` ``. Ten kinds
are ever expected, and they split into two shapes: eight have a canonical
source spelling (`{`, `;`, `,`, `:`, `(`, `)`, `class`, `catch`), while
`Identifier` and `Variable` have none and need a phrase.

`xtask` generates a **total** `SyntaxKind::describe() -> &'static str`
from the token table it already owns, which already carries every token's
spelling. Punctuation and keywords describe as their source text, the two
category tokens as phrases ("a name", "a variable"), and node kinds as
humanized names. `ParserDiagnosticKind::Expected` formats through it.

Totality is the point. An `Option` with a fallback would leave a path,
however unreachable today, by which a Rust variant name reaches a user;
a total function forecloses it structurally, for every message, not just
this one. It follows the house codegen pattern (`SyntaxKind` itself is
generated with a freshness test), keeps the spellings in the single place
that already holds them, and cannot drift when a token is added. The LSP
and sub-project 4's rich rendering will both want it.

## 5. `celerrate_cli`

**The database.** `celerrate_cli` owns `AnalysisDatabase`, a struct
holding `salsa::Storage<Self>` and implementing `salsa::Database`: the
first concrete database in the workspace, and the one the umbrella design
places at the composition root. `celerrate_db::testing::TestDatabase` is
its template.

**Dependencies.** `clap` with derive for argument parsing, `notify` for
the watcher. Both are permissively licensed and clear `cargo deny`.
`clap` earns its weight: it gives real `--help`, `--version`, subcommand
structure, and correct errors on bad flags, and sub-project 5 grows this
surface substantially (baseline, output formats, `migrate --from-phpstan`).
No color or terminal crate: the preview text format is plain by design.

**Startup.** Parse arguments, `discover(root)` for the Composer
configuration, walk what the project declares, load the files through the
`Vfs`, and set the four inputs the semantic query consumes: the file
bytes, the analyzed file set, the project configuration at medium
durability, and the embedded stub index at high durability. A stub blob
that fails to decode is reported as an internal error, never a panic.

**Analysis.** Rayon fans out over the files through database snapshots,
at the declared boundary, never inside a query. Each file's total is
`celerrate_db::file_diagnostics` (decode and syntax) concatenated with
`celerrate_semantics::semantic_diagnostics` (references and gating);
nothing composes those two today, and the CLI is where the umbrella
design says they meet. Results are sorted through the total order the
shared model already defines, so parallel collection is deterministic
before rendering.

**Panic isolation.** `catch_unwind` wraps only the outermost per-file
call, is transparent to `salsa::Cancelled` (always re-raised), and its
product is never memoized. A panic does not kill the run: the offending
file yields nothing, every other file still reports, and the
internal-error report prints at the end, naming the file and carrying a
pre-filled issue invitation. Exit code 2 dominates 1.

**Rendering.** Project notices print first, in their own spanless shape,
then the per-file diagnostics as `path:line:column identifier message`
with one-based line and column from `line_index`, then a summary:

    warning CEL0025: no composer.json found; analyzing the current directory
    warning CEL0027: no PHP version configured; assuming 8.5

    src/Kernel.php:14:9 CEL0018 unknown class App\Missing
    src/Kernel.php:31:5 CEL0024 readonly classes require PHP 8.2

    2 notices, 2 diagnostics

The notices are rendered as a separate block, not projected into the
shared `Diagnostic` model, because a project-level finding has no span:
`MISSING_COMPOSER_MANIFEST` describes a file that by definition does not
exist, and anchoring it to `composer.json:1:1` would be a fiction. The
richer model that can carry a spanless finding honestly belongs to
sub-project 4, which owns diagnostic anatomy.

**Exit codes**, as the umbrella design fixes them: 0 clean, 1 any
diagnostic reported (warning or error alike), 2 internal error. Notices
never affect the exit code: each one announces a fallback already taken,
and zero-configuration must never block.

## 6. `--watch`

The watcher observes the project walk roots plus `composer.json` and
`composer.lock`. It never watches `vendor/`: thousands of files that only
move when the lockfile does, and a lockfile change triggers full
re-discovery anyway.

A burst of events is coalesced before a cycle starts, because editors
write in bursts. Cancellation is not bolted on: mutating the salsa inputs
is what raises `salsa::Cancelled` in in-flight queries, and the CLI treats
it as a restart signal rather than an error. This is the first real
consumer of the invariant the umbrella design declared unretrofittable.

Each change class maps to an input mutation. A modified file sets its
bytes. A new file is added to the analyzed file set. A deleted file is
dropped from it: `SourceFile` has no deleted state, and a tombstone (an
empty byte string) would leave the file set lying about what it contains.
A changed lockfile re-runs discovery and rebuilds the configuration.

File addition and deletion are a genuinely new invalidation class. Every
part so far has mutated bytes within a fixed file set; membership changes
have never been exercised. Salsa's backdating should already handle the
benign case, where a file that declares nothing re-runs the symbol table
once and invalidates nothing downstream, but "should" is not a test, and
section 7 makes it one.

Each cycle then clears the screen, reprints the complete current state,
and reports files re-analyzed and elapsed milliseconds:

    0 diagnostics  |  1 file re-analyzed  |  4ms
    watching for changes...

The picture is always complete, never a stale log of past edits. The
timing line is where the differentiator stops being a claim: a one-file
edit in a large project reprints in single-digit milliseconds, and the
user sees that number on every save.

## 7. Testing

1. **TDD throughout**, as every part before it.
2. **The CLI is a library with a thin binary over it.** End-to-end tests
   drive `run(arguments, &mut output)` in process and snapshot the
   rendered text with `insta`: no process spawning, no timing flakiness,
   and the rendering is pinned exactly.
3. **The registry uniqueness test**, in `celerrate_cli`, per section 2.
4. **Anti-false-positive cases for `define()`**, including the method-body
   case that motivated the design, plus the namespace rule (a `define()`
   inside a namespace declares globally) and the dynamic-argument
   exclusion.
5. **An invalidation-scope test for file addition and deletion**, the
   membership class the harness has never exercised.
6. **The watcher is split for testability.** The reconciliation from VFS
   changes to input mutations is a pure unit, tested without the
   operating system; the `notify` adapter gets one tolerant integration
   test over a temporary directory (create, modify, delete).
7. **Exit-code tests**, covering clean, warning-only, error, and panic.
8. **Zero-panic lints** apply to `celerrate_cli` with no exception outside
   test modules. A CLI that must never panic returns `ExitCode` and
   `Result` throughout; the existing `stub-compiler` binary is the idiom.
