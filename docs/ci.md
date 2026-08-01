# Continuous integration

Celerrate is one binary and one command: `celerrate check`. Nothing
above needs a service, a database, or a warm-up step, so the workflow
below is the whole integration.

## The short version

```yaml
name: Celerrate
on:
  push:
    branches: [main]
  pull_request:
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - name: Install Celerrate
        run: curl -fsSL https://raw.githubusercontent.com/celerrate/celerrate/main/install.sh | sh
      - name: Check
        run: ~/.local/bin/celerrate check --output=github .
```

The install script downloads the latest release for the runner's
platform, verifies its checksum, and installs `celerrate` into
`~/.local/bin` (see [Installing Celerrate](installation.md)); nothing
here needs sudo or a package manager.

For a Composer project, skip the install step entirely:

```sh
composer require --dev celerrate/celerrate
vendor/bin/celerrate check --output=github .
```

The Composer plugin downloads the binary matching the package version
on install, so `vendor/bin/celerrate` is ready the moment
`composer install` (or `composer require`) finishes; see
[Installing Celerrate](installation.md#composer-all-platforms-from-v010).

## Exit codes

| Exit code | Meaning |
| --- | --- |
| 0 | Clean: no diagnostics reported. |
| 1 | Diagnostics reported. |
| 2 | Internal or usage error. |

Identical in every output format; see
[Output formats](output-formats.md#exit-codes) for the full statement,
including how the JSON summary and the SARIF invocation embed the same
number the process exits with. For a CI gate, the distinction that
matters is between 1 and 2: exit code 1 is the tool doing its job, a
pull request that genuinely introduces (or fails to hide behind a
baseline) a finding; exit code 2 means Celerrate itself hit a problem,
a bad root path, an incompatible flag combination, or an internal
error, and is worth surfacing differently from a normal red build.

## Pull request annotations

```sh
celerrate check --output=github .
```

emits GitHub Actions workflow commands: `::notice` for each
exit-neutral notice, then one `::error` or `::warning` per finding,
with `file`, `line`, `col`, `endLine`, and `endColumn` when the finding
is anchored in a file. GitHub renders these as native annotations on
the diff, inline on the exact lines a pull request changed, with
nothing further to configure. See
[Output formats](output-formats.md#github-actions) for the full
command shapes, including how internal errors and the end-of-run
summary are rendered.

## SARIF upload for GitHub code scanning

```yaml
      - name: Check
        run: ~/.local/bin/celerrate check --output=sarif . > celerrate.sarif || true
      - uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: celerrate.sarif
```

`celerrate check` exits 1 the moment it reports a single finding, and
without `|| true` that would stop the job before the upload step ever
ran. The `|| true` swallows exactly that exit code so the SARIF file
still reaches the upload step; it does not swallow exit code 2; a
usage error or an internal error still leaves `celerrate.sarif`
unwritten or incomplete, and the subsequent upload step surfaces that
on its own. Gate the build on a separate plain `celerrate check`
step, or on the code-scanning results the upload produces, rather than
on this step's own exit code. See [Output formats](output-formats.md#sarif)
for what the document itself carries: results with byte-precise fixes,
exit-neutral notices as `level: note`, and the baseline's hidden count
in the run's own `properties`.

## The baseline in CI

Commit `celerrate-baseline.toml`. A present baseline is applied
automatically on every plain `celerrate check`, no flag needed: known
findings stay hidden from the report and the exit code, and only
genuinely new problems fail the build. Adopting a fix removes its
finding from what the baseline can absorb; re-record locally with
`celerrate check --baseline` and commit the smaller file. `--baseline`
cannot be combined with `--ignore-baseline`, `--fix`, `--fix-suggestions`,
or `--watch`, so recording never happens by accident inside a
`check --output=github` step. See [Baseline](baseline.md) for the full
recording and applying flow, the file's structure, and how it interacts
with inline suppression.

## Caching

`celerrate check` persists its incremental cache to `.celerrate/` at
the project root. Restoring it between runs keeps a rerun on an
unchanged (or barely changed) tree on the warm path instead of the
cold one:

```yaml
      - uses: actions/cache@v4
        with:
          path: .celerrate
          key: celerrate-${{ hashFiles('composer.lock') }}
          restore-keys: celerrate-
      - name: Check
        run: ~/.local/bin/celerrate check --output=github .
```

The honest caveat: a cold run is already fast. The measured cold run
on a 9447-file corpus with its full vendor tree is a second and a half
(see [the benchmark protocol](../benchmarks/PROTOCOL.md) for the
reproducible numbers); on a typical project's pull-request job, the
checkout and the install step already cost more than the analysis
does. Caching earns its keep on repeated runs against a mostly
unchanged tree, not on the first run of a new pull request.
