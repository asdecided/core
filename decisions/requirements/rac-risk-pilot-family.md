---
schema_version: 1
id: RAC-KWK9FFGBTGVD
type: requirement
---
# Requirement: Risk Pilot Family

## Status

Accepted

Classification: `[external]` — a new user-facing artifact type. Initiatives
2 and 3 of the `artifact-family-factory` roadmap: the single end-to-end
instantiation that proves the family-creation contract.

## Problem

The family-creation contract is only trustworthy once it has produced a
family end to end. ADR-010 names Risk as an extractable knowledge type a
PRD already contains, and no current family models it — so risk statements
either live untyped in documents or get shoehorned into Requirements. Risk
is the recommended pilot: pure knowledge, pairs naturally with Decisions,
and small enough to prove every step of the contract without an open-ended
modelling effort.

## Requirements

- [REQ-001] A Risk artifact family MUST ship end to end by instantiating the family-creation contract: data model, schema, `rac new risk` template (ADR-021), deterministic classifier, structural validator on the shared core (ADR-060, ADR-059), CLI exposure, tests, and docs — every step, no shortcuts.
- [REQ-002] The Risk model MUST carry knowledge only: a risk statement with likelihood and impact as descriptive knowledge fields plus a lifecycle status — never ownership, assignment, mitigation-task tracking, or workflow state (ADR-017); Risk is recorded knowledge, not a risk register.
- [REQ-003] Risk artifacts MUST participate in the relationship graph: linkable to the Decisions and Requirements they bear on, validated by `rac relationships --validate` like every other family.
- [REQ-004] Classification MUST be deterministic and separate from validation: a malformed-but-recognisable Risk classifies as Risk and fails validation as Risk, never silently reclassifies (ADR-066 posture, no model in the loop).
- [REQ-005] The pilot MUST ship the contract's full boundary coverage: negative boundary tests for Risk, and adjacent-type non-misclassification both ways (Risk does not classify as Decision/Requirement/Design, nor they as Risk).
- [REQ-006] The pilot MUST land with its own ADR recording the Risk family's model and boundary (scheduled by the contract's REQ-006), ratified by human review (ADR-065).
- [REQ-007] The addition MUST be additive (ADR-007): existing families' classification, validation, JSON contracts, and exit codes are unchanged; a corpus without Risk artifacts behaves byte-identically to before.

## Acceptance Criteria

- `rac new risk` produces a template-valid artifact that classifies as Risk
  and passes `rac validate`.
- A Risk artifact linking a Decision and a Requirement passes
  `rac relationships --validate`, and the links appear in the graph export.
- Malformed-but-recognisable Risk fixtures classify as Risk and fail
  validation with Risk-specific findings.
- The adjacent-type suite passes in both directions across
  Risk/Decision/Requirement/Design fixtures.
- The full pre-existing test suite passes unchanged on a corpus with no
  Risk artifacts.
- Review confirms no work-management or content-storage field in the model.

## Success Metrics

- The family-creation contract is proven: the pilot's shape is the
  documented reference every subsequent family (Metric, Glossary — each its
  own future item) instantiates.

## Risks

- Risk quietly grows register semantics (owners, mitigation tasks, review
  cadences). Mitigation: REQ-002 and the contract's ADR-017 constraint;
  review rejects work fields.
- Likelihood/impact read as scores inviting prioritisation workflows.
  Mitigation: they are descriptive knowledge fields on the record, never
  inputs to any engine ranking or workflow.

## Assumptions

- Risk as named by ADR-010 is small enough to model in one scoped item, and
  adoption need justifies it as the pilot over Metric or Glossary.
- The engine's classifier and shared validator seams accommodate a sixth
  built-in family without artifact-specific branching (ADR-060's premise).

## Related Decisions

- ADR-007
- ADR-010
- ADR-017
- ADR-021
- ADR-059
- ADR-060
- ADR-065

## Related Roadmaps

- artifact-family-factory

## Related Requirements

- rac-family-creation-contract
