# Cold-Run Lever Implementation: Measurements

Machine: the reference 10-core machine behind every published figure.
Corpus: pinned PrestaShop comparison corpus
`fc96d0d4eae383e8c6f1f54f19cf592c221a62e3`, equalized file set (6932
first-party files, root `celerrate.toml` with `include = ["."]`).
Measurement base: `target/comparison-corpus-equalized`.

## Protocol

- Cold run: `rm -rf .celerrate` in the corpus directory, then
  `<binary> check .` from the corpus directory, binary named by
  absolute path.
- Every A/B is measured in one session, sides alternated
  (base, lever, base, lever, ...), machine otherwise idle, three
  repetitions minimum per side, medians reported, wall clock read
  from `/usr/bin/time -p`'s `real` line.
- Each session opens and closes with a control: three cold runs of
  the session's base binary, median recorded. Control drift above
  ~10 % invalidates the session's comparisons.
- Acceptance per lever: median gain positive and larger than each
  side's spread (max minus min); `cargo xtask corpus` and
  `cargo xtask mixed-rate` byte-identical; the full local gate suite
  green. A lever that fails is reverted and its measurement kept.

## 1. Session anchor (2026-08-08, commit 0222695)

`cargo xtask benchmark`, machine otherwise idle.

| Quantity | Value |
| --- | --- |
| PHPStan cold median | 36.468s |
| Celerrate cold median | 4.889s |
| Cold ratio | 7.5x |
| File-count cross-check | 6932 reported / 6932 counted |

The medians above come from the benchmark's own `scenario / median`
table. For reference, the same run's hyperfine mean and range lines
report PHPStan at 35.357s mean (32.706s to 36.896s over 3 runs) and
Celerrate at 4.898s mean (4.803s to 5.042s over 5 runs).

## 2. mimalloc supply-chain note

Facts gathered from `https://crates.io/crates/mimalloc` (crates.io API,
queried 2026-08-08) and from `cargo tree --invert --package mimalloc` /
`cargo tree --invert --package libmimalloc-sys` after declaring the
dependency.

| Quantity | Value |
| --- | --- |
| Crate | `mimalloc` |
| Version resolved | 0.1.52 |
| Licence | MIT |
| Last release | 2026-05-22 |
| Total downloads | 47,505,621 |
| Recent downloads (90 days) | 12,846,101 |
| Transitive dependency | `libmimalloc-sys` v0.1.49, MIT |

`cargo tree --invert --package mimalloc`:

```
mimalloc v0.1.52
└── celerrate_cli v0.1.0
```

`cargo tree --invert --package libmimalloc-sys`:

```
libmimalloc-sys v0.1.49
└── mimalloc v0.1.52
    └── celerrate_cli v0.1.0
```

`libmimalloc-sys` is the sole transitive dependency: it vendors
Microsoft's mimalloc C sources and builds them with the `cc` crate at
compile time; it carries the same MIT licence as `mimalloc` itself. Both
crates are single-purpose (allocator and allocator FFI binding
respectively), long-lived (created 2019), and under active maintenance
(latest release 2026-05-22, tens of millions of total downloads).

Reversibility: the swap is one attribute at the binary's composition
root (`crates/celerrate_cli/src/main.rs`) plus the two dependency
declarations. Removing the attribute and the two `Cargo.toml` entries
restores the system allocator with no other change anywhere in the
workspace.

## 3. Lever 1 A/B: mimalloc

Session date 2026-08-08, working tree on `perf-mimalloc-allocator` at
commit 6d3471d (the code change was carried uncommitted in the working
tree for the session; see the verdict below for its disposition).
Binaries: `target/ab/celerrate-base` (system allocator, the same binary
that produced the session anchor in section 1) and
`target/ab/celerrate-mimalloc` (freshly built after `#[global_allocator]`
was set, with all gates in Step 5 of the task green and
`cargo xtask corpus` / `cargo xtask mixed-rate` byte-identical to their
committed references). Corpus: `target/comparison-corpus-equalized`.

The machine was shared with other concurrently running agent sessions
for the whole of this task (`uptime` load averages moved between 6 and
10 during the attempts below, and multiple other `claude` processes were
independently consuming double-digit CPU percentages throughout). This
is not something the task had authority to quiesce: killing another
session's process was out of scope. Three full sessions were run and
discarded on the protocol's own control-drift gate before a fourth came
in clean; all four are recorded here for honesty rather than only the
one that passed.

### Discarded: session attempt 1

| Control | Values (s) | Median |
| --- | --- | --- |
| Open | 6.17, 4.65, 4.91 | 4.91 |
| Close | 5.47, 5.40, 5.14 | 5.40 |

Drift `(5.40 − 4.91) / 4.91 = 9.98%`. This is numerically just inside
the "~10%" bound, but the first open-control run (6.17s) was already an
outlier against the session anchor (4.889s), and `uptime` showed load
climbing through the session. Discarded out of caution rather than
banked as a pass on a technicality.

### Discarded: session attempt 2

| Control | Values (s) | Median |
| --- | --- | --- |
| Open | 5.58, 5.56, 5.32 | 5.56 |
| Close | 4.95, 4.67, 4.81 | 4.81 |

Drift `(5.56 − 4.81) / 5.56 = 13.5%`. Invalid; the drift reversed
direction from attempt 1, consistent with unrelated background load
rather than a monotonic warm-up or thermal effect.

### Discarded: session attempt 3

| Control | Values (s) | Median |
| --- | --- | --- |
| Open | 4.83, 5.04, 4.67 | 4.83 |
| Close | 4.93, 5.59, 6.09 | 5.59 |

Drift `(5.59 − 4.83) / 4.83 = 15.7%`. Invalid.

### Official session (attempt 4, valid)

| Control | Values (s) | Median |
| --- | --- | --- |
| Open | 5.49, 4.91, 4.82 | 4.91 |
| Close | 5.02, 4.72, 5.18 | 5.02 |

Drift `(5.02 − 4.91) / 4.91 = 2.2%`, well inside the ~10% bound. Session
valid.

| Side | Values (s) | Median | Min | Max | Spread (max − min) |
| --- | --- | --- | --- | --- | --- |
| base | 4.85, 4.66, 5.13 | 4.85 | 4.66 | 5.13 | 0.47 |
| mimalloc | 4.18, 4.50, 4.61 | 4.50 | 4.18 | 4.61 | 0.43 |

Gain: `4.85 − 4.50 = 0.35s`.

Acceptance requires the gain to exceed **each** side's own spread:

- `0.35 > 0.47` (base spread): false.
- `0.35 > 0.43` (mimalloc spread): false.

Both comparisons fail. mimalloc's median is lower in every one of the
four attempts above (base medians: 5.29s, 5.50s, 5.18s, 4.85s; mimalloc
medians: 4.67s, 4.53s, 4.50s, 4.50s), a directionally consistent signal,
but the gain never reliably clears the run-to-run noise on this
machine, and in the one session that passed the validity gate outright
it falls short of both spreads.

**Verdict: reject.** The gates are unaffected by this (all green,
`corpus` and `mixed-rate` byte-identical throughout every gate run), but
the acceptance rule is about the measured gain, not the gates, and the
gain does not clear its own bar. The code change
(`mimalloc`/`libmimalloc-sys` dependency declarations and the
`#[global_allocator]` attribute) is dropped from the working tree; only
this measurement record is kept. The supply-chain note in section 2
stands as a record of the due diligence performed even though the lever
itself is not adopted.
