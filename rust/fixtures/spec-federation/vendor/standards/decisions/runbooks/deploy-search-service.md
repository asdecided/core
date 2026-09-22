---
schema_version: 1
id: SPS-000000000001
type: runbook
---
# Deploy the Search Service

## Status

Active

## Purpose

Roll a new search-service build to production without dropping in-flight
queries.

## Steps

1. Drain the blue pool from the load balancer.
2. Deploy the build to the blue pool and wait for health checks.
3. Shift traffic to blue; repeat for green.

## Rollback

Shift traffic back to the untouched pool and redeploy the previous build.

## Verification

Error rate below 0.1% and p95 latency within 5% of the pre-deploy baseline
for fifteen minutes.

## Related Decisions

- adr-001-blue-green
