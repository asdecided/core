---
schema_version: 1
id: RAC-KWSJZJ30EN1J
type: decision
---
# ADR-107: Parallel Cold Build of the Derived Index

## Context

The cold build of the derived read-model — the whole-corpus walk that
`DerivedIndexCache` runs on a cache miss and the `FreshnessTracker` runs on its
first serve — is the last unparallelised scale path in Movement B. Profiled on
the 4-core reference node against a 20,000-artifact generator corpus, the serial
build spends its wall time as: **parse ~75% (19.2 s), derive ~24% (6.8 s: index
inbound counts, relationship resolution, field tokenisation, portfolio, scope),
store write ~10% (2.9 s)** — an aggregate of ~690 files/s, i.e. ~24 min/1M. The
performance lens (v2 §4) sets the gate at ≤~2 min/1M using all cores, records an
honest expectation of 3–4 min/1M, and names ≤2 min AT RISK because the build
does strictly more per file than the 1.2k files/s/core validate baseline it was
priced against, and the derive/serialise phases are a serial Amdahl term unless
partitioned.

Parsing is the dominant cost and is embarrassingly parallel: `parse_file` is a
pure function of a file's bytes and the ambient byte cap, and the corpus walk
yields files in a fixed sorted order. The constraints are hard: byte-parity with
a serial build is non-negotiable (ADR-002, ADR-103); workers must reproduce the
exact pinned parse semantics (`errors="replace"` lossy decode, BOM-defeats-
frontmatter, the two oversize messages, the unreadable sentinel — core-data
§1.3) by calling the *same* `parse_file`, never a reimplementation; and
correctness may never depend on the parallel rung (ADR-080).

A second, related residual: ADR-105's `FreshnessTracker` keeps the whole parsed
snapshot (`_entries`, the resident `Product` graph for every file) live for the
server's lifetime so it can re-derive incrementally, even after a compaction has
written a fresh mmap base that already holds every derived row. The performance
lens (v2 §1.3, §6 wall 4) names the resident snapshot a working-set residual on
the 15 GiB no-swap node.

## Decision

The cold build parses across processes and merges deterministically; the serving
tracker sheds its resident parsed snapshot after compaction.

**Parallel parse, deterministic merge (`services/parallel_build.py`).** Workers
each parse a *contiguous* range of the sorted `find_markdown_files` list through
the one true `parse_file` + `classify` path — a module-level worker function
under the `spawn` context, no lambdas or forked state crossing the boundary — and
the parent concatenates the ranges back in list order. The merged parsed snapshot
is therefore byte-for-byte the sequence `walk_corpus` yields, **regardless of
worker count**: the determinism rule is that merge order is fixed by sorted path
and the worker count is invisible to every downstream byte. The derive and
serialise phases run unchanged on that ordered snapshot, so the store segment
files and every served response are identical across worker counts, asserted by
hashing the segment files of a `workers=1` and a `workers=4` build.

**Where it applies, and the single-process threshold.** The parallel path is used
only where a store is built from nothing: `DerivedIndexCache.load_or_build`'s cold
miss and the `FreshnessTracker`'s cold start. The default no-cache one-shot CLI
paths stay single-process and byte-identical — they are one invocation each, and
below large N the spawn + IPC cost of returning parsed objects exceeds the parse
win. The build stays single-process when `cpu_count() <= 2` or the corpus is under
a file-count threshold (default 5,000, `RAC_PARALLEL_BUILD_MIN_FILES`); the
crossover is soft and the threshold is set where the measured win first clears the
fork overhead.

**Correctness never depends on the parallel rung.** Any worker fault — an
exception, a crashed child, a pickling failure — is caught and the build falls
back to the serial `walk_corpus`, which cannot produce a partial or corrupt
snapshot. A partially-mapped worker result is discarded whole; the store is never
written from a truncated parse.

**Post-compaction snapshot shedding.** After a compaction writes a fresh base for
the current hash and the tracker begins serving from the mmap view, the resident
parsed `Product` snapshot is dropped and a shed flag is set. Unchanged reads then
hold no whole-corpus snapshot; the next change repopulates the snapshot on demand
by re-parsing the current tree (parallel when large) before re-deriving. This
retires the resident-snapshot residual ADR-105 named, trading one re-parse per
compaction cycle for a bounded serving working set.

**Timing visibility.** When `RAC_TIMING` is set, the cold-build path writes one
`rac-timing: build_parse_ms=X build_derive_ms=Y build_write_ms=Z workers=N
files=M` line to stderr (env-gated, default absent, stdout untouched), mirroring
the incremental-validate scorecard line (ADR-106).

## Consequences

