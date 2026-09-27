---
schema_version: 1
id: RAC-KWK9FE07VFXE
type: requirement
---
# Requirement: Artifact Family Creation Contract

## Status

Accepted

Classification: `[internal]` — a documented, repeatable engineering
contract. Initiative 1 of the `artifact-family-factory` roadmap.

## Problem

RAC's artifact families have grown deliberately, but each addition has been
a bespoke effort: there is no documented sequence a contributor can follow
to add a family, so consistency depends on reading prior implementations.
The engine is already built for a contract — classification separate from
validation, shared structural validation (ADR-060) over a single parser
(ADR-059), templates as creation contracts (ADR-021) — but the contract
itself is unwritten, and without it every new family risks drifting in
shape or quietly acquiring work-management or content-store semantics.

## Requirements

- [REQ-001] The family-creation contract MUST document the canonical sequence and the artifact each step produces: the type's data model, its schema, its `rac new` template (ADR-021), its deterministic classifier rule, its validator built on the shared structural core (ADR-060) over the single parser instance (ADR-059), its CLI exposure, and its required tests and docs.
- [REQ-002] The contract MUST keep classification separate from validation: a recognisable-but-invalid instance still classifies as the type and then fails validation, never silently reclassifies.
- [REQ-003] The contract MUST require, for every new family, negative boundary tests and adjacent-type non-misclassification tests (the new type and existing types do not classify as each other) — the session-start test rules made a contract precondition.
- [REQ-004] The contract MUST forbid work-management semantics (ownership, assignment, prioritisation, workflow state, scheduling — ADR-017) and content-storage semantics (ADR-024, ADR-010) in any new family: families carry knowledge and a lifecycle status only.
- [REQ-005] Each family's behaviour MUST derive from its schema, template, and the shared structural validator — no artifact-specific branching added to the engine per family.
- [REQ-006] Instantiating the contract for a family — including the Risk pilot — MUST land its own ADR at implementation, recording the family's model and boundary; the ADR is scheduled by this contract, not pre-drafted.

## Acceptance Criteria

- A contributor produces a new family by following the documented contract
  alone — model, schema, template, classifier, validator, CLI surface,
  tests, docs — in the same shape as the pilot, without reading another
  family's implementation history.
- A malformed-but-recognisable instance of a contract-built family
  classifies as its type and fails validation with type-specific findings.
- The contract's test checklist includes the negative boundary and
  adjacent-type non-misclassification suites, and the pilot ships them.
- Review rejects a proposed family carrying an ownership, scheduling,
  workflow, or content-storage field, citing the contract.

## Success Metrics

- Adding an artifact family is a repeatable, documented contract rather
  than a bespoke effort — the factory that makes future families cheap and
  consistent without pre-committing to any of them.

## Risks

- The contract is written once and drifts from the engine's actual seams.
  Mitigation: the Risk pilot (`rac-risk-pilot-family`) proves it end to end
  immediately, and each future family re-walks it.
- A documented contract reads as an open invitation to add families.
  Mitigation: each family remains its own scoped roadmap item gated on
  need; the factory does not pre-commit the set.

## Assumptions

- ADR-010, ADR-017, and ADR-024 remain the governing boundary: more
  knowledge types are in scope; stored content and work-tracking are not.
- The shared validator and single parser remain the basis for per-type
  validation, so the contract's steps stay stable.

## Related Decisions

- ADR-010
- ADR-017
- ADR-021
- ADR-024
- ADR-059
- ADR-060
- ADR-065

## Related Roadmaps

- artifact-family-factory

## Related Requirements

- rac-risk-pilot-family
