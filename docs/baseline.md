# Baseline

The baseline lets you adopt Celerrate on an existing codebase without
fixing everything first. Recording a baseline freezes the current
findings as known and hides them from the report and the exit code;
only genuinely new problems fail the build from there on. `celerrate
migrate --from-phpstan` records one automatically as its last step (see
[Migrating from PHPStan](migration.md)); this page covers using it
directly.

## Recording: `celerrate check --baseline`

```sh
celerrate check --baseline
```

writes (or rewrites) `celerrate-baseline.toml` at the project root,
next to `composer.json`. The name and location are fixed: there is no
configuration key that moves them (see [What is not
configurable](configuration.md#what-is-not-configurable)).

`--baseline` cannot be combined with `--fix`, `--fix-suggestions`,
`--watch`, or `--ignore-baseline`; each pairing is a usage error, not a
silently resolved combination:

```text
$ celerrate check --baseline --fix
error: the argument '--baseline' cannot be used with '--fix'
```

Recording and applying are different modes of the same run, so
`--baseline` and `--ignore-baseline` are mutually exclusive for the
same reason `--fix` and `--watch` are: nothing about the request would
be well defined otherwise.

A clean project records nothing: if there are no findings and no
existing file, `--baseline` leaves the project exactly as it found it.
When a file already exists and the project just became clean, the
command still rewrites it, header-only, dropping every entry, rather
than leaving stale entries behind. The report only ever claims a
recording that actually happened; a run that changed nothing on disk
never prints the word "recorded".

## Applying: automatic, unless ignored

A present `celerrate-baseline.toml` is applied automatically, on every
plain `celerrate check`, with no flag needed. A summary line reports
how many findings it hid:

```text
2 baselined diagnostics hidden
```

`--ignore-baseline` runs strict, as if the file were not there: every
finding it would otherwise hide is reported, and nothing is hidden from
the exit code.

Filtering happens after analysis and after suppression, immediately
before rendering and the exit code are computed. The baseline never
enters the analysis cache: a cached verdict is exactly the same whether
or not a baseline is present, and the persisted cache always holds the
pre-baseline findings. Running with `--ignore-baseline` right after a
warm baselined run still reports every finding from the cache; nothing
about the cache's content depends on how the baseline treated it.

## The file

`celerrate-baseline.toml` is a versioned, deterministically sorted
list of entries, generated so its diffs stay small and reviewable in a
pull request. Recorded from a scratch project with two findings
(`celerrate check --baseline`), the file looks like this:

```toml
# Celerrate baseline: known findings hidden from the report and the exit code.
# Recorded by `celerrate check --baseline`. Entries are structural (no line
# numbers): they survive moving code and die with their finding.

version = 1

[[entry]]
path = "src/Checkout.php"
identifier = "CEL0018"
symbol = 'App\Service\Checkout::finalize'
message = "unknown class `AlsoMissing`"
count = 1

[[entry]]
path = "src/Kernel.php"
identifier = "CEL0018"
symbol = 'App\Kernel'
message = "unknown class `Missing`"
count = 1
```

Entries are sorted by path, then identifier, then symbol, then
message: the same input always serializes to the same bytes regardless
of the order findings were discovered in, so recording twice on an
unchanged project is byte-for-byte identical.

An entry carries no line number. Its key is the project-relative path
(forward slashes on every platform), the `CEL` identifier, the
enclosing symbol path (a fully qualified class or function name, a
`Class::method` pair, or `(top level)` for code outside any
declaration), and the full rendered message. Two findings that share
every one of those fields are the same entry, counted rather than
duplicated: `count` records how many occurrences it absorbs.

## The invariants, honestly stated

- **An entry survives line movement.** Because the key holds no line
  number, moving, wrapping, or otherwise shifting the surrounding code
  never orphans an entry. Verified by
  `an_entry_survives_line_movement` in
  `crates/celerrate_cli/tests/baseline.rs`.
- **An entry dies with its diagnostic.** An entry only ever matches the
  finding it was recorded from. Fix the underlying problem, or even
  just reword the diagnostic's message (an engine upgrade can do this),
  and the entry stops matching. It is never pruned silently: Celerrate
  reports it through an exit-neutral notice, **CEL0050** (see [Baseline
  notices](diagnostics.md#baseline-notices-cel0050-cel0051)), advising
  a re-record. Verified by
  `an_entry_dies_with_its_diagnostic_and_is_reported_obsolete`.
- **A count of N never hides occurrence N+1.** An entry with `count =
  2` absorbs at most two matching occurrences; a third is reported as
  new, on the same run, alongside the "N baselined diagnostics hidden"
  line for the two it did hide. Verified by
  `the_count_never_masks_occurrence_n_plus_one`.

## Owned failure modes

The baseline's structural key is a deliberate trade against line
numbers, and it has a cost the design accepts rather than hides:

- **Renaming a method orphans its entries.** The symbol path is part
  of the key, so renaming the enclosing method (or moving a finding
  into a different one) breaks every entry recorded against the old
  name. Their findings resurface, and CEL0050 announces the entries
  that no longer match. Noisy, not silent: nothing is lost, the
  project is just no longer clean until you re-record. Verified by
  `a_renamed_method_resurfaces_its_findings_and_reports_obsolescence`.
- **An engine upgrade that rewords messages does the same.** The
  message is part of the key too, so a Celerrate release that changes
  a diagnostic's wording orphans every entry recorded against the old
  wording, for the same reason and with the same outcome: resurfaced
  findings, a CEL0050 notice, and a straightforward fix.

In both cases the fix is the same: run `celerrate check --baseline`
again once you have confirmed the resurfaced findings are genuinely
still acceptable, not new regressions the rename or upgrade happened to
uncover.

## Interaction with suppression

Two mechanisms can hide a finding, and they run in a fixed order.
Suppression (`@celerrate-ignore`, see [the PHPDoc
bridge](phpdoc-bridge.md)) is applied first, inside the engine, before
the diagnostic list ever reaches the baseline step. The baseline
filters second, over whatever suppression left behind.

One consequence follows directly from that order: adding a suppression
to code whose finding is already baselined starves the matching entry
of the occurrence it used to absorb. The entry becomes obsolete, and
CEL0050 reports it, exactly as if the underlying problem had been
fixed. This is intended, not a bug to work around: once a finding is
suppressed at its source, the baseline entry that used to carry it has
nothing left to do, and re-recording drops it for good. Verified by
`a_new_suppression_makes_the_baseline_entry_obsolete`.

## In CI

Commit `celerrate-baseline.toml`. It is meant to be checked in and
reviewed like any other file: a pull request that adds a baseline
entry is a pull request that is knowingly accepting a finding, and a
reviewer sees exactly that in the diff. See [Continuous
integration](ci.md) for how a pipeline runs `celerrate check` against
the committed file.
