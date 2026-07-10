# Foundations Part 4: The Parser (Design)

Date: 2026-07-10
Status: Approved
Parent spec: `.claude/superpowers/specs/2026-07-09-celerrate-design.md` (sections 3, 8, 9)
Predecessor plan: `.claude/superpowers/plans/2026-07-10-foundations-3-lexer-follow-ups.md`

## 1. Goal and scope

Complete the `celerrate_syntax` crate with an error-resilient,
hand-written parser producing a lossless concrete syntax tree, plus a
typed AST layer on top of it. After this part, the crate turns any
decoded source text into a syntax tree that upper layers (the salsa
database, name resolution, the type engine) can consume, and the
Foundations sub-project is closed.

In scope:

- The tree layer: `rowan` as the red-green tree implementation, wrapped
  behind crate-owned types so it never leaks upward.
- The parser: hand-written recursive descent over the existing token
  stream, emitting events replayed by a tree builder; full PHP grammar
  through 8.5 (property hooks and asymmetric visibility from 8.4, the
  pipe operator and clone-with from 8.5).
- Parser diagnostics as structured data, merged with lexer diagnostics
  in one output.
- The typed AST layer, generated from an `ungrammar` grammar
  description by an `xtask` binary, with the generated code committed.
- Snapshot, error-corpus, and fuzz testing for the whole pipeline.

Out of scope (deliberately):

- Any version or semantic judgment. `readonly class` in an 8.1 project,
  redundant modifiers, `abstract` on a plain property: all of it parses;
  availability and validity are semantic diagnostics produced by upper
  layers. The parser rejects only what is structurally unanalyzable.
- Docblock content. `DocComment` tokens are attached in the tree
  (section 2) but their interior grammar belongs to the type engine
  (sub-project 3).
- Diagnostic rendering. Parser diagnostics are structured kinds and
  ranges; messages, annotated spans, and suggestions are sub-project 4.
- Measured performance. Benchmarks arrive with the full pipeline; the
  design is performance-oriented (green node deduplication, no
  allocation in the parser hot loop beyond the event buffer).

Architecture decision, recorded: the tree is `rowan`, not a homegrown
red-green implementation and not `cstree`. `rowan` is proven at
rust-analyzer scale and the `#[repr(u16)]` `SyntaxKind` was designed
for it. A homegrown tree is what Biome eventually built — after
starting on rowan, once real needs justified it; building one before a
parser exists to exercise it would be blind optimization. Two
containment guarantees make the choice reversible: crate-owned type
aliases keep `rowan` out of every public signature, and the
event-based parser never touches the tree, so only the builder would
change in a migration. `rowan` uses `unsafe` internally; that is
outside the workspace `forbid(unsafe_code)` (which governs our code)
and inside `cargo-deny`'s audit scope.

## 2. The tree layer

New `tree/` module in `celerrate_syntax`.

- **`PhpLanguage`** implements `rowan::Language`. The
  `SyntaxKind ↔ rowan::SyntaxKind` conversion is a `u16` transmute-free
  cast guarded by a `LAST` sentinel discriminant, so the inverse
  conversion is total and panic-free.
- **Crate-owned aliases** — `SyntaxNode`, `SyntaxToken`,
  `SyntaxElement`, `SyntaxNodePtr` — parameterized by `PhpLanguage`.
  Upper crates import these; no bare `rowan` type appears in any public
  signature.
- **`SyntaxKind` grows node kinds**, appended after the token kinds as
  the lexer part planned: `SourceFile` (the root), expression kinds,
  statement kinds, declaration kinds, plus an `Error` node kind for
  recovery wreckage. Node kinds are ultimately owned by the `ungrammar`
  description (section 5); until that plan lands they are added by
  hand.
- **`TreeBuilder`** replays parser events against the full token
  stream (trivia included) and materializes the green tree through
  `rowan::GreenNodeBuilder`.

**Trivia attachment.** Trivia remain ordinary tokens in the tree,
inserted by the builder where they occur. One rule matters now because
the type engine will depend on it: a `DocComment` attaches to the
declaration node that follows it (class, function, method, property,
constant, enum case), not to the preceding node. All other trivia sit
between siblings, ahead of the construct that follows: the builder
flushes pending trivia just before the next node or token starts, into
the node open at that point. A node's range therefore starts at its
first significant token and never includes leading trivia.

**Public API.** `parse(source: &str) -> Parse` chains `lex` and the
parser. `Parse` owns the root green node and the diagnostics — lexer
and parser diagnostics merged, in source order. `Parse::tree()` returns
the root `SyntaxNode` (`SourceFile`). The lossless invariant holds at
this level: `parse(source).tree().text() == source`, always, including
on degenerate input.

**Diagnostics.** `ParserDiagnostic { kind, range }`, the structural
mirror of `LexerDiagnostic`: enumerated kinds (an `Expected…` family,
`MissingToken`-style kinds), no message strings. Rendering is an upper
layer's business.

## 3. The parser

Hand-written recursive descent, event-based: the parser never builds
the tree.

- **`TokenSource`** gives the parser a trivia-free view of the token
  stream while remembering each significant token's real index, so the
  builder can reconcile events with the full stream.
- **Events** — `StartNode(kind)`, `Token`, `FinishNode`, `Error` — are
  appended to a buffer and replayed once parsing ends. Two
  rust-analyzer mechanisms are adopted as-is because the PHP grammar
  requires them:
  - **Forward parents**: a node can be opened retroactively around an
    already-parsed one. Binary and postfix expressions
    (`$a->b[0]() + $c`) wrap the existing expression at every step.
  - **`Marker` / `CompletedMarker`**: every grammar rule opens a marker
    and either completes it with a kind or abandons it cleanly. The
    type system guarantees no node is left open.

