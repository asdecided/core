---
schema_version: 1
id: RAC-KXS19P8363M7
type: decision
---
# ADR-117: Org-Wide Grounding Is a Serving Topology Before It Is a Federation Mechanism

## Status

Proposed

## Category

Architecture

## Context

At organisation scale the corpus unit and the decision unit diverge: a
~2,500-engineer adopter holds hundreds of repositories, while the decisions
that matter most — platform standards, golden paths, firm-wide ADRs — are
org-wide. Lore's answer to that divergence is federation, and ADR-089
deliberately accepted it in principle while deferring the mechanism:
`## inherits` is unrecognised, cross-repository references do not resolve,
and the resolver ships later under its own implementing ADR with the design
partner.

Deferral of the mechanism must not mean deferral of the reach. The serving
half already shipped: `rac mcp --transport http` fronts one `main`-backed
checkout as a shared, stateless, mandatory-audit-on endpoint (ADR-098), kept
fast by the derived-index cache (ADR-099) and attributed per request
(ADR-084). Nothing in those decisions restricts *which* corpus an endpoint
fronts, and nothing restricts an agent to mounting exactly one `lore`
server — MCP clients mount many servers side by side as ordinary
configuration.

The enterprise adoption review (`research/2026-07-enterprise-adoption-review.md`)
named the consequence: without a recorded topology, org-wide grounding waits
on the engine's deepest change, and the rollout cost of the eventual
declare-side mechanism is O(repositories). With one, reach is available now,
on shipped and decided surfaces — but only if the shape is recorded, so the
co-mount is not mistaken for federation and does not quietly grow into it.

## Decision

Org-wide grounding is delivered **today as a deployment topology** on the
shipped serving surface, and **later as a resolution mechanism** under
ADR-089's programme. The topology:

- **One org-standards corpus, one shared endpoint.** The organisation keeps
  its firm-wide knowledge as an ordinary single-root corpus in its own
  repository (ADR-018) and serves it over the shipped HTTP transport under
  ADR-098's whole posture — read-only, stateless, proxy-authenticated,
  mandatory-audit-on.
- **Agents co-mount, they do not merge.** A repository's agents mount the
  org endpoint as a second MCP server (`lore-org`) beside any local `lore`
  server. Each corpus remains its own canonical truth (ADR-018, ADR-080);
  provenance is carried by endpoint identity — an answer from `lore-org`
  *is* the org's knowledge, attributably so.
- **`rac init --org-endpoint <url>` emits the wiring.** The engine writes
  the `lore-org` entry — `{"type": "http", "url": <url>}` — into `.mcp.json`
  and `.cursor/mcp.json`. This stays inside ADR-088's boundary: client
  wiring a careful admin would hand-write, configuration only, never prose,
  no code path a solo developer cannot reach (ADR-085).
- **Org wiring is an explicit act, so it applies beyond creation time.**
  Unlike a profile — creation-time by design — the flag names a deliberate
  operator action and therefore also applies to an already-initialized
  repository: the entry is merged into an existing client config, updating
  only the `lore-org` key and never removing or rewriting what the user
  wrote. A second run with the same URL changes nothing.
- **No cross-corpus semantics enter the engine.** The engine gains no
  cross-corpus resolution, validation, identity, or precedence: references
  between the corpora remain prose, collisions remain out of scope, and
  ADR-089's five constraints are untouched for the mechanism to come.
- **The federation handoff is upgrade, not replacement.** When the
  ADR-089 mechanism ships, the same org corpus becomes the pinned,
  materialised parent; the profile unhollows its parent line (ADR-088); and
  the co-mount collapses toward one resolution space with explicit
  overrides and preserved provenance. Nothing in this topology needs to be
  undone.

## Consequences

### Positive

- Rollout cost for org-wide grounding drops from O(repositories) to O(1):
  wire the endpoint into the org's agent baseline (repo templates, or one
  `rac init --org-endpoint` per repo) and every engineer's agent grounds
  against org standards — including in repositories that have no corpus of
  their own.
- Every component is already shipped and already decided: the transport
  (ADR-098), the cache (ADR-099), the audit attribution (ADR-084), the
  config emission boundary (ADR-088). The topology adds reach, not trust
  surface.
- The federation programme keeps its sequencing and its design-partner
  iteration (ADR-089) without holding the org-reach question hostage.

### Negative

- Two mounted servers mean two tool surfaces in the agent's context until
  federation collapses them; the org endpoint should therefore be the only
  mount in repositories with no local corpus.
- Endpoint reach is corpus visibility: anyone who can reach the org
  endpoint can read the whole org corpus. Sensitivity partitioning is
  corpus topology (separate corpora, separate endpoints) — never
  per-artifact ACLs in the engine (ADR-085).
- Cross-corpus references stay unresolved prose until the mechanism lands;
  a child repo cannot yet cite a firm ADR as a validated relationship.

## Alternatives Considered

### Wait for federation

Deliver org-wide reach only when the ADR-089 mechanism ships.

#### Disadvantages

- Leaves reach at one repository for the duration of the engine's deepest
  and most deliberately-paced change, at exactly the population where reach
  is the multiplier. Rejected: the serving surface already carries the need.

### Vendor the org corpus into every repository now

Pin the org corpus as a submodule or vendored bundle per repository, ahead
of any resolver.

#### Disadvantages

- O(repositories) rollout plus a permanent pin-bump tax, with none of the
  mechanism's actual benefits (no resolution, no collision findings, no
  overrides) — the cost of federation without its semantics. As a *later
  materialisation step under ADR-089* the pin is right; as the day-1
  delivery vehicle it is the wrong end of the lever.

### Merge org artifacts into each repository's corpus

Copy the firm-wide ADRs into every child corpus.

#### Disadvantages

- Duplicate identities across the fleet, immediate drift, and silent
  divergence from the org's canonical state — the precise failure mode
  ADR-080 exists to prevent. Rejected.

## Related Decisions

- adr-018
- adr-080
- adr-084
- adr-085
- adr-088
- adr-089
- adr-098
- adr-099

## Related Requirements

- org-endpoint-wiring

## Related Roadmaps

- org-grounding-plane
- corpus-federation
- lore-at-team-scale
