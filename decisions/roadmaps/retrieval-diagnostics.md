---
schema_version: 1
id: RAC-KVTP8MN27M64
type: roadmap
---
# RAC — Retrieval Diagnostics

## Status

Planned

Prioritised as the rank-8 Tranche B item of the deterministic-substrate
programme, graduated out of `future/` now its recorded sequencing condition
is met: the relevance-ranking work these refinements complement (the
`deterministic-relevance-ranking` design and ADR-078) is Achieved, so the
gate has a shipped boost to bound and explain-miss has a shipped
explain-hit to mirror. Requirements are minted for Initiatives 1 and 2
only; the two-tier recall framing (Initiative 3) is a documentation
deliverable, not a behaviour. Execution is tracked in GitHub (ADR-093): the
epic in `## Related Tickets` carries ordering and task state.

## Context

The deep dive into hybrid-retrieval systems surfaced two deterministic ideas Lore
does not yet have, both inside its no-embeddings, no-LLM line (ADR-066, ADR-002):

- **Explain-miss.** GBrain ships `search diagnose` — "trace which retrieval layer
  surfaces or misses a page." Lore has explain-*hit* (the v0.23 explainable-
  retrieval evidence: winning field, matched terms, tier) but no way to ask "why
  did my query *not* surface artifact X." For an author maintaining a corpus, the
  absence is often the more useful question.
- **Floor-ratio gate.** GBrain bounds its boosts with a "floor-ratio gate" that
  "prevents weak candidates from exceeding primary results via boosting." Lore's
  planned graph/relevance boost (ADR-078) is specified as *bounded*; the floor-ratio
  gate is a concrete, deterministic mechanism that guarantees a boosted weak result
  can never outrank a strong lexical match.

Hermes adds a framing note worth recording: it cleanly separates *bounded always-
in-context facts* from *on-demand deterministic full-text recall* — a two-tier
recall model Lore can articulate (skills/summaries always surfaced vs. deep
`search_artifacts` on demand) without new code.

## Outcomes

- An author can ask "why did my query not surface artifact X?" and get a
  deterministic trace (which tier or term excluded it), complementing explain-hit.
- The relevance/graph boost is provably bounded: a floor-ratio gate makes "the
  boost cannot float a weak result above a strong match" a tested guarantee, not a
  hope.

## Initiatives

### Initiative 1 — Explain-miss (`diagnose`)

Extend the explainable-retrieval surface (`rac find --explain` / the search
evidence) to explain *absences*: given a query and a target artifact, report
deterministically why the target did not rank or did not match (no term hit a
tier, a term matched nothing, the budget truncated it). No model; pure trace over
the existing matcher.

### Initiative 2 — Floor-ratio bounded-boost gate

Fold a floor-ratio gate into the relevance-ranking boost so a boosted candidate
cannot exceed a primary lexical match beyond a bounded ratio — the concrete
realisation of ADR-078's "bounded graph boost." Pinned by golden tests.

### Initiative 3 — Two-tier recall framing

Document the recall model explicitly — always-surfaced context vs. on-demand
search — so the split is stated for agents and authors. Documentation/UX, not new
behaviour.

## Constraints

- Deterministic and offline (ADR-002, ADR-066): diagnostics and the gate are pure
  functions of corpus bytes and the query; no embeddings, no model.
- Additive contract (ADR-007): explain-miss output and any score detail are new
  optional fields; existing search responses are unchanged.
- Reuse the existing matcher, tokeniser (ADR-037), and tiers (ADR-038); no parallel
  search path.

## Non-Goals

- Vector or semantic diagnosis of why a result was or was not relevant.
- Any change to which artifacts match; diagnostics explain the existing behaviour,
  and the gate only re-orders within already-matched results.

## Success Measures

- For a query that fails to surface a known artifact, `diagnose` names the
  deterministic reason, reproducibly.
- With the floor-ratio gate, no boosted result outranks a stronger lexical match
  beyond the bounded ratio, proven by golden tests.

## Assumptions

- "Why didn't it find X" is a common authoring question once a corpus is large
  enough that relevance ranking matters (the v0.29 trigger).
- A bounded-boost guarantee is more valuable expressed as a tested gate than as
  prose in the ranking design.

## Risks

- Explain-miss could over-report trivial non-matches. Mitigation: scope it to a
  named target artifact (why did *this* not surface), not "everything that didn't
  match."

## Related Decisions

- adr-002
- adr-007
- adr-037
- adr-038
- adr-066
- adr-078
- adr-093

## Related Roadmaps

- deterministic-substrate
- relevance-ranking

## Related Requirements

- rac-explainable-retrieval
- rac-explain-miss-diagnostics
- rac-floor-ratio-boost-gate

## Related Tickets

- itsthelore/asdecided-core#248
