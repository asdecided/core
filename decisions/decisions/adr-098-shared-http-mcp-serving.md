---
schema_version: 1
id: RAC-KWMW45KXHZJP
type: decision
---
# ADR-098: Shared HTTP MCP Serving

## Status

Accepted

## Category

Architecture

## Context

Lore's MCP server is stdio-only: one process per developer against that
developer's own checkout (ADR-031, ADR-032). ADR-091's context reasons from
that model — "the MCP server is stdio-only … a hosted or shared server is a
non-goal" — to rule a Prometheus scrape endpoint out of the engine. But
ADR-080 records the opposite intent for the *serving* question: a shared,
`main`-backed read endpoint is blessed, "a transport feature, not a datastore".
The `lore-at-team-scale` roadmap graduated on a real trigger — an adopting
organisation expanding toward multi-thousand-seat scale — and its first
initiative (`rac-mcp-http-transport`) needs one always-current endpoint the
whole team can point every agent at, instead of individual checkouts that lag.

These two recorded positions must be reconciled deliberately, for everyone,
rather than by code drift (ADR-085): does adding an HTTP transport cross a
bright line, and if not, what keeps it from becoming the hosted, authenticated,
stateful service the red lines forbid? The MCP isolation battery also
deliberately forbids network imports outside the fenced `ping` module, so an
HTTP transport cannot land without an explicit, recorded revision of that
assertion.

## Decision

`rac mcp` gains a streamable HTTP transport, decided for everyone, under the
following boundaries.

- **A transport, not a datastore.** The HTTP server fronts one `main`-backed
  checkout and re-reads it per call (ADR-032); it holds no state beyond that
  checkout (ADR-080). It is served statelessly — no session store, one JSON
  response per request — so an HTTP response is payload-identical to stdio for
  identical corpus bytes (ADR-002). stdio stays the default: every existing
  `.mcp.json`, including ADR-088 profile output, is byte-unchanged.

- **This resolves ADR-091's premise, not its conclusion.** ADR-091's
  stdio-only *reasoning* is superseded for the serving question — a shared
  endpoint is now sanctioned intent (ADR-080) — but its *conclusion* stands:
  the engine still ships no Prometheus `/metrics` scrape endpoint. A scrape
  surface, if ever justified, remains the deployment wrapper's concern, never
  the engine's.

- **No authentication in the engine (ADR-085).** The HTTP transport grows no
  authentication, authorization, SSO, or RBAC. Identity stays the attributable
  principal (ADR-084), not a verified one; authentication belongs to the
  deployment proxy that fronts the endpoint. The standing red lines — no SSO on
  a shared MCP, no RBAC on MCP tools, no hosted multi-tenant service — stand.

- **The attribution carrier is the `X-AsDecided-Principal` request header.** A caller
  on the shared endpoint asserts who it is by setting the `X-AsDecided-Principal`
  HTTP header (case-insensitive); the audit recorder records that assertion as
  the per-request principal (`rac-shared-server-audit-identity`). It is
  *attribution, not authentication* (ADR-084): the engine records what the
  caller claimed and never verifies it, and the principal is never an
  access-control input — tool responses are identical whatever the header says.
  Verifying the claim, if a deployment needs it, is the fronting proxy's job
  (ADR-085): the proxy authenticates and sets the header it trusts. An unasserted
  call falls back to the recorder's resolution, never silently borrowing the host
  process's own git identity as if it were the caller's.

- **Mandatory audit-on for HTTP.** A shared endpoint serves reads no single
  developer's git identity can attribute, so HTTP serving refuses to start
  without a working audit sink (ADR-084's fail-loud posture): the audit log
  must be enabled and writable, proven at startup, or the server exits
  non-zero. stdio is unchanged — audit stays config-driven and default-absent
  there.

- **The isolation battery is revised, scoped to the transport layer.** The
  network-import allowance widens from the `ping` module alone to `ping` plus a
  fenced `transport` module; every other module, including all tool logic,
  stays network-import-free, and the battery still fails if a tool-logic module
  imports a network module. Keeping the checkout current with `main` — a merge
  webhook or periodic pull — is an external concern, never engine code.

## Consequences

### Positive

- A team points every agent at one always-current endpoint and gets the same
  answer for the same query at a given moment, without the engine becoming a
  hosted service or a database.
- The stdio-versus-shared tension between ADR-091 and ADR-080 is resolved on
  the record, so the next reader sees a decision rather than a contradiction.
- The trust story holds at shared scale: reads are attributable by mandatory
  audit, authentication is honestly the proxy's job, and the network surface is
  still answerable by reading two files.

### Negative

- Operators must front the endpoint with their own authenticating proxy; the
  engine offers no turnkey auth, by design.
- HTTP serving has a hard precondition (a working audit sink) that stdio does
  not, so a misconfigured shared deployment fails to start rather than serving.

### Risks

- The endpoint is mistaken for an authenticated surface. Mitigation: the
  proxy-owned authentication posture is documented honestly and the ADR-085 red
  lines are restated with the transport.
- The isolation-battery relaxation leaks network I/O into tool logic.
  Mitigation: the allowance is scoped to the fenced `transport` module and the
  battery retains a tool-logic-modules-network-free assertion.
- A shared server drifts from `main` if the keep-current step fails.
  Mitigation: the server re-reads per call (ADR-032), so staleness is bounded
  and observable, and the keep-current step lives in the deployment recipe.

## Alternatives Considered

### Keep the engine stdio-only and require a per-developer sidecar

Leave `rac mcp` stdio-only and tell teams to run a local proxy that bridges to
HTTP.

#### Disadvantages

- Pushes the same streamable-HTTP machinery into every deployment as bespoke
  glue, forks the serving story, and still leaves ADR-091 and ADR-080
  unreconciled. ADR-080 already blessed the shared server as engine intent.

### Ship HTTP with built-in token authentication

Add an auth layer to the HTTP transport so the endpoint is self-protecting.

#### Disadvantages

- Crosses ADR-085's bright line (no SSO/RBAC/credential handling in the
  engine), forks the trust model, and duplicates what a deployment proxy does
  better. Authentication belongs to the proxy.

A stateless, unauthenticated, mandatory-audit-on HTTP transport, with
authentication delegated to the deployment proxy, is selected.

## Relationship to Other Decisions

- ADR-080: records the shared `main`-backed server as blessed intent — "a
  transport feature, not a datastore" — which this ADR implements.
- ADR-091: its stdio-only premise is resolved for the serving question; its
  no-engine-scrape-endpoint conclusion stands.
- ADR-032: the stateless, per-call re-read contract the HTTP transport
  preserves.
- ADR-084: the read-access audit recorder HTTP serving makes mandatory.
- ADR-085: enterprise capability is configuration for everyone, never a mode;
  the red lines this ADR restates.
- ADR-002: deterministic, offline, byte-identical output across transports.
- ADR-033: the response budget the transport does not change.

## Related Requirements

- rac-mcp-http-transport

## Related Roadmaps

- lore-at-team-scale
