---
schema_version: 1
id: RAC-KZKMJA9JVF6J
type: decision
---
# ADR-142: Export the Inherited Layer by Default

## Context

Downstream viewers, document stores, and graph consumers need the same corpus an
agent and gate used. Exporting only the child by default would make a successful
federated validation produce an incomplete external record. Flattening parent
records into the child source would destroy the provenance needed to deduplicate
one shared parent across many children.

The corpus-sync programme defines schemas and source identity before federation,
but it does not decide inherited projection or pin conflicts.

## Decision

Viewer, documents, and graph exports include the effective inherited layer by
default.

- Every record retains its own `corpus.source` from ADR-135 and additive layer
  and verified-pin provenance where the schema carries federation metadata.
- Global consumer identity is `(source, id)`; a child export never rewrites a
  parent record into the child's source namespace.
- The same parent record exported through several children may deduplicate only
  when source, canonical ID, verified pin, and record body agree.
- The same `(source, id)` arriving with a different pin or body is an explicit
  aggregation conflict, never last-writer-wins replacement.
- ADR-137 overrides preserve both records: the parent is exported with its
  overridden state and mapping provenance, and the local replacement retains
  its own source identity.
- Human-facing export commands may request `--local-only` to emit the child
  projection. The default remains the corpus used by normal reads and
  enforcement.
- Export schemas land before source-aware federation fields, and federation
  reuses those schemas rather than creating a second export format.

## Consequences

An export faithfully represents the governing composed corpus, and a shared
parent can be stored once downstream even when many children export it. Pin
disagreement is visible instead of silently mixing historical versions.

Default export size grows with the inherited corpus. Operators wanting a child
diagnostic can opt out explicitly, but consumers must understand source-aware
identity before aggregating federated exports.

## Status

Proposed

## Category

Product

## Alternatives Considered

### Export only the local layer by default

Rejected. The external record would omit standards that influenced retrieval
and enforcement, making provenance incomplete.

### Stamp inherited records with the child source

Rejected. The same parent would become N different records and could no longer
be attributed or deduplicated globally.

### Deduplicate on `(source, id)` regardless of pin or body

Rejected. Two versions of a standard would silently collapse according to
arrival order.

### Omit the overridden parent

Rejected. The original policy and override provenance must remain available for
audit and historical reconstruction.

## Related Decisions

- adr-007
- adr-074
- adr-089
- adr-122
- adr-135
- adr-137
- adr-138

## Related Designs

- corpus-federation-mechanism

## Related Requirements

- rac-export-contract-schemas
- rac-export-source-identity
- rac-federated-resolution-provenance
