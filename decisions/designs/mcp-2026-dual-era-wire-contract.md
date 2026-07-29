---
schema_version: 1
id: RAC-MCP20260728C
type: design
---
# MCP 2026 Dual-Era Wire Contract

## Context

`decided-mcp` currently owns a deliberately small, dependency-free JSON-RPC
wire layer. One processor serves newline-delimited stdio and single-response
HTTP, preserving Python-oracle bytes for the legacy surface. MCP
`2026-07-28` replaces the initialize lifecycle with per-request negotiation and
adds transport-level requirements without changing AsDecided's tool semantics.

## User Need

An agent host should connect to AsDecided using either the established MCP
lifecycle or the current revision and receive the same deterministic,
read-only grounding answers. Operators should be able to upgrade clients
without coordinating an all-at-once AsDecided migration.

## Design

### 1. Protocol seam

Introduce a small protocol module between transport parsing and tool dispatch.
It owns:

- protocol revision constants and era classification;
- extraction of namespaced request metadata;
- `server/discover` and legacy `initialize` envelopes;
- standard JSON-RPC protocol errors;
- cache hints for current-era list results.

Tool implementations remain unaware of protocol revisions.

### 2. Revision matrix

| Selected revision | Lifecycle | Version carrier | Identity |
| --- | --- | --- | --- |
| `2024-11-05` through `2025-11-25` | `initialize` / `initialized` | `params.protocolVersion` during initialize | pinned legacy `lore` / `1.28.1` |
| `2026-07-28` | `server/discover`, then independent requests | namespaced request `_meta`; HTTP header cross-check | `decided-mcp` / package version |

Legacy initialize never negotiates `2026-07-28`. Current-era requests never
require or create a session.

### 3. stdio flow

1. Parse one newline-delimited JSON-RPC value.
2. If the method is `server/discover`, return the current discovery result.
3. Otherwise classify the request from
   `_meta["io.modelcontextprotocol/protocolVersion"]`.
4. Validate the selected revision and required per-request
   `clientCapabilities` before dispatch.
5. Dispatch valid tool requests through the existing shared processor.
6. Write diagnostics to stderr; keep only explicitly pinned legacy notification
   bytes.

A client receiving `-32601` from an older server can fall back to initialize.
The upgraded server responds to discovery directly.

### 4. HTTP flow

For a `2026-07-28` POST:

1. Require `MCP-Protocol-Version: 2026-07-28`.
2. Require `Mcp-Method` and compare it with JSON-RPC `method`.
3. For `tools/call`, compare `Mcp-Name` with `params.name`.
4. Compare the header revision with request `_meta`.
5. Return `400` plus a structured protocol error on revision or header
   validation failure.
6. Return `404` plus `-32601` for an unknown RPC.
7. Return one JSON response; emit no session identifier.

Legacy HTTP requests retain their existing transport behavior and audit
precondition.

### 5. Cache policy

| Result | `ttlMs` | `cacheScope` | Reason |
| --- | ---: | --- | --- |
| `tools/list` | 86,400,000 | `public` | fixed by the installed binary |
| `prompts/list` | 86,400,000 | `public` | fixed empty surface |
| `resources/list` | 0 | `private` | conservative default for future corpus-backed resources |

Only current-era envelopes carry these fields. A binary upgrade naturally
invalidates process-local list caches. Every current-era successful result also
carries `resultType: "complete"` as required by the final schema.

### 6. Errors

Protocol errors are generated centrally with stable data:

- `-32020` Header mismatch
- `-32022` Unsupported protocol version, including a `supported` list
- `-32601` Method not found
- existing JSON parse and invalid-request errors remain standard JSON-RPC

Tool argument failures continue as tool execution errors so agents can
self-correct. No current AsDecided RPC requires a client capability, so the
server does not manufacture a `-32021` path without a real capability boundary.

### 7. Test topology

- Keep current legacy oracle fixtures unchanged.
- Add small language-neutral JSON request/response fixtures for discovery,
  revision selection, cache hints, and errors.
- Run stdio process tests and direct protocol-unit tests.
- Run HTTP socket tests for headers, statuses, audit gating, and parity.
- Add an optional official conformance runner job only after it can be pinned
  deterministically; local CI must not fetch moving protocol data.

## Constraints

- No new MCP tools or write authority.
- No engine-owned OAuth, authentication, sessions, Tasks, Apps, roots, or
  sampling.
- No external `$ref` dereferencing in tool schemas.
- No model call or probabilistic judge in validation.
- Legacy `2025-06-18` bytes stay frozen.
- HTTP remains mandatory-audit-on.

## Rationale

A protocol seam isolates breaking wire evolution from stable product
semantics. Dual-era support matches how official SDKs migrate while retaining
AsDecided's stronger deterministic contract. Small fixtures are reviewable and
can later move into `asdecided/spec` without coupling Rust CI to another
implementation.

## Alternatives

### Fork the whole request processor by protocol revision

This minimizes conditionals but duplicates tool dispatch, audit, freshness,
and serialization logic, making semantic drift likely.

### Replace the wire layer with a prerelease SDK

This delegates conformance but makes legacy byte parity and release stability
dependent on a larger moving dependency. Reconsider after the Rust SDK's
current-protocol release is stable and parity can be demonstrated.

### Emit current-era fields to every client

This is simpler but breaks the frozen legacy contract and can surprise clients
that validate older result schemas.

## Accessibility

Not applicable to this machine-to-machine surface. Human-facing errors remain
plain, actionable text and do not rely on color or terminal formatting.

## Style Guidance

- Keep protocol names and header casing identical to the MCP specification.
- Prefer explicit revision names over “latest”.
- Describe compatibility as verified behavior, never as SDK affiliation.
- Keep wire fixtures compact enough to review directly.

## Open Questions

- Whether the official MCP Rust SDK can replace the bespoke wire layer after
  its stable `2026-07-28` release without breaking legacy fixtures.
- Whether `tools/list` should eventually use a shorter TTL when dynamic
  extension tools exist; the current six-tool binary surface is static.

## Related Requirements

- asdecided-mcp-2026-protocol
- rac-mcp-http-transport
- rac-mcp-surface-budget

## Related Decisions

- adr-032
- adr-066
- adr-098
- adr-120
- adr-121

## Related Roadmaps

- lore-at-team-scale

## Related Tickets

- asdecided/core#389
