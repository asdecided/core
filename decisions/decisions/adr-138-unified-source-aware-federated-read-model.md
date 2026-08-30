---
schema_version: 1
id: RAC-KZKMJ9K3AFB2
type: decision
---
# ADR-138: Build Federation Through One Source-Aware Read Model

## Context

> **Amended by ADR-144 and ADR-148.** Manifest version 2 generalises the one
> writable child plus read-only parent into one writable root plus a verified
> read-only unique-source closure. One immutable closure generation still feeds
> every consumer; no surface may construct an independent overlay.

The released Rust engine assumes one corpus across validation rows, resolver
entries, relationship endpoints, resolved artifacts, derived and persistent
indexes, freshness tracking, MCP `GraphView`, routing, and Sentry enforcement.
Several consumers call corpus loading directly. If each surface overlays the
parent independently, they can disagree about collisions, overrides,
provenance, or freshness.

ADR-103 already establishes one derived read-model for a single corpus.
Federation must extend that boundary rather than add a parallel resolver.

## Decision

Federation is composed once as a source-aware extension of the unified derived
read model.

- The loader produces verified `CorpusLayer` records and source-aware
  `ArtifactKey` and `ArtifactPath` values from ADR-135.
- Validation, reference resolution, relationship graphs, retrieval, path scope,
  exports, cache generation, freshness, MCP, routing, and enforcement derive
  from that same composed layer set. No consumer performs its own directory
  merge.
- The local corpus walk excludes the declared parent materialisation subtree,
  so a Markdown file cannot enter both layers.
- Mutation commands receive only the local writable layer and cannot target an
  inherited path.
- A parent structural or relationship error produces one sourced child error
  at the manifest and blocks composition. Parent warnings and review advisories
  remain parent-owned instead of being duplicated into every child.
- Composition-specific findings, including missing parent, stale digest,
  source mismatch, collision, and invalid override, belong to the child and
  retain all involved sources.
- Without `.decided/corpus.md`, the loader produces the ordinary single local
  layer; federation introduces no alternate single-corpus path.

Surface-specific ranking, enforcement, MCP, export, and cache contracts remain
separate decisions rather than being implied by this architecture.

## Consequences

Every read consumer observes the same verified identities, override state, and
parent validity. Adding a new read surface requires consuming the central model
instead of reconstructing federation semantics.

The source-aware identity change reaches several internal types and persistent
derivations even though the user-facing topology has only two layers. This is a
larger refactor than concatenating two directory walks, but it prevents
validation and enforcement from disagreeing with retrieval.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

### Merge parent entries separately in each consumer

Rejected. Independent overlays would inevitably drift on collisions,
provenance, overrides, and freshness.

### Add a second federation resolver beside the local resolver

Rejected. Two resolution systems would split relationship and lookup semantics
and violate ADR-103's unified-model direction.

### Flatten parent files into the child walk

Rejected. Path identity, read-only ownership, and source provenance would be
lost, and a parent subtree could be discovered twice.

## Related Decisions

- adr-018
- adr-026
- adr-089
- adr-103
- adr-105
- adr-119
- adr-121
- adr-134
- adr-135
- adr-137
- adr-144
- adr-146
- adr-148

## Related Designs

- corpus-federation-mechanism
- corpus-federation-graph-composition

## Related Requirements

- federated-resolution-provenance
- parent-corpus-inheritance
