---
schema_version: 1
id: RAC-KTXSTGNKHHVX
type: decision
---
# ADR-036: Lore Product Identity

> The current product, engine, and distribution naming in this historical
> decision is superseded by ADR-132. Stable `RAC-*` artifact IDs and published
> machine keys remain compatibility identifiers; current public surfaces use
> AsDecided, `decided`, and `decided-mcp`.

## Status

Accepted

## Category

Product

## Context

The v0.10.x launch positions the agent-grounding capability as a product
with its own name: Lore, "agents that know why". The README, header
banner, and outreach materials lead with that name.

Underneath it, everything ships as RAC — Requirements as Code: the PyPI
package (`requirements-as-code`), the CLI (`rac`), the MCP server
(`rac mcp`), the artifact model, and this corpus itself. Renaming those
surfaces at launch would break install instructions, client
configurations, pinned contracts, and muscle memory, for no user benefit.

Without a recorded naming structure, the two names will drift: new
documentation, tool text, and conversations will each pick a name ad hoc,
and the first question outreach generates — "what is Lore, and what is
RAC?" — has no corpus answer for Guide to serve.

## Decision

Lore is the product. RAC is the engine.

- **Lore** names the product: the experience of a coding agent grounded
  in a team's recorded knowledge — the corpus conventions, the MCP
  serving surface, and the launch narrative. Marketing-facing material
  (README lead, banner, demo, announcement) leads with Lore.
- **RAC — Requirements as Code** names the open-source engine: the
  artifact model, validation, relationships, services, CLI, and MCP
  server implementation. Technical documentation, contracts, and code
  keep the RAC name.
- **Distribution names are unchanged for now.** The PyPI package stays
  `requirements-as-code`; the CLI command and MCP server stay `rac` /
  `rac mcp`; internal consumer naming (Guide, Explorer) is unchanged.
  Renaming any shipped surface is a separate, future decision with its
  own migration plan — it does not follow implicitly from this one.
- Where both names appear, the relationship is stated once, in this
  form: "Lore is built on RAC — Requirements as Code — the open-source
  engine underneath."

## Consequences

### Positive

- The product can carry a brandable, memorable name without breaking any
  shipped surface, install path, or pinned contract.
- One recorded answer to "Lore vs RAC" exists, and Guide can serve it.
- Engine naming stays stable for contracts, tests, and integrations.

### Negative

- Two names must be explained wherever newcomers land; every major
  entry point needs the one-line relationship statement.
- Search and discovery split across two names until the brand settles.

### Risks

- Drift: future surfaces pick a name ad hoc. Mitigated by treating this
  decision as the naming reference for new material.
- A future package or CLI rename, if ever chosen, invalidates published
  configuration blocks and documentation; that cost belongs to the
  future decision, not this one.

## Alternatives Considered

### Rename everything to Lore at launch

Rejected: breaks install instructions, verified client configuration
blocks, and the pinned tool surface days before outreach, and couples a
brand experiment to a contract migration.

### Launch with the RAC name only

Rejected: the engine name describes the mechanism, not the outcome. The
launch story — agents that know why — is a product claim, and a product
claim warrants a product name that can outlive implementation details.

## Related Decisions

- ADR-029

## Related Roadmaps

- v0.10.2-guide-grounding-demo
