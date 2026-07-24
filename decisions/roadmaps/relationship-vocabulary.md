---
schema_version: 1
id: RAC-KW7VK36J3EXT
type: roadmap
---
# Relationship Vocabulary — Closing the Traceability Gaps

## Status

Planned

## Outcomes

Make the relationship graph express what the corpus already means, so a declared
`## Related …`-style link becomes a validated edge instead of silently producing
none. This is the resolution programme for the growth-programme traceability gap
audit (`.agent-context/GAPS_TRACEABILITY.md`); each gap is recorded as its own
requirement, designable independently.

- A reference to a non-artifact target — a repository file or an external URL —
  is expressible and existence/format-checked, not invisible prose.
- A roadmap can record what superseded it as a resolvable edge, not only a status.
- An author can type a relationship (implements / depends-on / satisfies) within
  a `## Related <Type>` section, validated, defaulting to untyped.
- A missing reverse edge and an unrecognised edge-label are surfaced as advisory
  findings rather than passing silently.
- A decision can declare what it applies to, enabling scoped grounding.
- Lifecycle and gate state (`Deferred` / `Blocked`) are enumerable data, not
  free-text lines.

## Initiatives

- **Bucket A — advisory only (no schema change):** an asymmetric-edge finding in
  `rac relationships --validate` (`rac-relationship-back-references`), plus
  tightening detection of unrecognised edge-labels. Ships like the
  `unlinked-reference` advisory (ADR-082); lowest risk.
- **Bucket B — additive non-artifact reference sections:** `## Satisfied By`
  (files) and `## Sources` (URLs) as one external-reference family
  (`rac-external-and-file-references`), and `## Supersedes` on roadmaps
  (`rac-roadmap-supersession`). Format/existence-checked, never fetched, reusing
  the ADR-087 `## Related Tickets` pattern.
- **Bucket C — model / lifecycle decisions (ADR-first):** typed relationship
  edges (`rac-typed-relationship-edges`, aligned with ADR-074), an `## Applies
  To` scope on decisions (`rac-decision-applies-to-scope`), and an extended
  status vocabulary (`rac-artifact-status-vocabulary`).

## Success Measures

- Each previously-silent declared link in the audit's evidence resolves to an
  edge or is surfaced as an advisory once its gap's fix ships.
- Every change is additive (ADR-007): existing sections, `schema_version`, and
  `supersedes`'s decision semantics are unchanged.
- `rac validate rac/`, `rac relationships rac/ --validate`, and `rac review rac/`
  stay clean across the programme; classification is unshifted (adjacent-type
  tests hold).

## Assumptions

- The relationship model stays "structural references in Markdown sections"
  (ADR-016); these gaps are about the vocabulary, not the mechanism.
- Determinism and offline operation hold (ADR-002, ADR-066): files and URLs are
  format/existence-checked, never fetched.

## Risks

- New sections shift the classification surface; mitigated by additive-only
  changes and the adjacent-type misclassification tests.
- Self-referential edges could invite cycles; mitigated by keeping `related_*`
  undirected, as today.

## Related Decisions

- adr-016
- adr-007
- adr-002
- adr-066
- adr-074
- adr-087
- adr-019

## Related Requirements

- rac-external-and-file-references
- rac-roadmap-supersession
- rac-typed-relationship-edges
- rac-relationship-back-references
- rac-decision-applies-to-scope
- rac-artifact-status-vocabulary
- rac-traceability-self-relationships

## Related Roadmaps

- growth-programme

## Related Tickets

- itsthelore/asdecided-core#236
