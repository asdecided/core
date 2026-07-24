---
schema_version: 1
id: RAC-KXS19M3YXEB9
type: roadmap
---
# RAC — Org Grounding Plane

## Status

Achieved

Delivered across all three initiatives (epic itsthelore/asdecided-core#348), under
the topology decision ADR-117: the recorded shape (one org-standards corpus
served over the shipped ADR-098 HTTP endpoint, co-mounted as `lore-org`
beside any local `lore` server); the fleet-wiring command (`rac init
--org-endpoint <url>` emitting and merging the client config entry on fresh
and already-initialized repositories, idempotently, per
`rac-org-endpoint-wiring`); and the operator runbook (the Org Grounding docs
page: corpus, endpoint, fleet wiring, boundaries, and the federation
handoff). One unblocker was folded in: unhashable frontmatter keys — the
fuzz campaign's pinned oracle-crash class — now surface as
`malformed-frontmatter` findings instead of crashing every whole-repository
walk. No cross-corpus semantics entered the engine; the `corpus-federation`
programme is untouched.

## Context

The enterprise adoption review (`research/2026-07-enterprise-adoption-review.md`)
identified reach as the binding constraint at organisation scale: value is
decisions × repositories they ground, the corpus unit is a repository, and
federation — the mechanism that would lift reach — is deliberately deferred
and deliberately paced (ADR-089, `corpus-federation`). Meanwhile the serving
half of org-wide reach already shipped in `lore-at-team-scale`: the shared
HTTP transport (ADR-098), the derived-index cache (ADR-099), and
per-request audit attribution (ADR-084).

This roadmap closes the gap between those two facts. It records and ships
the day-1 topology — org-wide grounding as a deployment shape on shipped
surfaces — so an organisation's agents ground against firm-wide decisions
now, while the federation programme proceeds at its own pace toward the
same corpus becoming a materialised parent.

## Outcomes

- An organisation stands up one org-standards corpus behind one shared
  endpoint, and every engineer's agent — in any repository, with or without
  a local corpus — grounds against it from the first session.
- Wiring a repository is one command (`rac init --org-endpoint`), safe on
  fresh and existing repositories alike, so fleet rollout is repo-template
  work rather than per-repo engineering.
- The topology is recorded (ADR-117) with its boundaries explicit: no
  cross-corpus resolution, no auth in the engine, no change to the
  federation programme's scope or sequencing.
- An operator has a runbook that composes the existing shared-server recipe
  with the org topology, including what the shape does *not* give them
  until federation lands.

## Initiatives

### Initiative 1 — Record the topology (ADR-117)

The decision that org-wide grounding is a serving topology before it is a
federation mechanism: the co-mount shape, the explicit-act wiring
semantics, the engine's untouched boundaries, and the federation handoff.

### Initiative 2 — Fleet wiring (`rac-org-endpoint-wiring`)

`rac init --org-endpoint <url>`: emit the `lore-org` HTTP entry into
`.mcp.json` and `.cursor/mcp.json`, creating the files when absent and
merging into them when present — updating only the `lore-org` key,
preserving everything the user wrote, idempotent on re-run, with the URL
validated and failures structured. Additive CLI and JSON contract
(ADR-007).

### Initiative 3 — Operator runbook

The Org Grounding docs page: create the org corpus (quickstart, seeded by
ingest), serve it (the existing shared-server recipe), wire the fleet
(repo templates, the init flag, `--agent-rules` blocks for non-MCP
clients), verify from a child repository, and the boundaries — visibility,
unresolved cross-references, and what changes when federation ships.

## Constraints

- No cross-corpus resolution, validation, identity, or precedence in the
  engine; ADR-089's five constraints and the `corpus-federation` programme
  are untouched.
- No authentication in the engine: the org endpoint inherits ADR-098's
  posture whole — proxy-authenticated, mandatory-audit-on, attributable
  not authenticated (ADR-084, ADR-085).
- Config-only emission, never prose (ADR-088, ADR-024, ADR-044); the
  wiring adds or updates exactly the `lore-org` key it owns and never
  removes user content.
- Files-in-git stay canonical for both corpora; each remains its own
  single-root truth (ADR-018, ADR-080).
- Additive contracts only (ADR-007): without the flag, `rac init` output
  is byte-identical to the previous engine.

## Non-Goals

- Corpus federation, `## inherits`, parent materialisation, or collision
  semantics — the `corpus-federation` programme owns them (ADR-089).
- Any auth, RBAC, SSO, or credential handling in the engine (ADR-085).
- Emitting client configs beyond the two targets the profile already
  wires (ADR-088).
- A registry of org endpoints, endpoint discovery, or any network call at
  init time — the flag writes configuration bytes and nothing else
  (ADR-002).

## Success Measures

- From a repository with no corpus, an agent mounting only the emitted
  `lore-org` entry answers grounding queries from the org corpus, with the
  read attributed in the org endpoint's audit log (verified end to end
  against a live local endpoint).
- `rac init --org-endpoint` on a fresh repo, an initialized repo, and a
  repo with a hand-written `.mcp.json` produces the documented result in
  each case; a second run reports and writes nothing.
- Without the flag, `rac init` behaviour and output are byte-identical to
  the previous engine, asserted by the existing profile and init
  batteries.
- `rac validate rac/`, `rac relationships rac/ --validate`, and
  `rac review rac/` stay clean across the roadmap's output.

## Assumptions

- MCP clients follow the documented many-servers configuration model, so
  co-mounting `lore` and `lore-org` needs no client-side change.
- The org corpus is operated under the shared-server recipe as shipped
  (container, proxy, audit sink, keep-current); this roadmap adds no
  serving machinery.
- The federation mechanism, when it ships, treats the org-standards corpus
  as the parent this topology already established, so adopters carry no
  migration cost for having started here.

## Risks

- Two mounted surfaces could double tool noise in the agent's context.
  Mitigation: the runbook's default is org-endpoint-only for repositories
  without a local corpus, and the surfaces are budgeted (ADR-033) so even
  both together stay lean.
- The co-mount could be mistaken for federation, or grow toward it by
  accretion. Mitigation: ADR-117 records the boundary and the handoff;
  cross-corpus semantics are a named non-goal gated on ADR-089's
  programme.
- Org-endpoint visibility could be over-assumed. Mitigation: the runbook
  states plainly that endpoint reach is whole-corpus read visibility and
  that partitioning is corpus topology.

## Related Decisions

- adr-002
- adr-007
- adr-018
- adr-033
- adr-080
- adr-084
- adr-085
- adr-088
- adr-089
- adr-093
- adr-094
- adr-098
- adr-099
- adr-117

## Related Roadmaps

- lore-at-team-scale
- corpus-federation
- corpus-setup-guidance

## Related Requirements

- rac-org-endpoint-wiring

## Related Tickets

- itsthelore/asdecided-core#348
