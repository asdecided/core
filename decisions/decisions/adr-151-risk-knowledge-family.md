---
schema_version: 1
id: RAC-M3C0K38CZYQ1
type: decision
tags: [artifact-family, risk, schema, spec]
---
# ADR-151: Risk Is a Built-In Knowledge Family

## Status

Accepted

**Accepted 2026-09-25.** Ratified by the maintainer with two changes to the
draft: `Mitigation` is optional rather than recommended, and `prompt`
declares `related_risks`. The specification change (spec 0.2) and the engine
change follow under the `artifact-family-factory` roadmap.

Drafted when the `artifact-family-factory` roadmap was taken into build, as
its family-creation contract requires (`rac-family-creation-contract`
REQ-006): instantiating a family lands its own ADR recording the family's
model and boundary, ratified by human review (ADR-065).

## Category

Architecture

## Context

ADR-010 names Risk as knowledge a PRD already contains, and no family models
it: risk statements live untyped in documents or as the free-text
`## Risks` section of a Requirement or Roadmap, which nothing can link to.
The `artifact-family-factory` roadmap proves the family-creation contract on
one pilot, and names Risk (`rac-risk-pilot-family`).

Two facts decide the shape of the change:

- **Risk must be a built-in, not a spec-bundle type.** The pilot requires
  Risk artifacts to be relationship targets that Decisions and Requirements
  link to (`rac-risk-pilot-family` REQ-003). ADR-083 decision 4 fixes that
  bundle types are never relationship targets and add no edge kinds, so a
  bundle cannot carry Risk; only the built-in registry can.
- **The built-in type set and relationship vocabulary are specification
  enums.** SPEC §6.1 declares the built-in type set closed and §8.2 the
  relationship vocabulary; §10.2 and §11 make adding an artifact type or a
  relationship type a **minor** version of the specification. The registry
  both engines read is `asdecided/spec`'s `schema/artifact-specs.json`
  (ADR-115), vendored byte-identically. Risk therefore lands in the
  specification first, then in the engine.

The standing boundaries apply unchanged: a family carries knowledge and a
lifecycle status, never ownership, assignment, prioritisation, scheduling, or
workflow (ADR-017), and stores no external content (ADR-010, ADR-024).

## Decision

1. **Add `risk` as the sixth built-in artifact type**, registered after
   `design` in the shared registry, in a minor version of the specification
   (spec 0.1 → 0.2). A corpus without Risk artifacts is byte-identical in
   every output (ADR-007).

2. **The Risk model is a recorded judgement, not a register entry.**
   - Required sections: `Risk` (the risk statement), `Likelihood`, `Impact`.
   - Recommended: `Context`, `Assumptions`.
   - Optional: `Mitigation`, and the relationship sections of decision 4.
     `Mitigation` is optional rather than recommended because it is the
     section most likely to attract task lists; a Risk is complete without
     one.
   - `Likelihood` and `Impact` are descriptive prose — how likely and how
     severe, in the author's words — not an enum and not a score. Nothing in
     the engine ranks, sorts, or prioritises by them.
   - `Mitigation` records the chosen response as knowledge (what reduces
     the risk and why), never tasks, owners, or dates.

3. **Status reuses the knowledge lifecycle.** Live: `Proposed`, `Accepted`;
   retired: `Superseded`, `Deprecated` — the same enum as Requirement,
   Decision, and Design. No `Mitigated`, `Open`, or `Closed`: those are
   delivery states (SPEC §7: status is knowledge currency, never delivery
   tracking). A risk that no longer applies is `Deprecated`; one restated by
   a newer artifact is `Superseded`.

4. **One new relationship edge, `related_risks`,** target `risk`,
   undirected, validated like every range-checked edge (resolve + range +
   status). It is declared by `requirement`, `decision`, `roadmap`,
   `design`, and `prompt`; a Risk declares `related_requirements`,
   `related_decisions`, `related_roadmaps`, and `related_designs`. A prompt
   can therefore cite the risks it guards against; a Risk does not declare
   `related_prompts`, keeping the new edge surface to what was ratified.

5. **The free-text `## Risks` sections stay.** Requirement's and Roadmap's
   recommended `Risks` sections remain prose (`risks` ≠ `risk`); a Risk
   artifact is the typed, linkable form when a risk needs its own record.
   Nothing converts one into the other.

6. **Exports and serving name the type additively.** OKF exports Risk as
   `type: Risk` with its own `index.md` section after the five fixed ones
   (ADR-122); the `stats --json` family key is `risk_artifacts`, because the
   top-level `risks` key already counts Requirement risk lines and cannot
   change meaning (ADR-007); MCP tools accept `risk` wherever they accept an
   artifact type.

7. **Behaviour comes from the registry, not a branch.** Risk validates
   through the shared structural core (ADR-060) and the status check every
   type uses; the engine gains no Risk-specific validation logic
   (`rac-family-creation-contract` REQ-005).

## Consequences

Risks become first-class, linkable knowledge: a Decision can cite the risks
it accepts, and a Requirement the risks it guards against, with referential
integrity. The cost is a specification minor version and a coordinated
engine release; consumers pinned to spec 0.1 see an unknown type and, per
SPEC §10.3, must refuse a corpus that declares 0.2.

The model stays deliberately thin. There is no severity matrix, owner, or
review date, which some teams expect from a risk register; they keep that
register in their work tracker and link to it through `## Related Tickets`.

## Alternatives Considered

- **Ship Risk as a spec-bundle type.** Rejected: bundle types cannot be
  relationship targets (ADR-083 decision 4), which the pilot requires.
- **Enumerated likelihood and impact (Low/Medium/High).** Rejected: an enum
  invites scoring and prioritisation — register semantics ADR-017 excludes;
  prose records the judgement without implying a rank.
- **Risk-specific statuses (`Open`, `Mitigated`, `Closed`).** Rejected: they
  track delivery, not knowledge currency (SPEC §7).
- **Reuse the `risks` stats key.** Rejected: it would change the meaning of
  an existing JSON field (ADR-007).

## Related Decisions

- adr-007
- adr-010
- adr-017
- adr-021
- adr-024
- adr-059
- adr-060
- adr-065
- adr-083
- adr-115
- adr-122

## Related Requirements

- rac-family-creation-contract
- rac-risk-pilot-family

## Related Roadmaps

- artifact-family-factory
