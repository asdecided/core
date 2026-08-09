---
schema_version: 1
id: RAC-KZKMJ9RP0KNV
type: decision
---
# ADR-139: Rank Federated Retrieval Without Source Preference

## Context

Adding a large standards parent changes corpus statistics and gives inherited
artifacts many relationship edges. A local-first quota or source boost could
hide the best standard merely because it is inherited; an inherited-first
policy could swamp precise local decisions. Separate indexes would also make
cross-layer score comparison arbitrary.

The v0.28 retrieval contract already bounds graph influence with a lexical
floor. Federation must decide whether source becomes a new ranking signal.

## Decision

Source and layer are provenance, not relevance signals.

- Effective local and inherited artifacts share one deterministic BM25 index
  and one relationship index derived from ADR-138's read model.
- BM25 corpus statistics are calculated over the effective combined corpus.
- There is no source boost, inherited penalty, local-first quota, or reserved
  result allocation.
- The v0.28 lexical graph-floor invariant remains active: graph popularity
  cannot displace a materially stronger lexical match.
- Exact score ties use `(source, relative_path)` as the final stable order.
- A valid ADR-137 override changes the effective live set before ranking. The
  overridden parent remains available through exact qualified lookup but does
  not compete as a second live recommendation.
- Federation quality is evaluated by extending DecisionGrounding with a
  large-parent, hard-negative track rather than creating a separate benchmark.

## Consequences

The same query has one explainable score space across child and parent, and
provenance does not become a hidden relevance heuristic. Precise inherited
standards can outrank weak local matches, while the lexical floor protects
against a highly connected parent dominating on graph popularity alone.

Combined BM25 statistics can legitimately change scores when a parent is added.
That is the consequence of searching the effective corpus, not a compatibility
bug; deterministic evaluation must measure it.

## Status

Proposed

## Category

Technical

## Alternatives Considered

### Reserve local-first result slots

Rejected. Locality is not evidence of relevance and would hide inherited
governance even when it is the stronger match.

### Boost inherited standards

Rejected. Firm-wide origin is provenance, not permission to outrank a stronger
local lexical match.

### Search separate indexes and interleave results

Rejected. Scores from separately calculated BM25 corpora are not directly
comparable, and interleaving introduces an arbitrary quota policy.

## Related Decisions

- adr-033
- adr-066
- adr-078
- adr-089
- adr-103
- adr-128
- adr-135
- adr-136
- adr-138

## Related Designs

- corpus-federation-mechanism

## Related Requirements

- rac-federated-resolution-provenance
