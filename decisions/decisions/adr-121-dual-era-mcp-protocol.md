---
schema_version: 1
id: RAC-MCP20260728A
type: decision
---
# ADR-121: Dual-Era MCP Protocol Compatibility

## Status

Accepted

## Category

Architecture

## Context

AsDecided `v0.24.1` implements MCP's initialize-based lifecycle through
protocol revision `2025-06-18`. MCP `2026-07-28` is a breaking revision: it
removes the initialize handshake from the new era, moves version and client
metadata onto each request, requires `server/discover`, standardizes HTTP
routing headers, and adds caching hints to cacheable results.

The existing native server is already stateless at the application boundary.
Its read-only tools, per-call corpus freshness, and session-free HTTP
implementation match the new protocol's architectural direction. Its wire
contract does not: a strict `2026-07-28` client cannot discover the server, and
the current generic unknown-method response is not a conforming downgrade
signal.

AsDecided also has installed clients that still use the legacy lifecycle.
Replacing that lifecycle outright would turn a protocol upgrade into an
avoidable compatibility break.

## Decision

`decided-mcp` supports two explicit MCP protocol eras from one deterministic
request processor.

- **Legacy era.** Initialize-based clients continue to negotiate
  `2024-11-05`, `2025-03-26`, `2025-06-18`, or `2025-11-25`. Existing
  `2025-06-18` fixtures remain authoritative for legacy byte compatibility.
- **Current era.** Clients select `2026-07-28` through per-request protocol
  metadata. The server implements `server/discover`; no initialize handshake
  or MCP session identifier is used for this era.
- **Transport selects framing, not semantics.** stdio reads the protocol
  revision from request `_meta`; HTTP reads and cross-checks the
  `MCP-Protocol-Version`, `Mcp-Method`, and applicable `Mcp-Name` headers.
  Both transports dispatch to the same read-only tool implementation.
- **Conforming failure enables safe fallback.** Unsupported versions, missing
  required metadata, header/body mismatch, and unknown methods use the
  revision-defined error codes and HTTP statuses. The server never silently
  claims a newer revision while serving legacy framing.
- **Cache hints describe the existing truth.** Static tool, prompt, and
  resource lists carry explicit `ttlMs` and `cacheScope` values in
  `2026-07-28`; legacy result bytes stay unchanged.
- **The surface does not expand.** Tasks, Apps, roots, sampling, write tools,
  and engine-owned authentication remain out of scope. JSON Schema 2020-12
  acceptance broadens what a valid tool schema may express without adding a
  tool or increasing response authority.
- **Protocol identity stops inheriting Python SDK identity.** The current-era
  discovery response identifies `decided-mcp` with the AsDecided package
  version. The historical `lore` / `1.28.1` identity remains only inside the
  pinned legacy compatibility response.

## Consequences

### Positive

- Strict current clients can use AsDecided without a downgrade, while older
  clients remain supported.
- The stateless design chosen in ADR-032 and ADR-098 becomes protocol-native
  rather than merely implementation-local.
- HTTP gateways can route and audit calls from standard headers without
  inspecting JSON bodies.
- Compatibility is an explicit matrix with fixtures and conformance tests,
  not an accidental property of one client SDK.

### Negative

- The server carries two lifecycle paths and revision-aware response framing.
- Legacy byte parity prevents immediately removing historical SDK-shaped
  identity and list payloads.
- HTTP validation becomes stricter for current-era callers.

### Risks

- A shared dispatcher could accidentally add current-era fields to legacy
  responses. Mitigation: immutable legacy fixtures and byte-for-byte tests.
- A client could send conflicting protocol metadata and HTTP headers.
  Mitigation: reject disagreement deterministically before tool dispatch.
- Upstream protocol interpretation could drift. Mitigation: vendor small,
  language-neutral contract fixtures and run the official MCP conformance
  scenarios where supported.

## Alternatives Considered

### Replace the legacy lifecycle with `2026-07-28`

This is simpler internally but breaks installed clients and contradicts the
published compatibility contract.

### Stay on `2025-06-18` and rely on client fallback

Fallback is not guaranteed for clients pinned to the current revision, and
AsDecided's current unknown-method and HTTP status behavior is not a reliable
conforming fallback signal.

### Adopt an MCP SDK as the Rust server implementation

An SDK may eventually reduce protocol maintenance, but replacing the
dependency-free wire layer during a breaking protocol transition would combine
two risks and threaten the deterministic legacy contract. The protocol seam
remains small enough to implement and test directly.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "Both protocol eras and current discovery have stable implementation and test anchors."
rules:
  - id: current-protocol-keeps-discovery
    kind: require_pattern
    path_glob: "rust/decided-mcp/src/protocol.rs"
    pattern: '"server/discover"'
    message: "The current MCP era must retain server discovery."
  - id: current-protocol-keeps-contract-tests
    kind: require_pattern
    path_glob: "rust/decided-mcp/tests/protocol_2026.rs"
    pattern: 'current_client_discovers_native_server_without_initialize'
    message: "The MCP 2026 discovery contract must remain pinned."
  - id: legacy-protocol-keeps-byte-tests
    kind: require_pattern
    path_glob: "rust/decided-mcp/tests/protocol_legacy.rs"
    pattern: 'legacy_2025_06_initialize_bytes_stay_pinned'
    message: "The legacy MCP byte-compatibility fixture must remain pinned."
```

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

## Related Requirements

- asdecided-mcp-2026-protocol

## Related Designs

- mcp-2026-dual-era-wire-contract
