---
schema_version: 1
id: RAC-KVSQ27BCJ0TC
type: roadmap
---
# Deterministic Relevance Ranking

## Status

Achieved

## Context

Search filters correctly but orders coarsely. The tier ladder (ADR-037, ADR-038)
ranks by id, then title, then path, then heading, then body, breaking ties by
sorted path — so within a tier, order is alphabetical, not relevant. Under the
MCP response budget that ordering decides what an agent even sees: a weak body
hit sorted ahead of a strong one can crowd the strong one out of the window.

Two deterministic signals are already in the corpus and unused for ranking: term
frequency within a field, and the validated relationship graph. This release uses
them. ADR-078 refines ADR-038's ranking clause to a deterministic relevance score
— BM25-family lexical scoring, RRF fusion, and a bounded graph boost — while
reaffirming the hard line both ADR-038 and ADR-066 draw: no embeddings, no
semantic scoring, no LLM, ever. The *how* is in the `deterministic-relevance-
ranking` design. It is the deterministic half of what hybrid retrieval systems do;
the vector half stays out by decision.

## Outcomes

- A search (CLI or MCP) returns its matches ordered by deterministic relevance, so
  the most relevant artifact lands at or near the top — and inside the response
  budget — instead of wherever path alphabetisation put it.
- The order stays reproducible and explainable: same bytes and query give the same
  ranking, and `--explain` / the JSON show the score components behind it.
- The improvement is measured, not asserted: the grounding benchmark holds or
  improves against its committed baseline.

## Initiatives

### Initiative 1 — BM25 lexical scoring (Core)

Score each matchable field (id, title, path, heading, body) with a BM25-family
term-frequency/IDF score over the query terms, reusing ADR-037's token-boundary
tokeniser, with field weighting that preserves the tiers' relative importance as a
graded contribution rather than a hard cutoff. No stemming, no synonyms, no
embeddings.

### Initiative 2 — RRF fusion (Core)

Fuse the per-field ranked lists with Reciprocal Rank Fusion (`Σ 1/(k + rank)`,
`k = 60`), replacing the "best tier then path" tiebreak with a parameter-light,
deterministic merge. The matched set is unchanged; RRF only reorders it.

### Initiative 3 — Bounded graph boost (Core)

Add a deterministic boost from the validated relationship graph (normalised,
capped inbound-reference signal from the existing relationship index) as one more
fused signal, so connectedness breaks near-ties toward the artifact the corpus
leans on — bounded so it cannot dominate lexical relevance.

### Initiative 4 — Explainability and contract

Extend the search result with additive score-component fields (per-signal
contributions and the fused score) alongside the existing `{field, terms, tier}`
evidence (ADR-007; `schema_version` unchanged), surfaced through `--explain` and
`--json` on both `rac find` and `search_artifacts` (ADR-031).

### Initiative 5 — Quality gate and determinism tests

Golden tests pin byte-stable ordering and tie-breaking; the grounding benchmark
(`rac eval --check`, ADR-066) must hold or improve Precision@k / Recall@k against
the committed baseline before the ranking change ships.

## Constraints

- No embeddings, semantic scoring, stemming, or synonym expansion — ever
  (ADR-038, ADR-066).
- Deterministic and offline: identical bytes and query yield byte-identical order.
- Additive contract (ADR-007): existing search fields unchanged.
- Reuse the existing matcher, tokeniser, relationship index, and eval harness;
  Core serves both surfaces (ADR-031).

## Non-Goals

- Any vector, embedding, or similarity-based ranking.
- Query reformulation, stemming, or synonym expansion (the agent's job).
- A new search verb or surface; this re-orders the existing ones.
- Widening the matched set; ranking only reorders what already matches.

## Success Measures

- `rac eval --check` Precision@k / Recall@k hold or improve versus the committed
  baseline after the ranking change.
- Re-running a query on an unchanged corpus yields byte-identical ordering.
- For a multi-body-match query, the maintainer-judged most relevant artifact ranks
  at or near the top where tier-then-path left it ordered by path.

## Assumptions

- Lexical term-frequency relevance plus graph connectedness orders results closer
  to intent than tier-precedence-then-path, and the gain is measurable on the
  benchmark.
- BM25, RRF, and graph centrality are deterministic and explainable, so they sit
  inside ADR-038/ADR-066's prohibition.

## Risks

- **Complexity** beyond the simple ladder ADR-038 chose. Mitigation: determinism,
  golden tests, explainable components, and the eval gate.
- **Hub over-boosting** onto unrelated queries. Mitigation: bound the graph signal
  and fuse via RRF rather than letting it lead; tune against the benchmark.

## Related Requirements

- rac-deterministic-relevance-ranking

## Related Decisions

- adr-078
- adr-038
- adr-037
- adr-066

## Related Designs

- deterministic-relevance-ranking

## Related Tickets

- itsthelore/asdecided-core#226
