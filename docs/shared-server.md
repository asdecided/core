# Operating a Shared AsDecided Server

By default AsDecided runs as one `decided-mcp` process per developer, over stdio, against
that developer's own checkout. That is the right model for almost everyone and
needs no operations at all.

At organisation scale a team may instead want **one always-current endpoint**
every agent queries, so reads come from a single `main`-backed source of truth
rather than checkouts that lag between pulls. This page is the recipe for
standing that up: a container running the HTTP transport, an authenticating
proxy in front, a step that keeps the checkout current with `main`, and where
observability lives. It assumes you have read the [MCP Server](mcp.md) page.

Everything here is deployment wrapper — containers, proxies, collectors — around
an unchanged engine. AsDecided gains no hosted service, no database, and no
authentication code; git stays the source of truth
([ADR-080](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-080-single-source-of-truth-git-not-database.md)).

## 1. Do you need it?

You probably do **not**. The local-clone default is correct whenever developers
already clone the knowledge repo, because git keeps a team consistent the same
way it keeps a codebase consistent: a checkout is either current with `main` or
a `git pull` behind it, never authoritative-and-divergent. A stale answer is
resolved exactly as stale code is — pull.

Reach for a shared server when:

- you want **every agent to read one always-current endpoint** rather than each
  developer's own checkout, or
- callers cannot or should not each hold a full clone, or
- you want **one place** where read access is audited per caller.

If none of these apply, stay on the per-developer stdio server — it is simpler,
needs no proxy, and has no operational surface.

## 2. The shape

```
 agents ──TLS──▶  authenticating proxy  ──HTTP──▶  decided-mcp --transport http
                  (terminates TLS,                 (read-only, stateless,
                   authenticates the caller,        mandatory audit-on,
                   sets X-AsDecided-Principal)           serves one main checkout)
                                                          ▲
                                        keep-current  ────┘
                                        (merge webhook or periodic git pull)
```

Three moving parts, and not one of them is a database:

- a **git checkout** of your knowledge repo's `main` branch,
- a **stateless reader** (`decided-mcp` over HTTP), and
- an **authenticating proxy** that the engine never knows about.

## 3. The container

Run the HTTP transport against a read-only checkout. The disposable
[derived-index cache](mcp.md#derived-index-cache) is on by default (ADR-112),
so per-call latency stays flat as the corpus grows; `--cache` remains accepted
as an explicit affirmation.

```bash
docker run --rm --entrypoint decided-mcp -p 8000:8000 -v "$PWD:/corpus:ro" \
  ghcr.io/itsthelore/asdecided-core:latest \
  --root /corpus --transport http \
  --host 0.0.0.0 --port 8000
```

HTTP serving is **mandatory audit-on**: the server refuses to start without a
working audit sink. Enable it in the corpus's `.decided/config.yaml` (committed, so
an auditor has one git-diffable artifact) — see
[the audit section](mcp.md#8-read-access-audit-log-enterprise-opt-in):

```yaml
audit:
  enabled: true
  path: /var/log/lore/audit.jsonl   # a writable volume; on_write_error blocks on HTTP
```

No secrets belong in this container: it holds no credentials, terminates no TLS,
and authenticates nobody. Bind it to the proxy's network, never to the public
internet.

## 4. The authenticating proxy

The engine does not authenticate — by design
([ADR-085](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-085-enterprise-configuration-not-mode.md)).
Put a reverse proxy in front to terminate TLS, authenticate the caller, and
assert the caller's identity to the audit log via the **`X-AsDecided-Principal`**
header. The engine records that header as the per-request principal
([ADR-098](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-098-shared-http-mcp-serving.md)).

```nginx
location /mcp {
    auth_request /_authn;                              # your authenticator
    auth_request_set $principal $upstream_http_x_user; # identity it resolved

    # Overwrite the header from the *authenticated* identity. This also strips
    # any client-supplied X-AsDecided-Principal, so a caller cannot forge another's
    # identity in the audit log.
    proxy_set_header X-AsDecided-Principal $principal;

    proxy_pass http://lore:8000/mcp;
}
```

The critical line is the last `proxy_set_header`: the proxy must **overwrite**
`X-AsDecided-Principal` with the identity it authenticated, never pass through a
client-supplied value. Attribution in AsDecided is *attributable, not
authenticated* — the engine records what it is told and never verifies it, so
the trust of the header is exactly the trust of the proxy that sets it. If you
run the endpoint with no authenticating proxy, principals are self-asserted and
the audit log records claims, not verified identities.

## 5. Keeping the checkout current with `main`

The server re-reads the corpus on every call, so it is only ever as fresh as the
checkout it fronts. Keeping that checkout current with `main` is a deployment
concern outside the engine (ADR-080). Two shapes work:

- **Merge webhook** — your git host calls a small endpoint on merge to `main`
  that runs `git -C /corpus pull --ff-only`. Fresh within seconds of a merge.
- **Periodic pull** — a sidecar that pulls on an interval:

  ```bash
  while true; do
    git -C /corpus pull --ff-only origin main
    sleep 60
  done
  ```

Because the server re-reads per call, staleness is bounded by the pull interval
and is observable, never silent. Mount the checkout read-only into the server
container: knowledge changes only by pull from `main`, never by the server.

## 6. Observability, and the engine boundary

Observability for the shared server lives in the **wrapper**, not the engine
([ADR-091](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-091-engine-observability-boundary.md)):

- **Engine diagnostics go to stderr.** `decided-mcp` writes startup and operational
  notices to stderr (stdout is the protocol channel). Collect the container's
  stderr with your normal log pipeline.
- **The audit log is your read-access record.** It is a local JSONL file by
  design; shipping it to Loki, S3, or Elastic is a collector's job, never the
  engine's ([ADR-084](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-084-read-access-audit-recorder.md),
  [ADR-073](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-073-backend-connectors-consolidate.md)).
  Tail the audit volume with a sidecar and forward it.
- **A metrics scrape endpoint belongs to the wrapper, not the engine.** The
  engine ships no Prometheus `/metrics` surface. If you need scrape metrics,
  expose them from the proxy or a sidecar (request counts, latencies, and status
  codes all live there); the engine stays a stateless reader.
- **Error reporting is bring-your-own.** There is no AsDecided-hosted sink; supply
  your own DSN if you want error aggregation (ADR-091).

## 7. What the engine will never do

The shared server is a transport in front of the same engine, so its red lines
stand (ADR-085):

- **No SSO, RBAC, tool-level authorization, or user store.** The principal is an
  attribution string in the audit record, never an access-control input — tool
  responses are identical whatever the header says. Authorisation, if you need
  it, is the proxy's (allow/deny at the edge).
- **No database or hosted multi-tenant service.** Git `main` is the single
  source of truth; the server is one reader of it (ADR-080).
- **No write path.** Knowledge changes only by pull request to `main` behind
  human review ([ADR-065](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-065-artifact-content-untrusted.md));
  no transport exposes a write.

These are not limitations to work around — they are the properties that let one
engine serve the solo developer and the regulated enterprise unchanged.
