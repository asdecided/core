---
schema_version: 1
id: RAC-KWJ4VMKVSS65
type: requirement
---
# Requirement: Multi-Corpus Source Identity

## Status

Proposed

Classification: `[internal]` — merge N corpora with zero collisions and give
federated artifacts one source identity across every surface. Feature E of the
`corpus-sync` programme and a prerequisite of `corpus-federation`.

## Problem

The export projections currently stamp the corpus directory basename as their
source. That value changes with checkout layout and commonly repeats across
repositories. A short `repository_key` namespaces newly generated artifact ids
but is not a globally stable corpus identity and can legitimately repeat in
different organisations.

Consumer-side aggregation and engine-side federation need the same answer to
"which corpus owns this artifact?" If export invents one identity and
federation invents another, provenance, cache keys, qualified references, and
deduplication will disagree.

## Requirements

- [REQ-001] The nearest `.decided/config.yaml` MUST accept an optional `corpus.source` string as the stable corpus identity. An explicit value MUST be independent of checkout location and directory spelling and MUST use a lower-case, slash-namespaced form suitable for display and deterministic comparison.
- [REQ-002] All three export projections MUST use one shared source derivation: explicit `corpus.source` when present, else a deterministic value derived from `repository_key`, else the current directory-basename fallback.
- [REQ-003] The derived value MUST land in every existing export source field and additively as `corpus.source` in the viewer payload; the existing viewer `corpus.name` field MUST remain unchanged (ADR-007, ADR-063).
- [REQ-004] Federation MUST require explicit `corpus.source` values for the child and every inherited layer, MUST reject duplicate source identities, and MUST use the same value in composite artifact and path keys, provenance, MCP responses, findings, caches, and exports.
- [REQ-005] Documentation MUST publish the consumer-side aggregation recipe: N corpora merge by concatenating documents streams and unioning graph nodes and edges, keyed globally on `(source, id)`. The recipe MUST require distinct explicit `corpus.source` values wherever fallback identities could collide. A shared parent exported through N children MAY deduplicate on that key only when the record body and, when present, verified pin agree; differing copies MUST surface as an aggregation conflict rather than last-writer-wins.
- [REQ-006] Repository keys MAY repeat across corpora because `source` is the outer namespace. A repeated canonical artifact id across distinct sources MUST remain distinguishable by `(source, id)` and MUST still follow federation's explicit collision and override rules inside one effective corpus.
- [REQ-007] Source identity alone MUST NOT introduce inheritance, cross-corpus resolution, or validation. Those semantics remain owned by `corpus-federation` and ADR-133 through ADR-143.
- [REQ-008] The precedence change MUST ship with a migration note for consumers keyed on the old basename value. A repository with neither `corpus.source` nor `repository_key` MUST produce byte-identical source values and export bytes to the released fallback behaviour.

## Acceptance Criteria

- With `corpus.source` configured, viewer, documents, and graph projections
  carry that exact value; different checkout paths and equivalent corpus
  arguments produce byte-identical output.
- With only `repository_key`, all projections use the documented derived
  value. With no repository configuration, existing export goldens remain
  byte-identical.
- A two-corpus aggregation fixture with distinct source identities has zero
  `(source, id)` collisions, including when both fixture corpora use the same
  `repository_key`.
- A federated export fixture stamps parent and child records with their own
  configured source identities and deduplicates the same pinned parent across
  two child exports; a different-pin fixture reports a conflict.
- The viewer payload gains `corpus.source` while `corpus.name` remains
  byte-identical, and the published export schemas validate every projection.

## Success Metrics

- An organisation aggregates multiple corpora without checkout-dependent keys
  or collision-handling code of its own.
- Every agent-facing and export surface names an inherited artifact with the
  same stable source value.

## Risks

- Consumers keyed on the old basename are surprised when a configured source
  starts taking precedence. Mitigation: the migration note and the exact
  unconfigured fallback.
- A human changes `corpus.source` as if it were a display name. Mitigation:
  documentation calls it stable identity, while `corpus.name` and federation
  aliases remain the mutable display surfaces.
- Source identity is mistaken for federation. Mitigation: REQ-007 keeps
  aggregation additive and cross-corpus semantics behind the separately
  ratified federation ADR set.

## Assumptions

- `repository_key` remains the artifact-id generation namespace, not the
  corpus-provenance identity.
- Aggregation remains consumer-side; the engine emits one effective corpus per
  invocation unless the federation manifest explicitly composes a parent.

## Related Decisions

- adr-002
- adr-007
- adr-026
- adr-063
- adr-073
- adr-080
- adr-085
- adr-089
- adr-133
- adr-134
- adr-135
- adr-136
- adr-137
- adr-138
- adr-139
- adr-140
- adr-141
- adr-142
- adr-143

## Related Designs

- corpus-export-shape-contract
- corpus-federation-mechanism

## Related Roadmaps

- corpus-sync
- corpus-federation

## Related Requirements

- rac-export-contract-schemas
- rac-parent-corpus-inheritance
- rac-federated-resolution-provenance
