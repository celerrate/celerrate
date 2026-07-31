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

/// The documentation files packaged alongside the binary in every
/// release archive.
const DOCUMENTATION_FILES: [&str; 3] = ["LICENSE-MIT", "LICENSE-APACHE", "README.md"];

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
        // Entry path plus the fixed header fields the packer sets: two
        // runs producing byte-identical archives could coincidentally
        // agree on a wall-clock mtime, so the header fields are asserted
        // directly rather than only inferred from byte equality above.
        let entries: Vec<(String, u32, u64, u64, u64)> = archive
            .entries()
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let mode = entry.header().mode().unwrap();
                let mtime = entry.header().mtime().unwrap();
                let uid = entry.header().uid().unwrap();
                let gid = entry.header().gid().unwrap();
                let path = entry.path().unwrap().display().to_string();
                (path, mode, mtime, uid, gid)
            })
            .collect();
        assert_eq!(
            entries,
            [
                (
                    "celerrate-x86_64-unknown-linux-musl/celerrate".to_owned(),
                    0o755,
                    0,
                    0,
                    0,
                ),
                (
                    "celerrate-x86_64-unknown-linux-musl/LICENSE-MIT".to_owned(),
                    0o644,
                    0,
                    0,
                    0,
                ),
                (
                    "celerrate-x86_64-unknown-linux-musl/LICENSE-APACHE".to_owned(),
                    0o644,
                    0,
                    0,
                    0,
                ),
                (
                    "celerrate-x86_64-unknown-linux-musl/README.md".to_owned(),
                    0o644,
                    0,
                    0,
                    0,
                ),
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
        let first_bytes = std::fs::read(&first).unwrap();
        assert_eq!(first_bytes, std::fs::read(&second).unwrap());
        // Entry name, order, and the fixed metadata `zip_entry_options`
        // sets: unlike the tar test above, byte equality alone never
        // exercised the archive structure or pinned the zip-epoch
        // timestamp and unix permissions a regression could silently drop.
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(first_bytes)).unwrap();
        let entries: Vec<(String, u32, zip::DateTime)> = (0..archive.len())
            .map(|index| {
                let file = archive.by_index(index).unwrap();
                let name = file.name().to_owned();
                let mode = file.unix_mode().unwrap() & 0o777;
                let last_modified = file.last_modified().unwrap();
                (name, mode, last_modified)
            })
            .collect();
        assert_eq!(
            entries,
            [
                (
                    "celerrate-x86_64-pc-windows-msvc/celerrate.exe".to_owned(),
                    0o755,
                    zip::DateTime::default(),
                ),
                (
                    "celerrate-x86_64-pc-windows-msvc/LICENSE-MIT".to_owned(),
                    0o644,
                    zip::DateTime::default(),
                ),
                (
                    "celerrate-x86_64-pc-windows-msvc/LICENSE-APACHE".to_owned(),
                    0o644,
                    zip::DateTime::default(),
                ),
                (
                    "celerrate-x86_64-pc-windows-msvc/README.md".to_owned(),
                    0o644,
                    zip::DateTime::default(),
                ),
            ],
        );
    }

    #[test]
    fn the_host_triple_is_a_target_triple() {
        let triple = super::host_triple().unwrap();
        assert!(triple.contains('-'), "not a triple: {triple}");
    }

    #[test]
    fn the_checksum_file_sits_next_to_the_archive_with_the_full_name() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory
            .path()
            .join("celerrate-x86_64-unknown-linux-musl.tar.gz");
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
}
