---
schema_version: 1
id: RAC-KWY0SHYCVY2D
type: decision
---
# ADR-108: Term-Range-Partitioned Parallel Merge for the Cold Build

## Context

ADR-107 fanned the cold build's *parse* across processes but left the whole
*derive* serial and shipped fully parsed `Product` objects back from each
worker. Its own accounting names the two costs that remain: returning parsed
`Product` graphs across the process boundary spends ~30–40% of the theoretical
parse gain, and the serial derive (inbound counts, relationship resolution,
field tokenisation, portfolio, scope) plus serialise is a ~34% Amdahl tail that
now dominates the parallel build. ADR-107 recorded the recovery lever — workers
emitting compact per-range fragments and a term-range-partitioned parallel
merge, so parse *and* derive fan out and only compact rows cross the boundary —
as the next lever, explicitly deferred because it requires moving the global
inbound-count, relationship-resolution, and portfolio-aggregation work behind a
merge that reproduces the serial bytes exactly, a change with real parity risk.

This is that bundle. The revised cold-build budget (ADR-107: 432 s/1M at 4
cores) is not reachable while the derive is serial; the fan-out is the
initiative that closes the gap. Bundle 1's streaming segment writes already
removed the memory ceiling; this bundle is the throughput half.

The parity contract is unchanged and absolute: the on-disk segment bytes are a
pure function of the sorted-path snapshot and invisible to worker count, and the
served read-model equals a fresh `build_derived_index` for every corpus state.
Determinism rests on the single sorted-path iteration order that drives docid
assignment, `build_resolution_index`'s append order, inbound counts, and the
portfolio. Any merge must reproduce that order exactly.

## Decision

Workers emit **compact per-document derived fragments**, not parsed `Product`
objects, and the parent assembles the read-model from them. Each worker owns a
contiguous path-range of the sorted `find_markdown_files` list (as ADR-107
established) and returns, per document, only the projection the derive reads:
the resolution identifiers (`artifact_identifiers`), the extracted declared
edges (`extract_relationships_full`), the search sections, the five tokenised
fields, the per-field lengths, the local vocabulary, and the per-document
validation/scope/live projection. The parsed `Product` never crosses the
boundary.

The parent merge (`services/parallel_merge.py`) reproduces the serial derive
byte-for-byte:

- **Docids** are assigned by concatenating the path-ranges in list order — the
  worker's chunk base offset plus its local index — reproducing the
  global-sequential sorted-path assignment.
- **Vocabulary and postings** merge by union of the local vocabularies into one
  `sorted` term dictionary, and per-term postings are the chunk-order
  concatenation of each range's ascending local-docid runs offset by the chunk
  base — ascending by construction, identical to the serial append order.
- **Resolution** rebuilds `build_resolution_index` over all identifier rows **in
  sorted-path order** and resolves every edge in the parent, reproducing
  `relationships_from_corpus` exactly; inbound counts fall out of the resolved
  edges (Bundle 1's `inbound_counts_from_relationships`). Resolution is global by
  nature — a reference in one range may target any other — so it stays in the
  parent; only its *inputs* are produced in parallel.
- **Portfolio** aggregates from the per-document validation projection the
  workers emit, in sorted-path order, reproducing `portfolio_from_corpus`
  including the order-sensitive `unknown_paths`.

**Determinism rule (unchanged from ADR-107):** merge order is fixed by sorted
path and the worker count is invisible to every output byte. **Correctness never
depends on the parallel rung:** any worker fault — exception, crash, pickling
failure — falls back to the serial `walk_corpus` + serial
`build_derived_index_from_entries`, which can never produce a partial or corrupt
snapshot; partial results are discarded whole. This bundle adopts Bundle 1's
streaming segment writes as its companion memory posture.

## Consequences

### Positive

- The derive fans out with the parse: tokenisation, identity and edge
  extraction, and the per-document validation projection all run in the workers,
  shrinking the serial Amdahl tail toward the revised budget.
- Only compact rows cross the process boundary, removing the ~30–40% `Product`
  pickling overhead ADR-107 measured.
- The parent holds compact rows, not a resident parsed snapshot, complementing
  Bundle 1's streaming writes on the memory axis.

### Negative

- The parent now reproduces `build_resolution_index`, `relationships_from_corpus`,
  the inbound counts, and the portfolio from compact rows rather than from
  `Product` objects — the most byte-parity-fragile code in the system. The risk
  is bounded by the per-mutation-class parity gate below and by the serial
  fallback that remains the correctness floor.
- A second construction path for the derived rows exists alongside the serial
  one; both are pinned to byte-equality, so a drift is a test failure, never a
  silent wrong answer.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

### Per-shard derive (each worker builds a whole sub-index)

Inbound counts, relationship resolution, and portfolio aggregation are global —
a shard cannot resolve a reference whose target lives in another shard — so a
per-shard build breaks byte-parity unless the parent recomputes them exactly.
Rejected: the merge must reproduce the global derivations, not shard them.

### Keep returning `Product` objects and only parallel-tokenise

Fanning out tokenisation while still shipping `Product` objects adds the token
vectors to the boundary payload on top of the Products — more IPC, not less, and
it leaves the ~30–40% pickling overhead in place. Rejected.

### An external merge or streaming framework

Rejected on ADR-104's red lines: single node, stdlib only, no external services.
The merge is plain `multiprocessing` (spawn) over compact rows, as ADR-107's
parse fan-out already is.

## Relationship to Other Decisions

- ADR-107 (RAC-KWSJZJ30EN1J): supersedes its "the derive stays serial" clause and
  takes the deferred recovery lever it recorded; keeps its worker-invariance
  determinism rule and segment-file parity assertion as this bundle's gate.
- ADR-104 (RAC-KWS7QCT10Q5A): the segment store this build writes; the term-major
  postings and sorted term dictionary are the format the merge targets, and
  streaming writes are the companion memory posture.
- ADR-103 (RAC-KWS4Y9KCTD90): the unified derived read-model the merge produces.
- ADR-078 (RAC-KVSQ24G2H2D6): the inbound-count graph signal the resolved edges
  feed.

## Related Roadmaps

- rebuild-scale

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "Indexed parallel collection and worker-count-invariant store bytes expose the deterministic merge boundary."
rules:
  - id: parallel-fragments-preserve-input-order
    kind: require_pattern
    path_glob: "rust/rac-engine/src/parallel_build.rs"
    pattern: '(?s)paths\s*\.par_iter\(\).*collect::<Vec<DocFragment>>\(\)'
    message: "Parallel document fragments must continue to collect in canonical input order."
  - id: merge-output-remains-worker-invariant
    kind: require_pattern
    path_glob: "rust/rac-engine/tests/parallel_build.rs"
    pattern: 'store bytes must be worker-count-invariant'
    message: "The persisted merge result must remain byte-identical across worker counts."
```
