---
schema_version: 1
id: RAC-KXBH6X2T4Q8R
type: requirement
---
# Requirement: Sentry Eligibility Audit

## Status

Accepted

## Problem

Sentry can enforce deterministic decision-to-code rules, but most historical
Decisions predate the eligibility contract. Treating every unclassified
Decision as unenforced produces an honest adoption number but does not show
which decisions are genuinely machine-checkable. Inferring eligibility in bulk
would create false precision and weak rules.

## Requirements

- [REQ-001] Eligibility MUST be audited in bounded, reviewable tranches rather than one corpus-wide mechanical rewrite.
- [REQ-002] Each classified Decision MUST state `eligibility: eligible` or `eligibility: ineligible`; an ineligible classification MUST carry a concrete reason.
- [REQ-003] Eligible Decisions SHOULD land with at least one deterministic rule when a stable source anchor exists; an empty eligible rule set MUST remain visible as a coverage gap.
- [REQ-004] Rules MUST enforce concrete repository properties, not paraphrase prose or approximate semantic intent.
- [REQ-005] A partially enforceable Decision MAY be eligible when its enforceable boundary and externally verified remainder are stated honestly.
- [REQ-006] Every tranche MUST pass full-tree Sentry, diff Sentry, corpus validation, relationship validation, and the native Rust test suite.
- [REQ-007] Reports MUST retain corpus adoption, eligible coverage, active-rule count, and unclassified count as separate measurements.

## Success Metrics

- Each tranche reduces the unclassified count without weakening validation.
- Every added rule passes full-tree certification before merge.
- Eligible coverage does not increase through inferred or empty classifications.
- Reviewers can trace each rule to one accepted Decision and one concrete source
  boundary.

## Risks

- A rule can appear precise while enforcing only a superficial token.
  Mitigation: narrow path globs, explicit messages, and full-tree tests.
- Historical Decisions may describe retired implementation states.
  Mitigation: exclude ambiguous cases from a tranche and resolve lifecycle
  truth before classification.
- Chasing a high percentage can reward low-value rules. Mitigation: prioritize
  security, determinism, compatibility, and merge-protection boundaries.

## Assumptions

- Unclassified is a valid temporary state.
- Human review remains authoritative for semantic and externally configured
  policy.
- The first tranche focuses on recent, high-confidence architectural Decisions.

## Related Decisions

- adr-066
- adr-123

## Related Designs

- sentry-eligibility-classification

## Verified By

- rust/rac-engine/src/sentry.rs
