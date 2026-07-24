---
schema_version: 1
id: RAC-KWGMTGNEJ0NN
type: roadmap
---
# TypeScript SDK Stable Release

## Status

Planned

Unscheduled member of the `agnostic-surfaces` programme, Track 2
(contract hardening), sequenced after the conformance fixture suite is
green. The council recorded "drive the TS SDK to a stable 1.0 so any
second SDK copies a proven surface" as part of the rank-1 workstream;
implementation lands in the `rac-sdk` repository (ADR-092), intent is
recorded here.

## Outcomes

- `@itsthelore/asdecided-sdk` publishes 1.0: a stated stable surface with a
  semver policy, so consumers (the VS Code extension first) can depend
  on it without tracking pre-1.0 churn.
- The documented API and the actual client surface are the same thing:
  every public client method appears in the README's API documentation.
- The SDK's CI proves the client against a real engine on every change,
  via the conformance suite, instead of skipping integration entirely.
- Any future language SDK starts by copying a proven, conformance-tested
  surface — the standing precondition the council's reordering triggers
  name ("TS SDK 1.0 + conformance suite operational").

## Initiatives

- Close the documentation drift: the README API table documents nine of
  the eighteen public client methods; document the missing nine
  (text/stdin validation, live-decision query, schema, rename, artifact
  creation, init, quickstart, HTML export, agent-rules) with the same
  usage-example quality as the existing entries.
- Wire the conformance suite into the SDK's CI as a required job
  against a real installed engine (delivered by the
  `conformance-fixtures` item; this item consumes it).
- State the stability policy for 1.0: what the public surface is (the
  package's exported names — the analogue of ADR-062's `rac.__all__`
  rule), semver semantics for the typed results under the engine's
  additive JSON contract (ADR-007), and the supported engine version
  range with the version-check behaviour on mismatch.
- Sweep the pre-1.0 surface once before freezing: naming consistency,
  error-hierarchy completeness, and removal of anything not worth
  supporting forever; then publish 1.0 through the existing trusted
  publishing release workflow.
- Fix packaging metadata leftovers from the repo move (the repository
  URL still points at the pre-consolidation repo slug).

## Constraints

- Thin client always (ADR-063): no engine logic enters the SDK on the
  way to 1.0; the surface wraps `rac … --json`, export payloads, and
  exit codes only.
- 1.0 follows the conformance suite, never precedes it — a stable label
  on an unproven surface is the failure mode this sequencing exists to
  prevent.
- The npm package name and repo home are settled (ADR-092,
  `rac-sdk/ts/`); this item changes neither.
- Corpus artifacts and gates live in the engine repo; the `rac-sdk`
  repo carries the implementation and its own CI.

## Success Measures

- README API documentation covers one hundred percent of the exported
  client surface, checked as part of the release sweep.
- The conformance job is required and green in the SDK's CI at the 1.0
  release commit.
- A published stability policy tells a consumer exactly what semver
  means for the typed surface and which engine versions are supported.
- The VS Code extension consumes 1.0 without code changes beyond the
  version bump.

## Assumptions

- The current eighteen-method surface is approximately the right 1.0
  surface; the pre-freeze sweep trims or renames at the margin rather
  than redesigning.
- The engine's `--json` contract remains additive (ADR-007), so 1.0 can
  promise forward compatibility with newer engines within a stated
  range.

## Risks

- Freezing the surface before the VS Code extension's needs stabilise
  forces an early 2.0; mitigated by the extension being the first
  consumer reviewed in the pre-freeze sweep.
- Engine and SDK version skew confuses users post-1.0; mitigated by the
  stated supported-range policy and the client's existing version
  check surfacing mismatches explicitly.

## Related Decisions

- ADR-007
- ADR-062
- ADR-063
- ADR-092

## Related Roadmaps

- agnostic-surfaces
- conformance-fixtures
