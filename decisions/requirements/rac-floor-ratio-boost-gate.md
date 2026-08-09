---
schema_version: 1
id: RAC-KWK9FCJ53BBC
type: requirement
---
# Requirement: Floor-Ratio Bounded-Boost Gate

## Status

Accepted

Classification: `[internal]` — a tested guarantee over the shipped ranking
boost. Initiative 2 of the `retrieval-diagnostics` roadmap.

## Problem

ADR-078's deterministic relevance score includes a bounded graph boost, and
"bounded" is currently a property of the formula's coefficients rather than
a stated, tested invariant. Without an explicit gate, a future retuning of
the boost could let a boosted weak candidate float above a strong lexical
match — the exact failure hybrid systems guard against with a floor-ratio
gate — and nothing would fail until an agent noticed worse grounding.

## Requirements

- [REQ-001] The ranking boost MUST enforce a floor-ratio gate: a boosted candidate's final score cannot exceed a primary lexical match beyond a stated, bounded ratio, so a boosted weak result can never outrank a strong lexical match past that bound.
- [REQ-002] The gate MUST be deterministic and offline (ADR-002, ADR-066): a pure function of the lexical scores and the boost, with no embeddings and no model.
- [REQ-003] The gate MUST only re-order within already-matched results (the roadmap's non-goal made testable): which artifacts match is unchanged; only ordering among matches is constrained.
- [REQ-004] The guarantee MUST be pinned by golden tests: fixtures demonstrate a boosted weak candidate staying below a strong lexical match, and the bound holds across the ranking fixture suite.
- [REQ-005] The gate MUST integrate with the shipped ADR-078 score as a refinement of its bounded-boost clause; any ranking-order change it introduces is confined to boost-dominated cases and re-pinned deliberately in the goldens under review, never as silent churn.
- [REQ-006] The gate's ratio MUST be a stated constant (or documented configuration with a stated default), visible in the ranking explanation output so an explained score shows when the gate clamped a boost.

## Acceptance Criteria

- A fixture where an artifact's graph boost would exceed the gate shows the
  clamped score, with the stronger lexical match ranked first.
- The full ranking golden suite passes, with any re-pins confined to
  fixtures constructed to exercise the gate.
- Match sets are identical before and after: the gate changes ordering
  bounds only, never membership.
- The explain output for a clamped result names the gate and the ratio.

## Success Metrics

- "The boost cannot float a weak result above a strong match" is a tested
  guarantee proven by goldens, not prose in the ranking design.

## Risks

- The gate interacts with RRF fusion in unanticipated ways and re-orders
  more than boost-dominated cases. Mitigation: REQ-005 confines and reviews
  any re-pin; the fixture suite is the tripwire.
- A configurable ratio invites tuning drift. Mitigation: REQ-006 requires a
  stated default and surfaces the clamp in explanations.

## Assumptions

- The bounded-boost guarantee is more valuable expressed as a tested gate
  than as prose — the roadmap's recorded premise.
- The shipped ADR-078 score exposes the lexical and boost components
  separately enough for a ratio gate to apply cleanly.

## Related Decisions

- adr-002
- adr-007
- adr-037
- adr-038
- adr-066
- adr-078

## Related Roadmaps

- retrieval-diagnostics
- relevance-ranking

## Related Requirements

- rac-explain-miss-diagnostics
- rac-explainable-retrieval

## Verification

- Core unit tests pin the inclusive 0.85 boundary and a real ranking in which a
  weak lexical hub is clamped.
- `asdecided/benchmarks` extends `search-artifacts` with four adversarial
  black-box cases and extends DecisionGrounding with two downstream adherence
  scenarios.
