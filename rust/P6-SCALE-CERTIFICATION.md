# P6.7 Scale Certification

Date: 2026-07-22

Decision: ADR-119

Result: **certified and cut over for the 5,000-artifact production envelope**

## Purpose

This certification compares the existing snapshot freshness path with the
ADR-119 base-plus-delta preview. It measures the complete freshness lifecycle,
not an isolated parser or index microbenchmark: cold establishment, unchanged
warm reads, one-file edit/add/delete/rename publication, and threshold
compaction.

The benchmark is `rac-engine/examples/p6_scale.rs`. It restores every corpus
mutation before exit and reports JSON with raw samples plus p50/p95 summaries.
It also reports the last parse count and overlay size so a fast result cannot
silently come from skipping freshness work.

## Adoption gates

The preview may become the default for the 5,000-artifact production envelope
only when all of these are true:

1. Existing mutation referees remain byte-identical to fresh whole-corpus
   generation.
2. A one-file mutation parses one file and leaves one delta row.
3. At 5,000 files, warm p95 is at most 25 ms and mutation p95 is at most
   150 ms.
4. Threshold compaction at 5,000 files is no more than 20% slower than the
   snapshot path.
5. Peak resident memory at 5,000 files is recorded before cutover and fits the
   deployment budget; the preview deliberately retains parsed documents.
6. The 5,000-file lifecycle passes a bounded soak without freshness,
   determinism, or cache/no-cache divergence.

Cold establishment is measured and published but is not an interactive cutover
gate: it occurs when establishing a disposable base, while warm freshness and
mutation publication define the normal serving experience. A regression that
affects operational safety or makes startup unreasonable remains a release
blocker.

The 100,000-file results are forward-looking scale evidence, not a gate for the
5,000-file release. Manifest freshness scanning and exact global
portfolio/search reductions can remain corpus-linear, but each supported scale
tier must fit its published user-visible budget.

## Environment and method

- Apple silicon (`arm64`, T8103), macOS Darwin 27.0.0
- Rust 1.96.0, release build
- synthetic corpus from `rust/tools/gen_corpus.py`, seed 1234
- 5,000-file distributions: seven delta iterations; three snapshot iterations
- 100,000-file scale probe: one iteration per mode, so the reported value is a
  point estimate rather than a statistically meaningful p95
- cache directories were distinct per mode and run

Build and run:

```sh
cd rust
cargo build --release -p asdecided-core --example p6_scale
target/release/examples/p6_scale snapshot CORPUS CACHE_DIR 7
target/release/examples/p6_scale delta CORPUS CACHE_DIR 7
```

Peak RSS is measured by running the same complete lifecycle command under
macOS `/usr/bin/time -l` and recording `maximum resident set size`.

## Results

All durations are milliseconds.

### 5,000 files

| lifecycle operation | snapshot | delta preview | gate |
|---|---:|---:|---|
| cold | 1007.44 | 1873.85 | measured: +86.0% |
| warm p50 / p95 | 10.21 / 13.14 | 12.11 / 17.92 | pass |
| edit p50 / p95 | 390.53 / 571.71 | 52.14 / 90.22 | pass |
| add p50 / p95 | 400.01 / 854.64 | 56.60 / 60.22 | pass |
| delete p50 / p95 | 617.95 / 688.93 | 56.07 / 62.25 | pass |
| rename p50 / p95 | 431.24 / 508.68 | 53.50 / 61.15 | pass |
| threshold compaction | 932.95 | 932.13 | pass |

The 5,000-file target is achieved for warm and mutation latency. Each final
mutation parsed one file and left one delta row. Threshold compaction is flat.
Cold establishment is slower but remains below two seconds on the reference
machine and is outside the normal warm serving lifecycle.

### 100,000 files

| lifecycle operation | snapshot | delta preview | gate |
|---|---:|---:|---|
| cold | 31585.06 | 54677.07 | fail: +73.1% |
| warm | 321.16 | 387.44 | pass |
| edit | 16859.92 | 3159.01 | fail |
| add | 13682.46 | 1801.63 | fail |
| delete | 15203.05 | 1709.89 | fail |
| rename | 14607.75 | 1881.99 | fail |
| threshold compaction | 28406.54 | 47100.41 | fail: +65.8% |

