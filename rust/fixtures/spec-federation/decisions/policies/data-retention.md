---
schema_version: 1
id: SPC-000000000001
type: policy
---
# Data Retention

## Status

Accepted

## Statement

Operational logs shall be retained for ninety days and then deleted.

## Scope

Every service that writes to the shared logging pipeline.

## Exceptions

Security may extend retention for an active investigation; the extension is
recorded as a decision.

## Related Decisions

- adr-002-local-runbook-shape
