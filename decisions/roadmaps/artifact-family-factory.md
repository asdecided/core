---
schema_version: 1
id: RAC-KVV611BA0CB0
type: roadmap
---
# Artifact Family Factory

## Status

Planned

Prioritised as the rank-7 Tranche C item of the deterministic-substrate
programme, graduated out of `future/` as the programme's ordered
extensibility step — deliberately ahead of third-party artifact types
(ADR-083), whose entry-point work builds on the contract proven here. This
is the mechanism for growing RAC's *artifact* footprint on-thesis: more
typed, deterministic, human-ratified families, never more stored content
(ADR-024) and never work-tracking (ADR-017). It is scoped by proving the
mechanism with one pilot family, not by adding an open-ended set at once.
Instantiating a family — including the Risk pilot — lands its own ADR at
implementation, scheduled here, not pre-drafted. Execution is tracked in
GitHub (ADR-093): the epic in `## Related Tickets` carries ordering and
task state.

## Context

RAC's families have grown deliberately — Requirements, Decisions, Roadmaps,
Prompts, Designs — and ADR-010 names the next candidates directly: a PRD "may
contain Requirements, Decisions, Risks, Success metrics, Roadmap information",
i.e. Risk and Metric are knowledge types already understood to be extractable but
not yet modelled. The engine is built for this: classification is separate from
validation, structural validation is shared across per-type validators (ADR-060)
over a single parser instance (ADR-059), and a template is the creation contract
for each type (ADR-021). Adding a family should therefore be a *repeatable
contract*, not a bespoke effort each time.

The footprint argument is the inverse of becoming a content store. RAC grows by
being the narrow, typed, ratified layer in *more knowledge dimensions* — Risk,
Metric, Glossary — each one deterministic and diffable, each one improving agent
grounding. That strengthens the moat (ADR-024, ADR-017) instead of diluting it
into a general-purpose store.

A "factory" must stay scoped. This item establishes the family-creation contract
and proves it by landing exactly one pilot family end to end (model → schema →
template → classifier → validator → CLI → negative boundary tests → docs). Each
subsequent family is then its own scoped roadmap item that instantiates the same
contract — the factory makes them cheap and consistent; it does not pre-commit
to all of them.

## Outcomes

- Adding a new artifact family is a **repeatable, documented contract**: a fixed
  sequence (model, schema, `rac new` template per ADR-021, deterministic
  classifier, structural validator reusing the shared core per ADR-060, CLI
  surface, tests, docs) that any new family follows identically.
- **One pilot family ships end to end** to prove the contract — Risk is the
  recommended pilot (named in ADR-010, pure knowledge, pairs with Decisions),
  with Metric/OKR and Glossary/Term as the next named candidates, each its own
  future scoped item.
- Each new family is **deterministic and ratified, never work or content**: it
  carries knowledge and a lifecycle status (ADR-061-style terminal states where
  meaningful), but no ownership, assignment, scheduling, or workflow (ADR-017),
  and stores no external content (ADR-024).
- The family-creation contract enforces RAC's standing test rules: **negative
  boundary tests** for the new type and **adjacent-type non-misclassification**
  (the new type and existing types do not classify as each other).

## Initiatives

### Initiative 1 — The family-creation contract

Document the canonical sequence for adding a family and the artifacts each step
produces: the type's data model, its schema, its `rac new` template (the creation
contract, ADR-021), its deterministic classifier rule, its validator built on the
shared structural core (ADR-060) over the single parser (ADR-059), its CLI
exposure, and its required tests and docs. Classification stays separate from
validation, so a recognisable-but-invalid instance still classifies as the type
and then fails validation.

### Initiative 2 — Pilot family: Risk (end to end)

Instantiate the contract once, fully, with Risk: model the fields a risk needs
(statement, likelihood/impact as descriptive knowledge — not a work score),
classify it deterministically, validate it structurally, expose it through the
CLI, and link it to the Decisions and Requirements it bears on. Risk must read as
recorded knowledge, not a risk *register* with work semantics (ADR-017).

### Initiative 3 — Boundary and non-misclassification coverage

Ship the negative tests the session-start rules require: malformed-but-recognised
Risk instances fail validation as Risk (not silently reclassified), and Risk does
not misclassify as Decision/Requirement/Design nor they as Risk. This coverage is
part of the contract every future family repeats.

## Constraints

- New families are knowledge artifacts only — no ownership, assignment,
  prioritisation, workflow state, scheduling, or delivery tracking (ADR-017).
- New families store no external content and treat their Markdown as externally
  owned source files (ADR-024, ADR-010: documents are containers, artifacts are
  extracted knowledge types).
- Each family is schema/spec-driven: behaviour comes from the type's schema,
  template, and shared structural validation (ADR-021, ADR-059, ADR-060), not
  artifact-specific branches.
- Knowledge is ratified, not synthesised: a family instance enters the trusted
  corpus by human PR review (ADR-065), not by automated inference.

## Non-Goals

- Adding many families at once. The factory proves itself on one pilot; each
  further family is a separate scoped roadmap item.
- Supporting more *content/data formats* or becoming a store (that would
  supersede ADR-024 and is explicitly out of scope here).
- Any work-management concept entering through a new family (ADR-017).
- Embeddings, scoring, or LLM-judged classification; families classify
  deterministically (ADR-066).

## Success Measures

- A contributor can add a new family by following the documented contract alone,
  producing model, schema, template, classifier, validator, CLI surface, tests,
  and docs in the same shape as the pilot.
- The Risk pilot passes `rac validate` and `rac relationships --validate`, and its
  negative boundary and adjacent-type tests pass.
- No new family introduces a work-management field or a content-storage surface;
  review rejects any that does.
- Each family's behaviour derives from its schema and the shared validator, with
  no new artifact-specific branching in the engine.

## Assumptions

- ADR-010, ADR-017, and ADR-024 remain the governing boundary: more knowledge
  *types* are in scope; more stored *content* and any work-tracking are not.
- The shared structural validator (ADR-060) and single parser (ADR-059) remain
  the basis for per-type validation, so a new family reuses them.
- `rac new <type>` remains the template-driven creation path (ADR-021).
- Adoption signal and corpus need justify which families graduate out of
  `future/` and in what order.

## Risks

- **Family sprawl / scope creep.** An unbounded "add every type" reading dilutes
  focus. Mitigation: the factory ships one pilot; each further family is its own
  scoped item gated on need.
- **Work-management leakage.** A Risk or Metric family quietly grows ownership or
  status-workflow fields. Mitigation: the constraint above and review against
  ADR-017; families carry knowledge and lifecycle, never assignment or scheduling.
- **Content-store drift.** A family becomes a place to park documents. Mitigation:
  ADR-010/ADR-024 framing — the artifact is extracted knowledge, the document is
  an externally owned container.

## Related Decisions

- ADR-004
- ADR-010
- ADR-017
- ADR-021
- ADR-024
- ADR-059
- ADR-060
- ADR-061
- ADR-065

## Related Roadmaps

- deterministic-substrate
- growth-programme

## Related Requirements

- rac-family-creation-contract
- rac-risk-pilot-family

## Related Tickets

- itsthelore/asdecided-core#248
