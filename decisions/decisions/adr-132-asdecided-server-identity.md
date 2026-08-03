---
schema_version: 1
id: RAC-01K8Q7MCP432
type: decision
---
# ADR-132: AsDecided MCP Server Identity

## Status

Accepted

## Category

Product

## Context

The native cutover completed the move from the historical RAC/Lore product
names to AsDecided. Some older decisions and requirements necessarily retain
the names that were true when they were recorded, but current generated
configuration and public guidance must have one identity. Emitting a retired
server key from the native scaffold makes a fresh install look like a legacy
integration and leaves agents with two competing names for the same service.

## Decision

- `asdecided` is the canonical local MCP server key and `decided-mcp` is the
  canonical native server binary.
- `asdecided-org` is the canonical key emitted by `decided init
  --org-endpoint <url>` for a shared organisation endpoint.
- New examples, generated configuration, documentation, and agent guidance
  MUST use the AsDecided names. The native scaffold MUST NOT emit the retired
  `lore` or `lore-org` keys.
- Historical ADRs, fixtures, and migration notes keep their original wording
  as evidence. They are not current configuration instructions; a later
  amendment or superseding decision is the route for changing their meaning.
- The `decided` CLI and `decided-mcp` server remain the public command surface.
  Compatibility state in old records does not create a supported `rac` command.

## Consequences

Fresh installs and generated agent configuration now have one unambiguous
server identity. Existing hand-written configurations are not rewritten by
this decision; operators can migrate their keys explicitly, and the current
docs show only the native names. The identity change is configuration-only and
does not alter MCP wire semantics or the read-only serving boundary.

## Related Decisions

- ADR-117
- ADR-121
- ADR-124
- ADR-131

## Related Requirements

- RAC-KXS19RDVX4DJ (Org Endpoint Wiring)
