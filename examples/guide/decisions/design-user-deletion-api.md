---
schema_version: 1
id: GUIDE-KTW9YBDZAY9F
type: design
---
# Design: User Deletion API

## Context

The Meridian user management service exposes a REST API for account lifecycle
operations. The deletion endpoint is the most consequential: it must satisfy
REQ-002 through REQ-005 (preserve history, block authentication, support
compliance) while presenting a conventional HTTP interface to callers who may
not be aware of the soft-delete policy.

## User Need

API consumers — the billing service, the admin dashboard, and the support
tooling — need to close user accounts through a single, consistent endpoint.
They should not need to know the storage representation to call the endpoint
correctly. The endpoint must behave identically whether or not the underlying
row is retained.

## Design

`DELETE /users/{id}` sets `deleted_at` on the matching row and returns
`204 No Content`. The response is identical to what a hard-delete endpoint
would return, so callers need no special handling.

A subsequent `GET /users/{id}` returns `404 Not Found` for the closed account,
preserving the API semantics of deletion even though the row exists. Internal
and compliance-scoped queries use `GET /admin/users/{id}` which lifts the
filter.

Authentication tokens for the deleted user are invalidated synchronously
before the `204` is returned.

## Constraints

- The `DELETE` handler must never issue a `DELETE` SQL statement against the
  `users` table (ADR-001 prohibition).
- The `deleted_at` timestamp is set to UTC now at the database layer, not
  in application code, to prevent clock-skew issues.
- The endpoint must be idempotent: a second `DELETE /users/{id}` on an already-
  deleted user returns `204` without error and without updating `deleted_at`.

## Rationale

Hiding the soft-delete from API consumers means the deletion contract can be
enforced structurally without requiring callers to change their usage patterns.
The `404` on subsequent reads maintains REST semantics while the record is
retained for compliance.

## Alternatives

### Expose deleted status in the API

Return `410 Gone` with the account metadata for deleted users.

Rejected for the public API: it leaks the storage representation and requires
callers to handle a new status code. Kept for the compliance-scoped API.

### Hard-delete with a trigger-based audit log

Use a PostgreSQL trigger to copy the row to an audit table before deletion.

Rejected: see ADR-001. Trigger-based solutions have the same synchronisation
risk as application-level audit logs.

## Accessibility

The endpoint is machine-to-machine; accessibility considerations do not apply
to the HTTP interface. Admin tooling that renders deletion history must meet
WCAG 2.1 AA for human operators.

## Style Guidance

Error responses follow the existing `{"error": "...", "message": "..."}` shape
used by all Meridian service endpoints.

## Open Questions

- Whether the admin endpoint should support bulk retrieval of deleted accounts
  for compliance exports; deferred to a follow-on design.

## Related Requirements

- GUIDE-KTW9YBE1WHA4

## Related Decisions

- GUIDE-KTW9YBDWDBFM

## Related Roadmaps

- GUIDE-KTW9YBE3CX84
