---
schema_version: 1
id: SPC-000000000002
type: decision
---
# ADR-002: Keep the Standards Runbook Shape

## Status

Accepted

## Category

Process

## Context

The standards corpus declares the runbook type; local operators asked for a
lighter shape.

## Decision

Runbooks keep the standards shape. This decision is the rationale a type
override would cite if the child ever declared its own runbook element.

## Consequences

No local runbook element; the inherited declaration governs.
