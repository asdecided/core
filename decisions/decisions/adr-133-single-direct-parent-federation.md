---
schema_version: 1
id: RAC-KZKMJ8Q49GHV
type: decision
---
# ADR-133: Start Corpus Federation With One Direct Parent

## Context

ADR-089 permits corpus federation under deterministic, offline, Git-native
constraints but does not decide the parent topology. Supporting several direct
parents or recursively inherited parents introduces ordering, diamond identity,
cycle, pin-coherence, and override-precedence questions before a single shared
standards corpus has been proven useful.

The initial organisation use case needs one firm-wide standards corpus inherited
by one repository. It does not require an arbitrary corpus graph.

## Decision

The first federation increment supports exactly one direct parent corpus.

- A child may declare either no parent or one parent.
- More than one direct parent is a deterministic validation error raised before
  any overlay is constructed.
- A declared parent that itself declares inheritance is a deterministic
  transitive-inheritance error raised before overlay.
- A valid composition therefore contains one local, writable child layer and
  one inherited, read-only parent layer.
- Multiple parents, transitive inheritance, and parent DAGs require a future
  decision. They are not accepted as dormant syntax in the first manifest
  version.

This decision narrows ADR-089's parent-corpus direction; it does not accept the
remaining mechanism decisions or authorize engine implementation by itself.

## Consequences

The first implementation avoids ordering and diamond-resolution semantics while
covering the real shared-standards use case. Validation can reject unsupported
topologies before loading parent artifacts, so every accepted composition has a
bounded two-layer shape.

Teams needing several standards corpora must combine them into one materialised
parent for this increment. Adding a second parent later will require an explicit
migration and new decisions about ordering, collisions, cycles, and overrides.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

### Support an arbitrary parent DAG immediately

Rejected for the first increment. It commits the engine to ordering, cycle, and
diamond semantics before the simpler use case has been validated.

### Support several direct parents but reject transitivity

Rejected for the first increment. Direct-parent ordering and cross-parent
collisions still require precedence rules that the product does not need yet.

### Keep federation permanently single-parent

Not decided. This ADR constrains the first increment only; evidence may justify
a separately ratified extension.

## Related Decisions

- adr-002
- adr-018
- adr-080
- adr-085
- adr-089

## Related Designs

- corpus-federation-mechanism

## Related Requirements

- rac-parent-corpus-inheritance
