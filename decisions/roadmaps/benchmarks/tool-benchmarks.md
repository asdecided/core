---
schema_version: 1
id: RAC-KWFVAB1XD9AC
type: roadmap
---
# Per-Tool Benchmark Suite (tool-benchmarks)

## Status

Planned

## Context

A stress-test of the existing grounding benchmark against rac-core's claims
found the coverage uneven: `find_decisions` — the one tool with a structural
supersession defense — had no benchmark anywhere; the hard-negative window
was top-5 while `search_artifacts` serves the full match list; the P@1 / R@5
gate was blind to ordering within ranks 2–5; `get_related`'s outgoing
direction was unscored; and the eval scores pre-budget output while agents
receive ADR-033 budget-truncated responses. This roadmap builds one benchmark
per Lore MCP tool in `itsthelore/asdecided-benchmarks` (subdir per benchmark,
ADR-092) on a shared harness, under the family contract of ADR-097.

Known corpus inconsistencies observed during the stress-test, flagged here
rather than silently fixed: ADR-066 records an ascending-artifact-id
tie-break that was never implemented (production ties break by path), and
ADR-078 is `Status: Proposed` while its BM25+RRF ranking is shipped and
gating CI.

## Outcomes

- Five per-tool benchmark subdirs (`search-artifacts`, `find-decisions`,
  `get-artifact`, `get-related`, `get-summary`) green in `rac-benchmarks`
  CI, each with a committed baseline, calibrated floors, and regression
  proofs.
- A shared harness package the benchmarks consume as thin entry points,
  driving `rac` strictly as an external CLI on `PATH` (DG-ADR-0001), with
  the `rac eval` flag surface and exit-code contract.
- `decisiongrounding` ported onto the shared harness without changing its
  scored results, once its restructure move has settled.

## Initiatives

- **Shared harness** — subprocess runner, deterministic scorer (P@k, R@k,
  MRR, full-list negatives, conformance), scorecard writer, and gate in
  `rac-benchmarks/harness/`.
- **find-decisions benchmark** — the supersession defense under test:
  retired decisions must never surface, even as the lexically best match.
- **search-artifacts benchmark** — ranked retrieval at realistic scale:
  thirty artifacts across all five types, six query categories, MRR gated.
- **get-artifact benchmark** — resolution conformance: exact-id, alias, and
  case-insensitive hits; duplicate and not-found error shapes.
- **get-related benchmark** — exact incoming AND outgoing edge sets per
  artifact (the outgoing direction was previously unguarded).
- **get-summary benchmark** — portfolio contract: counts by type, the
  empty-corpus shape, byte stability.
- **decisiongrounding backport** — port the moved benchmark onto the shared
  harness with byte-equal scored results as the acceptance bar. Recorded
  finding: `decisiongrounding` is a scenario/provider benchmark with a
  structural adherence scorer and has no `{metrics, baseline}` scorecard
  today, so the port first needs an equivalence surface defined; it must not
  expand the frozen restructure item's scope.
- **MCP-stdio harness (deferred)** — the workstream that closes what the
  CLI cannot see: `get_related` depth-greater-than-one neighborhoods, the
  ADR-033 response budget and truncation markers, and provenance enrichment
  on `get_artifact` payloads.

## Success Measures

- `rac-benchmarks` CI runs every benchmark's `--check` on every push and
  pull request; a deliberately injected retrieval regression fails it.
- Each benchmark's serialized `metrics` block is byte-identical across
  repeated runs on an unchanged corpus (asserted by tests).
- Three regression-injection proofs per benchmark assert exit codes and the
  named fired rules.
- No benchmark or harness file imports engine code (asserted by a test).

## Assumptions

- The `rac` CLI's JSON contracts (`find`, `resolve`, `relationships`,
  `portfolio`, `index`) stay additive per ADR-007.
- The decisiongrounding restructure item (repo-topology) remains frozen in
  scope; this roadmap's backport initiative is where port work is tracked.

## Risks

- **Fixture vocabulary drift**: the full-list negative window means a query
  token leaking into a hard-negative artifact fails the gate for lexical
  rather than structural reasons. Mitigation: corpus edits re-run their
  benchmark before commit; the benchmark-local ADRs record the partitioning
  rule.
- **Floating rac version in CI**: benchmarks install rac from source, so an
  engine ranking change can fail the gate before the baseline is reviewed.
  Mitigation: that is the point — the gate exists to make such changes
  reviewed, and re-baselining stays human-gated.

## Related Decisions

- adr-097
- adr-066
- adr-092
- adr-093
- adr-007

## Related Roadmaps

- rac-benchmarks

## Related Tickets

- itsthelore/asdecided-benchmarks#2
- itsthelore/asdecided-benchmarks#3
- itsthelore/asdecided-benchmarks#4
- itsthelore/asdecided-benchmarks#5
- itsthelore/asdecided-benchmarks#6
- itsthelore/asdecided-benchmarks#7
- itsthelore/asdecided-benchmarks#8
- itsthelore/asdecided-benchmarks#9
