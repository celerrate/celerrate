//! `cargo xtask dist [--target <triple>]`: build the release binary
//! for one target and package it exactly as the release publishes it:
//! `celerrate-<triple>.tar.gz` (`.zip` for Windows targets) plus a
//! `.sha256` checksum line in `sha256sum` format, under `target/dist/`.
//! Archive metadata is deterministic (fixed timestamps, ownership, and
//! entry order), so two runs over the same commit produce
//! byte-identical archives; the CI dist job pins that property.

use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::Digest;

use crate::Result;

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
        assert_eq!(
            std::fs::read(&first).unwrap(),
            std::fs::read(&second).unwrap()
        );
    }
}