The cold build is faster and its parse cost scales with cores, while the store it
writes is provably identical to a single-process build — the cache and server pay
less latency on first fill with zero parity risk. The serving working set is
bounded through a compaction cycle: after compaction the tracker's RSS returns
near its pre-parse baseline instead of carrying the whole parsed corpus for the
process lifetime.

The honest cost accounting is part of the decision. **Measured on the 4-core
reference node (a legacy benchmark pinned one core throughout — noted, not
hidden), 20k corpus: serial 692 files/s; parallel with 4 workers 887 files/s — a
1.28x end-to-end speedup, from a 1.79x parse speedup (1,043 to 1,873 files/s).**
Extrapolated linearly that is **~18.8 min/1M measured, ~15 min/1M on an
uncontended box — ≤2 min/1M is missed, and the 3–4 min expectation is also
missed.** Two reasons, both stated: (1) parallel-parse efficiency is ~1.8x, not
4x, because one core was contended (caps the box at ~3x) and returning parsed
`Product` objects across the process boundary costs ~30–40% of the theoretical
parse gain; (2) this bundle ships **only** the byte-safe parse-parallelism — the
derive (~24%) and serialise (~10%) phases stay serial, a ~34% Amdahl tail that
now dominates the parallel build.

The recovery lever the performance lens names — workers emitting per-range partial
structures (token vectors, postings-run fragments, identity/edge rows) and a
term-range-partitioned parallel merge, so parse *and* derive fan out and only
compact rows cross the process boundary — is **not** taken here: it requires
moving global inbound-count, relationship-resolution, and portfolio-aggregation
work behind a merge that reproduces the serial bytes exactly, a large change with
real parity risk. It is recorded as the next lever, gated on the same segment-file
parity assertion this bundle establishes. Per the scalecorpus rule the miss is
reported with numbers, never narrowed.

**Budget revision.** The ≤2 min/1M gate (120 s/1M) was set against a
several-fold larger node than the 4-core reference box these numbers are
measured on; it is not reachable there even at full parallel efficiency. The
cold-build budget is therefore revised to **432 s/1M at 4 workers
(≈1.73 core-ms/artifact, stated per-core so it is node-portable)** — the
realistic target the term-range-partitioned merge is the initiative to reach.
The ~18.8 min/1M above is the *current* measured cost, still over the revised
target until that merge lands; it is not narrowed. A later 1M run also exposed a
harder wall than throughput: the materialise-all-then-write build was
**OOM-killed at 15.9 GiB on the 15 GiB no-swap node**, so at one million
artifacts it could not be built there at all as first shipped. Streaming the
segment writes (encode → flush → free one segment at a time, byte-identically)
removes the whole-store co-residence that caused it, so the build fits under the
node ceiling; the numeric gate lives in the scalecorpus harness.

The snapshot shed costs one full re-parse on the first change after each
compaction (the shed branch of the tracker's apply). Because compaction only fires
after a large delta window, the amortised cost is acceptable and the common
unchanged-read path pays nothing.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

**Ship the parsed snapshot resident forever (ADR-105 as-is).** Rejected for the
serving path: it holds the whole `Product` graph for the process lifetime, the
named working-set residual, when the mmap base already carries every derived row.

**Parallelise derive too, by per-shard `DerivedIndex` plus merge, in this
bundle.** Rejected for now: inbound counts, relationship resolution, and portfolio
aggregation are global, so a per-shard build breaks byte-parity unless the merge
recomputes them exactly. High parity risk for a bundle whose gate is
determinism; recorded as the next lever instead.

**Return only compact per-file derived rows from workers (avoid shipping
Products).** The right long-term shape and the precondition for the term-range
merge above, but it requires the parent's global-merge rewrite; deferred with it.

**`fork` instead of `spawn`.** Rejected: `fork` inherits interpreter state and is
unsafe with threads and the long-lived server's open mmaps; `spawn` with a
module-level worker is the portable, state-clean choice the constraint demands.

## Related Decisions

- ADR-103
- ADR-104
- ADR-105
- ADR-106

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "Parallel construction and its serial fault floor are named native paths with byte-parity tests."
rules:
  - id: native-build-keeps-bounded-rayon-pool
    kind: require_pattern
    path_glob: "rust/rac-engine/src/parallel_build.rs"
    pattern: '(?s)rayon::ThreadPoolBuilder::new\(\).*num_threads\(n_workers\)'
    message: "The native cold build must retain its explicitly bounded worker pool."
  - id: parallel-fault-keeps-serial-floor
    kind: require_pattern
    path_glob: "rust/rac-engine/tests/parallel_build.rs"
    pattern: 'a fault must land on the serial floor'
    message: "A parallel worker failure must remain covered by the serial fallback test."
```
