---
schema_version: 1
id: RAC-KZKMJ97Z2PBE
type: decision
---
# ADR-136: Resolve Cross-Corpus References Without Implicit Precedence

## Context

Once child and parent artifacts share a resolver, an unqualified identifier or
alias may match both layers. Choosing the child or parent by load order would
make relationship meaning depend on an implementation accident. Requiring every
existing local reference to become qualified would make federation needlessly
disruptive.

ADR-135 supplies a composite source identity but does not decide the authored
reference syntax or ambiguity behavior.

## Decision

Cross-corpus reference resolution has no implicit child-first or parent-first
precedence.

- Existing unqualified references resolve when exactly one artifact across the
  composed corpus matches.
- A qualified reference uses `alias::canonical-id`, where `alias` is the
  child-local parent alias and the right-hand side is a canonical artifact ID.
  Legacy aliases and titles are not accepted after `::`.
- An unqualified reference with more than one match is a deterministic
  ambiguity error.
- A repeated canonical ID across child and parent is a deterministic
  cross-corpus collision error; load order never clears it.
- Relationship endpoints carry the source-aware artifact key. Existing cycle,
  type, and target validation runs over those keys across both layers.
- Only an explicit override accepted under ADR-137 may replace an inherited
  artifact in the effective unqualified view.

The qualified syntax is an escape from ambiguity and a way to address parent
history; it does not establish source precedence.

## Consequences

Most existing references remain valid when they are unique, while ambiguous
references fail with a stable remedy. Authored relationships can cite a parent
artifact precisely without embedding a checkout path or global source string.

Moving an artifact into a newly ambiguous composition may require an explicit
qualified reference. Canonical-ID collisions are stricter than the composite
storage key alone requires, but they prevent surprising behavior on legacy
unqualified surfaces.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

### Child always wins

Rejected. A local artifact could silently replace firm-wide governance merely
by sharing an identifier or alias.

### Parent always wins

Rejected. It would erase the child's canonical authority over its own corpus and
make intentional local exceptions impossible.

### Require qualified references for every inherited artifact

Rejected. Unique existing references can remain deterministic without forcing
source syntax into every relationship.

### Permit repeated canonical IDs because storage keys include source

Rejected for the first increment. Legacy consumers and unqualified references
still expose bare IDs, so a collision must fail until those surfaces are
unambiguously source-aware.

## Related Decisions

- adr-016
- adr-026
- adr-078
- adr-089
- adr-135

## Related Designs

- corpus-federation-mechanism

## Related Requirements

- federated-resolution-provenance
- parent-corpus-inheritance
