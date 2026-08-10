---
schema_version: 1
id: RAC-KZN54DB4QY0R
type: decision
---
# ADR-147: Permit Decision-Backed Override Chains Across the Federation Graph

## Context

ADR-137 requires every override to name one inherited target, one same-type
local replacement, and one live local Decision rationale. It rejects chains
because the first increment has only a child and one parent. In a transitive
graph, an intermediate corpus may make a legitimate recorded exception and a
root corpus may later replace that exception with its own. A diamond may also
bring the same shared ancestor through branches with different explicit
exceptions.

Flattening those mappings or choosing one branch by traversal order would lose
the decisions that explain the policy lineage. Rejecting every chain would make
transitive federation unusable for controlled local adaptation.

## Decision

Version 2 permits explicit, Decision-backed override chains across ancestry
levels. The exact `## overrides` section contains one fenced YAML mapping:

```yaml
version: 2
items:
  - target: acme/standards::STD-KWJ4VMKVSS65
    with: APP-KWJ9ABCD1234
    rationale: APP-KWJ9D3C1S10N
```

Every entry keeps the first-increment safeguards:

- `target` is a globally source-qualified canonical id in the declaring
  corpus's inherited effective view. An alias, unqualified id, local artifact,
  or unreachable sibling is invalid.
- `with` is one canonical artifact id local to the declaring corpus and of the
  same type as the target. Parent-to-parent or inherited replacement remains
  invalid.
- `rationale` is one live Decision local to the declaring corpus. An inherited,
  ambiguous, or retired rationale remains invalid.
- A manifest may name a target at most once. Multiple targets may deliberately
  converge on the same local replacement. Mapping order has no meaning.

The version-2 override mapping is valid only in a manifest whose `## inherits`
mapping is also version 2. A present overrides mapping must match the inherits
version; mixed versions and overrides without inheritance are rejected under
ADR-145.

Composition evaluates nodes bottom-up. A node unions the effective projections
of its direct parents, deduplicates identical keys, and then applies only the
overrides declared by that node. An exception declared in branch A therefore
does not silently suppress the same shared ancestor still inherited through
sibling B.

The complete set of mappings forms a source-aware override graph. Chains such
as `A -> B -> C` are valid when each hop is declared by the corpus that owns its
local replacement. Same-manifest indirect chains, type changes, inherited
replacements, cycles, and order-dependent mappings are errors. Defensive cycle
detection remains mandatory even though the inherited-to-local rule normally
makes a cycle impossible.

A shared historical key may acquire different effective branch targets. At a
join, that is `corpus-federation-override-divergence` and blocks the root
effective composition unless the joining corpus explicitly maps every live
branch target to one local same-type replacement. A fork is valid only when all
paths explicitly reconverge on one terminal `ArtifactKey`; traversal order
never selects a terminal.

The catalog retains every original, intermediate, terminal, mapping-owner, and
rationale record. Global source-qualified lookup always retrieves the named
historical record. In the root effective view, nonterminal artifacts do not
contribute search rows, graph popularity, scope, routing, or enforcement; each
nonterminal bare canonical id redirects to the unique terminal. Historical
relationships retain the authored token and complete sorted historical
candidate set defined by ADR-146, while effective relationships carry the one
terminal or fail as ambiguous.

For a returned or exported historical artifact, per-artifact provenance
contains every mapping edge on every compiled origin-to-terminal path that
contains that artifact key. For a returned or exported effective terminal, it
contains every mapping edge whose compiled terminal is that key. The union of
per-artifact provenance in a default catalog export therefore contains the
complete mapping table; there is no standalone state-bearing mapping row. Each
per-artifact set includes owner source, target, replacement, rationale, and
state for every hop in a deterministic total order. `state` is `overridden`
when the carrying artifact is that hop's target, `replacement` when it is that
hop's direct replacement, and `lineage` when the hop is included only to make
the longer path complete. An intermediate artifact may therefore have
replacement and overridden entries. The additive `lineage` value is
version-2-only; existing version-1 values and output remain exact.
Owners use the closure's parents-before-child, source-lexicographic Kahn rank.
Mapping edges then sort by `(owner rank, owner source, target source, target
canonical id, replacement source, replacement canonical id, rationale source,
rationale canonical id)`, all as UTF-8 byte order. Override-chain provenance is
atomic under ADR-128: optional content and list entries are reduced first, and
if the complete set still cannot fit the response fails with
`response_budget_exceeded`. ADR-127 audit records remain intentionally smaller
and never copy override mappings.

Version-1 roots retain ADR-137's direct mapping, local replacement, chain
rejection, and exact output behavior. Within a version-2 closure, a valid nested
version-1 override remains valid under those rules and is normalized as one
source-aware graph hop. A version-2 ancestor may target its effective local
replacement and thereby extend the chain; the original version-1 record, edge
pin, mapping spelling, and rationale remain attributable.

This decision amends ADR-137 only where it rejects chained mappings. Every hop
remains explicit, same-type, local-replacement, and backed by a live local
Decision. It also amends ADR-142 so composed exports retain complete ordered
chain provenance rather than only one direct mapping and add the `lineage`
role where a carrying artifact is not a hop's direct operand. It amends
ADR-141's MCP response provenance for version 2 to carry that same complete
ordered chain and to fail with ADR-128's hard budget error rather than emit a
partial chain; the six-tool surface remains unchanged. ADR-141's prohibition
on copying mappings into audit records remains unchanged.

## Consequences

An organisation can evolve policy through several corpus levels without
erasing the exceptions and Decisions that led to the root's effective rule.
Diamonds with conflicting policy cannot silently choose a branch; the joining
corpus must record a deliberate reconciliation.

Override validation and provenance become graph operations rather than one
parent-to-child lookup. Catalog and effective relationship projections must be
separate, and tight MCP budgets may return an explicit budget error for a long
chain rather than an incomplete explanation.

## Status

Proposed

## Category

Architecture

## Alternatives Considered

### Continue rejecting all chains

Rejected. A root could not explicitly replace an intermediate corpus's valid
exception without copying or deleting its history.

### Let the closest descendant override win

Rejected. Graph distance is implicit precedence and does not reconcile diamond
branches.

### Permit an override to select another inherited artifact

Rejected. That is parent-to-parent precedence. Each exception must remain an
owned local policy choice with a local Decision rationale.

### Keep only the terminal mapping in provenance

Rejected. It would remove the intermediate decisions needed to audit why the
effective policy changed.

## Related Decisions

- adr-016
- adr-026
- adr-065
- adr-080
- adr-127
- adr-128
- adr-137
- adr-141
- adr-142
- adr-144
- adr-146

## Related Designs

- corpus-federation-mechanism
- corpus-federation-graph-composition

## Related Requirements

- federated-resolution-provenance
- parent-corpus-inheritance

## Related Roadmaps

- corpus-federation
