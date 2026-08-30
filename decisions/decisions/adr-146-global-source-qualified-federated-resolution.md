---
schema_version: 1
id: RAC-KZN54DB3V0ZC
type: decision
---
# ADR-146: Resolve Federated Artifacts by Global Source Identity Without Precedence

## Context

ADR-136 uses a child-local parent alias for qualified references and rejects a
canonical-id collision across the two first-increment layers. A source graph
has no single parent alias table, and the same stable artifact id may
legitimately occur in independent source namespaces. Flattening every nested
alias into one global map would let an unrelated sibling change the meaning of
an immutable parent's relationship. Treating traversal order as precedence
would make that meaning depend on YAML order.

ADR-135 already establishes `(corpus.source, canonical_id)` as stable global
identity. Graph resolution should expose that identity directly while retaining
short aliases as local authoring conveniences.

## Decision

Manifest version 2 uses `corpus.source::canonical-id` as the globally stable
qualified reference form:

```text
acme/standards::STD-KWJ4VMKVSS65
```

- The qualifier is an exact configured `corpus.source` visible from the
  reference's source context. It works for the root and every inherited node.
- The right-hand side is a canonical artifact id only. Filename, title, legacy,
  and other aliases remain invalid after `::`.
- Global qualification returns the retained record owned by that exact source.
  It does not follow an override redirect and therefore remains a reliable
  historical lookup.
- A direct edge alias remains accepted as `alias::canonical-id`, but only in the
  lexical context of the corpus that declares that edge. Aliases do not leak
  upward, become global identity, or appear as persisted/exported keys. Public
  CLI and MCP lookup uses the root manifest's alias table. Alias qualification
  selects the same source-owned historical record as global qualification and
  never follows an override.
- Version-2 sources contain `/` and aliases do not, so the two qualifier forms
  are syntactically distinct. Alias validation is shared by manifest parsing
  and composition.

Resolution is source-contextual within the one central graph model:

1. A root-authored reference and a public root lookup see the root's effective
   closure.
2. A reference authored by source A sees A and the closure A declares, not
   unrelated sibling branches later added above A.
3. A's direct aliases are resolved with `(A source, alias)`.
4. A catalog relationship endpoint retains the authored token and a sorted,
   non-empty `historical_candidates` set. A qualified or uniquely resolved
   token has one candidate. An unqualified token whose several candidates all
   converge has every original candidate in `(source, canonical_id)` order and
   one separate `effective_terminal`. An ancestor may change only that terminal
   in its effective graph; it never invents one preferred historical key.

The engine must not construct independent per-source overlays to implement
this rule. The verified closure owns one source-aware catalog, visibility
relation, alias-edge table, and resolver; source context is an input to that
resolver.

In version 2, equal canonical ids in distinct sources are valid catalog
records. An unqualified reference resolves only when its visible effective
candidates reduce to exactly one `ArtifactKey`, including when every candidate
explicitly redirects to the same terminal override. Otherwise that reference
is a deterministic sourced ambiguity. The mere presence of unused equal ids
does not invalidate composition. Duplicate canonical ids within one source
remain invalid.

When a version-2 closure contains a version-1 node, references authored by that
node are first resolved and validated under its exact version-1 local/parent
rules. The enclosing version-2 model retains those resolved keys and may apply
only explicit ancestor redirects. Global source qualification and legal
cross-source equal ids apply to version-2 root/public lookup over the resulting
catalog; they do not retroactively make a version-1-local collision valid.

No source, graph depth, local layer, parent layer, direct edge, or manifest
position receives implicit precedence. Deterministic tie ordering is
`(source, relative_path)` after the existing source-neutral ranking contract;
it never converts an identity ambiguity into a winner.

Version-1 roots retain ADR-136's exact alias qualification, cross-layer
canonical-collision error, stable findings, and output bytes. Version 2 is the
only mode that enables global source qualification and legal cross-source equal
ids.

This decision supersedes ADR-136. It preserves ADR-136's prohibition on
implicit precedence and its canonical-ID-only qualified suffix, but replaces
alias-only qualification with stable source qualification plus scoped alias
convenience, and replaces composition-time cross-source canonical collision
with deterministic unqualified ambiguity. It amends ADR-141 only where that
decision names `alias::canonical-id` as the MCP-qualified form: the six-tool
surface and bounded provenance rules remain unchanged, while version-2 ID
arguments also accept the global source-qualified form.

## Consequences

Every durable reference can name the same artifact independently of which path
or alias reached it. Adding an unrelated sibling cannot reinterpret a parent's
authored references, while root-authored references still see the complete
root policy closure.

Global source names are longer than local aliases, and a user must qualify an
ambiguous bare id. That explicitness is preferable to silent source order. The
resolver and relationship store must retain source context and separate
historical endpoints from effective redirected endpoints.

## Status

Proposed

## Category

Architecture

## Supersedes

- adr-136

## Alternatives Considered

### Export aliases as global qualifiers

Rejected. Aliases are mutable and edge-local; the same spelling may correctly
name different sources in different manifests.

### Keep cross-source canonical-id collisions as composition errors

Rejected for v2. The source namespace already distinguishes the records, and
unqualified ambiguity is sufficient without preventing qualified use.

### Let the nearest or root-local source win

Rejected. It hides governance based on graph shape and contradicts the
source-neutral resolution contract.

### Resolve all authored references against the final flat root closure

Rejected. Adding sibling B could make an unchanged relationship inside A
ambiguous even though A cannot see or edit B in its own composition.

## Related Decisions

- adr-016
- adr-026
- adr-078
- adr-089
- adr-135
- adr-136
- adr-139
- adr-141
- adr-144
- adr-145

## Related Designs

- corpus-federation-mechanism
- corpus-federation-graph-composition

## Related Requirements

- corpus-source-identity
- federated-resolution-provenance

## Related Roadmaps

- corpus-federation
