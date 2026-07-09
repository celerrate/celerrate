# Foundations Part 2: Source-File Representation — Design

Date: 2026-07-10
Status: Approved
Parent spec: `.claude/superpowers/specs/2026-07-09-celerrate-design.md` (sections 3, 8, 9)
Predecessor plan: `.claude/superpowers/plans/2026-07-09-foundations-1-workspace-and-source.md`
(which deferred the source-file representation to this part)

## 1. Goal and scope

Complete `celerrate_source` with the two primitives the rest of the engine
references files through: an opaque file identifier and a decoded source
text. After this part, the crate offers everything `celerrate_syntax` needs
to lex real files.

In scope:

- `FileId`: an opaque, compact file handle.
- `SourceText`: decoding raw bytes into engine-ready UTF-8 text with
  provenance (BOM, replacement ranges) and a single, data-carrying failure
  mode (oversized input).
- Documentation updates in `lib.rs` (the "loading is added by a later plan"
  note becomes reality).

Out of scope (deliberately):

- Disk I/O of any kind. File contents are future salsa inputs: they arrive
  from outside (a disk walk in the CLI, editor buffers in the LSP, literal
  strings in tests), and everything below must be a pure function of them.
  File reading lives where file discovery lives: sub-project 2 (semantic
  core). This mirrors rust-analyzer, whose virtual file system sits at the
  edge, not at the bottom.
- Path semantics: normalization, case sensitivity, symlinks. The
  `FileId ↔ path` mapping belongs to whoever assigns identifiers (the salsa
  database, sub-project 2).
- Encoding transcoding (UTF-16, Latin-1 detection). Real-world PHP is
  overwhelmingly UTF-8/ASCII; legacy files are still analyzed through lossy
  replacement instead of being rejected. YAGNI.
- The lexer and parser (`celerrate_syntax`): the next Foundations part.

## 2. `FileId`

```rust
pub struct FileId(u32);
```

- Derives: `Copy`, `Clone`, `PartialEq`, `Eq`, `Hash`, `PartialOrd`, `Ord`,
  `Debug`.
- Constructor `FileId::new(u32)` and a raw accessor for the layer that
  assigns identifiers.
- The crate attaches no meaning to the value: it is a key, assigned
  elsewhere. Keeping it 4 bytes keeps future salsa keys and diagnostic spans
  cheap.

## 3. `SourceText`

The product of decoding raw bytes into engine-ready text:

```rust
pub struct SourceText {
    text: String,                 // normalized UTF-8, BOM stripped
    had_utf8_bom: bool,
    replacements: Vec<TextRange>, // ranges in `text` holding U+FFFD substitutions
}

impl SourceText {
    pub fn from_bytes(bytes: &[u8]) -> Result<SourceText, SourceTooLarge>;
    pub fn text(&self) -> &str;
    pub fn had_utf8_bom(&self) -> bool;
    pub fn replacements(&self) -> &[TextRange];
    pub fn is_pristine(&self) -> bool; // no BOM, no replacements: bytes == text
}
```

### Decoding semantics

- **UTF-8 BOM.** A leading UTF-8 BOM (`EF BB BF`) is stripped from the text
  (the lexer must never see it) and recorded in `had_utf8_bom`. Writers can
  re-emit it, and a BOM before `<?php` is a genuine PHP hazard (bytes before
  the opening tag are emitted to output) worth a lint someday.
- **Invalid UTF-8.** Invalid sequences are replaced with U+FFFD; each
  replacement's range **in the normalized text** is recorded. Analysis
  proceeds — no user input is ever rejected outright (parent spec
  section 8). The recorded ranges distinguish real corruption from a
  literal U+FFFD present in the file, and mark the file unsafe for autofix
  write-back: `is_pristine()` is the flag upper layers consult before
  applying structured edits.
- **No other normalization.** Line endings, tabs, and NUL bytes pass
  through untouched. The lossless-CST guarantee (`text(tree) == source`) is
  measured against this normalized text.
- **Oversized input.** The only failure is `SourceTooLarge`: the decoded
  text would exceed the 4 GiB cap of `TextSize`. Lossy replacement can grow
  the text (one invalid byte becomes a three-byte U+FFFD), so the check
  applies to the decoded length, not the input length. `SourceTooLarge` is
  plain data the caller renders as a diagnostic — nothing panics, honoring
  the existing `LineIndex` contract that oversized inputs are rejected
  before indexing.

## 4. `LineIndex`

Unchanged, and deliberately not bundled into `SourceText`: in sub-project 2
it becomes a derived salsa query over the text, so identity (input key),
text (input value), and line index (derived data) stay separate. The stale
`lib.rs` documentation note is updated to reflect that decoding now exists.

## 5. Error handling

- Zero-panic policy applies mechanically (workspace lints).
- `from_bytes` is total over all byte sequences except the 4 GiB cap, which
  is a typed error, never a panic.
- No `Result` anywhere else: BOM and invalid bytes are provenance data, not
  errors.

## 6. Testing

TDD throughout, unit tests at the crate boundary:

- Plain ASCII, multi-byte UTF-8, empty input.
- BOM alone, BOM plus content; `had_utf8_bom` in both.
- Invalid bytes at start, middle, end; consecutive invalid sequences;
  replacement ranges point at the U+FFFD characters in the normalized text.
- A literal U+FFFD in valid input yields no replacement range.
- `is_pristine` across all combinations (clean, BOM only, replacements
  only, both).
- The 4 GiB failure path is exercised through a small internal length-check
  function rather than allocating 4 GiB in CI.
- `FileId` ordering, hashing, and raw round-trip.
