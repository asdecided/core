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

These clauses supersede ADR-036's current product, engine, and distribution
naming and ADR-039's current server identity. ADR-121 alone preserves the
legacy `lore` handshake bytes. Durable `RAC-*` artifact IDs, published machine
keys such as `rac_version`, and pre-cutover internal paths remain compatibility
identifiers; they are not names for new public surfaces.

## Consequences

Fresh installs and generated agent configuration now have one unambiguous
server identity. Existing hand-written configurations are not rewritten by
this decision; operators can migrate their keys explicitly, and the current
docs show only the native names. The identity change is configuration-only and
does not alter MCP wire semantics or the read-only serving boundary.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The generated server keys and current viewer guidance have stable source anchors."
rules:
  - id: scaffold-keeps-asdecided-local-key
    kind: require_pattern
    path_glob: "rust/rac-engine/src/scaffold.rs"
    pattern: 'mcpServers.*asdecided.*decided-mcp'
    message: "Generated local MCP configuration must retain the AsDecided key and native binary."
  - id: scaffold-keeps-asdecided-org-key
    kind: require_pattern
    path_glob: "rust/rac-engine/src/scaffold.rs"
    pattern: 'const ORG_SERVER_KEY: &str = "asdecided-org";'
    message: "Generated organisation MCP configuration must retain the AsDecided key."
  - id: scaffold-does-not-restore-retired-server-keys
    kind: forbid_pattern
    path_glob: "rust/rac-engine/src/scaffold.rs"
    pattern: '(?i)\blore(?:-org)?\b'
    message: "Generated MCP configuration must not restore retired Lore server keys."
  - id: viewer-guidance-uses-decided-command
    kind: forbid_pattern
    path_glob: "rac-localview/VIEWER_CONTRACT.md"
    pattern: '(?i)Lore export viewer|`rac export|RAC CLI'
    message: "Current viewer guidance must use AsDecided and the decided command."
```

## Supersedes

- adr-036
- adr-039

## Related Decisions

- ADR-117
- ADR-121
- ADR-124
- ADR-131

## Related Requirements

- org-endpoint-wiring
