---
schema_version: 1
id: GUIDE-KTW9YBDWDBFM
type: decision
---
# ADR-001: Soft-Delete User Records

## Status

Accepted

## Category

Architecture

## Context

The Meridian user management service stores user accounts in PostgreSQL.
Support, compliance, and billing teams regularly need to audit the history of
an account after it is closed — to verify data was handled correctly, resolve
disputes, and satisfy GDPR subject-access requests that arrive after deletion.

A hard `DELETE` statement makes that history unrecoverable unless a separate
audit log is maintained and kept in sync. Keeping a second log of deleted rows
introduces a synchronization surface that has failed in practice (the billing
team has opened five support tickets in the last quarter tracing to missing
audit records).

## Decision

User records are never hard-deleted from the `users` table. Deletion is
represented by setting `deleted_at` to the current UTC timestamp. All
application queries filter on `deleted_at IS NULL` by default. Administrative
and compliance queries may lift that filter explicitly.

Hard `DELETE` statements against the `users` table are prohibited in
application code. The prohibition is enforced by a linter rule and reviewed
in every schema migration.

## Consequences

### Positive

- Full account history is always available to support, compliance, and billing.
- GDPR subject-access requests can be fulfilled without reconstructing state
  from a separate audit log.
- Accidental deletions are recoverable by clearing `deleted_at`.
- Audit trail consistency is structural, not dependent on a secondary system.

### Negative

- The `users` table grows without bound; a separate archival or anonymisation
  job is required for GDPR erasure requests (right-to-erasure path uses
  anonymisation, not deletion).
- Every query that must exclude deleted users carries a `deleted_at IS NULL`
  predicate; missing it silently returns stale data.
- Schema migrations that add `NOT NULL` columns must handle legacy deleted
  rows.

## Alternatives Considered

### Hard-delete with audit log

Delete rows immediately and write a copy to an `audit_users` table.

Rejected: synchronisation failures between the two tables have caused real
support incidents. Structural guarantees are preferred over procedural ones.

### Hard-delete with event stream

Emit a deletion event to a message bus before deleting.

Rejected: the message bus is not yet a dependency of the user service, and
adding it solely for audit is out of scope for this decision.

## Related Requirements

- GUIDE-KTW9YBE1WHA4

## Related Designs

- GUIDE-KTW9YBDZAY9F
