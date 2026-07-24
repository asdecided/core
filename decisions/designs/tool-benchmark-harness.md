---
schema_version: 1
id: RAC-KWFVACN74BHW
type: design
---
# Tool Benchmark Harness and Scorecard

## Context

The `tool-benchmarks` roadmap builds one benchmark per Lore MCP tool in
`itsthelore/asdecided-benchmarks` (subdir per benchmark, ADR-092). The non-trivial
structure worth recording — sibling to `grounding-eval-scorecard` — is the
shared harness those subdirs consume and the scorecard/gate shapes it emits,
so five benchmarks stay one design rather than five drifting ones. Scoring
posture is fixed by ADR-097 (extending ADR-066); this design pins the data
shapes and seams.

## User Need

The maintainer and CI need one answer per tool, deterministically and
offline: "does this tool still honour its retrieval or resolution contract,
and did this change make it worse?" — with a human-readable summary, a
machine-readable scorecard to diff against a committed baseline, and an exit
code CI can gate on.

## Design

### Tool-to-CLI seam

The harness drives `rac` strictly as an external CLI on `PATH` (the
benchmark repository's DG-ADR-0001 — zero engine imports), so the scored
surface is exactly the published CLI contract:

- `search_artifacts` → `rac find <query> <root> --json [--type T]`
- `find_decisions` → `rac find <query> <root> --decisions --json`
- `get_artifact` → `rac resolve <id> <root> --json`
- `get_related` → `rac relationships <root> --json`, with
  `rac resolve` / `rac index` as id↔path plumbing that never re-orders tool
  output
- `get_summary` → `rac portfolio <root> --json`

Production output is consumed verbatim: no re-sort, no re-rank, membership
compared by id only.

### Benchmark kinds

The benchmark's committed `config.json` names its tool, which picks the case
shape and metrics:

- **Retrieval** (`search_artifacts`, `find_decisions`): ranked cases with
  `relevant` (≥ 1) and optional `must_not_return`. Metrics: P@k / R@k at
  `k ∈ {1, 3, 5}` and MRR, macro-averaged, reported `overall` and
  `by_category`; `negative_violations` counts `must_not_return` ids anywhere
  in the FULL returned list.
- **Conformance** (`get_artifact`, `get_related`, `get_summary`): contract
  cases scored as named deterministic checks. Metrics: `conformance` pass
  rate (gated 1.0, zero tolerance), `cases_passed`, `cases_total`, and
  `negative_violations` for edge-set negatives.

### Scorecard shape

Three top-level objects; only `metrics` is gate-compared, `metadata` and
`per_query` are diagnostic, so a clock or hash never fails a build. Floats
round to six decimals so the serialized `metrics` block is byte-identical
across runs on an unchanged corpus. `metadata` records the `rac --version`
string, `corpus_hash` (`sha256:…` over the fixture files), `query_set_hash`,
`n_queries`, and `generated_at`. The scorecard JSON is an additive, stable
contract (ADR-007).

### Entry point and gate

Every subdir ships a thin `run.py` over the harness with the `rac eval` flag
surface: `--check | --update-baseline`, `--json`, `--root`, `--queries`,
`--baseline`, `--config`; exit 0 clean, 1 gate failure, 2 usage error. The
gate fails when `negative_violations` exceeds its limit, a gated metric is
below its configured floor, or a gated metric is below
`baseline − tolerance`; it prints one line per fired rule with the rule
name, metric, and both values. Gated metrics are whatever the config's
floors declare, so retrieval and conformance benchmarks share one gate
implementation. Re-baselining is human-gated; CI never passes
`--update-baseline` (asserted by a test).

## Constraints

- Deterministic and offline (ADR-066/ADR-097): no network, keys, randomness,
  or clock in the scored path.
- No engine imports anywhere in the benchmark repository (DG-ADR-0001),
  enforced by a test that scans for `import rac` / `from rac`.
- Per-category floors only where a category has at least four cases: with
  two cases the metrics quantize to {0, 0.5, 1.0} and a 0.9 floor is
  theater.
- Fixture corpora are schema-valid (`rac validate` exit 0) with a distinct
  id prefix per benchmark (`SAB-`, `FDB-`, `GAB-`, `GRB-`, `GSB-`).

## Rationale

One shared harness with config-declared gated metrics keeps five benchmarks
byte-compatible in scorecard shape while letting retrieval and contract
tools score in their own terms. Mirroring the `rac eval` flag surface and
exit codes means CI treats every benchmark identically, and mirroring the
grounding-eval-scorecard split (gated `metrics` vs diagnostic `metadata` /
`per_query`) keeps the gate byte-stable without hiding run context.

## Alternatives

- One benchmark with a `tool` field per case (the `rac eval` shape):
  rejected — ADR-092's family form wants one subdir per benchmark, and
  per-tool corpora need deliberately partitioned vocabulary.
- Importing rac-core's `eval` service instead of a subprocess seam:
  rejected — DG-ADR-0001 makes engine decoupling the repository's contract,
  and the CLI JSON is the surface agents actually consume.
- Scoring conformance tools with ranked metrics: rejected in ADR-097.

## Accessibility

The human summary prints plain-text tables (overall, by-category) and an
explicit Violations section listing each offending case with its returned
ids or failed checks — legible in a terminal and in CI logs without colour.

## Style Guidance

Failure output names the rule, metric, and both values on one line each,
matching `rac eval --check`. JSON output follows the scorecard shape
exactly; `--update-baseline` writes the `metrics` object alone.

## Open Questions

- When the MCP-stdio harness workstream lands, whether budget-truncated
  responses are scored as a separate kind or as a variant seam on the
  retrieval kind.
- Whether the decisiongrounding backport defines its equivalence surface as
  a new `metrics` projection of its existing run results or by freezing its
  current report shape.

## Related Decisions

- adr-097
- adr-066
- adr-092
- adr-007

## Related Roadmaps

- tool-benchmarks
