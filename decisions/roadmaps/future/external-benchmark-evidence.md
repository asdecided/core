---
schema_version: 1
id: RAC-KWGKQMZ68A1Z
type: roadmap
---
# External Benchmark Evidence (Future)

## Status

Planned

Unscheduled — captured as future intent, not yet on a release. Codename:
`external-benchmark-evidence`.

## Context

The evidence for "rac-core is actually useful" is currently self-authored: the
per-tool suite (`tool-benchmarks`) proves the retrieval and contract
mechanisms, and SWE-DecisionBench proves the outcome on our own scenario set.
Third-party credibility needs recognized external benchmarks — the kind hosted
on HuggingFace and cited in vendor comparison tables. The agent-memory market
already runs on exactly such a table: Mem0, Zep, Letta, and newer entrants all
publish LoCoMo and LongMemEval numbers, and buyers read those comparisons.
Lore does not appear in any of them.

A survey of the landscape (July 2026) found two joint first-priority
candidates and a tracked tier:

- **LongMemEval** — around five hundred questions over interaction histories
  of roughly 115k tokens; its *knowledge update* and *temporal reasoning*
  categories are the closest external analogue to Lore's supersession
  defense. This is the arena where memory vendors publish. Caveats: a
  conversational-QA contract (needs a capture adapter from sessions to
  artifacts) and partially LLM-judged scoring.
- **GitChameleon 2.0** — 116 version-conditioned Python problems with
  executable unit tests. "Code against the superseded API" is Lore's thesis
  in code form, and execution-based scoring is deterministic — the only
  external candidate that fits the ADR-066 posture natively.
- Tracked tier: LoCoMo (the most widely reported memory benchmark; shares the
  LongMemEval adapter), tau-bench and tau2-bench (policy-adherent agents;
  stochastic simulated users), SWE-ContextBench (arXiv:2602.08316, already
  the SWE-DecisionBench paper's named neighbour), ContextBench
  (arXiv:2602.05892, context retrieval in coding agents), CRAG / RAGTruth /
  FreshQA (RAG correctness, faithfulness, and recency; thematic support
  only), and the newer memory wave (MemBench, StreamMemBench, BEAM).

## Outcomes

- Lore appears in at least one recognized external benchmark table
  (LongMemEval first) with reproducible, honestly framed numbers and a
  published one-command reproduction path.
- A deterministic external result (the GitChameleon rac-grounding arm)
  showing version-correct code generation improving when the agent is
  grounded in version-governing decisions, scored by the benchmark's own
  executable tests.
- External evidence runs stay structurally distinct from merge gates:
  upstream scoring (including LLM judges) never gates CI (ADR-066, ADR-097);
  evidence reports are labelled with the upstream harness version and scoring
  mode.

## Initiatives

- **LongMemEval adapter arm** — a new `rac-benchmarks` subdir (ADR-092 family
  form) wrapping the upstream harness with a Lore arm: ingest interaction
  history as captured artifacts, answer from Lore retrieval, score with the
  upstream scorer. Report per-category results, leading with knowledge-update
  and temporal-reasoning.
- **GitChameleon rac-grounding arm** — a new subdir reusing the benchmark
  repository's DG-ADR-0001 provider pattern: a held-constant answering model
  with arms differing only in grounding (none, RAG, a rac corpus of
  version-governing decisions), scored by the upstream executable tests.
- **Licensing and provenance check per dataset** before anything is
  committed (upstream licenses vary; ADR-071 posture on our side).
- **LoCoMo extension** — reuse the LongMemEval capture adapter once it
  exists.
- **Publish SWE-DecisionBench to HuggingFace** (dataset plus leaderboard
  space) — the flip side of the same credibility goal; extends the
  `publish-swe-decisionbench` roadmap's submit initiative without expanding
  this item.

## Success Measures

- A published Lore LongMemEval result with a reproduction command, cited in
  at least one third-party comparison within two quarters of publication.
- The GitChameleon rac arm shows a statistically defensible delta over
  no-grounding on version-correct generation, or the losing result is
  published plainly (the SWE-DecisionBench honesty rule applies).
- No external benchmark's scoring path ever appears in a CI merge gate.

## Assumptions

- Upstream datasets remain available under licenses compatible with running
  and publishing results.
- The memory-vendor comparison table remains the buying-decision surface for
  agent-memory products.

## Risks

- **Vendor-tuned-harness credibility trap.** Self-reported memory benchmark
  numbers are widely discounted (independent reproductions of vendor scores
  differ by tens of points). Mitigation: publish the exact reproduction
  command and upstream harness pin with every number; never report a number
  that cannot be reproduced from a clean clone.
- **Contract mismatch.** LongMemEval's conversational-QA shape is not Lore's
  document-corpus shape; a weak adapter would measure the adapter, not Lore.
  Mitigation: the adapter is reviewed as its own design artifact before any
  funded run.
- **LLM-judge scoring drift.** Upstream judges change; evidence numbers are
  dated and pinned, never treated as stable contracts.

## Related Decisions

- adr-066
- adr-092
- adr-097
- adr-071

## Related Roadmaps

- tool-benchmarks

## Related Tickets

- itsthelore/asdecided-benchmarks#10
