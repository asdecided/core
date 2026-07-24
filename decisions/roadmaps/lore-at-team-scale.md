---
schema_version: 1
id: RAC-KVSTYENH8X0S
type: roadmap
---
# RAC — Lore at Team Scale

## Status

Achieved

Delivered across all four initiatives (epic itsthelore/asdecided-core#262). `rac mcp`
gained a streamable HTTP transport so a team points every agent at one
always-current `main`-backed endpoint (#263, under the serving decision ADR-098);
an opt-in, content-addressed derived-index cache keeps per-call latency flat as
the corpus grows, byte-identical to the uncached path (#264, ADR-099); the audit
recorder resolves a per-request principal from the `X-Lore-Principal` header so
every caller on the shared endpoint is attributed distinctly, never as the host
(#265); and an operator recipe — container, authenticating proxy, keep-current
step, and the observability boundary — documents how to run it (#266). Both
substrate entry conditions held throughout: **mandatory audit-on for shared
deployments** (HTTP refuses to start without a working sink and blocks on write
failure) and **cache coherency guaranteed as byte-parity with the uncached
path**. No database, no hosted service, no authentication in the engine — servers
and caches over git `main`, exactly as scoped. Execution was tracked in GitHub
(ADR-093), a sub-issue per initiative.

The recorded entry trigger had been met: an adopting organisation expanding Lore
toward multi-thousand-seat scale, far past the 50+ developer signal this artifact
named. Per this artifact's own graduation clause and the deterministic-substrate
programme's deferred list, it graduated out of `future/` as a live scoped
roadmap.

## Context

Lore is built as files-in-git: one canonical corpus on `main`, served
read-only and re-read per call (ADR-032), with no database (ADR-080). That
model is correct and keeps consistency where git already provides it. Three
questions appear only at team scale, and all have answers that are *servers
and caches, never a database*:

- **Consistency.** In the default topology each developer runs a local
  `rac mcp` against their own checkout, so two developers can be on
  different commits until they pull. A team may want every agent to call
  one always-current source of truth instead of individual copies.
- **Performance.** As a corpus grows into the thousands of artifacts,
  re-reading and re-indexing on every call starts to cost real latency,
  which ADR-032 explicitly defers optimising "behind the corpus-snapshot
  seam" until a real user reports it. That report has now effectively
  arrived with the organisation-scale rollout.
- **Attribution.** The audit recorder resolves one principal at recorder
  construction and stamps every event with it — correct for a
  single-developer process, wrong for a shared endpoint, where every
  caller would be recorded as the host identity. A shared server needs
  per-request attribution before ADR-084's audit trail means anything.

One recorded tension must be resolved deliberately rather than by code
drift: ADR-091's context reasons from "the MCP server is stdio-only … a
hosted or shared server is a non-goal", while ADR-080 and this artifact
record the shared `main`-backed server as blessed intent — "a transport
feature, not a datastore". The serving initiative below therefore ships
under its own ADR, decided for everyone (ADR-085), never silently.

## Outcomes

- A team can point every developer's agent at one always-current endpoint
  that reflects `main`, so reads come from a single source of truth rather
  than individual checkouts that lag.
- Per-call latency stays acceptable as the corpus grows large, without
  changing the determinism or freshness contract.
- Every call on a shared server is attributed to a per-request principal
  in the audit trail, preserving ADR-084's attributable-not-authenticated
  posture at shared scale.
- An operator can deploy the whole topology from a recipe — container,
  keep-current step, read-only posture — with no secrets handling and no
  authentication code in the engine.
- No database is introduced: git stays the system of record (ADR-080).

## Initiatives

### Initiative 1 — Shared HTTP MCP server (`rac-mcp-http-transport`)

`rac mcp` gains an HTTP (streamable) transport so a single instance,
fronting an auto-updated `main` checkout, serves the whole team from one
endpoint. It stays read-only and stateless per call (ADR-032); a merge
webhook or periodic pull keeps the checkout current, outside the engine.
The capability ships under its own serving ADR, which resolves the ADR-091
stdio-only premise against ADR-080's recorded shared-server intent,
sanctions the explicit revision of the MCP isolation battery, and records
the no-authentication-in-engine posture: identity stays the attributable
principal, and authentication belongs to the deployment proxy (ADR-085).

### Initiative 2 — Derived-index cache (`rac-derived-index-cache`)

A content-addressed, rebuild-on-change persistence of the derived search
structures and relationship graph, so per-call work stops scaling with
corpus size: derived once, reused while the corpus content-hash is
unchanged, invalidated by any byte change. It is a rebuildable cache, not
a store — "files are truth, the index is disposable" (ADR-080). The
capability ships under its own ADR answering ADR-032's recorded review
clause; the entry condition is byte-parity — cached output identical to
the uncached path on every fixture.

### Initiative 3 — Shared-server audit identity (`rac-shared-server-audit-identity`)

Per-request principal threading through the audit recorder, so concurrent
callers on a shared endpoint are recorded as distinct, attributable
principals rather than the host identity. The entry condition holds:
shared deployments are mandatory audit-on, failing loudly per ADR-084's
`on_write_error: block` posture.

### Initiative 4 — Operate-it documentation

A deployment recipe for the shared server (container, the keep-current
webhook or periodic pull, read-only posture, no secrets), guidance on when
a team needs it versus the local-clone default, and the observability
story within ADR-091's boundary: structured logs belong to the engine; any
metrics scrape endpoint belongs to the deployment wrapper, never the
engine.

## Constraints

- No database as a system of record; files-in-git stay canonical (ADR-080).
- Read-only serving; knowledge still changes only by PR to `main`
  (ADR-065).
- The determinism and freshness contract of ADR-032 holds for both the
  shared server and the cache: identical corpus bytes and identical input
  produce identical output, and no call can observe stale state.
- Transport and cache changes ride new ADRs this roadmap schedules; no
  settled decision is bypassed by code (ADR-032, ADR-091, ADR-085).
- No SSO, RBAC, or hosted multi-tenant service — the recorded red lines
  stand (ADR-085); audit identity is attributable, not authenticated
  (ADR-084), and mandatory-on for shared deployments.
- The MCP tool surface and response budget are unchanged (ADR-033).

## Non-Goals

- SSO, RBAC, tool-level authorization, or any credential handling in the
  engine (ADR-085).
- A database, vector store, or any persistent system of record other than
  git (ADR-080).
- Embeddings or semantic indexing (ADR-066).
- Write-through the server: no write path exists over any transport
  (ADR-065).
- A metrics scrape endpoint inside the engine (ADR-091).
- Corpus federation — the sibling `corpus-federation` roadmap owns it.

## Success Measures

- With the shared server, every team agent returns the same answer for the
  same query at a given moment, because all read one `main`-backed
  checkout; HTTP and stdio responses are payload-identical for identical
  corpus bytes.
- With the cache enabled, output is byte-identical to the uncached path on
  golden fixtures, repeated reads of an unchanged large corpus skip
  re-indexing with a measured latency floor, and a single byte change
  invalidates and rebuilds.
- Two concurrent clients on a shared server are recorded as two distinct
  principals in the audit trail; a shared server without a working audit
  sink refuses to start.
- No new datastore appears in the deployment: the only moving parts are a
  git checkout, a stateless reader, and a disposable derived index.
- `rac validate rac/`, `rac relationships rac/ --validate`, and
  `rac review rac/` stay clean across the roadmap's output.

## Assumptions

- Teams that want a single live endpoint are a real segment; the
  local-clone default remains correct for everyone else.
- The installed MCP SDK's HTTP transport support is sufficient; no
  transport machinery needs to be written from scratch.
- The three implementing ADRs (serving, cache, and the isolation-battery
  revision the serving ADR sanctions) are authored and ratified before
  their capabilities ship; this roadmap schedules them and does not
  pre-decide them.

## Risks

- A shared server drifts from `main` if the keep-current step fails.
  Mitigation: the server re-reads per call (ADR-032) and the pull is
  driven by merge events, so staleness is bounded and observable.
- A persistent cache serves stale results if invalidation is wrong.
  Mitigation: content-addressed keying — any byte change to the corpus
  changes the key and forces a rebuild — with byte-parity to the uncached
  path asserted in CI.
- Relaxing the MCP isolation battery leaks network I/O into tool logic.
  Mitigation: the revision is an explicit, ADR-backed act; the revised
  battery still forbids network imports in tool modules, with the
  allowance scoped to the transport layer only.
- Asserted principals are mistaken for authenticated identity.
  Mitigation: the attributable-not-authenticated posture is documented
  honestly (ADR-084, ADR-085), and shared-server audit records carry an
  additive transport field so an auditor can tell the difference.

## Related Decisions

- adr-001
- adr-002
- adr-032
- adr-033
- adr-065
- adr-066
- adr-073
- adr-080
- adr-084
- adr-085
- adr-091
- adr-093
- adr-094
- adr-098
- adr-099

## Related Roadmaps

- deterministic-substrate
- corpus-federation
- corpus-sync

## Related Requirements

- rac-mcp-http-transport
- rac-derived-index-cache
- rac-shared-server-audit-identity

## Related Tickets

- itsthelore/asdecided-core#262
