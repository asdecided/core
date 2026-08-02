---
schema_version: 1
id: RAC-01K8Q7MCP413
type: decision
---
# ADR-127: Attributable MCP Audit Records

## Context

The Rust MCP server already makes HTTP audit mandatory, but the implementation
drifted from the accepted audit contract in three ways. It read the old
`X-AsDecided-Principal` spelling instead of ADR-098's canonical
`X-Lore-Principal`; it only extracted top-level and a subset of collection
identities; and it emitted bare strings even though ADR-084 requires each
returned identity to carry resolution state and a provenance reference. Audit
activation was also silent beyond the generic HTTP listening message.

That drift is especially dangerous on a shared endpoint: a green audit file can
look complete while omitting the artifacts an agent actually received, or can
attribute a caller through an ambiguous duplicate header.

## Decision

The canonical shared-server attribution carrier is `X-Lore-Principal`, as
already decided by ADR-098. During migration, the Rust server accepts the
legacy `X-AsDecided-Principal` spelling only as a compatibility alias:

- empty or whitespace-only values are treated as no assertion;
- control characters and values over 512 bytes are malformed and rejected;
- duplicate occurrences of either spelling are rejected;
- if both spellings occur, equal trimmed values resolve to the canonical value;
- conflicting values are rejected with HTTP 400 and JSON-RPC error `-32023`.

The principal remains attribution, never authentication or authorization. A
fronting proxy must authenticate the caller and overwrite the carrier it sends
to the engine. The legacy alias is a migration aid, not a second identity
contract, and may be removed in a later recorded decision.

Every audit event records `returned` as a deduplicated, first-seen list of
objects with this shape:

```json
{"id":"RAC-…","resolved":true,"provenance":{"path":"decisions/adr.md"}}
```

The extractor covers the primary `id` and the `matches`, `items`, `decisions`,
`incoming`, and `neighborhood` result collections. `resolved` is copied when a
result supplies it and otherwise defaults to `true`; `provenance.path` is a
reference to the returned artifact's corpus path. Artifact bodies and full
provenance payloads never enter the audit file. Raw outgoing relationship text
is not a returned artifact identity and remains excluded. Error responses
record an empty list.

When audit is enabled, startup writes one stderr diagnostic naming the resolved
path, the recorded scope (`MCP read tools`), the transport, and the effective
`on_write_error` failure mode. HTTP retains its mandatory-audit gate and forced
`block` posture; stdio remains configuration-driven and default-absent.

This decision supersedes the implementation-era `X-AsDecided-Principal`-only
carrier and string-only returned projection. ADR-084 and ADR-098 remain the
governing audit and shared-HTTP decisions.

## Status

Accepted

## Category

Technical

## Consequences

- A shared audit log can answer who received which artifact, with a stable path
  reference and an explicit resolution state.
- Existing proxies using the old header continue to work during migration, but
  ambiguous or malformed attribution fails closed before a tool call runs.
- Audit records are richer side-file data without changing MCP response bytes.
- Operators can see the audit sink and failure posture at process activation.

## Alternatives Considered

### Keep accepting only `X-AsDecided-Principal`

Rejected. It contradicts the canonical carrier already fixed by ADR-098 and
would keep the public documentation and implementation out of sync.

### Record only bare artifact IDs

Rejected. IDs alone cannot tell an auditor whether a result was resolved or
where the returned identity came from, and they do not satisfy ADR-084.

### Let the last duplicate header win

Rejected. Header coalescing order is intermediary-dependent and would make
attribution ambiguous. Duplicate or conflicting carriers fail closed.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The attribution carrier, collection coverage, and activation diagnostic are deterministic source contracts."
rules:
  - id: mcp-audit-uses-canonical-principal
    kind: require_pattern
    path_glob: "rust/decided-mcp/src/audit.rs"
    pattern: 'CANONICAL_PRINCIPAL_HEADER: &str = "X-Lore-Principal"'
    message: "Shared MCP audit attribution must keep X-Lore-Principal canonical."
  - id: mcp-audit-covers-all-result-collections
    kind: require_pattern
    path_glob: "rust/decided-mcp/src/audit.rs"
    pattern: 'matches", "items", "decisions", "incoming", "neighborhood'
    message: "Audit extraction must cover every artifact-bearing MCP result collection."
  - id: mcp-audit-announces-activation
    kind: require_pattern
    path_glob: "rust/decided-mcp/src/audit.rs"
    pattern: 'scope=.*, on_write_error='
    message: "Enabled audit must announce its scope and failure mode at startup."
```

## Related Decisions

- adr-007
- adr-084
- adr-085
- adr-098
- adr-120
- adr-121

## Related Requirements

- rac-mcp-http-transport
- rac-shared-server-audit-identity

## Applies To

- rust/decided-mcp/src/audit.rs
- rust/decided-mcp/src/http.rs
- rust/decided-mcp/tests/http_transport.rs
