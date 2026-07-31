# CLI Product Distribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the distribution slice of the CLI product: `cargo xtask dist` as the single deterministic packaging logic, the release workflow delegating to it with artifact attestations, a checksum-verified `install.sh`, and the Composer bootstrap package, all proven hermetically in CI against the current commit.

**Architecture:** One packaging brain, hermetic tests. `cargo xtask dist` is the only place that knows how a release artifact is assembled; `release.yml` calls it on each matrix runner instead of duplicating shell/pwsh packaging. `install.sh` and the Composer plugin accept a base-URL override so CI integration tests consume artifacts built from the current commit, offline. Spec: `.claude/superpowers/specs/2026-07-31-cli-product-distribution-design.md`.

**Tech Stack:** Rust (xtask: `tar`, `flate2`, `zip`, `sha2`), POSIX `sh`, GitHub Actions, PHP 7.4 (Composer plugin, `composer-plugin-api ^2.0`, PHPUnit), `php -S` for the hermetic fixture server.

## Global Constraints

- Branch: all work happens on `feat-cli-distribution`, branched from `main`.
- Rust toolchain 1.94, edition 2024. Workspace clippy denies `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`; `unsafe_code` is forbidden. Test modules may locally `#[allow]` these lints (existing pattern: `#![allow(clippy::unwrap_used)]` at the top of `mod tests`).
- New Rust dependencies enter `[workspace.dependencies]` in the root `Cargo.toml` (alphabetical order) and must pass `cargo deny check` (`deny.toml` allows MIT, Apache-2.0, Zlib, Unicode-3.0, ISC, CC0-1.0, BSD-2-Clause; extend only with a justifying comment, following the existing comment style).
- PHP code under `packages/composer-bootstrap/` must run on PHP 7.4: no constructor promotion, no enums, no `readonly`, no `0o755` literals (write `0755`), no named arguments, no first-class callable syntax. `declare(strict_types=1)` in every file.
- Everything is written in English, full words, no abbreviated names.
- Commits: gitmoji + Conventional Commits (`<emoji> <type>(<scope>): <summary>`), authored with the repository-configured identity, never referencing plans, specs, phases, or tasks.
- The five release targets: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`. Repository: `https://github.com/celerrate/celerrate`.
- Archive layout (fixed): `celerrate-<triple>.tar.gz` for Unix targets, `celerrate-<triple>.zip` for Windows targets, each containing a `celerrate-<triple>/` directory with the binary (`celerrate`, `celerrate.exe` on Windows), `LICENSE-MIT`, `LICENSE-APACHE`, `README.md`. Checksums use the `sha256sum` line format: `<64 hex chars><two spaces><file name>`.
- The environment overrides (fixed names): `CELERRATE_INSTALL_BASE_URL` (install.sh), `CELERRATE_DOWNLOAD_BASE_URL` and `CELERRATE_BINARY` (Composer plugin).

---

### Task 1: The deterministic packaging core in `xtask dist`

**Files:**
- Create: `xtask/src/dist.rs`
- Modify: `xtask/src/lib.rs` (add `pub mod dist;` to the module list, alphabetical: between `dependency_shape` and `emission_scan`... actual order in the file is alphabetical; place `pub mod dist;` after `pub mod dependency_shape;`)
- Modify: `xtask/Cargo.toml` (dependencies)
- Modify: `Cargo.toml` (workspace dependencies)
- Test: unit tests inside `xtask/src/dist.rs`

**Interfaces:**
- Consumes: `crate::Result` and `crate::workspace_root()` from `xtask/src/lib.rs` (existing).
- Produces (used by Task 2 and the tests):
  - `pub fn binary_file_name(triple: &str) -> &'static str`
  - `pub fn archive_file_name(triple: &str) -> String`
  - `pub fn checksum_line(contents: &[u8], file_name: &str) -> String`
  - `pub fn package(binary: &Path, documentation: &[PathBuf], triple: &str, output_directory: &Path) -> Result<PathBuf>`

- [ ] **Step 1: Create the branch**

```bash
git checkout main && git pull && git checkout -b feat-cli-distribution
```

- [ ] **Step 2: Add the dependencies**

In the root `Cargo.toml`, add to `[workspace.dependencies]` (keep the table alphabetical):

```toml
flate2 = "1"
sha2 = "0.10"
tar = "0.4"
zip = { version = "2", default-features = false, features = ["deflate"] }
```

In `xtask/Cargo.toml`, extend `[dependencies]`:

```toml
[dependencies]
flate2 = { workspace = true }
serde_json = { workspace = true }
sha2 = { workspace = true }
tar = { workspace = true }
ungrammar = { workspace = true }
zip = { workspace = true }
```

Note: if the `zip` crate has moved to a newer major and `"2"` fails to resolve, keep major 2 pinned only if it still resolves; otherwise use the latest major and adapt the two API points used below (`SimpleFileOptions`, `DateTime::default()`), which exist from 1.x onward.

- [ ] **Step 3: Write the failing tests**

Create `xtask/src/dist.rs` containing only the module documentation, the `use` lines, and the tests; the functions the tests call do not exist yet, so the failure is a compilation error, which is the red step here:

```rust
//! `cargo xtask dist [--target <triple>]`: build the release binary
//! for one target and package it exactly as the release publishes it:
//! `celerrate-<triple>.tar.gz` (`.zip` for Windows targets) plus a
//! `.sha256` checksum line in `sha256sum` format, under `target/dist/`.
//! Archive metadata is deterministic (fixed timestamps, ownership, and
//! entry order), so two runs over the same commit produce
//! byte-identical archives; the CI dist job pins that property.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::Digest;

use crate::Result;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::PathBuf;

    use super::{archive_file_name, binary_file_name, checksum_line, package};

    fn fixture(directory: &std::path::Path) -> (PathBuf, Vec<PathBuf>) {
        let binary = directory.join("celerrate");
        std::fs::write(&binary, b"#!/bin/sh\necho fake\n").unwrap();
        let mut documentation = Vec::new();
        for name in ["LICENSE-MIT", "LICENSE-APACHE", "README.md"] {
            let path = directory.join(name);
            std::fs::write(&path, format!("{name} body\n")).unwrap();
            documentation.push(path);
        }
        (binary, documentation)
    }

    #[test]
    fn names_follow_the_target_family() {
        assert_eq!(binary_file_name("x86_64-unknown-linux-musl"), "celerrate");
        assert_eq!(binary_file_name("x86_64-pc-windows-msvc"), "celerrate.exe");
        assert_eq!(
            archive_file_name("aarch64-apple-darwin"),
            "celerrate-aarch64-apple-darwin.tar.gz",
        );
        assert_eq!(
            archive_file_name("x86_64-pc-windows-msvc"),
            "celerrate-x86_64-pc-windows-msvc.zip",
        );
    }

    #[test]
    fn the_checksum_line_matches_sha256sum_format() {
        // The SHA-256 of the empty input is a published constant.
        assert_eq!(
            checksum_line(b"", "celerrate-x86_64-unknown-linux-musl.tar.gz"),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  celerrate-x86_64-unknown-linux-musl.tar.gz\n",
        );
    }

    #[test]
    fn tar_packaging_is_deterministic_and_lays_out_the_archive_directory() {
        let directory = tempfile::tempdir().unwrap();
        let (binary, documentation) = fixture(directory.path());
        let first_output = directory.path().join("first");
        let second_output = directory.path().join("second");
        std::fs::create_dir_all(&first_output).unwrap();
        std::fs::create_dir_all(&second_output).unwrap();
        let triple = "x86_64-unknown-linux-musl";
        let first = package(&binary, &documentation, triple, &first_output).unwrap();
        let second = package(&binary, &documentation, triple, &second_output).unwrap();
        let first_bytes = std::fs::read(&first).unwrap();
        assert_eq!(first_bytes, std::fs::read(&second).unwrap());
        let decoder = flate2::read::GzDecoder::new(first_bytes.as_slice());
        let mut archive = tar::Archive::new(decoder);
        let paths: Vec<String> = archive
            .entries()
            .unwrap()
            .map(|entry| entry.unwrap().path().unwrap().display().to_string())
            .collect();
        assert_eq!(
            paths,
            [
                "celerrate-x86_64-unknown-linux-musl/celerrate",
                "celerrate-x86_64-unknown-linux-musl/LICENSE-MIT",
                "celerrate-x86_64-unknown-linux-musl/LICENSE-APACHE",
                "celerrate-x86_64-unknown-linux-musl/README.md",
            ],
        );
    }

    #[test]
    fn zip_packaging_is_deterministic_and_used_for_windows_targets() {
        let directory = tempfile::tempdir().unwrap();
        let (binary, documentation) = fixture(directory.path());
        let first_output = directory.path().join("first");
        let second_output = directory.path().join("second");
        std::fs::create_dir_all(&first_output).unwrap();
        std::fs::create_dir_all(&second_output).unwrap();
        let triple = "x86_64-pc-windows-msvc";
        let first = package(&binary, &documentation, triple, &first_output).unwrap();
        assert!(first.to_string_lossy().ends_with(".zip"));
        let second = package(&binary, &documentation, triple, &second_output).unwrap();
        assert_eq!(std::fs::read(&first).unwrap(), std::fs::read(&second).unwrap());
    }
}
```