The preview reduces one-file mutation latency by roughly 4.8x to 8.9x versus
snapshot rebuilds, and the final mutation still parses one file with one delta
row. However, 1.7-3.2 seconds is not a satisfactory interactive publication
budget. This scale is therefore experimental evidence for a later tier, not a
reason to delay the 5,000-artifact production envelope.

## Findings

Certification exposed and removed two avoidable costs:

- Cold preview construction previously staged every document through the
  cumulative overlay, producing quadratic work. Cold establishment now builds
  compact bases directly from the parsed corpus.
- Candidate completeness previously cloned all live documents merely to count
  them. `DeltaDocuments::live_len()` now computes exact cardinality from base,
  tombstones, replacements, and upserts without materialization.

The remaining 100,000-file mutation time is therefore not a full reparse:
instrumentation confirms one parsed file. It is publication/derivation work,
including exact corpus-global reductions identified in ADR-119 P6.5. The next
scale release should profile those reductions and make their maintained state
incremental where exactness permits.

## Scale release ladder

RAC does not reject a corpus at 5,001 artifacts. The number defines the largest
currently recommended production envelope with an interactive-latency promise.
Larger corpora remain usable on a best-effort basis and are promoted through
measured scale releases:

| tier | corpus | release objective |
|---|---:|---|
| S1 | 5,000 | production baseline; warm p95 <= 25 ms and mutation p95 <= 150 ms |
| S2 | 10,000 | preserve the S1 interactive budget where practical |
| S3 | 25,000 | mutation publication comfortably below 500 ms |
| S4 | 50,000 | bounded incremental publication and compaction |
| S5 | 100,000 | mutation publication below 1 second |

Each tier requires correctness invariants, peak-RSS evidence, and the complete
lifecycle matrix. Demand or a clearly reusable architectural improvement—not
the existence of the next round number—triggers work on a higher tier.

## S1 cutover certification

Issue #375 repeated the complete 5,000-file matrix on a regenerated,
validation-clean seed-1234 corpus before changing the production constructor.

| lifecycle operation | snapshot | delta | S1 gate |
|---|---:|---:|---|
| cold | 983.42 ms | 2272.48 ms | measured, non-interactive |
| warm p95 | 10.69 ms | 17.60 ms | pass: <=25 ms |
| edit p95 | 513.60 ms | 140.08 ms | pass: <=150 ms |
| add p95 | 552.60 ms | 127.69 ms | pass: <=150 ms |
| delete p95 | 894.60 ms | 114.68 ms | pass: <=150 ms |
| rename p95 | 526.84 ms | 104.76 ms | pass: <=150 ms |
| threshold compaction | 1502.16 ms | 1440.16 ms | pass: delta 4.1% faster |
| maximum resident set | 466 MiB | 593 MiB | pass |

The S1 memory budget is at most 768 MiB peak RSS and at most 1.5 times the
snapshot lifecycle. Delta used 1.27 times snapshot RSS, incurred no swaps, and
remained 175 MiB below the absolute budget.

The release-mode `p6_soak` certification then ran 100 unchanged reads and 21
certified transitions over three rounds, including edit, restore, add, delete,
rename, rename-back, threshold compaction, and first edit after compaction. It
reported zero validity, determinism, freshness, cache/no-cache, or persisted
segment byte-equality divergence. The first post-compaction edit parsed one
file.

With every S1 gate satisfied, `FreshnessTracker::new()` now selects the delta
lifecycle used by normal MCP serving. `FreshnessTracker::new_snapshot()` keeps
the established snapshot implementation as an explicit rollback path for the
initial soak release.

## Verdict

P6's architecture and correctness work is complete, and S1 is certified on
latency, memory, lifecycle safety, and correctness. Delta is the production
default; snapshot is the explicit rollback path. Preserve the higher-scale
matrix as roadmap and regression evidence rather than a release blocker.
