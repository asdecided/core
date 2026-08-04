---
schema_version: 1
id: GUIDE-KTW9YBE1WHA4
type: requirement
---
# Requirement: User Account Lifecycle

## Status

Accepted

## Problem

The Meridian platform needs to manage user accounts through their full
lifecycle: creation, update, deactivation, and closure. Account closure must
preserve history for compliance and support purposes while preventing the
closed account from being used for authentication.

## Requirements

- [REQ-001] The service MUST create a user account with a unique identifier, email address, and creation timestamp.
- [REQ-002] The service MUST deactivate a user account without destroying its history or audit trail.
- [REQ-003] Deactivated accounts MUST NOT be returned by default user queries.
- [REQ-004] The full history of a user account MUST be recoverable by support and compliance staff after deactivation.
- [REQ-005] The service MUST fulfil GDPR subject-access requests for deactivated accounts.

## Success Metrics

- Support tickets caused by missing audit records fall to zero within one
  quarter of deployment.
- GDPR subject-access requests can be fulfilled without manual reconstruction
  from logs.
- No deactivated user can authenticate after the deactivation timestamp.

## Risks

- Queries that omit the deactivation filter silently expose deleted accounts.
  Mitigated by a mandatory default filter at the repository layer and a
  linter rule on raw SQL.

## Assumptions

- Compliance requires account history to be retained for at least seven years.
- GDPR erasure requests are fulfilled by anonymising the record, not deleting
  the row.

## Related Decisions

- GUIDE-KTW9YBDWDBFM

## Related Designs

- GUIDE-KTW9YBDZAY9F