- [ ] **Step 4: Register the module and run the tests to verify they fail**

Add `pub mod dist;` to `xtask/src/lib.rs` (after `pub mod dependency_shape;`).

Run: `cargo test --package xtask dist`
Expected: compilation FAILURE (`archive_file_name`, `binary_file_name`, `checksum_line`, `package` not found).

- [ ] **Step 5: Implement the packaging core**

Fill `xtask/src/dist.rs` between the `use` lines and `mod tests`:

```rust
/// Documentation files shipped next to the binary in every archive.
const DOCUMENTATION_FILES: [&str; 3] = ["LICENSE-MIT", "LICENSE-APACHE", "README.md"];

/// `celerrate.exe` for Windows targets, `celerrate` everywhere else.
pub fn binary_file_name(triple: &str) -> &'static str {
    if is_windows_triple(triple) {
        "celerrate.exe"
    } else {
        "celerrate"
    }
}

/// `celerrate-<triple>.zip` for Windows targets, `.tar.gz` everywhere else.
pub fn archive_file_name(triple: &str) -> String {
    if is_windows_triple(triple) {
        format!("celerrate-{triple}.zip")
    } else {
        format!("celerrate-{triple}.tar.gz")
    }
}

fn is_windows_triple(triple: &str) -> bool {
    triple.contains("windows")
}

/// One `sha256sum`-format line: the hex digest, two spaces, the name.
/// The release workflow concatenates these lines into `SHA256SUMS`, and
/// `install.sh` and the Composer plugin verify against that file, so
/// the format must match what `sha256sum --check` accepts.
pub fn checksum_line(contents: &[u8], file_name: &str) -> String {
    use std::fmt::Write as _;
    let digest = sha2::Sha256::digest(contents);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    format!("{hex}  {file_name}\n")
}

/// Packages the binary and the documentation files under a
/// `celerrate-<triple>/` directory inside the archive. Entry order,
/// timestamps, ownership, and permissions are fixed so the archive
/// bytes depend only on the packaged file contents.
pub fn package(
    binary: &Path,
    documentation: &[PathBuf],
    triple: &str,
    output_directory: &Path,
) -> Result<PathBuf> {
    let archive_path = output_directory.join(archive_file_name(triple));
    if is_windows_triple(triple) {
        package_zip(binary, documentation, triple, &archive_path)?;
    } else {
        package_tar(binary, documentation, triple, &archive_path)?;
    }
    Ok(archive_path)
}

fn package_tar(
    binary: &Path,
    documentation: &[PathBuf],
    triple: &str,
    archive_path: &Path,
) -> Result<()> {
    let file = std::fs::File::create(archive_path)?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::best());
    let mut builder = tar::Builder::new(encoder);
    let prefix = format!("celerrate-{triple}");
    append_tar_entry(
        &mut builder,
        binary,
        &format!("{prefix}/{}", binary_file_name(triple)),
        0o755,
    )?;
    for path in documentation {
        let name = file_name(path)?;
        append_tar_entry(&mut builder, path, &format!("{prefix}/{name}"), 0o644)?;
    }
    builder.into_inner()?.finish()?;
    Ok(())
}

fn append_tar_entry<W: Write>(
    builder: &mut tar::Builder<W>,
    source: &Path,
    entry_path: &str,
    mode: u32,
) -> Result<()> {
    let contents = std::fs::read(source)?;
    let mut header = tar::Header::new_ustar();
    header.set_size(contents.len() as u64);
    header.set_mode(mode);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    builder.append_data(&mut header, entry_path, contents.as_slice())?;
    Ok(())
}

fn package_zip(
    binary: &Path,
    documentation: &[PathBuf],
    triple: &str,
    archive_path: &Path,
) -> Result<()> {
    let file = std::fs::File::create(archive_path)?;
    let mut writer = zip::ZipWriter::new(file);
    let prefix = format!("celerrate-{triple}");
    writer.start_file(
        format!("{prefix}/{}", binary_file_name(triple)),
        zip_entry_options(0o755),
    )?;
    writer.write_all(&std::fs::read(binary)?)?;
    for path in documentation {
        let name = file_name(path)?;
        writer.start_file(format!("{prefix}/{name}"), zip_entry_options(0o644))?;
        writer.write_all(&std::fs::read(path)?)?;
    }
    writer.finish()?;
    Ok(())
}

/// Fixed timestamp (the zip epoch, 1980-01-01) and explicit
/// permissions: the entry metadata never varies between runs.
fn zip_entry_options(mode: u32) -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(mode)
        .last_modified_time(zip::DateTime::default())
}

fn file_name(path: &Path) -> Result<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} has no file name", path.display()).into())
}
```

Note: `Command` and `crate::workspace_root` are not used yet (Task 2 uses them); if clippy flags the unused imports, drop `use std::process::Command;` for now and re-add it in Task 2.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --package xtask dist`
Expected: 4 passed.

- [ ] **Step 7: Lints, format, dependency audit**

Run: `cargo clippy --package xtask --all-targets -- -D warnings && cargo fmt --all && cargo deny check`
Expected: clean. If `cargo deny` reports a license outside the allow list from the new dependency trees, add it to `deny.toml`'s `allow` with a comment naming the crate that brings it and why the license is acceptable, following the existing comment style.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock xtask/Cargo.toml xtask/src/lib.rs xtask/src/dist.rs deny.toml
git commit -m "✨ feat(xtask): package release archives deterministically"
```

---

### Task 2: `cargo xtask dist` end to end: build, stage, package

**Files:**
- Modify: `xtask/src/dist.rs` (add `run`, `host_triple`, `build`, `write_checksum`)
- Modify: `xtask/src/main.rs` (full rewrite of the argument match, below)
- Modify: `xtask/src/lib.rs` (module documentation sentence)

**Interfaces:**
- Consumes: `package`, `archive_file_name`, `binary_file_name`, `checksum_line` from Task 1.
- Produces: `pub fn run(target: Option<&str>) -> Result<()>` — invoked as `cargo xtask dist` and `cargo xtask dist --target <triple>`. Artifacts land in `target/dist/`: the archive plus `<archive>.sha256`. Tasks 3, 5, and 8 rely on those paths and names.

- [ ] **Step 1: Write the failing test**

Append to `mod tests` in `xtask/src/dist.rs`:

```rust
    #[test]
    fn the_host_triple_is_a_target_triple() {
        let triple = super::host_triple().unwrap();
        assert!(triple.contains('-'), "not a triple: {triple}");
    }

    #[test]
    fn the_checksum_file_sits_next_to_the_archive_with_the_full_name() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("celerrate-x86_64-unknown-linux-musl.tar.gz");
        std::fs::write(&archive, b"archive body").unwrap();
        let checksum = super::write_checksum(&archive).unwrap();
        assert_eq!(
            checksum,
            directory
                .path()
                .join("celerrate-x86_64-unknown-linux-musl.tar.gz.sha256"),
        );
        let line = std::fs::read_to_string(&checksum).unwrap();
        assert!(line.ends_with("  celerrate-x86_64-unknown-linux-musl.tar.gz\n"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package xtask dist`
Expected: compilation FAILURE (`host_triple`, `write_checksum` not found).

- [ ] **Step 3: Implement the wiring**

Add to `xtask/src/dist.rs` (above the private helpers):

```rust
/// Builds one target's release binary and packages it with its
/// checksum under `target/dist/`.
pub fn run(target: Option<&str>) -> Result<()> {
    let root = crate::workspace_root()?;
    let triple = match target {
        Some(triple) => triple.to_owned(),
        None => host_triple()?,
    };
    build(&root, &triple)?;
    let binary = root
        .join("target")
        .join(&triple)
        .join("release")
        .join(binary_file_name(&triple));
    let output_directory = root.join("target/dist");
    std::fs::create_dir_all(&output_directory)?;
    let documentation: Vec<PathBuf> = DOCUMENTATION_FILES
        .iter()
        .map(|name| root.join(name))
        .collect();
    let archive = package(&binary, &documentation, &triple, &output_directory)?;
    let checksum = write_checksum(&archive)?;
    println!("packaged {}", archive.display());
    println!("checksum {}", checksum.display());
    Ok(())
}

/// The running toolchain's target, from `rustc -vV`. Passing the
/// resolved triple to `cargo build --target` keeps the output path
/// uniform (`target/<triple>/release/`) whether or not `--target`
/// was given on the command line.
fn host_triple() -> Result<String> {
    let output = Command::new("rustc").args(["-vV"]).output()?;
    if !output.status.success() {
        return Err("rustc -vV failed".into());
    }
    let stdout = String::from_utf8(output.stdout)?;
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(|host| host.trim().to_owned())
        .ok_or_else(|| "rustc -vV printed no host line".into())
}

fn build(root: &Path, triple: &str) -> Result<()> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let status = Command::new(cargo)
        .current_dir(root)
        .args([
            "build",
            "--release",
            "--package",
            "celerrate_cli",
            "--target",
            triple,
        ])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("the release build for {triple} failed").into())
    }
}

/// Writes `<archive>.sha256` next to the archive.
fn write_checksum(archive_path: &Path) -> Result<PathBuf> {
    let contents = std::fs::read(archive_path)?;
    let name = file_name(archive_path)?;
    let checksum_path = archive_path.with_file_name(format!("{name}.sha256"));
    std::fs::write(&checksum_path, checksum_line(&contents, &name))?;
    Ok(checksum_path)
}
```

