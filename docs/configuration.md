# Configuration

Celerrate reads at most one file: `celerrate.toml` at the project
root, next to `composer.json`. There is no tree-walking, no
`include`-style file composition, and no global user configuration
outside the project. Zero configuration is fully supported: a project
with no `celerrate.toml` behaves exactly like one with an empty
`celerrate.toml`, and a missing file is not reported as an event.

## The full surface

```toml
[project]
php = "8.2"                 # optional; collapses the detected version range to a point
include = ["src", "tests"]  # optional; default: the Composer autoload roots
exclude = ["src/Generated"] # optional; subtracted from the walk

[rules.null-dereference]
enabled = false              # opt out of a Default-tier rule

[severity]
"CEL0034" = "warning"        # per-identifier remap, error <-> warning only
```

Every table is optional. Two spellings of the same table are
interchangeable: `[project]` on its own line and `project = { ... }`
inline parse to the same configuration.

## `[project]`

- **`php`**: a version point, `"8.2"`, nothing else. Ranges, carets,
  and prose (`"^8.1"`, `">=8.1"`) are rejected. When present, it wins
  outright: neither `composer.json`'s `require.php` nor its
  `config.platform.php` is consulted, and no PHP-version-detection
  notice fires, even when the manifest declares one. The point is
  clamped to the range Celerrate supports, exactly like an
  out-of-range `config.platform.php` would be.
- **`include`**: relative paths, in declaration order. A non-empty
  `include` replaces the Composer autoload-derived walk roots
  entirely; an empty array behaves exactly like an absent key. Paths
  are lexical: nothing checks that they exist on disk.
- **`exclude`**: the same path shape, subtracted from the walk by
  prefix match, independently of where the walk roots came from
  (Composer autoload or an explicit `include`).

An absolute path (a leading `/` or `\`, or a Windows drive letter), an
empty string, or a non-array value under `include` or `exclude` is
rejected entry by entry: the malformed entry is reported and dropped,
the well-formed entries in the same array stay.

## `[rules]`

Activation is per rule, under `[rules.<name>]`. The only key a rule
table recognizes today is `enabled`, a boolean; any other key is a
configuration diagnostic, because no shipped rule takes options yet.
An empty `[rules.<name>]` table is a valid no-op.

Every rule belongs to one of two tiers:

- **Default**: active unless `enabled = false` turns it off.
- **Nursery**: inactive unless `enabled = true` turns it on.

Setting `enabled` to a rule's own tier default (`true` on a Default
rule, `false` on a Nursery rule) is accepted as a no-op rather than
rejected, so a configuration written against one release keeps working
if a later release promotes or demotes a rule's tier.

`[rules.<name>]` accepts these names, all of them Default tier in this
release:

| Rule name |
| --- |
| argument-checks |
| null-dereference |
| symbol-version-gating |
| syntax-version-gating |
| unknown-members |
| unknown-suppression-identifier |
| unknown-symbols |
| unused-suppression |

No rule in this release ships as Nursery tier, so there is nothing to
opt into today; the tier and the no-op rule above exist for rules that
will ship as Nursery later. Naming anything else under `[rules.<name>]`
(a typo, a rule that does not exist) is a configuration diagnostic, not
a silent no-op.

## `[severity]`

Each key is a diagnostic identifier as a string, `"CEL0034"`; each
value is `"error"` or `"warning"`, nothing else, there is no third
state. Only the identifiers a shipped rule can actually emit are
remappable. Resilience identifiers, the ones that report on the
project's own inputs rather than on a rule finding (parse errors,
project discovery notices, and configuration diagnostics themselves)
are neither disableable nor remappable: naming one under `[severity]`
is a configuration error, not a way to quiet it.

## What is not configurable

`celerrate.toml` does not, and will not, hold:

- **The baseline file's name or location.** It is always
  `celerrate-baseline.toml` at the project root. `celerrate check
  --baseline` records it; `celerrate check --ignore-baseline` runs
  without it.
- **The output format.** It is a command-line choice,
  `--output human|json|sarif|github`, never a file setting.
- **Per-identifier disabling.** There is no key that turns off one
  diagnostic identifier outright. Suppress a specific occurrence with
  `@celerrate-ignore` (see [the PHPDoc bridge](phpdoc-bridge.md) for
  the full suppression table), or absorb existing findings with the
  baseline; `[rules]` only ever turns a whole rule on or off.

`[plugins]` is recognized structurally, but no plugin takes options
yet, so any key written under it is an unknown configuration key today.

## Errors are diagnostics

An unknown key, an unknown rule name, or an invalid remap is never
silently ignored: it is one of CEL0043 to CEL0049, each span-anchored,
each with an explain page (`celerrate explain CEL0043` and so on). A
typoed configuration fails the run on its own diagnostic while that
same run still analyzes the project under the default configuration:
a mistake in `celerrate.toml` never quietly disables nothing. CEL0043
(the file cannot be read as TOML at all) drops the whole file back to
defaults; every other identifier in the range skips only the malformed
part and keeps the rest of the file. See
[Configuration (CEL0043 to CEL0049)](diagnostics.md#configuration-cel0043-to-cel0049)
for the full table.

## Configuration and the cache

For a cached verdict to be reusable at all, several things about the
run that produced it must match the current run exactly, including
one digest computed over the normalized `[rules]` and `[severity]`
sections of `celerrate.toml` (entries sorted, so declaration order
does not matter; content compared, so an actual edit does). Changing
either section invalidates every cached verdict in one shot; an
unchanged `celerrate.toml`, or none at all, keeps the warm path.

`[project]` is not part of that same digest. A `php` override still
invalidates the cache, but through the PHP version range it produces,
not through the `[rules]`/`[severity]` digest. `include` and `exclude`
do not invalidate anything by themselves: they change which files are
walked, and a file that was already cached under an unchanged
configuration keeps serving its cached verdict the moment it is walked
again.
