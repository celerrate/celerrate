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
