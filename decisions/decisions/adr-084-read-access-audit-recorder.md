---
schema_version: 1
id: RAC-KW2YW6XK593X
type: decision
---
# ADR-084: Read-Access Audit Recorder

## Status

Accepted

## Category

Product

## Context

The corpus records *what was decided*; nothing records *who consulted which
decision, and when*. Regulators ask exactly that — "did X have access to
decision D at the time of change Y" — and the MCP query log is the missing half
of the audit trail. ADR-040 telemetry answers none of it by design: it is
content-free, recording no query text and no artifact IDs.

The recorded constraints pull against each other. ADR-040 and ADR-041 pin
telemetry to record no content and fence the only network import to
`mcp/ping.py`. ADR-032 makes tool output a pure function of repository bytes and
tool input. ADR-002 keeps the engine offline and deterministic. ADR-065 and
ADR-077 locate the trust boundary at repository ACL plus human pull-request
review, not tool-level authentication — which is also the position an enterprise
adopter arrives at independently ("no SSO or RBAC on the MCP; ACL the repo,
audit is the answer").

An audit trail must record precisely the content — queries and returned IDs —
that telemetry is pinned never to touch. The decision is the shape of a
content-bearing recorder that can sit beside a content-free engine without
becoming a second operating posture.

## Decision

A read-access audit recorder ships as a sibling to `mcp/telemetry.py`:
content-bearing by design, default-absent, local-only, and persistently enabled.

- **Default-absent.** With no `audit:` stanza (the default), no audit file is
  created and the default engine's content-free guarantee is byte-for-byte
  intact. This strict-superset property is the load-bearing clause: it is what
  keeps an enterprise deployment a configuration of the same engine, not a
  second posture.
- **Scope: the MCP read tools only** — `get_artifact`, `search_artifacts`,
  `find_decisions`, `get_related`, `get_summary`. CLI reads are out of scope:
  they are local, repository-ACL-bounded, and already covered by shell history.
- **Record: one JSON line per call**, with a pinned schema — `schema_version`,
  `ts` (ISO 8601 UTC), `session` (per-process hex), `principal`, `tool`,
  `query` (the read's arguments verbatim: id, query string, topic, or depth),
  `returned` (the list of artifact references, each with an `id`, a `resolved`
  flag, and a body-free `provenance.path` reference as pinned by ADR-127),
  `outcome`, and `duration_ms`. Artifact bodies and any repository content
  beyond the returned references are never recorded. Adding a field is a
  recorded decision, not a patch.
- **Enablement: an `audit:` stanza in `.rac/config.yaml`** — `enabled: true`, an
  optional `path` (default `$XDG_STATE_HOME/rac/audit.jsonl`, redirectable via
  `RAC_AUDIT_PATH` for residency), and `on_write_error` (`warn` | `block`,
  default `warn`). The stanza is committed, team-wide, deterministic, and
  git-diffable: the single artifact an auditor points at. Unlike ADR-040's
  per-invocation flag, audit is persistent by design — an audit log that can be
  silently turned off per call is not an audit log. When enabled, `rac mcp`
  announces on stderr what is recorded and where.
- **Identity: attributable, not authenticated.** The principal defaults to the
  git `user.email` and `user.name`, and is overridable via `RAC_AUDIT_PRINCIPAL`
  for CI or service contexts. The log records who *claimed* to query; the
  enforced boundary remains repository ACL plus pull-request review (ADR-065,
  ADR-077). This is recorded as attribution, never sold as authentication.
- **Local-only, no network.** The audit module imports no network code; the
  isolation battery is extended to forbid `urllib` and `socket` imports outside
  `mcp/ping.py` and within the audit module specifically. Shipping events to a
  sink (Loki, S3, Elastic) is a `lore-audit` satellite that tails the JSONL,
  never the engine.
- **Write-only, outside the request/response contract** (ADR-032): the log
  never feeds a response, and payload-stability tests compare responses
  byte-for-byte with and without the recorder. Unlike telemetry, a write failure
  is fail-loud rather than silently swallowed — an audit gap is a compliance
  event; a strict install may set `on_write_error: block` to refuse serving when
  it cannot record.
- **Never folded into the ping or consent payload.** It is a separate,
  separately governed artifact; the content-free telemetry schema (ADR-040,
  ADR-041) is untouched.

## Consequences

### Positive

- The missing half of the audit trail exists: "did X have access to decision D
  at the time of change Y" becomes answerable from local JSONL.
- The strict-superset property is provable. Default-absent means an auditor
  reading the source still gets the one-file answer to "is exfiltration
  possible", because the default path is unchanged.
- An enterprise adopter clears its compliance gate without SSO or RBAC on the
  MCP — "ACL the repo, audit is the answer" becomes literally true.

### Negative

- A content-bearing local artifact now exists; the deployment that enables it
  inherits retention, residency, and access obligations the engine never had.
  Those attach to the deployment, not the engine, but they are real and the docs
  must say so.
- "Attributable, not authenticated" is a weaker claim than an SSO-backed audit
  trail; an auditor expecting verified principals must be told the boundary is
  the repository ACL.
- Persistent, config-based enablement diverges from ADR-040's deliberate
  anti-persistence; the precedent must be defended (silently-off is acceptable
  for telemetry, disqualifying for audit).

### Risks

- Creep toward recording bodies or content under "richer audit" pressure.
  Mitigation: the schema is pinned here and in a contract battery; `returned` is
  body-free references only, and the absent body field is a test, not a comment.
- A network import creeps into the audit module to "just ship it". Mitigation:
  the isolation battery forbids it; transmission is satellite-only.
- The log is mistaken for authentication. Mitigation: the identity clause, the
  stderr announcement, and the docs all state attributable-not-authenticated.
- An audit gap is silently tolerated. Mitigation: writes are fail-loud, and the
  `block` option lets strict installs refuse to serve without recording.

## Alternatives Considered

### Fold audit fields into the telemetry log

One log, less code.

#### Disadvantages

- It makes the named-absent fields (query, returned references) present, destroying the
  content-free guarantee of ADR-040 and ADR-041 and the strict-superset property
  the whole posture rests on. Rejected.

### SSO or authenticated audit on a shared MCP

Verified principals, in a form regulators recognise.

#### Disadvantages

- Requires the hosted, shared MCP that the architecture (and the adopter)
  reject; repository ACL already bounds access. Rejected.

### Transmit audit events directly from the engine to a sink

Turnkey aggregation with no satellite.

#### Disadvantages

- A second engine network surface, contradicting ADR-002 and the
  single-fenced-ping isolation rule. The sink belongs in the `lore-audit`
  satellite. Rejected.

### No audit log; rely on the corpus and git history

Zero new surface.

#### Disadvantages

- Git shows what *changed*, never who *read* a decision before changing it; the
  regulator question stays unanswerable. Rejected.

A default-absent, local-only, content-bearing-by-design recorder under a
committed config stanza, with transmission delegated to a satellite, is
selected.

## Relationship to Other Decisions

- ADR-040 (local telemetry) and ADR-041 (anonymous ping): this recorder is their
  deliberate inversion — the same write-only, outside-the-contract posture, but
  content-bearing and persistent. It does not modify the telemetry or ping
  schema, and the content-free guarantee holds whenever audit is absent (the
  default). It shares the one-fenced-network-module rule.
- ADR-032 (stateless reads): the recorder is write-only observability outside
  the request/response contract; responses stay byte-identical with it attached.
- ADR-002 (AI-optional, offline): the engine stays offline; transmission is a
  satellite concern.
- ADR-065 (artifact content untrusted) and ADR-077 (two-gate capture write
  model): the log records attributable-not-authenticated access; it complements,
  never replaces, the repository-ACL plus pull-request-review boundary.
- ADR-064 (multi-repo extraction) and ADR-073 (backend connectors consolidate):
  the `lore-audit` collector consumes the published JSONL contract; no engine
  network code.
- ADR-013 (leverage existing source control): the audit log is machine state
  under XDG directories, never repository state; it never enters the corpus.
- ADR-127 pins the canonical shared-server carrier, migration conflict
  behavior, returned-record reference shape, and activation diagnostic.

## Success Measures

- With no `audit:` stanza, no audit file is created and MCP responses are
  byte-identical to today; the content-free guarantee battery passes unchanged.
- With audit enabled, each MCP read-tool call appends exactly one line matching
  the pinned schema, carrying the query and body-free returned references and
  no artifact body.
- The isolation battery rejects any network or socket import in the audit
  module.
- `rac mcp` announces audit recording on stderr when it is enabled.
- A `lore-audit` satellite can tail the JSONL to a sink with zero engine
  changes.

## Review Date

Review when a design partner needs a field the pinned schema lacks, when
`on_write_error: block` sees real use, or when the `lore-audit` satellite ships.

## Related Requirements

- rac-trust-transparency
