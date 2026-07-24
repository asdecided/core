---
schema_version: 1
id: RAC-KWJ8S31YDEAZ
type: requirement
---
# Requirement: Shared-Server Audit Identity

## Status

Accepted

Classification: `[internal]` — every caller on a shared endpoint is
attributable. Initiative 3 of the `lore-at-team-scale` roadmap:
per-request principal resolution in the audit recorder, with mandatory
audit-on as the shared-deployment entry condition. Delivered
(itsthelore/asdecided-core#265): the audit recorder resolves a per-request
principal from the `X-Lore-Principal` header (the carrier ADR-098 fixes),
with precedence assertion > env override > git (skipped on the shared
transport) > unattributed; records grow additive `transport` and
`attribution` fields; HTTP forces `on_write_error: block`; and the
principal never enters a tool body — attribution, never authorization.

## Problem

The audit recorder resolves one principal when it is constructed — the
environment override, else the git identity, else "unattributed" — and
stamps every event with it for the life of the process. That is correct
for a single developer's stdio server and wrong for a shared endpoint:
every caller would be recorded as the host identity, defeating ADR-084's
attributability exactly where an organisation needs it most. The
deterministic-substrate programme recorded mandatory audit-on as an entry
condition for team-scale serving; this requirement is that condition made
concrete.

## Requirements

- [REQ-001] The audit recorder MUST support per-request principal resolution: the observe path gains a per-call principal sourced from a client-supplied attribution channel, whose exact carrier the serving ADR fixes.
- [REQ-002] The principal stays attributable, not authenticated (ADR-084): precedence is per-request assertion, then the environment override, then git identity, then "unattributed"; verifying the assertion is the deployment proxy's job (ADR-085), and the documentation MUST say so honestly.
- [REQ-003] Shared (HTTP) deployments MUST be mandatory audit-on: startup without a working sink fails, and sink write failures block the call, per ADR-084's fail-loud posture.
- [REQ-004] stdio behaviour MUST be unchanged: construction-time resolution remains the local default, and existing audit record shapes grow only additively (ADR-007).
- [REQ-005] The engine MUST NOT gain SSO, RBAC, tool-level authorization, or a user store (ADR-085): the principal is an attribution string in the record, never an access-control input — tool responses are identical regardless of principal.
- [REQ-006] Shared-server records MUST be distinguishable from local ones via an additive transport field, so an auditor can tell asserted-over-HTTP attribution from locally resolved identity.

## Acceptance Criteria

- Two concurrent fixture clients asserting different principals produce
  interleaved audit records carrying two distinct principals.
- A client asserting nothing is recorded with the fallback-chain result,
  never the server host's identity.
- HTTP startup without a sink fails loudly; stdio startup and record
  shapes are unchanged.
- A sink write failure blocks the call on the shared transport, matching
  the ADR-084 `on_write_error: block` contract.
- A test asserts identical tool output across principals — attribution
  never becomes authorization.

## Success Metrics

- An auditor reading the shared server's JSONL can answer "who read what,
  when" per caller, with the trust status of each attribution (asserted
  versus locally resolved) legible from the record itself.

## Risks

- Asserted principals are treated as authenticated identity. Mitigation:
  REQ-002 and REQ-006 keep the posture and the provenance of each
  attribution explicit; the proxy owns verification.
- Audit becomes an access-control hook. Mitigation: REQ-005 pins that
  responses are principal-independent; ADR-085's red lines forbid the
  alternative.

## Assumptions

- The serving ADR fixes the attribution carrier; this requirement
  constrains its semantics, not its wire format.
- Deployments that need verified identity terminate authentication at the
  proxy in front of the endpoint, per ADR-085.

## Related Decisions

- adr-002
- adr-007
- adr-032
- adr-084
- adr-085
- adr-098

## Related Roadmaps

- lore-at-team-scale

## Related Requirements

- rac-mcp-http-transport
