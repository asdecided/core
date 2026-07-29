# 20 — MCP `2026-07-28`: dual-era protocol contract

Scope: the current MCP wire revision served by `decided-mcp` over stdio and
HTTP. Addendum 10 remains authoritative for legacy stdio bytes; addendum 19
remains authoritative for legacy HTTP behavior. This addendum records the
current-era extension selected by ADR-121.

Normative upstream artifact: the immutable MCP JSON schema at
`modelcontextprotocol/modelcontextprotocol`, tag `2026-07-28`,
`schema/2026-07-28/schema.json`.

## 0 — Compatibility matrix

| Era | Revisions | Lifecycle | Identity |
| --- | --- | --- | --- |
| Legacy | `2024-11-05`, `2025-03-26`, `2025-06-18`, `2025-11-25` | `initialize` / `initialized` | pinned `lore` / `1.28.1` |
| Current | `2026-07-28` | `server/discover`; metadata on every request | `decided-mcp` / Cargo package version |

Legacy frames do not gain current-era fields. In particular, the
`2025-06-18` initialize and tools/list bytes in addendum 10 remain frozen.

## 1 — Required request metadata

Every current-era request carries:

```json
{
  "_meta": {
    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
    "io.modelcontextprotocol/clientCapabilities": {}
  }
}
```

`io.modelcontextprotocol/clientInfo` is accepted but not trusted and does not
change behavior. Missing or malformed required metadata is `-32602 Invalid
params`.

## 2 — Discovery

`server/discover` is mandatory and returns:

- `resultType: "complete"`
- `supportedVersions: ["2026-07-28"]`
- the tools, prompts, and resources capabilities AsDecided serves
- `serverInfo.name: "decided-mcp"` and the Cargo package version
- `ttlMs: 86400000`, `cacheScope: "public"`
- concise instructions describing the read-only grounding surface

The discovery result advertises per-request-metadata revisions. Legacy
initialize revisions remain available through the legacy fallback lifecycle
and are not mixed into `supportedVersions`.

## 3 — Successful results

Every current-era successful result has `resultType: "complete"`.

Cacheable lists additionally carry:

| Method | `ttlMs` | `cacheScope` |
| --- | ---: | --- |
| `tools/list` | `86400000` | `public` |
| `prompts/list` | `86400000` | `public` |
| `resources/list` | `0` | `private` |

The six tool definitions and their order are the same as the legacy pinned
surface. Their schemas are a valid JSON Schema 2020-12 subset; input roots
remain objects and external `$ref` values are forbidden.

Current `tools/call` results retain `content`, `structuredContent`, and
`isError`, with `resultType` added. Tool argument and execution failures remain
tool results so an agent can self-correct.

## 4 — Removed current-era methods

`initialize`, `ping`, and other methods absent from the `2026-07-28` client
request union return `-32601 Method not found`. Their legacy behavior remains
available only in the legacy era.

## 5 — HTTP headers and statuses

Every current-era HTTP POST requires:

- `MCP-Protocol-Version: 2026-07-28`
- `Mcp-Method: <JSON-RPC method>`
- `Mcp-Name: <params.name>` for `tools/call`

Header values are compared with the JSON-RPC body before dispatch. Missing or
conflicting values return HTTP 400 with `-32020 Header mismatch`. Unsupported
versions return HTTP 400 with `-32022 Unsupported protocol version`. Unknown
RPCs return HTTP 404 with `-32601 Method not found`.

No response carries `Mcp-Session-Id`; one request can be served by any process
instance.

## 6 — Verification

- `decided-mcp/tests/protocol_legacy.rs`: frozen initialize/list compatibility
- `decided-mcp/tests/protocol_2026.rs`: discovery, metadata, results, caching,
  errors, and schema subset
- `decided-mcp/tests/http_transport.rs`: current headers, statuses, discovery,
  and tool calls over a real TCP server

The full AsDecided corpus is validated separately. The official MCP conformance
runner may be pinned in CI after its current server scenarios cover a partial,
read-only server without requiring unadvertised capabilities.
