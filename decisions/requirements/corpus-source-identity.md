---
schema_version: 1
id: RAC-KWJ4VMKVSS65
type: requirement
---
# Requirement: Multi-Corpus Source Identity

## Status

Accepted

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

- [REQ-001] All three export projections MUST stamp one consistently derived corpus identity. The nearest `.decided/config.yaml` MUST accept an optional `corpus.source` string, and the shared derivation MUST use explicit `corpus.source` when present, else a deterministic value derived from `repository_key`, else the current directory-basename fallback. An explicit value MUST be independent of checkout location and directory spelling and MUST use a lower-case, slash-namespaced form suitable for display and deterministic comparison.
- [REQ-002] The derived value MUST land in every projection's existing source field and additively as `corpus.source` in the viewer payload's corpus block. The existing viewer `corpus.name` field MUST remain byte-unchanged (ADR-007, ADR-063).
- [REQ-003] The derivation MUST be deterministic and spelling-independent: equivalent argument spellings of the same initialised corpus and any checkout location MUST produce the same source value and byte-identical export output (ADR-002).
- [REQ-004] Documentation MUST publish the consumer-side aggregation recipe: N corpora merge by concatenating documents streams and unioning graph nodes and edges, keyed globally on `(source, id)` (ADR-026). The recipe MUST require distinct source identities wherever fallback identities could collide. A shared inherited record exported through N roots MAY deduplicate only when the source, canonical id, record body, and verified owning-node pin agree; differing copies MUST surface as an aggregation conflict rather than last-writer-wins.
- [REQ-005] Source identity alone MUST NOT introduce inheritance, cross-corpus resolution, cross-corpus validation, or relationship-resolution changes. Those federation semantics remain owned by `corpus-federation` and ADR-134, ADR-135, and ADR-137 through ADR-142 as amended by ADR-144 through ADR-148. The new decisions explicitly preserve the applicable version-1 behavior recorded by superseded ADR-133, ADR-136, and ADR-143.
- [REQ-006] The value-precedence change MUST ship with a migration note for consumers keyed on the old basename value. A repository with neither `corpus.source` nor `repository_key` MUST produce byte-identical source values and export bytes to the released fallback behaviour.
- [REQ-007] Federation MUST require an explicit, non-empty `corpus.source` for the invocation root and every logical inherited node and MUST use those same values in composite artifact and path keys, provenance, MCP responses, findings, caches, and exports. Every logical node's source MUST be distinct. Several independently verified ancestry branches MAY reach one logical node only when their source and canonical version-2 node digest match; duplicate direct sources and same-source routes with divergent digests MUST fail.
- [REQ-008] Repository keys MAY repeat across corpora because `source` is the outer namespace. Equal canonical artifact ids in distinct sources MUST remain distinguishable by `(source, id)`. Version 1 MUST retain its accepted cross-layer collision behavior. Version 2 MUST retain every source-owned record: source-qualified lookup MUST remain exact, while unqualified use MUST be a deterministic sourced ambiguity unless explicit Decision-backed overrides converge every visible candidate on one terminal key. Duplicate canonical ids within one source remain invalid, and no source may receive implicit precedence.

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
- A version-2 diamond that reaches the same source and canonical node digest
  through independent branches contributes one logical node; the same source
  with a different digest reports a divergent-pin error.
- A version-2 fixture with the same canonical id in distinct sources keeps
  every `(source, id)` record qualified-addressable and reports a bare-reference
  ambiguity until explicit overrides converge all visible candidates on one
  terminal.
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
- Source identity is mistaken for federation. Mitigation: REQ-005 keeps
  aggregation additive and cross-corpus semantics behind the separately
  ratified federation ADR set.

## Assumptions

- `repository_key` remains the artifact-id generation namespace, not the
  corpus-provenance identity.
- Aggregation remains consumer-side; the engine emits one effective corpus per
  invocation unless the federation manifest explicitly composes an inherited
  closure.

## Related Decisions

- adr-002
- adr-007
- adr-026
- adr-063
- adr-073
- adr-080
- adr-085
- adr-089
- adr-134
- adr-135
- adr-137
- adr-138
- adr-139
- adr-140
- adr-141
- adr-142
- adr-144
- adr-145
- adr-146
- adr-147
- adr-148

## Related Designs

- corpus-export-shape-contract
- corpus-federation-mechanism
- corpus-federation-graph-composition

## Related Roadmaps

- corpus-sync
- corpus-federation

## Related Requirements

- export-contract-schemas
- parent-corpus-inheritance
- federated-resolution-provenance
