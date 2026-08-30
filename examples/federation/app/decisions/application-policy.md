---
schema_version: 1
id: APP-01K000000001
type: decision
---
# Application policy

## Status

Accepted

## Category

Technical

## Context

The application needs a narrower policy than the shared engineering default.

## Decision

The application uses its reviewed local policy as the effective terminal.

## Consequences

The shared record stays inspectable as history while application reads use this decision.
