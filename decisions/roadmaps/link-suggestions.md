---
schema_version: 1
id: RAC-KVSNP3C2T4WA
type: roadmap
---
# Relationship Link Suggestions

## Status

Achieved

## Context

RAC's relationship graph is only as complete as the edges authors remember to
declare in `## Related <Type>` sections. The text is usually ahead of the graph:
artifacts name each other in prose — an ADR id, a requirement, a design slug —
without a matching declared edge. The link is real and intended; the graph just
does not carry it, so traversal, coverage, and the export contract under-report
how connected the corpus actually is. `rac doctor` finds orphans but cannot find
a link the author already wrote in prose and simply did not promote.

This release closes that gap on RAC's own terms. ADR-082 sets the boundary —
detect mentioned-but-unlinked references and **suggest** them, deterministically,
but never auto-create an edge — so the graph stays *validated*, not inferred from
prose (ADR-074), and promotion stays a human review act (ADR-065). The *how* is
in the `mentioned-but-unlinked-detection` design; this item schedules it.

It is the in-domain, deterministic half of what general agent-memory tools do by
auto-wiring — kept on RAC's side of the inference line (ADR-002, ADR-066).

## Outcomes

- A maintainer running `rac doctor` sees, as advisory findings, the references
  an artifact's body makes to other artifacts that are missing from its declared
  `## Related` sections — each with a paste-ready line to promote it.
- The declared graph gets measurably denser over time without any edge ever
  being written by the tool: every promotion is a reviewed human edit.
- The behaviour is deterministic and offline end to end: same corpus,
  byte-identical findings; no model, embedding, or network call.

## Initiatives

### Initiative 1 — Detection service (Core)

A pure function over the parsed corpus and the existing relationship index that
returns, per artifact, the set of body references (canonical id, alias, or
filename-style ref) that resolve to another artifact not already declared as a
`## Related` edge. Reuses the resolver, token-boundary matching (ADR-037), the
relationship index, and the shared Markdown parser; excludes frontmatter, the
`## Related` sections, fenced code blocks, and self-references. The *how* is in
the `mentioned-but-unlinked-detection` design.

### Initiative 2 — `rac doctor` surface

Expose the findings as a new advisory `rac doctor` code (`unlinked-reference`)
alongside the existing orphan and hub warnings. Each finding names the source,
the matched target and token, the `## Related <Type>` section that would capture
it (derived from the target's type via the registry), and a paste-ready
suggested line. Human output plus `--json`, exit zero — advisory, never a gate.

### Initiative 3 — Boundary and contract coverage

Golden and boundary tests pin the determinism (byte-identical findings on an
unchanged corpus), the exclusions (code fences, declared edges, self-refs), the
one-finding-per-pair rule, and the advisory exit code — proving the detector
never changes the `rac validate` / `rac relationships --validate` contract.

### Initiative 4 — Documentation

Document the new `rac doctor` finding in `docs/cli.md`: what it detects, why it
is advisory, and how to promote a suggestion to a declared edge.

## Constraints

- AI-optional, offline (ADR-002) and deterministic with no embeddings or LLM
  judge (ADR-066).
- Suggest, never apply (ADR-082): findings only; the tool writes no edge.
- Advisory severity: exit zero; the `rac validate` / `rac relationships
  --validate` contract is unchanged (ADR-075 gate discipline).
- Reuse the shared resolver, matcher, relationship index, and parser; no
  parallel mechanism.

## Non-Goals

- Auto-creating, writing, or modifying any relationship edge.
- Any model, embedding, or similarity-based suggestion.
- Title or free-text matching in this release (a deferred design question).
- Making a mentioned-but-unlinked reference a validation error or a merge gate.

## Success Measures

- On the dogfood corpus, `rac doctor` surfaces only references a maintainer
  agrees should be promoted (high precision; near-zero spot-checked false
  positives).
- `rac validate` and `rac relationships --validate` exit codes are identical
  with and without the detector present.
- Re-running `rac doctor` on an unchanged corpus yields byte-identical findings.

## Assumptions

- A body reference by id or filename ref is usually a link the author intended
  and would promote if reminded.
- Deterministic id/alias/filename matching is high-precision enough to be
  trusted without a model.

## Risks

- **Noise** from over-broad matching trains maintainers to ignore the channel.
  Mitigation: id/alias/filename matching only; title matching deferred.
- **Misreading a suggestion as an asserted edge.** Mitigation: advisory framing
  and copy that names it a suggestion to review (ADR-082); the tool never
  applies it.

## Related Requirements

- rac-unlinked-reference-detection

## Related Decisions

- adr-082
- adr-074
- adr-065
- adr-066
- adr-002

## Related Designs

- mentioned-but-unlinked-detection

## Related Tickets

- itsthelore/asdecided-core#225
