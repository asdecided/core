---
schema_version: 1
id: RAC-KZKMJ9DGR69Z
type: decision
---
# ADR-137: Require Decision-Backed Explicit Federation Overrides

## Context

ADR-136 rejects implicit child-first and parent-first precedence. A child still
needs a controlled way to depart from an inherited standard without editing or
silently hiding the parent corpus. A bare replacement mapping would identify
what changed but not why the organisation accepted the exception.

Overrides must preserve parent history, remain reviewable in Git, and be strict
enough that a typo cannot redirect one artifact type to another.

## Decision

An inherited artifact is replaced only by an explicit, Decision-backed mapping
under `## overrides` in `.decided/corpus.md`.

- Each versioned mapping names a qualified parent canonical ID, a local
  canonical replacement ID, and a live local Decision that explains the
  exception.
- The replacement must resolve to exactly one local artifact of the same type
  as the parent target.
- The rationale must resolve to exactly one live local Decision. An inherited
  Decision cannot serve as the child's override rationale.
- Missing, ambiguous, cross-type, retired-rationale, parent-to-parent, and
  chained mappings are validation errors.
- A valid mapping makes the local replacement effective for live unqualified
  retrieval, routing, and enforcement, including lookups using the overridden
  parent's canonical ID.
- The original parent remains read-only and qualified-addressable. Exports
  retain it with an overridden state and the mapping provenance.
- Removing the mapping restores the verified parent artifact; no parent bytes
  are rewritten or deleted.

An override is therefore recorded policy, not resolution precedence.

## Consequences

Every local exception has a same-type implementation and a durable local
rationale. Agents receive the effective local rule while auditors and humans
can still inspect the inherited original and the reason it was displaced.

Creating an exception requires an additional Decision artifact and manifest
edit. This is deliberate friction: an undocumented child shadow is rejected
rather than treated as convenience behavior.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

### Let any local collision override the parent

Rejected. Identity coincidence or load order is not evidence of intentional
policy and would silently weaken inherited governance.

### Record only parent and replacement IDs

Rejected. The mapping would show what happened without preserving why the
exception exists or who should reconsider it.

### Copy and edit the parent artifact locally

Rejected. Duplication destroys shared provenance, creates drift, and makes the
original standard disappear from the child's history.

### Delete the parent from the effective corpus entirely

Rejected. Qualified lookup and export must preserve the inherited record and
its override state for auditability.

## Related Decisions

- adr-016
- adr-026
- adr-065
- adr-080
- adr-089
- adr-134
- adr-136

## Related Designs

- corpus-federation-mechanism

## Related Requirements

- federated-resolution-provenance
- parent-corpus-inheritance
