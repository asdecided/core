---
schema_version: 1
id: RAC-KZKMJ9YA8BRG
type: decision
---
# ADR-140: Apply Inherited Decisions to Child Code

## Context

Federation would be incomplete if inherited standards appeared in search but
not in the Record -> Route -> Enforce loop. Parent Decisions commonly describe
paths and mechanical code constraints that are meaningful in each child
repository, while the parent materialisation itself must remain read-only and
outside child-code enforcement.

The product must also decide whether a diagnostic local-only view can suppress
firm-wide governance for agents or gates.

## Decision

Applicable inherited Decisions govern the child code tree through the existing
scope and Sentry machinery.

- Inherited `## Applies To` paths and code constraints are evaluated against the
  child repository's code tree, never relative to the parent materialisation.
- Their syntax is validated in the parent corpus; target matching and target
  existence are evaluated in the composed child context.
- Inherited live Decisions participate in `decisions-for`,
  `retrieve_grounding`, MCP `find_decisions` path lookup, `gate`, `sentry`, and
  `gate --code`.
- ADR-137 overrides are applied before scope lookup and enforcement so the
  effective local replacement governs where declared.
- Enforcement never scans or modifies code beneath the parent materialisation
  root.
- Human diagnostic and export commands may expose a local-only projection, but
  MCP and enforcement expose no `--local-only` bypass.

This decision extends ADR-123 to the verified inherited layer; it does not add a
second enforcement engine.

## Consequences

An agent receives the same inherited governance that a child code gate enforces,
closing the loop across repositories. A firm-wide deterministic constraint can
block violating child code without being copied into every repository.

Opting into federation therefore changes the governing decision set, not only
search results. Teams must use an explicit override when a child needs an
exception; a convenience flag cannot silently weaken enforcement.

## Status

Accepted

## Category

Product

## Alternatives Considered

### Use inherited artifacts for retrieval only

Rejected. Agents could be grounded by a standard that the gate ignores, leaving
the product's route and enforce surfaces inconsistent.

### Evaluate inherited paths against the parent checkout

Rejected. The declared standard governs child code; matching the vendored or
submodule path would make it operationally useless and could inspect read-only
parent files.

### Allow enforcement to run local-only

Rejected. A bypass flag would make inherited governance optional precisely on
the surfaces intended to enforce it.

## Related Decisions

- adr-049
- adr-089
- adr-103
- adr-123
- adr-137
- adr-138
- adr-139

## Related Designs

- code-scope-consumption
- corpus-federation-mechanism
- sentry-code-constraint-evaluation

## Related Requirements

- deterministic-decision-code-enforcement
- rac-federated-resolution-provenance
- rac-parent-corpus-inheritance
- rac-path-decisions-lookup
