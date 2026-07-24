---
schema_version: 1
id: RAC-KW7VKMG2TCW6
type: requirement
---
# RAC Decision Scope — Applies-To

## Status

Accepted

Delivered as Initiative 1 of the `decision-to-code-proximity` roadmap
(itsthelore/asdecided-core#274): the optional `## Applies To` section on decisions,
with path entries existence-checked by `rac relationships --validate` and the
scope surfaced as a filesystem-scoped edge via `get_related`. This also closes
the decision-scope box on the traceability epic (itsthelore/asdecided-core#236). The
path→decisions lookup that queries this vocabulary is a separate requirement
(`rac-path-decisions-lookup`, itsthelore/asdecided-core#275).

## Problem

A decision often governs a specific part of the repository — a directory, a
component, a surface — but says so only in prose ("this decision applies to RAC
Core", "to the CI workflows"). So "which decisions apply to the file I am
editing?" is unanswerable, and scoped grounding through the Lore MCP server is
impossible. The scope is meaning the corpus holds but cannot express as data.

This is gap 7 of the traceability audit.

## Evidence

- `adr-035` states it applies to RAC Core and open-source extensions, in prose.
- `adr-018` governs `rac/`; `adr-023` scopes itself to `src/rac/` internals;
  `adr-027` applies to the CI workflows; `adr-033` to the Guide tool surface —
  none of these scopes is data.

## Requirements

- [REQ-001] The decision schema accepts an optional `## Applies To` section listing the paths or component names the decision governs, recognised and extracted like other optional sections.

- [REQ-002] Path-style `## Applies To` entries are existence-checked by `rac relationships --validate`; component-name entries are recorded without resolution.

- [REQ-003] The section is surfaced via the Lore MCP server / `get_related` so a consumer can ask which decisions apply to a given path (scoped grounding).

- [REQ-004] The change is additive (ADR-007) and does not shift decision classification (adjacent-type tests hold).

## Success Metrics

- The evidence decisions declare a checkable `## Applies To` scope.
- A query for "decisions applying to path X" returns them deterministically.
- Decision classification is unchanged by the new section.

## Risks

- Scope expressed as free-text components is unresolvable; mitigated by
  existence-checking the path form and treating component names as recorded labels.

## Assumptions

- Path-or-component scope covers the real need; a richer scope language is out of scope.

## Related Decisions

- adr-016
- adr-007

## Related Roadmaps

- relationship-vocabulary
- decision-to-code-proximity
