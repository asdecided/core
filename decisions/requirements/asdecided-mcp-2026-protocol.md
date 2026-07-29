---
schema_version: 1
id: RAC-MCP20260728B
type: requirement
---
# Requirement: MCP 2026-07-28 Protocol Compatibility

## Status

Accepted

## Problem

The native AsDecided MCP server is stateless and read-only, but its wire
contract ends at MCP `2025-06-18`. It cannot answer the mandatory
`server/discover` request, does not enforce the current HTTP routing headers,
and does not emit current-era caching hints or protocol errors. Clients pinned
to `2026-07-28` therefore cannot use AsDecided reliably, while removing the
legacy lifecycle would break existing installations.

## Requirements

- [REQ-001] `decided-mcp` MUST implement `server/discover` and advertise `2026-07-28`, its read-only capabilities, and its real package identity.
- [REQ-002] Every `2026-07-28` request MUST carry the selected protocol revision and `clientCapabilities` in namespaced request `_meta`; HTTP requests MUST additionally carry `MCP-Protocol-Version`.
- [REQ-003] Current-era HTTP POST requests MUST carry `Mcp-Method` and, where the RPC names a tool or resource, the corresponding `Mcp-Name`; a mismatch with the JSON-RPC body MUST fail before dispatch.
- [REQ-004] The server MUST return the revision-defined errors for header mismatch (`-32020`), unsupported protocol version (`-32022`), and unknown method (`-32601`); no AsDecided RPC currently requires a client capability that would justify `-32021`.
- [REQ-005] An unknown current-era HTTP method MUST return HTTP `404`; malformed or revision-invalid current-era requests MUST return HTTP `400` with a structured JSON-RPC error.
- [REQ-006] `server/discover`, `tools/list`, `prompts/list`, and `resources/list` results MUST include `resultType`, non-negative `ttlMs`, and an explicit `cacheScope` for `2026-07-28`; every other successful current-era result MUST include `resultType`.
- [REQ-007] Tool input and output schemas MUST be valid JSON Schema 2020-12, MUST retain the object-root input constraint, and MUST NOT dereference external references.
- [REQ-008] Initialize-based clients MUST continue to negotiate `2024-11-05`, `2025-03-26`, `2025-06-18`, or `2025-11-25`; the pinned `2025-06-18` response and tool payload fixtures MUST remain byte-identical.
- [REQ-009] stdio and HTTP MUST dispatch identical valid tool calls to the same implementation and return semantically identical tool results.
- [REQ-010] The six-tool read-only surface, corpus freshness behavior, response budgets, mandatory HTTP audit posture, and proxy-owned authentication boundary MUST remain unchanged.
- [REQ-011] New-protocol diagnostics MUST use stderr or structured protocol errors rather than the deprecated MCP logging capability; legacy parse-error compatibility MAY retain its pinned notification.
- [REQ-012] CI MUST exercise legacy and current protocol scenarios without a model call, embeddings, or network-dependent assertions.

## Success Metrics

- A strict `2026-07-28` conformance client discovers the server and completes
  `tools/list` and representative `tools/call` requests over stdio and HTTP.
- All pre-existing MCP compatibility fixtures remain green.
- Protocol tests produce identical results across repeated runs.

## Risks

- Dual-era branching creates response drift. Mitigation: revision-specific
  envelope tests around one shared tool dispatcher.
- Strict HTTP validation exposes previously tolerated malformed clients.
  Mitigation: strictness applies only when the caller selects `2026-07-28`;
  legacy handling remains available.
- Cache hints hide corpus changes. Mitigation: only static list surfaces receive
  positive TTLs; corpus-dependent reads use zero TTL or remain uncached.

## Assumptions

- MCP `2026-07-28` is the published current specification.
- Official clients retain a legacy fallback path during the ecosystem
  transition.
- AsDecided does not require Tasks, Apps, roots, sampling, or protocol-level
  sessions to provide deterministic decision grounding.

## Related Decisions

- adr-007
- adr-029
- adr-030
- adr-032
- adr-065
- adr-066
- adr-084
- adr-085
- adr-098
- adr-120
- adr-121

## Related Designs

- mcp-2026-dual-era-wire-contract

## Related Requirements

- rac-mcp-http-transport
- rac-mcp-surface-budget

## Related Tickets

- asdecided/core#389

## Verified By

- rust/decided-mcp/tests/protocol_2026.rs
- rust/decided-mcp/tests/protocol_legacy.rs
- rust/decided-mcp/tests/http_transport.rs
