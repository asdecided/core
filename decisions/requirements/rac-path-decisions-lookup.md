---
schema_version: 1
id: RAC-KWK4V9CF3DKZ
type: requirement
---
# Requirement: Path-to-Decisions Lookup

## Status

Accepted

Classification: `[internal]` — the decisions binding a file, in one call.
Delivered as Initiative 2 of the `decision-to-code-proximity` roadmap
(itsthelore/asdecided-core#275): the `rac decisions-for` CLI subcommand and the
additive `path` argument on the `find_decisions` MCP tool, over one shared
core, consuming the `## Applies To` vocabulary of
`rac-decision-applies-to-scope`.

## Problem

An agent editing `src/auth/` has no deterministic way to ask which recorded
decisions govern that code. Once `## Applies To` makes the scope data, the
data still has no query face — neither the CLI nor the MCP surface can
answer "decisions affecting this path," so the proximity the vocabulary
creates never reaches the point of edit.

## Requirements

- [REQ-001] The CLI MUST expose a deterministic path→governing-decisions lookup: given a file or directory path, return every decision whose declared `## Applies To` entries cover that path, each with id, title, status, and the matching declared entry — a new read-only subcommand (ADR-005), reading fresh per call (ADR-032).
- [REQ-002] The MCP surface MUST expose the same lookup as an additive optional `path` argument on the existing `find_decisions` tool, holding the five-tool surface; the pinned tool description (ADR-030) MUST be revised additively and deliberately in the same change — under its own decision if the pinned-surface posture requires ratification, never silently.
- [REQ-003] The result MUST be a pure function of the declared references and the file tree (ADR-066): deterministic stdlib-semantics glob and path matching, deterministically sorted results, no dependence on filesystem enumeration order, no code parsing, and no persisted index or database (ADR-002, ADR-080).
- [REQ-004] A path matching no declarations — including a path outside the repository — MUST return a valid empty result, never an error; unresolvable *declared* paths remain the declaration requirement's validation concern (`rac relationships --validate`), not the lookup's.
- [REQ-005] The response MUST report which decisions bind and their status, never a compliance judgement (ADR-034); the MCP face MUST respect the response budget (ADR-033); both faces MUST be served by one shared core service (ADR-031) and stay payload-consistent.
- [REQ-006] The change MUST be additive (ADR-007): no existing CLI or MCP contract field changes, and `find_decisions` invoked without a path is byte-identical to today's output.
- [REQ-007] Matching MUST be platform-independent: declared entries and query paths normalise to POSIX-style repository-relative form, so the same corpus yields byte-identical results on any OS (ADR-002).

## Acceptance Criteria

- On a fixture corpus with literal-path, directory, and glob scopes,
  querying a nested file returns exactly the governing decisions with their
  matching entries, deterministically ordered, byte-identical across
  repeated runs.
- `find_decisions` without a path is byte-identical to pre-change goldens;
  with a path, the additive payload validates and stays within budget on an
  oversized fixture.
- Ungoverned-path and outside-repository queries both return the documented
  empty shape (CLI exit 0, no MCP protocol error).
- The CLI and MCP faces return the same decision set for the same query.
- The pinned-description battery passes with the revised `find_decisions`
  description, updated in the same change.

## Success Metrics

- An agent at the point of edit gets the decisions binding a file in one
  call, with no searching and no judgement layered on top.

## Risks

- Revising the pinned description destabilises consumers pinned by the
  ADR-030 battery. Mitigation: REQ-002's additive-only, deliberate revision
  with the battery updated in the same change.
- Glob-semantics ambiguity across platforms yields divergent matches.
  Mitigation: REQ-003 and REQ-007 pin stdlib semantics over normalised
  POSIX-style paths, documented with the command.

## Assumptions

- `rac-decision-applies-to-scope` ships first or in the same change; the
  lookup consumes its vocabulary and defines no second one.
- Component-name entries are excluded from path matching until a registry
  exists — a later decision, not pre-decided here.

## Related Decisions

- adr-002
- adr-005
- adr-007
- adr-030
- adr-031
- adr-032
- adr-033
- adr-034
- adr-065
- adr-066
- adr-080

## Related Roadmaps

- decision-to-code-proximity

## Related Requirements

- rac-decision-applies-to-scope
