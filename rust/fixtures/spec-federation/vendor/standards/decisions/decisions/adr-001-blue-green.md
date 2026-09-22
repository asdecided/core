---
schema_version: 1
id: SPS-000000000002
type: decision
---
# ADR-001: Blue-Green Deploys for Stateless Services

## Status

Accepted

## Category

Architecture

## Context

Stateless services need zero-downtime releases.

## Decision

Every stateless service deploys blue-green behind the shared load balancer.

## Consequences

Two pools per service; runbooks describe the traffic shift.
