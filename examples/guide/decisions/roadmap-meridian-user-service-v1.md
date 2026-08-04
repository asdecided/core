---
schema_version: 1
id: GUIDE-KTW9YBE3CX84
type: roadmap
---
# Roadmap: Meridian User Service v1

## Status

Planned

## Context

The Meridian user management service is under active development. This roadmap
covers the v1 delivery: the account lifecycle API, authentication integration,
and the compliance tooling required before production launch.

## Outcomes

- Support and compliance teams can fulfill GDPR subject-access requests without
  manual log reconstruction.
- The account lifecycle API is stable and documented for downstream consumers
  (billing, admin dashboard, support tooling).
- No deactivated account can authenticate after closure.

## Initiatives

### Initiative 1 — Account Lifecycle API

Implement `POST /users`, `GET /users/{id}`, `PUT /users/{id}`, and
`DELETE /users/{id}` with soft-delete semantics as specified in the user
deletion API design.

### Initiative 2 — Compliance Query Path

Implement `GET /admin/users/{id}` and `GET /admin/users?deleted=true` for
support and compliance use cases. Access is restricted to internal service
tokens.

### Initiative 3 — Authentication Invalidation

Integrate with the Meridian auth service to invalidate tokens synchronously
on account deletion, satisfying REQ-003.

## Success Measures

- All five lifecycle requirements (REQ-001 through REQ-005) pass acceptance
  tests before the v1 tag.
- Zero support tickets caused by missing audit records in the first quarter
  post-launch.

## Assumptions

- The Meridian auth service token-invalidation API is available before
  Initiative 3 begins.
- PostgreSQL is the authoritative store for user records; no migration to
  another store is planned for v1.

## Risks

- Auth service integration blocks Initiative 3. Mitigated by stubbing
  invalidation in non-production environments.
- Query authors omit `deleted_at IS NULL`. Mitigated by the repository-layer
  default filter and the linter rule established in ADR-001.

## Related Requirements

- GUIDE-KTW9YBE1WHA4

## Related Decisions

- GUIDE-KTW9YBDWDBFM

## Related Designs

- GUIDE-KTW9YBDZAY9F
