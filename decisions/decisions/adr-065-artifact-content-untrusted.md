---
schema_version: 1
id: RAC-KV6KFBDZ4D23
type: decision
tags: [security, trust, mcp, grounding, agents]
---
# ADR-065: Artifact Content Is Untrusted Input; the Trust Boundary Is Human PR Review

## Status

Accepted

## Category

Architecture

## Context

A coding agent connected to Lore ingests artifact content as **authoritative
grounding** — it reads a decision and treats it as the team's settled position.
That is the whole point (ADR-034: Lore serves facts, the agent reasons over
them). It also makes artifact content an attack surface that earlier decisions
did not address.

The four MCP tools are read-only (ADR-030, ADR-032). The read-only guarantee
protects the *store*: nothing the agent or server does can mutate the corpus.
It says nothing about protecting the *agent*. A poisoned artifact — or a hostile
pull request that adds one — can carry text engineered to steer the consuming
agent: imperative overrides ("ignore previous instructions…"), impersonation of
system/agent/tool messages, or content that argues the agent away from a
recorded decision. The server will serve that text faithfully and deterministically,
because serving recorded content faithfully is exactly its job.

The red-team of the v0.23.0 plan flagged this as the highest-priority gap: we
assert artifacts are trustworthy without recording *why* they are trustworthy or
where that trust comes from. Determinism (ADR-002) guarantees the server returns
the same bytes every time; it does not guarantee those bytes are benign.

## Decision

Artifact content is **untrusted input**. It becomes authoritative for an agent
only because a human reviewed and merged it.

- The **trust boundary is human pull-request review.** An artifact is trusted
  to the extent that a human accepted it into the corpus through the project's
  PR process. Content that has not passed human review — unreviewed branches,
  machine-ingested documents not yet merged, anything outside the reviewed
  corpus — is **out of scope and MUST NOT be treated as trusted.**
- The **read-only MCP server protects the store, not the agent** (ADR-030,
  ADR-032). It is not, and will not become, a content sanitizer or a verdict
  engine; that would require the semantic judgment ADR-034 keeps out of core.
- The mitigation against poisoned content is twofold and deterministic:
  (1) the human PR gate, and (2) a `lore doctor` check that **flags
  injection-style content** — imperative overrides, system/agent/tool
  impersonation, and decision-steering language — as a reviewable warning,
  never an auto-edit.
- This trust model is **documented** (`SECURITY.md`) so users understand what
  the read-only guarantee does and does not cover.

This decision does not weaken determinism or the read-only surface; it names the
threat those properties do not address and places the mitigation at the human
review boundary, where it belongs.

## Consequences

### Positive

- The product's central claim — "the agent grounds on trustworthy decisions" —
  is now backed by a stated reason (human review) rather than an unexamined
  assumption.
- A poisoned-artifact attempt is catchable before merge: the `doctor` flag
  surfaces it to the reviewer who is already the trust boundary.
- The read-only server keeps a single, honest job; we do not bolt a fragile
  content-sanitizer onto the serving path.

### Negative

- The guarantee is procedural, not technical: a project that does not actually
  review PRs inherits no protection. We document this limit rather than pretend
  to solve it in the server.
- The injection-content heuristic is lexical and will both miss novel phrasings
  and occasionally false-positive; it assists a human reviewer, it does not
  replace one.

### Risks

- A reader mistakes the `doctor` flag for a guarantee and stops reviewing.
  Mitigation: `SECURITY.md` states plainly that PR review is the boundary and
  the flag is an aid, not a gate.
- Pressure to make the server "just sanitize the content." Mitigation: this ADR
  names the boundary; crossing it (a semantic verdict in core) requires
  superseding both this decision and ADR-034.

## Alternatives Considered

### Sanitize or rewrite artifact content in the server

Strip or neutralize suspicious content before serving it.

#### Disadvantages

- Rewriting served content silently changes recorded knowledge — it breaks the
  byte-stable, read-only contract (ADR-032) and the no-auto-edit principle.
- Deciding what is "suspicious" is semantic judgment core must not contain
  (ADR-034).

### A trust/verdict score on each artifact

Return a computed "trustworthiness" number per artifact.

#### Disadvantages

- A deterministic lexical score dressed as a verdict is wrong in both directions
  and consumed as authority anyway — the exact failure ADR-034 rejects.

### Do nothing — rely on determinism alone

Treat the read-only guarantee as sufficient.

#### Disadvantages

- Conflates protecting the store with protecting the agent; leaves the
  highest-priority red-team gap unaddressed.

Naming artifact content as untrusted, with PR review as the boundary and a
`doctor` flag as the aid, is selected.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The deterministic diagnostic and documented human-review boundary have concrete source anchors."
rules:
  - id: doctor-keeps-injection-diagnostic
    kind: require_pattern
    path_glob: "rust/rac-engine/src/doctor.rs"
    pattern: 'CODE_INJECTION_CONTENT'
    message: "The deterministic injection-style diagnostic must remain available."
  - id: security-doc-keeps-review-boundary
    kind: require_pattern
    path_glob: "SECURITY.md"
    pattern: '(?i)human review'
    message: "Security documentation must retain human review as the trust boundary."
```

## Related Decisions

- adr-030
- adr-032
- adr-034
- adr-049

## Related Requirements

- rac-artifact-trust-model
- rac-doctor-diagnostic-validator

## Related Roadmaps

- v0.23.0-hardening
