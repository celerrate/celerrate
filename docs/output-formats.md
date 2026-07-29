# Output formats

`celerrate check` serializes its report with `--output`:

- `human` (the default): the rich terminal report.
- `json`: a stable, versioned document for tooling.
- `sarif`: SARIF 2.1.0 for code-scanning integrations.
- `github`: GitHub Actions workflow commands for pull-request
  annotations.

One format per run. Every format serializes the same final stream: the
same diagnostics, in the same order, after suppression and after the
baseline, with the same exit code. A machine format writes exactly one
document to standard output; anything else the run reports (the
statistics line, for example) goes to standard error, so a machine
format's standard output is never polluted with a second stream to
parse around.

A machine format cannot be combined with `--watch`, `--fix`,
`--fix-suggestions`, or `--baseline` (recording a new baseline): those
runs loop or mutate, and their interactive reporting is the human
channel's job. Applying an existing baseline works with every format,
and `--ignore-baseline` does too.

## JSON

```sh
celerrate check --output=json
```

The root object carries `schema_version` (currently 1), a `summary`
(`errors`, `warnings`, `notices`, `baselined_hidden`, `internal_errors`,
`exit_code`), the exit-neutral `notices`, an `internal_errors` array,
and the `diagnostics` in the total deterministic order.

Each diagnostic exposes its identifier, severity, owning rule name in a
`rule` field (present only when a rule owns the identifier; syntax,
project, and configuration identifiers have none), anchor (`project`,
or a `span` with a project-relative path, 1-based lines and columns,
and exact byte offsets), message, resolved secondary labels, notes, and
suggestions with their edits and confidence (`safe` or `needs-review`).
Columns count Unicode code points; byte offsets index the file's UTF-8
bytes.

Each entry of `internal_errors` carries `kind`, `message`, and `bug`.
`message` is the same sentence the human channel prints after its
`internal error:` prefix. `kind` is a kebab-case name for the condition
(for example `file-unreadable`, `analysis-panicked`, or
`fix-write-failed`); the schema only constrains its shape, not an
enumerated list, so a new condition never breaks validation. `bug`
separates a defect in Celerrate (`true`) from a condition of the run's
environment (`false`), such as an unreadable file or an exhausted watch
budget. This is the same detail behind exit code 2, now available
without re-running the tool on the human channel;
`summary.internal_errors` still holds only the count, unchanged.

The schema is committed at `schemas/celerrate-json-report.v1.schema.json`
and the test suite validates real output against it.

Compatibility policy: adding a field is non-breaking (and updates the
schema file in the same release); removing a field or changing its
meaning increments `schema_version`.

## SARIF

```sh
celerrate check --output=sarif
```

SARIF 2.1.0, validated against the official schema in CI. Referenced
identifiers are described under `tool.driver.rules` (short description,
full description, and help text pointing at `celerrate explain`); a
rule-owned identifier's `reportingDescriptor` additionally carries the
owning rule's name in `properties.rule`, since the identifier-to-rule
relation is constant per run and repeating it on every result would be
wasteful.

Findings become `results` with physical locations (`columnKind` on the
run is `unicodeCodePoints`); exit-neutral notices become `level: note`
results without a location. Safe suggestions become `fixes` with
byte-precise replacements; what SARIF cannot carry honestly
(needs-review suggestions, engine notes) rides in the result's
`properties` (`needsReviewSuggestions`, `notes`).

The run's own `properties` carry `baselinedHidden`, the count of
findings the baseline hid: nothing else in the document can express
it, so without it a run with no results and a dozen baselined findings
would be indistinguishable from a genuinely clean project.

Internal errors become `toolExecutionNotifications` on the invocation,
the standard SARIF place for a problem the tool itself hit rather than
a finding about the analyzed code, each carrying its message and a
`descriptor.id` naming its `kind`. Every `kind` a run's notifications
reference is described, sorted and deduplicated, under
`tool.driver.notifications`, exactly like `tool.driver.rules` describes
every referenced identifier: SARIF resolves a notification's descriptor
reference against that array, so leaving it undescribed would dangle
the reference. Both `toolExecutionNotifications` and `notifications`
are omitted entirely when a run hit no internal error, rather than
shipped as empty arrays.

Upload to GitHub code scanning:

```yaml
- run: celerrate check --output=sarif > celerrate.sarif || true
- uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_file: celerrate.sarif
```

## GitHub Actions

```sh
celerrate check --output=github
```

`::notice` for each exit-neutral notice, then one workflow command per
finding (`::error` or `::warning` with `file`, `line`, `col`,
`endLine`, `endColumn`) for a finding with a `span` anchor. A finding
with a `project` anchor instead, the same distinction the JSON section
describes, emits the command with no file properties, exactly like an
internal error's line below. Then one `::error::` line per internal
error with no file properties (an internal error is a problem the tool
itself hit, not a finding anchored in the analyzed code), and finally
the end-of-run summary. The internal-error lines print after the
diagnostics and before the summary, so the summary genuinely closes the
output whether or not the run degraded. GitHub renders these as native
pull-request annotations with no further setup:

```yaml
- run: celerrate check --output=github
```

## Exit codes

Identical in every format: 0 clean, 1 diagnostics reported, 2 internal
or usage error. The JSON summary and the SARIF invocation embed the
same number the process exits with. The detail behind exit code 2 now
travels in every machine format, not just its count: the JSON
`internal_errors` array, the SARIF `toolExecutionNotifications`, and
the GitHub `::error::` lines all carry it, so tooling that sees exit
code 2 can learn why without re-running the tool on the human channel.