- [ ] **Step 4: Rewrite the `main.rs` argument match**

`dist --target <triple>` is three tokens; the current two-slot matcher cannot express it. Replace the body of `xtask/src/main.rs` with a slice match (same arms, one added):

```rust
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let argument_references: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let outcome = match argument_references.as_slice() {
        ["bench"] => xtask::bench::run(false),
        ["bench", "--ceilings"] => xtask::bench::run(true),
        ["memory"] => xtask::memory::run(false),
        ["memory", "--ceiling"] => xtask::memory::run(true),
        ["codegen"] => xtask::codegen::run(),
        ["dependency-shape"] => xtask::dependency_shape::run(),
        ["dist"] => xtask::dist::run(None),
        ["dist", "--target", triple] => xtask::dist::run(Some(triple)),
        ["emission-scan"] => xtask::emission_scan::run(),
        ["fetch-stubs"] => xtask::stubs::fetch(),
        ["compile-stubs"] => xtask::stubs::compile(false),
        ["compile-stubs", "--check"] => xtask::stubs::compile(true),
        ["fetch-corpus"] => xtask::corpus::prepare().map(|_| ()),
        ["corpus"] => xtask::corpus::check_snapshot(false),
        ["corpus", "--bless"] => xtask::corpus::check_snapshot(true),
        ["ground-truth"] => xtask::ground_truth::run(false),
        ["ground-truth", "--bless"] => xtask::ground_truth::run(true),
        ["mixed-rate"] => xtask::mixed_rate::check(false),
        ["mixed-rate", "--bless"] => xtask::mixed_rate::check(true),
        ["fetch-phpdoc-parser"] => xtask::phpdoc_corpus::fetch().map(|_| ()),
        ["phpdoc-cases"] => xtask::phpdoc_corpus::extract(false),
        ["phpdoc-cases", "--check"] => xtask::phpdoc_corpus::extract(true),
        ["release-notes", version] => xtask::release::run(version),
        _ => {
            eprintln!(
                "usage: cargo xtask <codegen | dependency-shape | dist [--target <triple>] | emission-scan | fetch-stubs | compile-stubs [--check] | fetch-corpus | corpus [--bless] | ground-truth [--bless] | mixed-rate [--bless] | fetch-phpdoc-parser | phpdoc-cases [--check] | bench [--ceilings] | memory [--ceiling] | release-notes <version>>"
            );
            return ExitCode::FAILURE;
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
```

Also extend the module documentation of `xtask/src/lib.rs`: after the `release-notes` clause, mention that `dist` builds and packages one target's release archive with its checksum. Keep the sentence style of the surrounding paragraph.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --package xtask dist`
Expected: 6 passed.

- [ ] **Step 6: Run the command end to end**

Run: `cargo xtask dist`
Expected: a release build, then `packaged .../target/dist/celerrate-<host triple>.tar.gz` and `checksum .../target/dist/celerrate-<host triple>.tar.gz.sha256`.

Run: `tar -tzf target/dist/celerrate-*.tar.gz`
Expected: exactly four entries under `celerrate-<host triple>/`: `celerrate`, `LICENSE-MIT`, `LICENSE-APACHE`, `README.md`.

Run: `cd target/dist && shasum -a 256 -c celerrate-*.sha256 && cd ../..`
Expected: `celerrate-<host triple>.tar.gz: OK`.

- [ ] **Step 7: Lints and format**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add xtask/src/dist.rs xtask/src/main.rs xtask/src/lib.rs
git commit -m "✨ feat(xtask): build and package one target with cargo xtask dist"
```

---

### Task 3: The release workflow delegates to `xtask dist` and attests its artifacts

**Files:**
- Modify: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: `cargo xtask dist --target <triple>` from Task 2, producing `target/dist/celerrate-<triple>.{tar.gz|zip}` and `.sha256`.
- Produces: the GitHub Release layout every consumer downloads from: the five archives, `SHA256SUMS`, provenance attestations. `install.sh` (Task 4) and the Composer plugin (Task 7) depend on the archive names and the `SHA256SUMS` file at `releases/download/<tag>/`.

- [ ] **Step 1: Rewrite the workflow**

Replace the content of `.github/workflows/release.yml` with:

```yaml
name: Release

on:
  push:
    tags: ["v*"]
  workflow_dispatch:

permissions:
  contents: read

env:
  CARGO_TERM_COLOR: always

jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: x86_64-unknown-linux-musl
            runner: ubuntu-latest
          - target: aarch64-unknown-linux-musl
            runner: ubuntu-24.04-arm
          - target: x86_64-apple-darwin
            runner: macos-15
          - target: aarch64-apple-darwin
            runner: macos-15
          - target: x86_64-pc-windows-msvc
            runner: windows-latest
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}
      - if: contains(matrix.target, 'musl')
        run: sudo apt-get update && sudo apt-get install --yes musl-tools
      - run: cargo xtask dist --target ${{ matrix.target }}
      - uses: actions/upload-artifact@v4
        with:
          name: celerrate-${{ matrix.target }}
          path: |
            target/dist/celerrate-*.tar.gz
            target/dist/celerrate-*.zip
            target/dist/celerrate-*.sha256
          if-no-files-found: error

  publish:
    if: startsWith(github.ref, 'refs/tags/')
    needs: build
    runs-on: ubuntu-latest
    permissions:
      contents: write
      id-token: write
      attestations: write
    steps:
      - uses: actions/checkout@v7
      - uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
      - name: Check the tag against the workspace version
        run: |
          version="${GITHUB_REF_NAME#v}"
          grep --quiet "^version = \"$version\"\$" Cargo.toml
      - uses: actions/download-artifact@v4
        with:
          path: artifacts
          merge-multiple: true
      - name: Assemble and verify the checksums
        run: |
          cd artifacts
          cat -- *.sha256 | sort -k 2 > SHA256SUMS
          rm -- *.sha256
          sha256sum --check SHA256SUMS
      - name: Attest the build provenance
        uses: actions/attest-build-provenance@v3
        with:
          subject-path: |
            artifacts/celerrate-*.tar.gz
            artifacts/celerrate-*.zip
      - name: Create the release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          cargo xtask release-notes "${GITHUB_REF_NAME#v}" > notes.md
          gh release create "$GITHUB_REF_NAME" --title "$GITHUB_REF_NAME" --notes-file notes.md artifacts/*
```

What changed and why, for the reviewer:
- The inline Package/Package (Windows) steps are gone; `cargo xtask dist --target` is the only packaging logic (the spec's load-bearing decision).
- Per-archive `.sha256` files travel as artifacts; the publish job concatenates them into `SHA256SUMS`, then re-verifies with `sha256sum --check`, which also proves the artifacts survived upload/download intact.
- `actions/attest-build-provenance` covers the archives; the workflow-level permission drops to `contents: read` and the publish job carries the elevated ones.

- [ ] **Step 2: Verify the YAML parses**

Run: `ruby -ryaml -e 'YAML.load_file(".github/workflows/release.yml"); puts "ok"'`
Expected: `ok`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "👷 ci(release): package through xtask dist and attest the artifacts"
```

Note for the final task: the executable proof of this workflow is a `workflow_dispatch` dry-run after merge; it cannot run from a branch-local check.

---

### Task 4: `install.sh`

**Files:**
- Create: `install.sh` (repository root, mode 755)

**Interfaces:**
- Consumes: the release layout from Task 3 (`celerrate-<triple>.tar.gz` and `SHA256SUMS` under a common base URL) and, for the local test, `target/dist/` from Task 2.
- Produces: `install.sh [--version vX.Y.Z] [--to <directory>]`, honoring `CELERRATE_INSTALL_BASE_URL`. Task 5 runs it in CI; Task 9 documents it.

- [ ] **Step 1: Write the script**

Create `install.sh`:

```sh
#!/bin/sh
# Celerrate installer: downloads the release binary for this platform,
# verifies its SHA-256 checksum, and installs it into ~/.local/bin.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/celerrate/celerrate/main/install.sh | sh
#   install.sh [--version vX.Y.Z] [--to <directory>]
#
# CELERRATE_INSTALL_BASE_URL overrides the download base (corporate
# mirrors, hermetic tests). It replaces the whole
# .../releases/download/<tag> base, so the URL it names must serve the
# release archives and the SHA256SUMS file directly.
set -eu

repository="celerrate/celerrate"
version=""
install_directory="${HOME}/.local/bin"

usage() {
    echo "usage: install.sh [--version vX.Y.Z] [--to <directory>]"
}

fail() {
    echo "error: $1" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || fail "--version needs a value, for example: --version v0.1.0"
            version="$2"
            shift 2
            ;;
        --to)
            [ "$#" -ge 2 ] || fail "--to needs a directory"
            install_directory="$2"
            shift 2
            ;;
        --help | -h)
            usage
            exit 0
            ;;
        *)
            usage >&2
            fail "unknown argument: $1"
            ;;
    esac
done

operating_system="$(uname -s)"
machine="$(uname -m)"
case "$operating_system" in
    Linux)
        case "$machine" in
            x86_64) target="x86_64-unknown-linux-musl" ;;
            aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
            *) fail "unsupported architecture: $machine (supported: x86_64, aarch64)" ;;
        esac
        ;;
    Darwin)
        case "$machine" in
            x86_64) target="x86_64-apple-darwin" ;;
            arm64 | aarch64) target="aarch64-apple-darwin" ;;
            *) fail "unsupported architecture: $machine (supported: x86_64, arm64)" ;;
        esac
        ;;
    *)
        fail "unsupported operating system: $operating_system. On Windows, download the zip archive from https://github.com/${repository}/releases or use the Composer package: composer require --dev celerrate/celerrate"
        ;;
esac

archive="celerrate-${target}.tar.gz"

if [ -n "${CELERRATE_INSTALL_BASE_URL:-}" ]; then
    base_url="$CELERRATE_INSTALL_BASE_URL"
elif [ -n "$version" ]; then
    base_url="https://github.com/${repository}/releases/download/${version}"
else
    base_url="https://github.com/${repository}/releases/latest/download"
fi

command -v curl >/dev/null 2>&1 || fail "curl is required"
if command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1; then
    :
else
    fail "neither sha256sum nor shasum is available; one is required to verify the download"
fi

# Reads sha256sum-format lines on stdin and verifies them against the
# files in the current directory, with whichever tool the platform has.
verify_checksum() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum --check - >/dev/null 2>&1
    else
        shasum -a 256 --check - >/dev/null 2>&1
    fi
}

temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT

echo "downloading ${base_url}/${archive}"
curl -fsSL --output "${temporary_directory}/${archive}" "${base_url}/${archive}" \
    || fail "downloading ${archive} failed; check the version and your network"
curl -fsSL --output "${temporary_directory}/SHA256SUMS" "${base_url}/SHA256SUMS" \
    || fail "downloading SHA256SUMS failed; refusing to install an unverified binary"

expected_line="$(grep " ${archive}\$" "${temporary_directory}/SHA256SUMS")" \
    || fail "SHA256SUMS has no entry for ${archive}"
(cd "$temporary_directory" && echo "$expected_line" | verify_checksum) \
    || fail "checksum verification failed for ${archive}; refusing to install"

tar -xzf "${temporary_directory}/${archive}" -C "$temporary_directory"
mkdir -p "$install_directory"
install -m 755 "${temporary_directory}/celerrate-${target}/celerrate" "${install_directory}/celerrate"

echo "installed ${install_directory}/celerrate"
case ":${PATH}:" in
    *":${install_directory}:"*) ;;
    *) echo "note: ${install_directory} is not on your PATH; add it to your shell profile" ;;
esac
"${install_directory}/celerrate" --version
```

Then: `chmod 755 install.sh`

- [ ] **Step 2: Shellcheck**

Run: `shellcheck install.sh`
Expected: no output, exit 0. Fix any finding; do not suppress with directives unless the finding is a false positive, and then justify the directive with a comment.

- [ ] **Step 3: Run the hermetic install locally**

```bash
cargo xtask dist
cat target/dist/*.sha256 > target/dist/SHA256SUMS
destination="$(mktemp -d)"
CELERRATE_INSTALL_BASE_URL="file://$PWD/target/dist" sh install.sh --to "$destination"
"$destination/celerrate" --version
```

Expected: `downloading file://...`, `installed .../celerrate`, the PATH note, and a version line reporting 0.0.3.

- [ ] **Step 4: Verify the failure paths**

```bash
corrupted="$(mktemp -d)"
cp target/dist/celerrate-*.tar.gz "$corrupted/"
printf '%s  %s\n' "0000000000000000000000000000000000000000000000000000000000000000" "$(cd target/dist && ls celerrate-*.tar.gz)" > "$corrupted/SHA256SUMS"
CELERRATE_INSTALL_BASE_URL="file://$corrupted" sh install.sh --to "$(mktemp -d)" ; echo "exit: $?"
rm -rf "$corrupted"
```

Expected: `error: checksum verification failed ...; refusing to install` and `exit: 1`.

- [ ] **Step 5: Commit**

```bash
git add install.sh
git commit -m "✨ feat(dist): add the checksum-verified install script"
```

---

### Task 5: The CI `dist` job: shellcheck, determinism, the install script end to end

**Files:**
- Modify: `.github/workflows/ci.yml` (append one job after `rules-render`)

**Interfaces:**
- Consumes: `cargo xtask dist --target` (Task 2), `install.sh` (Task 4).
- Produces: the `dist` CI job. Task 8 appends the Composer steps to this same job (it reuses the freshly packaged `target/dist/` artifacts).

- [ ] **Step 1: Append the job**

Every step gates on `needs.changes.outputs.code == 'true'`, like every other job in this file (required branch-protection contexts must never skip at the job level; see the comment atop `ci.yml`). The Linux runner packages the musl target (the published Linux triple; the runner's host triple is gnu, which is not a release target), the macOS runner its native arm64 triple:

```yaml
  dist:
    needs: changes
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-musl
          - os: macos-latest
            target: aarch64-apple-darwin
    runs-on: ${{ matrix.os }}
    steps:
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: actions/checkout@v7
      - if: ${{ needs.changes.outputs.code == 'true' && runner.os == 'Linux' }}
        run: shellcheck install.sh
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: dtolnay/rust-toolchain@v1
        with:
          toolchain: "1.94"
          targets: ${{ matrix.target }}
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: Swatinem/rust-cache@v2
        with:
          key: dist-${{ matrix.target }}
      - if: ${{ needs.changes.outputs.code == 'true' && contains(matrix.target, 'musl') }}
        run: sudo apt-get update && sudo apt-get install --yes musl-tools
      - if: ${{ needs.changes.outputs.code == 'true' }}
        run: cargo xtask dist --target ${{ matrix.target }}
      - if: ${{ needs.changes.outputs.code == 'true' }}
        name: Check packaging determinism
        run: |
          mkdir determinism
          cp target/dist/celerrate-* determinism/
          rm -r target/dist
          cargo xtask dist --target ${{ matrix.target }}
          for file in determinism/*; do
            cmp "$file" "target/dist/$(basename "$file")"
          done
      - if: ${{ needs.changes.outputs.code == 'true' }}
        name: Install through install.sh
        run: |
          cat target/dist/*.sha256 > target/dist/SHA256SUMS
          CELERRATE_INSTALL_BASE_URL="file://$PWD/target/dist" sh install.sh --to "$RUNNER_TEMP/celerrate-bin"
          "$RUNNER_TEMP/celerrate-bin/celerrate" --version
```

- [ ] **Step 2: Verify the YAML parses**

Run: `ruby -ryaml -e 'YAML.load_file(".github/workflows/ci.yml"); puts "ok"'`
Expected: `ok`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "👷 ci: prove packaging determinism and the install script on tier 1"
```

Note: the job's real proof runs when the pull request opens; watch it there.

---

### Task 6: The Composer bootstrap package: scaffold and pure logic

**Files:**
- Create: `packages/composer-bootstrap/composer.json`
- Create: `packages/composer-bootstrap/.gitignore`
- Create: `packages/composer-bootstrap/phpunit.xml.dist`
- Create: `packages/composer-bootstrap/src/Platform.php`
- Create: `packages/composer-bootstrap/src/ReleaseUrl.php`
- Create: `packages/composer-bootstrap/src/Checksum.php`
- Test: `packages/composer-bootstrap/tests/PlatformTest.php`, `tests/ReleaseUrlTest.php`, `tests/ChecksumTest.php`

**Interfaces:**
- Consumes: nothing from the Rust side; only the fixed archive/checksum naming from the Global Constraints.
- Produces (Task 7 consumes these exact signatures, namespace `Celerrate\Bootstrap`):
  - `Platform::targetTriple(string $operatingSystemFamily, string $machine): ?string`
  - `Platform::archiveFileName(string $targetTriple): string`
  - `Platform::binaryFileName(string $targetTriple): string`
  - `ReleaseUrl::baseUrl(string $packageVersion, ?string $override): ?string`
  - `Checksum::expectedFor(string $fileName, string $sums): ?string`
  - `Checksum::matches(string $filePath, string $expectedHash): bool`

- [ ] **Step 1: Scaffold the package**

`packages/composer-bootstrap/composer.json`:

```json
{
    "name": "celerrate/celerrate",
    "description": "Celerrate for Composer projects: installs the platform's celerrate binary and exposes vendor/bin/celerrate",
    "type": "composer-plugin",
    "license": ["MIT", "Apache-2.0"],
    "keywords": ["static analysis", "php", "phpstan", "analyzer"],
    "require": {
        "php": ">=7.4",
        "composer-plugin-api": "^2.0"
    },
    "require-dev": {
        "composer/composer": "^2.0",
        "phpunit/phpunit": "^9.6 || ^11.5 || ^12.0"
    },
    "autoload": {
        "psr-4": {
            "Celerrate\\Bootstrap\\": "src/"
        }
    },
    "autoload-dev": {
        "psr-4": {
            "Celerrate\\Bootstrap\\Tests\\": "tests/"
        }
    },
    "extra": {
        "class": "Celerrate\\Bootstrap\\Plugin"
    },
    "bin": ["celerrate"],
    "scripts": {
        "test": "phpunit"
    }
}
```

The `phpunit` constraint spans majors so the package installs under PHP 7.4 (CI proves the floor with 9.6) and under a current local PHP (11/12). The `extra.class` plugin entry point and the `celerrate` bin shim arrive in Task 7; declaring them now is fine — Composer only resolves them when the package is installed as a dependency, which first happens in Task 8.

`packages/composer-bootstrap/.gitignore`:

```
/vendor/
/composer.lock
/bin-cache/
/bin-cache.download/
```

`packages/composer-bootstrap/phpunit.xml.dist` (minimal, valid for PHPUnit 9 through 12):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<phpunit bootstrap="vendor/autoload.php" colors="true">
    <testsuites>
        <testsuite name="bootstrap">
            <directory>tests</directory>
        </testsuite>
    </testsuites>
</phpunit>
```

- [ ] **Step 2: Write the failing tests**

`packages/composer-bootstrap/tests/PlatformTest.php`:

```php
<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap\Tests;

use Celerrate\Bootstrap\Platform;
use PHPUnit\Framework\TestCase;

final class PlatformTest extends TestCase
{
    public function testMapsTheSupportedPlatformsToTheReleaseTriples(): void
    {
        self::assertSame('x86_64-unknown-linux-musl', Platform::targetTriple('Linux', 'x86_64'));
        self::assertSame('aarch64-unknown-linux-musl', Platform::targetTriple('Linux', 'aarch64'));
        self::assertSame('x86_64-apple-darwin', Platform::targetTriple('Darwin', 'x86_64'));
        self::assertSame('aarch64-apple-darwin', Platform::targetTriple('Darwin', 'arm64'));
        self::assertSame('x86_64-pc-windows-msvc', Platform::targetTriple('Windows', 'AMD64'));
    }

    public function testReturnsNullForUnsupportedPlatforms(): void
    {
        self::assertNull(Platform::targetTriple('BSD', 'x86_64'));
        self::assertNull(Platform::targetTriple('Linux', 'riscv64'));
        self::assertNull(Platform::targetTriple('Windows', 'arm64'));
    }

    public function testArchiveAndBinaryNamesFollowTheTargetFamily(): void
    {
        self::assertSame(
            'celerrate-x86_64-unknown-linux-musl.tar.gz',
            Platform::archiveFileName('x86_64-unknown-linux-musl')
        );
        self::assertSame(
            'celerrate-x86_64-pc-windows-msvc.zip',
            Platform::archiveFileName('x86_64-pc-windows-msvc')
        );
        self::assertSame('celerrate', Platform::binaryFileName('aarch64-apple-darwin'));
        self::assertSame('celerrate.exe', Platform::binaryFileName('x86_64-pc-windows-msvc'));
    }
}
```

`packages/composer-bootstrap/tests/ReleaseUrlTest.php`:

```php
<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap\Tests;

use Celerrate\Bootstrap\ReleaseUrl;
use PHPUnit\Framework\TestCase;

final class ReleaseUrlTest extends TestCase
{
    public function testBuildsTheGithubReleaseBaseForATaggedVersion(): void
    {
        self::assertSame(
            'https://github.com/celerrate/celerrate/releases/download/v0.1.0',
            ReleaseUrl::baseUrl('0.1.0', null)
        );
    }

    public function testDoesNotDoubleALeadingV(): void
    {
        self::assertSame(
            'https://github.com/celerrate/celerrate/releases/download/v0.1.0',
            ReleaseUrl::baseUrl('v0.1.0', null)
        );
    }

    public function testTheOverrideWinsAndLosesItsTrailingSlash(): void
    {
        self::assertSame(
            'http://127.0.0.1:8737',
            ReleaseUrl::baseUrl('0.1.0', 'http://127.0.0.1:8737/')
        );
        self::assertSame(
            'http://127.0.0.1:8737',
            ReleaseUrl::baseUrl('dev-main', 'http://127.0.0.1:8737')
        );
    }

    public function testDevelopmentVersionsHaveNoReleaseToDownloadFrom(): void
    {
        self::assertNull(ReleaseUrl::baseUrl('dev-main', null));
        self::assertNull(ReleaseUrl::baseUrl('0.1.x-dev', null));
    }
}
```

`packages/composer-bootstrap/tests/ChecksumTest.php`:

```php
<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap\Tests;

use Celerrate\Bootstrap\Checksum;
use PHPUnit\Framework\TestCase;

final class ChecksumTest extends TestCase
{
    private const EMPTY_HASH = 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855';

    public function testFindsTheHashForAFileNameInASumsBody(): void
    {
        $sums = self::EMPTY_HASH . "  celerrate-a.tar.gz\n"
            . str_repeat('a', 64) . "  celerrate-b.tar.gz\n";
        self::assertSame(self::EMPTY_HASH, Checksum::expectedFor('celerrate-a.tar.gz', $sums));
        self::assertSame(str_repeat('a', 64), Checksum::expectedFor('celerrate-b.tar.gz', $sums));
    }

    public function testReturnsNullWhenTheFileHasNoEntry(): void
    {
        self::assertNull(Checksum::expectedFor('celerrate-c.tar.gz', self::EMPTY_HASH . "  celerrate-a.tar.gz\n"));
        self::assertNull(Checksum::expectedFor('celerrate-a.tar.gz', ''));
    }

    public function testMatchesComparesTheFileAgainstTheExpectedHash(): void
    {
        $path = tempnam(sys_get_temp_dir(), 'celerrate-checksum-test');
        self::assertIsString($path);
        file_put_contents($path, '');
        self::assertTrue(Checksum::matches($path, self::EMPTY_HASH));
        self::assertFalse(Checksum::matches($path, str_repeat('0', 64)));
        unlink($path);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

```bash
composer install --working-dir packages/composer-bootstrap
composer test --working-dir packages/composer-bootstrap
```

Expected: PHPUnit errors, the `Celerrate\Bootstrap\Platform`, `ReleaseUrl`, and `Checksum` classes do not exist.

- [ ] **Step 4: Implement the three classes**

`packages/composer-bootstrap/src/Platform.php`:

```php
<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap;

/**
 * Maps the running platform onto the release target triple and the
 * artifact names the release publishes for it.
 */
