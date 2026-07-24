---
schema_version: 1
id: RAC-KWJ8RYW0ADY7
type: requirement
---
# Requirement: MCP HTTP Transport

## Status

Accepted

Classification: `[internal]` — one always-current endpoint for the whole
team. Initiative 1 of the `lore-at-team-scale` roadmap: an HTTP transport
for `rac mcp`, shipped under its own serving ADR. Delivered
(itsthelore/asdecided-core#263): `rac mcp --transport http` serves the five tools
statelessly over streamable HTTP, payload-identical to stdio, mandatory
audit-on, with authentication delegated to the deployment proxy — under
ADR-098, which resolves the ADR-091 stdio-only premise against ADR-080's
recorded shared-server intent.

## Problem

`rac mcp` is stdio-only: the transport is hardcoded at the single run
call, the CLI exposes no transport, host, or port options, and the MCP
isolation battery deliberately forbids network imports outside the fenced
ping module. A team that wants every agent to read one always-current
`main`-backed endpoint — the topology ADR-080 records as blessed intent —
has no way to run it. The installed MCP SDK already supports a streamable
HTTP server transport; what is missing is the deliberate, ADR-backed act
of adopting it without eroding the read-only, stateless, no-auth engine
posture.

## Requirements

- [REQ-001] `rac mcp` MUST gain an HTTP (streamable) transport selected by explicit options (`--transport`, `--host`, `--port`, `--path`) using the installed MCP SDK's transport support, with stdio remaining the default so every existing `.mcp.json` — including ADR-088 profile output — is byte-unchanged.
- [REQ-002] The transport MUST be serving-layer only: the server builder and all five tools keep the read-only, per-call re-read contract (ADR-032); this requirement introduces no caching, sessions, or server-held state.
- [REQ-003] The capability MUST ship under its own serving ADR that resolves the ADR-091 stdio-only premise against ADR-080's recorded shared-server intent — decided for everyone per ADR-085, never silently; this requirement does not pre-decide that ADR.
- [REQ-004] The engine MUST NOT grow authentication, authorization, SSO, or RBAC (ADR-085): identity stays the attributable principal (ADR-084), and authentication belongs to the deployment proxy, documented as such.
- [REQ-005] The MCP isolation battery MUST be revised as an explicit, ADR-backed act: the network-import allowance is scoped to the transport layer only, and tool-logic modules remain network-import-free under the revised assertion.
- [REQ-006] For identical corpus bytes and identical tool input, responses over HTTP MUST be payload-identical to stdio, asserted by a shared-fixture parity test (ADR-002, ADR-032).
- [REQ-007] HTTP serving MUST enforce the mandatory audit-on entry condition: starting the HTTP transport without a working audit sink fails loudly, composing with `rac-shared-server-audit-identity` and ADR-084's fail-loud posture.
- [REQ-008] The server MUST hold no state beyond the checkout it fronts (ADR-080); keeping that checkout current with `main` is an external concern — a merge webhook or periodic pull — documented in the operate-it initiative, never engine code.

## Acceptance Criteria

- `rac mcp --transport http` on a fixture corpus answers all five tools
  with payloads identical to the stdio transport for the same queries.
- Bare `rac mcp` still speaks stdio, and ADR-088 profile `.mcp.json`
  output is byte-identical to today.
- The revised isolation battery passes, and still fails when a tool-logic
  module imports a network module.
- HTTP startup without a configured audit sink exits non-zero with an
  actionable message; stdio startup is unchanged.
- No credential-handling code path exists in the engine, asserted by the
  battery.

## Success Metrics

- A team points every agent at one endpoint and gets the same answer for
  the same query at a given moment, with the endpoint's freshness bounded
  by the keep-current step rather than individual pulls.

## Risks

- The isolation-battery relaxation leaks network I/O into tool logic.
  Mitigation: REQ-005 scopes the allowance to the transport layer and
  keeps the tool-module assertion.
- The endpoint is mistaken for an authenticated surface. Mitigation:
  REQ-004 documents the proxy-owned authentication posture honestly;
  ADR-085's red lines are restated in the docs.

## Assumptions

- The installed MCP SDK's streamable HTTP transport is sufficient; no
  bespoke transport machinery is needed.
- The serving ADR is authored and ratified before the transport ships;
  the roadmap schedules it.

## Related Decisions

- adr-002
- adr-032
- adr-033
- adr-065
- adr-080
- adr-084
- adr-085
- adr-091
- adr-098

## Related Roadmaps

- lore-at-team-scale

## Related Requirements

- rac-shared-server-audit-identity
- rac-derived-index-cache
