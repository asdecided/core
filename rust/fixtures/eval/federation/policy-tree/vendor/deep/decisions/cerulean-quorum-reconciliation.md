---
schema_version: 1
id: FEDEVAL-000000000004
type: decision
---
# Cerulean Quorum Rollback Reconciliation

## Status

Accepted

## Context

Services need one exact standard for reconciling a cerulean quorum after a
rollback.

## Decision

Cerulean quorum rollback reconciliation MUST compare the signed epoch ledger
before accepting a recovered replica.

## Consequences

The decision remains discoverable through two inheritance edges.

## Category

Technical