final class Platform
{
    /** Returns the target triple, or null when the platform is unsupported. */
    public static function targetTriple(string $operatingSystemFamily, string $machine): ?string
    {
        $machine = strtolower($machine);
        $isX64 = in_array($machine, ['x86_64', 'amd64'], true);
        $isArm64 = in_array($machine, ['aarch64', 'arm64'], true);
        switch ($operatingSystemFamily) {
            case 'Linux':
                if ($isX64) {
                    return 'x86_64-unknown-linux-musl';
                }
                return $isArm64 ? 'aarch64-unknown-linux-musl' : null;
            case 'Darwin':
                if ($isX64) {
                    return 'x86_64-apple-darwin';
                }
                return $isArm64 ? 'aarch64-apple-darwin' : null;
            case 'Windows':
                return $isX64 ? 'x86_64-pc-windows-msvc' : null;
            default:
                return null;
        }
    }

    public static function archiveFileName(string $targetTriple): string
    {
        $extension = strpos($targetTriple, 'windows') !== false ? 'zip' : 'tar.gz';
        return "celerrate-{$targetTriple}.{$extension}";
    }

    public static function binaryFileName(string $targetTriple): string
    {
        return strpos($targetTriple, 'windows') !== false ? 'celerrate.exe' : 'celerrate';
    }
}
```

`packages/composer-bootstrap/src/ReleaseUrl.php`:

```php
<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap;