**Expressions are Pratt-parsed.** One precedence table, transcribed
from the Zend grammar (the PHP manual's operator precedence table),
drives binary operators, associativity — including right-associative
`**` and the non-associative comparison group — and the ternary.
Prefix operators (`!`, casts, `++`, `--`, `-`, `~`, `@`, `&`), postfix
chains (`->`, `?->`, `::`, calls, indexing, `++`/`--`), and the 8.5
pipe operator `|>` are loops inside the expression parser.

**Grammar areas**, one parser module each:

- **Declarations**: classes, enums (pure and backed), traits,
  interfaces, functions; members and modifiers; property hooks and
  asymmetric visibility (8.4); `use`/`namespace` (both forms);
  constants.
- **Statements**: control flow, loops, `try`/`catch`/`finally`,
  `switch`, `goto`, the alternative syntax (`endif`, `endwhile`, ...),
  and inline HTML, which may interrupt any statement list.
- **Expressions**: literals, calls, member access, `match`, closures
  and arrow functions, `new`, arrays and destructuring, `yield`,
  `throw` expressions, string interpolation — the lexer already
  delivers the structure as tokens; the parser builds interpolation
  nodes over them.
- **Types**: nullable, unions, intersections, DNF forms `(A&B)|C`.
- **Attributes**: `#[...]` groups, allowed before declarations,
  parameters, and closures.

**Semi-reserved keywords.** `$object->list()`, `const FOR = 1;`,
`enum` as a plain name: keyword kinds are accepted at identifier
positions by contextual remapping, as the lexer anticipated. The
remapping is per-position (member names, constant names, goto labels),
mirroring Zend's semi-reserved list.

## 4. Error handling

The parser always terminates and always produces a full-coverage tree.

1. **Guaranteed progress**: every iteration consumes a token or
   finishes. A token no rule accepts is wrapped in an `Error` node with
   a diagnostic, and parsing continues. This is the termination
   argument, and the fuzzer verifies it.
2. **Contextual recovery sets**: a derailed statement rule
   resynchronizes on `;`, `}`, or a statement-starting keyword; a class
   member list on modifiers, `function`, `const`, `use`, or `}`. The
   node being built is completed partially, never discarded.
3. **Missing elements**: an expected-but-absent token produces an
   `Expected…` diagnostic and a node without that child. The typed AST
   exposes `Option` accessors everywhere, so a partial tree is a normal
   citizen, not a special case.
4. **Mechanical zero-panic**: the workspace deny lints apply with no
   exceptions outside test modules; the marker discipline and the
   sentinel-guarded kind conversion keep the tree layer panic-free.

## 5. The typed AST

New `ast/` module, generated — not hand-written — because the PHP
grammar is far too large to maintain accessors by hand.

- **`php.ungram`** describes the nominal shape of every node — fields,
  cardinalities, token children — using the `ungrammar` DSL from
  rust-analyzer. It describes shape, not parsing: the recursive-descent
  parser remains the sole authority on how text becomes nodes.
- **An `xtask` binary** (new workspace member, dev-only) generates two
  artifacts from it: the node-kind variants of `SyntaxKind`, and the
  typed node structs with accessors (`ClassDeclaration::name() ->
  Option<Identifier>`, `ClassDeclaration::members() -> impl Iterator`).
  Every accessor returns `Option` or an iterator; this is what makes
  the partial trees of section 4 free to consume.
- **Generated code is committed**, and a sourcegen test asserts it is
  fresh with respect to `php.ungram` (rust-analyzer's model): no
  `build.rs`, generation stays readable and debuggable.
- **Hand-written extensions** live in `impl` blocks beside the
  generated code, for logic a generator cannot express (for example
  resolving a semi-reserved member name).

## 6. Testing

1. **Unit tests, TDD**: every grammar rule and recovery behavior starts
   from a failing test.
2. **`insta` snapshots**: a corpus of PHP files, each snapshotted as an
   indented tree rendering (node kinds, token kinds with text) plus
   diagnostics. The day-to-day tool of grammar development, extending
   the lexer's corpus infrastructure.
3. **The lossless invariant**: `parse(source).tree().text() == source`
   asserted on the whole corpus through a shared helper, and by the
   fuzzer.
4. **Error corpus**: deliberately broken files — truncations, stray
   tokens, unclosed nesting — with snapshots of the partial trees.
   Resilience is tested as a feature, not hoped for.
5. **`cargo-fuzz`**: the existing fuzz harness gains a `parse` target;
   invariants: no panic, lossless, termination. The seed corpus reuses
   and extends the lexer's.
6. **Sourcegen freshness**: the committed generated code matches
   `php.ungram`, asserted by a test.

## 7. Implementation plans

One design (this document), several implementation plans, each its own
TDD cycle, in order:

1. **Tree and skeleton**: rowan integration, `PhpLanguage`, aliases,
   events, `TreeBuilder`, minimal `parse()` (`SourceFile`, inline HTML,
   `echo` and expression statements), snapshot infrastructure.
2. **Expressions**: the full Pratt table, literals, calls and access
   chains, string interpolation nodes, `match`, closures and arrow
   functions.
3. **Statements**: control flow, loops, alternative syntax,
   `try`/`catch`, simple top-level declarations.
4. **Declarations**: classes, enums, traits, interfaces, members,
   property hooks, types (unions, intersections, DNF), attributes,
   `use`/`namespace`.
5. **Typed AST**: `php.ungram`, the `xtask` generator, migration of the
   hand-added node kinds to generated ones, accessors, sourcegen test.

Error recovery is not a separate plan: every plan ships its own,
tested, as it goes.
