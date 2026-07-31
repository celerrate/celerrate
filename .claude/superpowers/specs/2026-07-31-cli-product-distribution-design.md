# Celerrate: CLI Product Distribution Design

Date: 2026-07-31
Status: Draft
Parent: `.claude/superpowers/specs/2026-07-24-cli-product-v0.1-design.md`
(section 8, the release; closure gate 6) and, through it,
`.claude/superpowers/specs/2026-07-09-celerrate-design.md` (the
distribution channels and platform tiers).

## 1. Scope

This design covers the distribution slice of the CLI product
sub-project: `cargo xtask dist`, the release workflow upgrade, the
install script, and the Composer bootstrap package. It ends with closure
gate 6 of the parent: the release dry-run, entirely in CI, entirely
against the current commit.

**Already in place** (inherited from the internal v0.0.1 and v0.0.2
releases): `release.yml` builds the five targets (Linux musl x64/arm64,
macOS x64/arm64, Windows x64), packages archives inline, assembles a
`SHA256SUMS`, checks the tag against the workspace version, and creates
the GitHub Release with notes extracted from the changelog
(`cargo xtask release-notes`).

**Missing, and delivered here**: artifact attestations; `cargo xtask
dist` as the single packaging logic; `install.sh` with its tier 1
integration test; the Composer bootstrap package with its fixture test.