/**
 * Resolves where the artifacts download from. The binary version is
 * locked 1:1 to the package version: the base URL always names the
 * release tag matching the installed package.
 */
final class ReleaseUrl
{
    /**
     * The override wins when set (corporate mirrors, hermetic tests).
     * A development version has no release to download from: null, and
     * the caller reports it.
     */
    public static function baseUrl(string $packageVersion, ?string $override): ?string
    {
        if ($override !== null && $override !== '') {
            return rtrim($override, '/');
        }
        if (strpos($packageVersion, 'dev-') === 0 || substr($packageVersion, -4) === '-dev') {
            return null;
        }
        $tag = 'v' . ltrim($packageVersion, 'v');
        return "https://github.com/celerrate/celerrate/releases/download/{$tag}";
    }
}
```

`packages/composer-bootstrap/src/Checksum.php`:

```php
<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap;

/** SHA-256 verification against the release's SHA256SUMS body. */
final class Checksum
{
    /** Returns the hex hash recorded for the file, or null when absent. */
    public static function expectedFor(string $fileName, string $sums): ?string
    {
        foreach (preg_split('/\r?\n/', $sums) ?: [] as $line) {
            if (preg_match('/^([0-9a-f]{64})[ \t]+\*?(.+)$/', trim($line), $matches) === 1
                && $matches[2] === $fileName
            ) {
                return $matches[1];
            }
        }
        return null;
    }

