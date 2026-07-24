---
schema_version: 1
id: RAC-KWQH3TZ4Y01Y
type: roadmap
---
# Rebuild for Simplicity and Single-Node Scale

## Status

Planned

## Context

The engine re-derives its expensive structures from disk on every read.
ADR-032 chose that deliberately at hundreds-of-artifacts scale, ADR-099
answered its review trigger with an opt-in content-addressed derived cache —
but the serving path is still corpus-bound per call: the cache key is
recomputed by re-hashing every file, search scores over materialized
per-corpus vectors, and the CLI paths do not use the cache at all. Measured
on a generated 1,000-artifact corpus, a warm MCP retrieval is ~900 ms
uncached and ~190 ms cached; both lines grow with corpus size.

This roadmap records the intent of a two-movement rebuild of `rac-core`:
first simplify with behavior frozen, then supersede the speed-pinning
decisions on the record so retrieval works at enterprise scale on a single
node. Distribution and sharding are explicitly out of scope; the levers are
single-node — parse-once, a persistent memory-mapped index, prune-early
traversal, and incremental recompute.

## Outcomes

- Externally observable behavior is unchanged: CLI and JSON contracts, exit
  codes, golden outputs, and the public API hold, enforced by the frozen
  test suite and a cross-repo `rac-connectors` contract check.
- The codebase is measurably simpler and more maintainable: complexity,
  duplication, and LOC reduced; one obvious home per concern; fully typed.
- Retrieval is scale-invariant on one node: warm retrieval p99 under 100 ms
  and p50 under 30 ms, flat from 1M toward 10M artifacts; incremental
  re-validation of a ~1,000-file changeset under 5 s independent of corpus
  size; cold full build linear and parallel (~2 min per 1M artifacts);
  working-set RSS bounded (index memory-mapped on disk, never resident).
- Each architecture change that supersedes a recorded decision ships with a
  new decision record, new pinning tests, and a measured win on the
  performance harness.

## Initiatives

- Baseline and before-evidence: a deterministic scale-corpus generator and a
  rerunnable performance harness in `itsthelore/asdecided-benchmarks`
  (`scalecorpus/`), measuring the legacy engine across a 1k → 3M curve.
- Examiner hardening: characterization tests for unpinned behavior and a
  performance gate asserting the scale target, both human-approved before
  the suite freezes.
- Movement A — simplify with behavior frozen: rebuild subsystems from
  audited briefs for simplicity and maintainability; land fully green.
- Movement B — scale with architecture open: supersede the speed-pinning
  decisions (per-call re-read, per-call re-hash, re-parse on every read) via
  recorded decisions; persistent mmap-backed index, incremental validation.
- After-evidence and publication: the same harness rerun, the before/after
  scale curve as the headline, honest reporting of any missed target.

## Success Measures

- The frozen examiner passes twice consecutively from clean with the rebuilt
  engine, and `rac-connectors` builds and tests green against it unchanged.
- The performance harness gates pass at the top measured corpus point, with
  a flat operational-latency curve and the 10M claim stated as measured or
  extrapolated, never faked.
- Complexity, duplication, and LOC are reduced against the recorded
  baseline; typing and lint gates hold.

## Assumptions

- The reference node is fixed and stated with every number: 4 vCPU / 15 GiB
  RAM (no swap) / ~30 GB free disk. The measured curve tops out where disk
  allows (~3M artifacts); 10M is an extrapolated claim from curve flatness.
- The benchmark repo consumes `rac` strictly as an external CLI on `PATH`
  (DG-ADR-0001), so harness numbers survive engine rebuilds unchanged.

## Risks

- A latency target may be physically unreachable on the reference node; the
  wall is then reported with numbers, not narrowed or faked (no sharding, no
  external services — those are out of scope by decision).
- Freezing the examiner too early leaves contract gaps; mitigated by the
  characterization-test pass and the human checkpoint before the freeze.

## Related Decisions

- RAC-KTW0M81E7TRA
- RAC-KWMZ3MR9DZ09
- RAC-KV4ZAGWPAA6X
- RAC-KTQ63DRPK57V
- RAC-KWFVA38YT2C0

## Related Roadmaps

- lore-at-team-scale
