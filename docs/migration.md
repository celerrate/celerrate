# Migrating from PHPStan

```sh
celerrate migrate --from-phpstan
```

One command converts a PHPStan configuration to `celerrate.toml`,
reports everything that does not carry over, and records a Celerrate
baseline so the first `celerrate check` is clean and only new problems
fail from there on.

## What it reads

The command looks for `phpstan.neon` at the project root, then
`phpstan.neon.dist`, then `phpstan.dist.neon` if the first is absent:
the same discovery order PHPStan itself uses. Its `includes` are
resolved recursively, relative to the file that declares them, with a
cycle guard against a file that includes itself back into the tree.
An include is followed wherever it points, including outside the
project root, exactly as PHPStan itself would follow it. The command
only ever reads those files.

Only three settings are consumed: `parameters.paths`,
`parameters.excludePaths`, and `parameters.level`. `excludePaths`
accepts the plain list form and the mapping form
(`analyse`/`analyseAndScan`) alike. Everything else the configuration
declares, and everything the reader cannot parse, is listed in the
report, line by line: the report is always generated, never silent.

## What it writes

`celerrate.toml`, generated from the paths and the level found. The
command refuses to overwrite an existing `celerrate.toml`; pass
`--force` to replace it.

`celerrate-baseline.toml` is written only when the first analysis
under the new configuration finds something to record. An empty
baseline file is never written, so a project with a clean generated
configuration ends the migration with no baseline file at all.

Unlike `celerrate.toml`, the baseline is not protected by `--force`:
an existing `celerrate-baseline.toml` is replaced wholesale by the one
the migration records, and that happens even without `--force`. The
file is regenerable at any time with `celerrate check --baseline`, so
the cost is low, but keep a copy first if the old entries matter to
you.

The command never modifies `phpstan.neon`, `phpstan.neon.dist`,
`phpstan.dist.neon`, or any PHPStan baseline file. Rollback stays
free: deleting `celerrate.toml` and `celerrate-baseline.toml` returns
the project to exactly where it started. Once the Celerrate baseline
is in place, the PHPStan baseline includes named in the report (see
below) can be deleted from `phpstan.neon`.

## The level table

PHPStan's `level` becomes a `[severity]` table in `celerrate.toml`.
Nine identifiers, across three typed families, are remapped to
`"warning"` at level 5 and below (PHPStan's own default, when no
`level` is set, is level 0):

| PHPStan `level` | Generated `[severity]` |
| --- | --- |
| absent (PHPStan defaults to level 0) | CEL0030 to CEL0038 remapped to `"warning"` |
| 0, 1, 2, 3, 4, 5 | CEL0030 to CEL0038 remapped to `"warning"` |
| 6 and above | none; identifiers keep their default severities |
| `max` | none; identifiers keep their default severities |
| anything else (not understood) | none; identifiers keep their default severities, and the report notes the unrecognized value |

The nine identifiers: CEL0030, CEL0031, CEL0032, and CEL0033 (the
`unknown-members` family), CEL0034 (the `null-dereference` family),
and CEL0035, CEL0036, CEL0037, and CEL0038 (the `argument-checks`
family). `celerrate explain CEL0030` and its siblings document why
each one fires. The severities the migration writes are a starting
point, not a fixed choice: edit `[severity]` in `celerrate.toml`
afterward to raise, lower, or remove any of them.

## What is not converted, and why

- **Paths `celerrate.toml` cannot express**: `include` and `exclude`
  under `[project]` take plain relative paths inside the project root,
  so four kinds of entry are dropped rather than mistranslated:
  absolute paths, paths carrying a `%parameter%` placeholder, `*` or
  `?` glob patterns, and paths that escape the project root with `..`.
  Globs in `excludePaths` are common in real projects, and a dropped
  exclusion means the analysis sees more code and reports more
  findings. The report names every dropped path with its reason, and
  the recorded baseline absorbs the consequence: those extra findings
  are baselined like any other, so the first `celerrate check` is still
  clean. Narrow the paths by hand afterward if you would rather not
  carry them in the baseline.
- **Message-based `ignoreErrors`**: PHPStan's ignore patterns match
  against PHPStan's own message text, a vocabulary Celerrate does not
  share. There is nothing to translate a regular expression against.
  The recorded Celerrate baseline carries the migration's continuity
  instead: whatever the first analysis finds is baselined, and the
  project starts clean regardless of how those findings used to be
  silenced.
- **PHPStan baseline files**: for the same reason, a PHPStan baseline
  is never read or converted entry by entry. An include that names one
  (or any include whose target does not end in `.neon`) is listed by
  name in the report and never parsed.
- **Bootstrap and stub files**: Celerrate does not execute project
  code before analysis, and it ships its own PHP stubs rather than
  consuming PHPStan's.
- **Extension configuration**: PHPStan services, custom rules, and
  conditional tags have no Celerrate equivalent; Celerrate's own
  analysis families and first-party plugins are enabled by default
  instead.

Inline `@phpstan-ignore` comments need no migration at all: Celerrate
reads and honors them directly at analysis time, alongside its own
`@celerrate-ignore` directive. See
[the PHPDoc bridge](phpdoc-bridge.md) for the full suppression table.

## After migrating

Run `celerrate check`. Because the migration already recorded a
baseline from the project's own findings, this first run is clean, and
only genuinely new problems are reported from here on. See
[Baseline notices](diagnostics.md#baseline-notices-cel0050-cel0051)
for how the baseline is refreshed (`celerrate check --baseline`) and
how to run without it (`celerrate check --ignore-baseline`).