    public static function matches(string $filePath, string $expectedHash): bool
    {
        $actual = hash_file('sha256', $filePath);
        return is_string($actual) && hash_equals($expectedHash, $actual);
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `composer test --working-dir packages/composer-bootstrap`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add packages/composer-bootstrap
git commit -m "✨ feat(composer): scaffold the bootstrap package with its platform logic"
```

---

### Task 7: The Composer plugin: download, verify, extract, expose

**Files:**
- Create: `packages/composer-bootstrap/src/Plugin.php`
- Create: `packages/composer-bootstrap/src/BinaryInstaller.php`
- Create: `packages/composer-bootstrap/src/Archive.php`
- Create: `packages/composer-bootstrap/celerrate` (the bin shim, mode 755)
- Test: `packages/composer-bootstrap/tests/ArchiveTest.php`

**Interfaces:**
- Consumes: `Platform`, `ReleaseUrl`, `Checksum` (Task 6, exact signatures listed there); Composer's plugin API (`PluginInterface`, `EventSubscriberInterface`, `ScriptEvents`, `HttpDownloader`).
- Produces:
  - `Archive::extractBinary(string $archivePath, string $targetTriple, string $destinationDirectory): string` (returns the extracted binary path)
  - `Plugin` registered through `extra.class`, installing the binary on `post-install-cmd`/`post-update-cmd` into `<package>/bin-cache/`
  - the `celerrate` shim resolved by Composer's `vendor/bin` proxying (`.bat` on Windows is generated by Composer itself)
  - Environment contract: `CELERRATE_BINARY` skips the download; `CELERRATE_DOWNLOAD_BASE_URL` overrides the release base URL. Task 8's fixture proves both the default path and the override.

- [ ] **Step 1: Write the failing extraction test**

`packages/composer-bootstrap/tests/ArchiveTest.php` — the fixture archive is built with `PharData` itself (tar writing does not require `phar.readonly` off; that ini setting only restricts `.phar` files):

```php
<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap\Tests;

use Celerrate\Bootstrap\Archive;
use PHPUnit\Framework\TestCase;

final class ArchiveTest extends TestCase
{
    private const TRIPLE = 'x86_64-unknown-linux-musl';

    private function makeReleaseArchive(string $directory): string
    {
        $stage = $directory . '/stage/celerrate-' . self::TRIPLE;
        mkdir($stage, 0755, true);
        file_put_contents($stage . '/celerrate', "#!/bin/sh\necho fake\n");
        file_put_contents($stage . '/LICENSE-MIT', "license\n");
        $tarPath = $directory . '/celerrate-' . self::TRIPLE . '.tar';
        $archive = new \PharData($tarPath);
        $archive->buildFromDirectory($directory . '/stage');
        $archive->compress(\Phar::GZ);
        unlink($tarPath);
        return $tarPath . '.gz';
    }

    public function testExtractsTheBinaryOutOfATarGzArchive(): void
    {
        $directory = sys_get_temp_dir() . '/celerrate-archive-test-' . bin2hex(random_bytes(8));
        mkdir($directory, 0755, true);
        $archivePath = $this->makeReleaseArchive($directory);
        $binary = Archive::extractBinary($archivePath, self::TRIPLE, $directory . '/out');
        self::assertFileExists($binary);
        self::assertStringEndsWith('celerrate-' . self::TRIPLE . '/celerrate', $binary);
        self::assertSame("#!/bin/sh\necho fake\n", file_get_contents($binary));
    }

    public function testRefusesAnArchiveWithoutTheExpectedBinary(): void
    {
        $directory = sys_get_temp_dir() . '/celerrate-archive-test-' . bin2hex(random_bytes(8));
        mkdir($directory . '/stage/unexpected', 0755, true);
        file_put_contents($directory . '/stage/unexpected/file', "body\n");
        $tarPath = $directory . '/celerrate-' . self::TRIPLE . '.tar';
        $archive = new \PharData($tarPath);
        $archive->buildFromDirectory($directory . '/stage');
        $archive->compress(\Phar::GZ);
        unlink($tarPath);
        $this->expectException(\RuntimeException::class);
        Archive::extractBinary($tarPath . '.gz', self::TRIPLE, $directory . '/out');
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `composer test --working-dir packages/composer-bootstrap`
Expected: `Celerrate\Bootstrap\Archive` does not exist.

- [ ] **Step 3: Implement `Archive`**

`packages/composer-bootstrap/src/Archive.php`:

```php
<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap;

/** Extraction of the release archives (PharData: no ext-zip needed). */
final class Archive
{
    /**
     * Extracts a release archive and returns the path of the binary it
     * must contain at celerrate-<triple>/<binary name>.
     */
    public static function extractBinary(string $archivePath, string $targetTriple, string $destinationDirectory): string
    {
        if (!is_dir($destinationDirectory) && !mkdir($destinationDirectory, 0755, true)) {
            throw new \RuntimeException("celerrate: cannot create {$destinationDirectory}");
        }
        $archive = new \PharData($archivePath);
        if (substr($archivePath, -7) === '.tar.gz') {
            // decompress() writes the sibling .tar and returns a
            // PharData over it.
            $archive = $archive->decompress();
        }
        $archive->extractTo($destinationDirectory, null, true);
        $binaryPath = $destinationDirectory . '/celerrate-' . $targetTriple . '/' . Platform::binaryFileName($targetTriple);
        if (!is_file($binaryPath)) {
            throw new \RuntimeException(
                "celerrate: the archive does not contain celerrate-{$targetTriple}/" . Platform::binaryFileName($targetTriple)
            );
        }
        chmod($binaryPath, 0755);
        return $binaryPath;
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `composer test --working-dir packages/composer-bootstrap`
Expected: all tests pass.

- [ ] **Step 5: Implement the installer and the plugin**

`packages/composer-bootstrap/src/BinaryInstaller.php`:

```php
<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap;

use Composer\Composer;
use Composer\IO\IOInterface;
use Composer\Util\HttpDownloader;

/**
 * Downloads the release binary matching the installed package version,
 * verifies its SHA-256 against the release's SHA256SUMS, and places it
 * in <package>/bin-cache/ where the shim finds it.
 *
 * Failure stance: every failure is loud and actionable (a checksum
 * mismatch or a missing SHA256SUMS entry aborts), with one tolerance:
 * an unsupported platform warns and skips, because the bootstrap
 * package must never make the host project uninstallable. The shim
 * carries the error if celerrate is actually invoked.
 */
final class BinaryInstaller
{
    public function install(Composer $composer, IOInterface $io): void
    {
        $external = getenv('CELERRATE_BINARY');
        if (is_string($external) && $external !== '') {
            $io->write("celerrate: using the external binary at {$external} (CELERRATE_BINARY)");
            return;
        }
        $triple = Platform::targetTriple(PHP_OS_FAMILY, php_uname('m'));
        if ($triple === null) {
            $io->writeError(
                '<warning>celerrate: unsupported platform (' . PHP_OS_FAMILY . '/' . php_uname('m')
                . '); the binary was not installed. See https://github.com/celerrate/celerrate for the other install channels.</warning>'
            );
            return;
        }
        $package = $composer->getRepositoryManager()->getLocalRepository()->findPackage('celerrate/celerrate', '*');
        if ($package === null) {
            return;
        }
        $packageDirectory = $composer->getInstallationManager()->getInstallPath($package);
        if (!is_string($packageDirectory) || $packageDirectory === '') {
            return;
        }
        $binaryPath = $packageDirectory . '/bin-cache/' . Platform::binaryFileName($triple);
        if (is_file($binaryPath)) {
            return;
        }
        $override = getenv('CELERRATE_DOWNLOAD_BASE_URL');
        $baseUrl = ReleaseUrl::baseUrl(
            $package->getPrettyVersion(),
            is_string($override) && $override !== '' ? $override : null
        );
        if ($baseUrl === null) {
            throw new \RuntimeException(
                'celerrate: version ' . $package->getPrettyVersion()
                . ' is a development version with no released binary; require a tagged release,'
                . ' or point CELERRATE_BINARY at an existing binary.'
            );
        }
        $archiveName = Platform::archiveFileName($triple);
        $downloader = new HttpDownloader($io, $composer->getConfig());
        $io->write("celerrate: downloading {$baseUrl}/{$archiveName}");
        $archiveBody = (string) $downloader->get("{$baseUrl}/{$archiveName}")->getBody();
        $sums = (string) $downloader->get("{$baseUrl}/SHA256SUMS")->getBody();
        $expected = Checksum::expectedFor($archiveName, $sums);
        if ($expected === null) {
            throw new \RuntimeException(
                "celerrate: SHA256SUMS has no entry for {$archiveName}; refusing to install an unverified binary."
            );
        }
        $workingDirectory = $packageDirectory . '/bin-cache.download';
        self::removeDirectory($workingDirectory);
        if (!mkdir($workingDirectory, 0755, true)) {
            throw new \RuntimeException("celerrate: cannot create {$workingDirectory}");
        }
        try {
            $archivePath = $workingDirectory . '/' . $archiveName;
            file_put_contents($archivePath, $archiveBody);
            if (!Checksum::matches($archivePath, $expected)) {
                throw new \RuntimeException(
                    "celerrate: checksum verification failed for {$archiveName}; refusing to install."
                );
            }
            $extracted = Archive::extractBinary($archivePath, $triple, $workingDirectory);
            $binaryDirectory = dirname($binaryPath);
            if (!is_dir($binaryDirectory) && !mkdir($binaryDirectory, 0755, true)) {
                throw new \RuntimeException("celerrate: cannot create {$binaryDirectory}");
            }
            if (!rename($extracted, $binaryPath)) {
                throw new \RuntimeException("celerrate: cannot move the binary into {$binaryPath}");
            }
            chmod($binaryPath, 0755);
        } finally {
            self::removeDirectory($workingDirectory);
        }
        $io->write("celerrate: installed the {$triple} binary");
    }

    private static function removeDirectory(string $directory): void
    {
        if (!is_dir($directory)) {
            return;
        }
        $entries = new \RecursiveIteratorIterator(
            new \RecursiveDirectoryIterator($directory, \FilesystemIterator::SKIP_DOTS),
            \RecursiveIteratorIterator::CHILD_FIRST
        );
        foreach ($entries as $entry) {
            $entry->isDir() ? rmdir($entry->getPathname()) : unlink($entry->getPathname());
        }
        rmdir($directory);
    }
}
```

`packages/composer-bootstrap/src/Plugin.php`:

```php
<?php

declare(strict_types=1);

namespace Celerrate\Bootstrap;

use Composer\Composer;
use Composer\EventDispatcher\EventSubscriberInterface;
use Composer\IO\IOInterface;
use Composer\Plugin\PluginInterface;
use Composer\Script\ScriptEvents;

/** The composer-plugin entry point declared in extra.class. */
final class Plugin implements PluginInterface, EventSubscriberInterface
{
    /** @var Composer */
    private $composer;

    /** @var IOInterface */
    private $io;

    public function activate(Composer $composer, IOInterface $io): void
    {
        $this->composer = $composer;
        $this->io = $io;
    }

    public function deactivate(Composer $composer, IOInterface $io): void
    {
    }

    public function uninstall(Composer $composer, IOInterface $io): void
    {
    }

    public static function getSubscribedEvents(): array
    {
        return [
            ScriptEvents::POST_INSTALL_CMD => 'installBinary',
            ScriptEvents::POST_UPDATE_CMD => 'installBinary',
        ];
    }

    public function installBinary(): void
    {
        (new BinaryInstaller())->install($this->composer, $this->io);
    }
}
```

`packages/composer-bootstrap/celerrate` (the shim; `chmod 755` it):

```php
#!/usr/bin/env php
<?php

// Celerrate shim: locates the platform binary the Composer plugin
// downloaded into bin-cache/ and executes it, forwarding arguments,
// the standard streams, and the exit code. CELERRATE_BINARY overrides
// the binary location. Composer generates the vendor/bin proxy (and
// the .bat proxy on Windows) for this file.

$binary = getenv('CELERRATE_BINARY');
if (!is_string($binary) || $binary === '') {
    $binary = null;
    foreach (['celerrate', 'celerrate.exe'] as $name) {
        $candidate = __DIR__ . '/bin-cache/' . $name;
        if (is_file($candidate)) {
            $binary = $candidate;
            break;
        }
    }
}
if ($binary === null || !is_file($binary)) {
    fwrite(STDERR, "celerrate: the binary is not installed.\n");
    fwrite(STDERR, "Run `composer install` with plugins and scripts enabled (no --no-plugins, no --no-scripts), or point CELERRATE_BINARY at an existing binary.\n");
    exit(1);
}
$process = proc_open(
    array_merge([$binary], array_slice($argv, 1)),
    [0 => STDIN, 1 => STDOUT, 2 => STDERR],
    $pipes
);
if (!is_resource($process)) {
    fwrite(STDERR, "celerrate: failed to start {$binary}\n");
    exit(1);
}
exit(proc_close($process));
```

- [ ] **Step 6: Run the tests again**

Run: `composer test --working-dir packages/composer-bootstrap`
Expected: all tests still pass (the plugin classes autoload; `composer/composer` in require-dev provides the interfaces).

- [ ] **Step 7: Commit**

```bash
git add packages/composer-bootstrap
git commit -m "✨ feat(composer): download, verify, and expose the platform binary"
```

---

### Task 8: The Composer fixture project and its CI proof

**Files:**
- Create: `packages/composer-bootstrap/tests/fixture/composer.json`
- Create: `packages/composer-bootstrap/tests/fixture/.gitignore`
- Modify: `.github/workflows/ci.yml` (extend the `dist` job from Task 5)

**Interfaces:**
- Consumes: the full package (Tasks 6-7), `target/dist/` artifacts plus `SHA256SUMS` (Tasks 2 and 5), `php -S` as the hermetic server.
- Produces: the release dry-run proof for the Composer channel (spec closure item): `composer install` in the fixture downloads, verifies, and exposes `vendor/bin/celerrate`.

- [ ] **Step 1: Write the fixture**

`packages/composer-bootstrap/tests/fixture/composer.json` — `symlink: false` matters: the plugin writes `bin-cache/` into the installed copy, which must be the fixture's `vendor/` copy, never the source package:

```json
{
    "name": "celerrate/fixture",
    "description": "Fixture project proving the Composer bootstrap installs the binary",
    "license": ["MIT", "Apache-2.0"],
    "repositories": [
        {
            "type": "path",
            "url": "../..",
            "options": {
                "symlink": false
            }
        }
    ],
    "require": {
        "celerrate/celerrate": "*"
    },
    "minimum-stability": "dev",
    "config": {
        "allow-plugins": {
            "celerrate/celerrate": true
        }
    }
}
```

`packages/composer-bootstrap/tests/fixture/.gitignore`:

```
/vendor/
/composer.lock
```

- [ ] **Step 2: Run the fixture end to end locally**

```bash
cargo xtask dist
cat target/dist/*.sha256 > target/dist/SHA256SUMS
php -S 127.0.0.1:8737 -t target/dist &
server_pid=$!
sleep 1
(cd packages/composer-bootstrap/tests/fixture \
  && rm -rf vendor composer.lock \
  && CELERRATE_DOWNLOAD_BASE_URL="http://127.0.0.1:8737" composer install \
  && ./vendor/bin/celerrate --version)
kill $server_pid
```

Expected: `celerrate: downloading http://127.0.0.1:8737/celerrate-<host triple>.tar.gz`, `celerrate: installed the <host triple> binary`, then a version line reporting 0.0.3. (Composer permits plain-http for 127.0.0.1; `secure-http` exempts localhost.)

- [ ] **Step 3: Verify the checksum failure path locally**

Corrupt the sums: copy the archives to a temporary directory, replace the recorded hash with 64 zeros, serve that directory, and re-run the fixture install:

```bash
corrupted="$(mktemp -d)"
cp target/dist/celerrate-*.tar.gz "$corrupted/"
printf '%s  %s\n' "$(printf '0%.0s' $(seq 1 64))" "$(cd target/dist && ls celerrate-*.tar.gz)" > "$corrupted/SHA256SUMS"
php -S 127.0.0.1:8737 -t "$corrupted" &
server_pid=$!
sleep 1
(cd packages/composer-bootstrap/tests/fixture \
  && rm -rf vendor composer.lock \
  && CELERRATE_DOWNLOAD_BASE_URL="http://127.0.0.1:8737" composer install) \
  ; echo "exit: $?"
kill $server_pid
```

Expected: the install fails with `celerrate: checksum verification failed ...; refusing to install.` and a non-zero exit.

- [ ] **Step 4: Extend the CI `dist` job**

Append to the `dist` job in `.github/workflows/ci.yml`, after the "Install through install.sh" step. Ubuntu runs PHP 7.4 to prove the plugin's floor; macOS runs a current PHP (setup-php does not provide 7.4 builds for arm64 macOS):

```yaml
      - if: ${{ needs.changes.outputs.code == 'true' }}
        uses: shivammathur/setup-php@v2
        with:
          php-version: ${{ runner.os == 'Linux' && '7.4' || '8.3' }}
      - if: ${{ needs.changes.outputs.code == 'true' }}
        name: Composer plugin unit tests
        run: |
          composer install --working-dir packages/composer-bootstrap
          composer test --working-dir packages/composer-bootstrap
      - if: ${{ needs.changes.outputs.code == 'true' }}
        name: Composer fixture install
        run: |
          php -S 127.0.0.1:8737 -t target/dist &
          for _ in $(seq 1 40); do
            curl -fs http://127.0.0.1:8737/SHA256SUMS >/dev/null 2>&1 && break
            sleep 0.5
          done
          cd packages/composer-bootstrap/tests/fixture
          CELERRATE_DOWNLOAD_BASE_URL="http://127.0.0.1:8737" composer install
          ./vendor/bin/celerrate --version
```

(The `SHA256SUMS` file already exists in `target/dist/` from the install.sh step.)

- [ ] **Step 5: Verify the YAML parses**

Run: `ruby -ryaml -e 'YAML.load_file(".github/workflows/ci.yml"); puts "ok"'`
Expected: `ok`.

- [ ] **Step 6: Commit**

```bash
git add packages/composer-bootstrap/tests/fixture .github/workflows/ci.yml
git commit -m "✅ test(composer): prove the bootstrap installs into a fixture project"
```

---

### Task 9: Installation documentation and changelog

**Files:**
- Create: `docs/installation.md`
- Modify: `README.md` (installation section)
- Modify: `CHANGELOG.md` (Unreleased)

**Interfaces:**
- Consumes: every surface shipped by Tasks 2-8 (names, flags, environment variables). Documentation must match them exactly.
- Produces: the user-facing documentation of the three channels; the full README/`docs/` pass stays with the release step of the parent sequencing.

- [ ] **Step 1: Write `docs/installation.md`**

```markdown
# Installing Celerrate

Celerrate ships as a single static binary per platform. Linux and macOS
are tier 1 platforms; Windows is tier 2 (built and tested, best-effort
analysis correctness).

## Install script (Linux, macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/celerrate/celerrate/main/install.sh | sh
```

The script detects your platform, downloads the latest release from
GitHub, verifies its SHA-256 checksum, and installs `celerrate` into
`~/.local/bin`. It never uses sudo.

Options (pass them after `sh -s --` when piping):

- `--version vX.Y.Z` installs an exact release instead of the latest.
- `--to <directory>` installs somewhere else than `~/.local/bin`.

```sh
curl -fsSL https://raw.githubusercontent.com/celerrate/celerrate/main/install.sh | sh -s -- --version v0.1.0
```

`CELERRATE_INSTALL_BASE_URL` overrides the download base URL for
corporate mirrors: point it at a URL serving the release archives and
the `SHA256SUMS` file directly.

## Composer (all platforms)

```sh
composer require --dev celerrate/celerrate
```

On install, the Composer plugin downloads the binary matching the
package version, verifies its checksum, and exposes
`vendor/bin/celerrate` (with a `.bat` proxy on Windows). The binary
version is locked 1:1 to the package version.

Environment overrides:

- `CELERRATE_BINARY`: use an existing binary; nothing is downloaded.
- `CELERRATE_DOWNLOAD_BASE_URL`: download from a mirror instead of
  GitHub Releases.

On a platform without a published binary, `composer install` warns and
continues; `vendor/bin/celerrate` reports the situation if invoked.

## Manual download

Every release publishes archives for five targets, a `SHA256SUMS` file,
and build provenance attestations:
`https://github.com/celerrate/celerrate/releases`

- Linux: `celerrate-x86_64-unknown-linux-musl.tar.gz`,
  `celerrate-aarch64-unknown-linux-musl.tar.gz` (static musl builds)
- macOS: `celerrate-x86_64-apple-darwin.tar.gz`,
  `celerrate-aarch64-apple-darwin.tar.gz`
- Windows: `celerrate-x86_64-pc-windows-msvc.zip`

Verify a download:

```sh
sha256sum --ignore-missing --check SHA256SUMS
gh attestation verify celerrate-x86_64-unknown-linux-musl.tar.gz --repo celerrate/celerrate
```
```

- [ ] **Step 2: Update the README**

Read `README.md` first. In its installation area (create the section if none exists, placed after the introduction), make sure these two one-liners appear, keeping the surrounding structure and tone:

```markdown
## Installation

```sh
curl -fsSL https://raw.githubusercontent.com/celerrate/celerrate/main/install.sh | sh
```

Or, for Composer projects:

```sh
composer require --dev celerrate/celerrate
```

All channels, manual downloads, and checksum verification:
[docs/installation.md](docs/installation.md).
```

- [ ] **Step 3: Update the changelog**

Read `CHANGELOG.md` first; under `## [Unreleased]`, in its `### Added` section (create it if absent), append, matching the existing bullet style:

```markdown
- The install script: `curl -fsSL .../install.sh | sh` downloads the
  platform binary, verifies its SHA-256 checksum, and installs it into
  `~/.local/bin` (`--version`, `--to`, and a base-URL override for
  mirrors).
- The Composer bootstrap package `celerrate/celerrate`: installs the
  checksum-verified platform binary on `composer install` and exposes
  `vendor/bin/celerrate`.
- `cargo xtask dist` builds and packages one target's release archive
  deterministically; the release workflow now packages through it and
  attests its artifacts.
```

- [ ] **Step 4: Convention pass**

Re-read the three files against the project's markdown conventions (wrapped prose, heading and bullet style consistent with the surrounding documents) and against the shipped surfaces: every flag, file name, and environment variable named in the documentation must match Tasks 2-8 exactly.

- [ ] **Step 5: Commit**

```bash
git add docs/installation.md README.md CHANGELOG.md
git commit -m "📝 docs(installation): document the install script, Composer, and manual channels"
```

---

### Task 10: Full verification

**Files:** none created; this task runs the gates.

**Interfaces:**
- Consumes: everything above.
- Produces: the green light for the pull request.

- [ ] **Step 1: The mechanical suite**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cargo xtask dependency-shape
cargo xtask emission-scan
```

Expected: all clean. `dependency-shape` is untouched by this work (no crate was added); it must pass unchanged.

- [ ] **Step 2: The corpus gates**

```bash
cargo xtask fetch-corpus
cargo xtask corpus
cargo xtask mixed-rate
```

Expected: the snapshot and the baseline are byte-identical to the committed ones. Nothing in this plan touches analysis; any delta is a defect — stop and investigate with the systematic-debugging skill.

- [ ] **Step 3: The distribution surfaces, one last local pass**

```bash
shellcheck install.sh
composer test --working-dir packages/composer-bootstrap
cargo xtask dist
cat target/dist/*.sha256 > target/dist/SHA256SUMS
destination="$(mktemp -d)"
CELERRATE_INSTALL_BASE_URL="file://$PWD/target/dist" sh install.sh --to "$destination"
"$destination/celerrate" --version
```

Expected: all pass; the installed binary reports 0.0.3.

- [ ] **Step 4: Push and open the pull request**

Use the superpowers:finishing-a-development-branch skill. The pull request description covers: the single packaging logic (`xtask dist`) and the workflow delegation with attestations, the install script, the Composer bootstrap package, and the hermetic CI proofs. Watch the `dist` CI job: it is the release dry-run and must pass on both tier 1 runners.

- [ ] **Step 5: Post-merge follow-up (record it in the pull request description)**

After merge, trigger the `Release` workflow manually (`workflow_dispatch` on `main`): all five targets must build and package through `xtask dist`. This is the only proof a branch cannot produce.