**Out of scope**: Packagist registration and submission (they happen at
release time, alongside the `v0.1.0` tag: nothing irreversible ships
before the release); Homebrew, the Docker image, the GitHub Action
(v0.1.x follow-ups); the README and `docs/` pass and the benchmark
protocol (the next step of the parent's sequencing); any change to the
analysis crates. The corpus snapshot and the mixed-rate baseline cannot
move by construction.

**The load-bearing decision**: one packaging brain, hermetic tests.
`cargo xtask dist` is the only place that knows how an artifact is
assembled; the release workflow calls it instead of duplicating it; the
install script and the Composer plugin accept a base-URL override so the
CI integration tests consume artifacts built from the current commit,
offline. The dry-run therefore proves today's packaging, never a stale
release's.

## 2. `cargo xtask dist` and the release workflow

`cargo xtask dist [--target <triple>]` is the single packaging logic:

- Builds `cargo build --release --package celerrate_cli --target
  <triple>`, defaulting to the host triple. Cross-compilation is not
  dist's business: passing `--target` requires the matching toolchain to
  be installed, and the five-target matrix stays in CI where each runner
  builds natively.
- Assembles the archive directory `celerrate-<triple>/` (the binary,
  `LICENSE-MIT`, `LICENSE-APACHE`, `README.md`), then `tar.gz` on Unix
  targets and `zip` on Windows targets, plus one `.sha256` file per
  archive. Everything lands in `target/dist/`.
- Archiving is implemented in Rust inside the xtask (the `tar`,
  `flate2`, `zip`, and `sha2` crates, admitted through `cargo deny`)
  with deterministic metadata: fixed mtime, sorted entries, stable
  permissions. Two builds of the same commit produce the same checksum,
  locally and in CI. That is the concrete meaning of "reproducible", and
  it is pinned by test (section 5).
- The xtask being portable Rust, the Windows PowerShell packaging branch
  of the workflow disappears along with the Unix shell one.

`release.yml` changes accordingly:

- Each matrix runner calls `cargo xtask dist --target <triple>` and
  uploads `target/dist/*`.
- The publish job keeps the tag-versus-workspace-version check, the
  `SHA256SUMS` assembly, and the changelog-extracted notes, and gains
  artifact attestations: `actions/attest-build-provenance` over the
  archives, with the `id-token: write` and `attestations: write`
  permissions.
- The `workflow_dispatch` trigger stays: it is the full five-target
  dry-run, exercised before tagging (the release step of the parent's
  sequencing puts it on its checklist). The v0.0.1 and v0.0.2 releases
  already prove the five targets build.

## 3. `install.sh`

A POSIX `sh` script at the repository root, shellcheck-clean, served at
the stable URL `raw.githubusercontent.com/celerrate/celerrate/main/install.sh`.
The README documents `curl -fsSL <url> | sh`.

- **Platform detection**: Linux/Darwin crossed with x86_64/aarch64,
  mapped to the target triple. Windows is refused with a clear message
  pointing at the zip archive and the Composer package: the install
  script is a tier 1 surface.
- **Version resolution without the GitHub API** (no rate limits): the
  default uses the `releases/latest/download/...` URL form;
  `--version vX.Y.Z` switches to `releases/download/vX.Y.Z/...`.
- **Flow**: download the archive and `SHA256SUMS`, verify the checksum
  (`sha256sum` or `shasum -a 256`, whichever the platform has), extract,
  install into `~/.local/bin` (or `--to <dir>`). Never sudo. Warn when
  the install directory is not on `PATH`.
- **Error handling**: `set -eu`; every failure is loud, precise, and
  actionable. A checksum mismatch is a hard error, not a warning.
- **The hermetic override**: `CELERRATE_INSTALL_BASE_URL` replaces the
  `https://github.com/celerrate/celerrate/releases/download/<tag>` base.
  Combined with `--version`, it lets the CI test point at local
  artifacts through `file://` URLs (curl supports them), and it serves
  users behind a corporate mirror.

## 4. The Composer bootstrap package

Source in `packages/composer-bootstrap/`, package name
`celerrate/celerrate`, type `composer-plugin`. Requirements:
`php >= 7.4` (the PHPStan 2.x floor: every project that can run PHPStan
today can install Celerrate; the plugin code is tiny and 7.4 syntax
costs nothing) and `composer-plugin-api ^2.0`. The directory sits
outside the Cargo workspace and the crate DAG; `dependency-shape` is not
involved.

- **1:1 version locking**: the plugin reads its own installed version
  (`InstalledVersions::getPrettyVersion`) and downloads the binary from
  `releases/download/v<version>/`. A `dev-*` branch version has no
  binary: clear error asking for a tagged version.
- **Download through Composer's `HttpDownloader`**, no hand-rolled curl:
  the user's Composer proxy, authentication, and TLS configuration are
  honored for free. The SHA-256 is verified against the `SHA256SUMS` of
  the same release; a mismatch is a hard error.
- **The shim**: the package declares `"bin": ["celerrate"]`, a small
  committed PHP script that locates the downloaded binary (stored inside
  the package's own directory) and executes it, forwarding arguments and
  the exit code. Composer generates the `.bat` proxy on Windows itself,
  which covers the parent's requirement. The cost (a few tens of
  milliseconds of PHP per invocation) is negligible next to an analysis
  run. When the binary is missing (an install ran with `--no-scripts` or
  `--no-plugins`), the shim fails with the message explaining what to
  re-run.
- **Resilience**: on an unsupported platform the plugin warns and skips
  the binary, but never fails the host project's `composer install`; the
  shim carries the error if invoked. Consistent with the project's
  stance: never break the user's environment.
- **Overrides**: `CELERRATE_BINARY` (use an external binary, skip the
  download entirely) and `CELERRATE_DOWNLOAD_BASE_URL` (corporate
  mirrors, and the CI fixture test).

## 5. Testing

All integration tests consume the artifacts of the current commit's
`cargo xtask dist`: the dry-run proves today's packaging.

- **Reproducibility pinned**: the CI job runs `xtask dist` twice on the
  same commit and asserts identical checksums.
- **`install.sh`**: shellcheck in CI; an integration test on both tier 1
  runners (ubuntu, macos) that builds the host artifact with `xtask
  dist`, assembles a local `SHA256SUMS`, runs the script with the
  base-URL override, and asserts the installed binary answers
  `celerrate --version`.
- **The Composer fixture** (closure gate 6): a fixture project requires
  the package through a `path` repository; `composer install` runs with
  `CELERRATE_DOWNLOAD_BASE_URL` pointing at the dist artifacts and their
  assembled local `SHA256SUMS`, served by `php -S` (PHP's built-in
  server, already present since Composer needs PHP); the test asserts
  `vendor/bin/celerrate --version` answers. Both
  tier 1 runners, PHP pinned through `setup-php`.
- **Plugin unit tests**: platform detection, URL resolution, checksum
  verification, with PHPUnit, executed inside the fixture job. No
  heavier framework.
- **Untouched invariants**: the corpus snapshot, the mixed-rate
  baseline, `dependency-shape`, `emission-scan`, and the full mechanical
  suite (`cargo test --workspace`, clippy `-D warnings`, `cargo fmt`,
  `cargo deny check`) guard every change as always. `cargo deny` gates
  the new xtask dependencies.

## 6. Error handling, the common line

Every failure is loud, precise, and actionable. Checksum mismatches are
hard errors everywhere. The single tolerance is the unsupported platform
on the Composer side: warn without breaking the host project's install,
and let the shim carry the error at invocation time.

## 7. Documentation

A `docs/installation.md` page covering the three channels (the install
script, the Composer package, manual download with checksum and
attestation verification) and the environment overrides. Changelog
entries under Unreleased. The full README and `docs/` pass stays in the
release step of the parent's sequencing, as planned.

## Explicitly rejected

- **Five-target cross-compilation in `xtask dist`** (cargo-zigbuild or
  cross): a heavy toolchain dependency and a build path different from
  CI's native runners, for a capability nobody needs locally. Host
  target by default, `--target` when the toolchain exists.
- **Integration tests against real GitHub Releases**: network-dependent,
  and they test a stale release's packaging; a regression in the current
  commit's packaging would surface only at tag time, the exact opposite
  of what the dry-run gate exists to prove. Post-release smoke testing
  can hit the real path later.
- **Keeping the inline workflow packaging next to `xtask dist`**: two
  packaging logics drift silently, and dist degrades into a local-only
  duplicate.
- **Publishing to Packagist now** (reserving the name against v0.0.2):
  it would expose an internal version as the package's first public
  impression before the product is announced. Registration and
  submission happen with the `v0.1.0` tag.
- **Downloading the binary straight into `vendor/bin`** (no PHP shim):
  mutating Composer's proxy directory behind its back is fragile across
  Composer versions and platforms; the committed shim plus Composer's
  own `.bat` generation is the honest route, at a negligible
  per-invocation cost.
- **A hand-rolled HTTP client in the plugin** (curl or
  `file_get_contents`): it forfeits Composer's proxy, authentication,
  and TLS handling, precisely the environments a corporate PHP shop
  runs.
- **Failing `composer install` on unsupported platforms**: the bootstrap
  package must never make a project uninstallable; the error belongs to
  the moment Celerrate is actually invoked.
- **An absolute-URL override per artifact**: one base-URL override per
  consumer (`install.sh`, the plugin) covers both the hermetic tests and
  corporate mirrors; finer knobs are surface without a consumer.
