---
schema_version: 1
id: TST-00000000000R
type: risk
---
# Vendor lock-in on the managed queue

## Status

Accepted

## Risk

Moving off the managed queue later could cost a quarter of engineering time.

## Likelihood

Moderate: pricing has changed twice in two years.

## Impact

Migration work across every producer and consumer.

## Context

ADR-001 chose the managed queue.

## Mitigation

Producers publish through one adapter, so a move touches one module.

## Related Decisions

- TST-00000000000A
