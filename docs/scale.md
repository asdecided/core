# Scale & performance

AsDecided is designed so a growing corpus stays fast on a single node. The engine's
original posture (ADR-032) re-derived its expensive structures — the repository
index, the resolved relationship graph, and the search token vectors — from disk
on **every** read. That is the right default at hundreds of artifacts, but its
per-call cost grows with corpus size. The work described here supersedes those
speed-pinning decisions with recorded ones, so retrieval stays scale-invariant at
enterprise size on one node.

Every mechanism below shares three guarantees, so turning it on can never change
an answer — only its speed:

- **Byte-identical.** Cached and uncached paths produce the same bytes. This is a
  hard test gate (a store built from the cache equals a fresh build, and a
  parallel build equals a single-process one, segment-for-segment).
- **Content-addressed.** Every cache is keyed on a hash of the corpus (or of a
  file × the active config). Any byte change to any artifact changes the key and
  forces a rebuild — there is no time- or event-based staleness.
- **Disposable.** The files in git are the truth; the index is a rebuildable
  derived structure. Deleting it — or hitting a corrupt or format-outdated one —
  costs only latency, never correctness. No daemon, no lockfile, no database
  (ADR-080).

Nothing here uses AI or approximation: retrieval stays deterministic and lexical
(ADR-037/038/066).

## Supported scale envelope

RAC's current recommended production envelope is **up to 5,000 artifacts** on
a single node. At that S1 tier, the Rust delta engine targets warm freshness at
or below 25 ms p95 and one-file mutation publication at or below 150 ms p95.

This is a performance promise, not a hard corpus limit. RAC continues to accept
larger corpora, but operation above the certified envelope is best-effort until
the corresponding scale tier is promoted:

| tier | corpus | release objective |
| --- | ---: | --- |
| S1 | 5,000 | production baseline; delta is the default freshness lifecycle |
| S2 | 10,000 | preserve the S1 interactive budget where practical |
| S3 | 25,000 | mutation publication comfortably below 500 ms |
| S4 | 50,000 | bounded incremental publication and compaction |
| S5 | 100,000 | mutation publication below 1 second |

Every promotion requires the complete cold, warm, edit, add, delete, rename,
compaction, and peak-memory matrix plus validity, determinism, freshness, and
cache/no-cache equality. Higher-tier engineering is demand-led; an uncertified
tier does not delay improvements or releases inside the current envelope.

## 1. A persistent, memory-mapped index

The derived index is a persistent **memory-mapped segment store** (ADR-104), not
an in-memory blob rebuilt per call. Segments — the term dictionary, term-major
postings, the document store, the relationship graph — are mapped from disk and
paged in on demand, so the working-set memory stays **bounded** regardless of
corpus size: the index lives on disk and is never fully resident. Search is
served directly from the term-major postings (ADR-038), and point lookups resolve
through mapped identity segments without materialising the whole corpus.

## 2. Caching on by default on the paths that matter

Reuse of that store is **the default** (ADR-112) on the three surfaces where
repeated reads against a stable corpus dominate — `--no-cache` disables it per
invocation, `DECIDED_NO_CACHE=1` per environment:

| Command | Default reuse | What it reuses |
| --- | --- | --- |
| `decided-mcp` | on | The whole derived read-model for a long-lived server (ADR-099/104). |
| `decided find` | on | The persistent store for one-shot queries, instead of a fresh walk (ADR-110/112). |
| `decided validate` | on | A per-file result cache, so re-validation is incremental (ADR-106). |

For the long-lived `decided-mcp` server, freshness is tracked **incrementally** by
an event-sourced watcher on Linux (ADR-105/118), so a clean warm endpoint can
answer without walking the corpus. Platforms without a synchronous
completed-write barrier use the parallel stat-manifest fallback; this preserves
freshness but remains O(files). A one-shot `decided find` has no long-lived process
to hold that watcher, so it verifies freshness through a **persisted stat
manifest** (ADR-112): every
enumerated file is stat'ed and only stat-changed files are re-read, so an
unchanged corpus is confirmed at O(files) stat cost with zero artifact-byte
reads. The one rewrite shape stats cannot see — a size- and mtime-preserving
in-place rewrite (ADR-105's S5) — is caught by `--verify`, which forces the
full byte re-hash floor. The first-ever query against a corpus pays the cold
build; every later warm query rides the store.

`decided validate` keys each file's result on its content hash × the active config
fingerprint, so validating a large corpus after a small edit does work
proportional to what changed. A changed config invalidates exactly the affected
results; a corrupt cache recomputes from scratch; `--verify` applies the same
full-hash freshness floor.

## 3. A parallel cold build

Building the index from nothing (the cold-miss path) fans out across CPU cores
(ADR-107/108). Workers each parse a contiguous range of the sorted file list and
emit compact per-document *derived fragments*; the parent reproduces the
read-model from them in sorted-path order. Only compact rows cross the process
boundary — never parsed documents — so both the parse **and** the derive
parallelise while the result stays worker-count-invariant: a build with four
workers writes the same store, segment-for-segment, as a single-process build.
The parallel path is a latency lever, never a correctness dependency — below a
file-count / core threshold, or on any worker fault, the build falls back to the
authoritative single-process path and discards partial results whole.

## 4. The numbers

Concrete latency and throughput figures — the before/after scale curve across a
1k → millions-of-artifacts corpus on a fixed reference node — live in the
[`rac-benchmarks`](https://github.com/itsthelore/asdecided-benchmarks) harness, which
consumes `rac` strictly as an external CLI so the numbers survive engine
rebuilds. The design target is a **flat** warm-retrieval curve as the corpus
grows and a cold build that scales linearly and parallel; the harness reports
each number as measured or extrapolated, never faked, with the reference node
stated alongside it. Distribution and sharding are explicitly out of scope — the
levers here are single-node.

## Related decisions

- [ADR-104](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-104-persistent-mmap-index-store.md) — persistent memory-mapped index store
- [ADR-105](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-105-event-sourced-serving-freshness.md) — event-sourced serving freshness
- [ADR-106](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-106-incremental-validation.md) — incremental directory validation
- [ADR-107](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-107-parallel-cold-build.md) — parallel cold build
- [ADR-108](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-108-term-range-partitioned-parallel-merge.md) — term-range-partitioned parallel merge
- [ADR-110](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-110-one-shot-find-store-reuse.md) — one-shot `decided find` store reuse
