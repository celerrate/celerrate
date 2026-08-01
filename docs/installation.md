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
version is locked 1:1 to the package version. The package requires
the `phar` and `zlib` PHP extensions, which it uses to extract the
downloaded archive.

Environment overrides:

- `CELERRATE_BINARY`: use an existing binary; nothing is downloaded.
- `CELERRATE_DOWNLOAD_BASE_URL`: download from a mirror instead of
  GitHub Releases. Like `CELERRATE_INSTALL_BASE_URL` above, it
  replaces the whole base including the release tag, so it must serve
  the release archives and `SHA256SUMS` directly; it must also be an
  HTTPS URL, since Composer's `secure-http` setting rejects a plain
  HTTP download base.

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
