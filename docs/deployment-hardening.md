# Deployment Hardening

The native HTTP transport is a read-only, unauthenticated server for one
checkout. It is loopback-only by default. A non-loopback bind is a deliberate
deployment choice: `decided-mcp` requires the explicit `--behind-proxy` flag as
an acknowledgement that an authenticating TLS proxy fronts the process.

The flag is **not** authentication, encryption, or an authorization policy. It
only prevents an accidental `0.0.0.0` launch from looking like a safe default.
The engine still follows the proxy-owned authentication boundary in
[ADR-085](https://github.com/asdecided/core/blob/main/decisions/decisions/adr-085-enterprise-configuration-not-mode.md)
and the shared-server contract in
[ADR-098](https://github.com/asdecided/core/blob/main/decisions/decisions/adr-098-shared-http-mcp-serving.md).

## Preflight checklist

Before exposing a shared endpoint, verify every item:

- [ ] **Private engine network:** bind the engine only on a private interface
  reachable by the proxy. Never publish the engine port directly to the
  internet.
- [ ] **Explicit acknowledgement:** use `--behind-proxy` for a non-loopback
  host. Keep the default loopback bind for local or single-user use.
- [ ] **TLS at the edge:** terminate TLS at the proxy; redirect or reject
  plaintext public traffic. The engine has no TLS or certificate store.
- [ ] **Authentication at the edge:** authenticate callers at the proxy and
  overwrite `X-AsDecided-Principal` with the identity it verified. Strip any
  client-supplied copy before proxying; the engine records attribution but does
  not authenticate it.
- [ ] **Origin and rate controls:** apply an allowlist for browser origins and
  rate/connection limits at the proxy. The engine's request and connection
  caps are a second boundary, not a replacement for edge policy.
- [ ] **Read-only corpus:** mount the checkout read-only into the engine
  container. Only a reviewed pull to `main` may change knowledge; keep-current
  pulls belong in a separate sidecar or job.
- [ ] **Audit is durable:** enable the committed `audit:` stanza, place the
  JSONL on a writable persistent volume, and fail deployment if the sink is
  unavailable. Rotate and ship it with the organisation's normal log system.
- [ ] **No credentials in the engine:** keep proxy credentials, registry tokens,
  and collector credentials out of the corpus container and its environment.
- [ ] **Operational logs:** collect stderr from `decided-mcp` and the proxy;
  keep the audit stream and edge access logs together for incident review.
- [ ] **Freshness is observable:** monitor the checkout update job and alert if
  the server is serving an old `main` revision. The engine cannot pull or
  authenticate a git remote for you.

## Reference launch

This is the shape for a private container network with a proxy in front:

```bash
decided-mcp --root /corpus \
  --transport http \
  --host 0.0.0.0 \
  --behind-proxy \
  --port 8000 \
  --path /mcp
```

The proxy should be the only public listener. It terminates TLS, authenticates
the caller, overwrites `X-AsDecided-Principal`, applies origin/rate policy, and
forwards only the MCP path to the private engine address. See the
[shared-server recipe](shared-server.md) for the checkout, audit, and proxy
topology.

## What this checklist does not change

The engine remains read-only and stateless. It does not grow SSO, RBAC,
credential handling, a `/metrics` endpoint, or a content trust verdict. Those
boundaries are intentional; the deployment wrapper supplies the controls that
belong at the edge.
