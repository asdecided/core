---
schema_version: 1
id: RAC-KWNYXB150JEE
type: requirement
---
# Requirement: SWE-DecisionBench Publication Study

## Status

Proposed

Classification: `[internal]` — the publication-grade evidence the paper
needs. Initiative 2 of the `decision-grounding-paper` roadmap. The multi-arm
head-to-head already exists as the `decisiongrounding` implementation in
`itsthelore/asdecided-benchmarks` (ADR-092), whose published dataset/paper identity
is **SWE-DecisionBench** (the name already recorded in
`external-benchmark-evidence`). This requirement scopes the deltas that take
it from a working benchmark to a citable SWE-family study; it does not build a
new harness. Referenced in prose because there is no relationship type for an
external-repository artifact (the gap recorded in `growth-essay-mapping`).

## Problem

ADR-081 records the open question — *"no published proof that curated decision
context lifts agent task success."* `decisiongrounding` already runs the
head-to-head: four arms (`no_grounding`, `naive_rag` — Voyage embeddings +
top-k, `rac` — typed retrieval following supersedes edges, and `context_dump`
— paste everything), each feeding the **same fixed answering model with the
same prompt scaffold**, so only grounding varies. Its primary metric is
**decision-adherence** (did the agent propose a prohibited or superseded
change), scored deterministically and structurally with no embeddings and no
LLM judge (ADR-066), swept across corpus size (10/50/150/300) with a
real-corpus pilot (PEP 386→440) and PEP/RFC distractor pools.

Publishing it as **SWE-DecisionBench** — a member of the SWE-bench family —
carries an expectation the current design does not yet fully meet: the family
(SWE-bench, SWE-ContextBench) implies real-repository, executable
verification, not only structural inspection of a proposed change. Earning the
name therefore requires a second, **executable, decision-conditioned
resolution** outcome to sit alongside adherence — both deterministic, so both
stay ADR-066-native. What is missing for the paper is thus: the executable
co-primary outcome, publication-grade statistical rigor, a scaled real-corpus
arm, and explicit positioning in the SWE-bench lineage.

## Requirements

- [REQ-001] The published evaluation MUST be **SWE-DecisionBench** — the published identity of the existing `decisiongrounding` multi-arm benchmark (the `no_grounding` / `naive_rag` / `rac` / `context_dump` arms over the same knowledge with a fixed answering model) — extended for publication, not a re-implemented head-to-head.
- [REQ-002] SWE-DecisionBench MUST report two co-primary, deterministic outcomes: (a) **decision-adherence** — structural, the novel construct that distinguishes it from resolution benchmarks; and (b) **decision-conditioned resolution** — executable task success where the correct patch depends on a governing decision, scored by upstream tests. Both are deterministic and ADR-066-native (no LLM judge in the scored path).
- [REQ-003] The executable resolution arm MUST use real-repository or version-conditioned tasks whose correct answer depends on a recorded governing decision (seeded by e.g. GitChameleon-style version-conditioned problems with executable tests), and MUST be built in `rac-benchmarks` (ADR-092), driving `rac` as an external CLI — never inside `rac-core`.
- [REQ-004] The `naive_rag` (embedding) arm and any LLM-judge fallback MUST remain comparative evidence only and MUST NEVER enter the scored or gated path of either co-primary outcome (ADR-066, ADR-097).
- [REQ-005] The study MUST add publication-grade statistical rigor: a pre-registered hypothesis and analysis, paired significance testing across arms on the same scenarios (e.g. McNemar) for each co-primary outcome, reported effect sizes with confidence intervals, and the corpus-size sweep reported as a curve.
- [REQ-006] The study MUST mature the real-corpus adherence arm beyond the PEP 386→440 pilot to a study-grade set (scaled PEP/RFC or comparable governing-decision corpora with their distractor pools), so the result generalises past the synthetic scenarios.
- [REQ-007] The paper MUST position SWE-DecisionBench explicitly in the SWE-bench lineage — SWE-bench (issue *resolution*) → SWE-ContextBench (episodic *context retrieval*, arXiv:2602.08316, the recorded neighbour) → **SWE-DecisionBench** (durable governing-*decision* adherence plus decision-conditioned resolution) — citing the family without implying endorsement or affiliation.
- [REQ-008] The study MUST report its result faithfully, including a null or partial outcome (the recorded SWE-DecisionBench honesty rule): a result where `rac` beats `naive_rag` on stale/prohibited-decision adherence but not on raw resolution is a publishable finding, not one to suppress.
- [REQ-009] No embeddings, vector search, or LLM judge MAY be introduced into `rac-core` itself (ADR-066, ADR-080); the `naive_rag` and the executable resolution arms live in `rac-benchmarks` (ADR-092).

## Acceptance Criteria

- Both co-primary outcomes are reported: structural decision-adherence and
  executable decision-conditioned resolution, per arm, with the `rac` arm's
  deterministic scoring reproducing exactly on re-run.
- The executable arm runs real-repository or version-conditioned tasks scored
  by upstream tests, built in `rac-benchmarks`, with no engine dependency.
- Results report paired-test significance and effect sizes with confidence
  intervals for each outcome, plus the adherence-vs-corpus-size curve.
- The real-corpus adherence arm runs on a set materially larger than the
  PEP 386→440 pilot, with its distractor pool documented.
- The `naive_rag` arm reports its pinned embedding model, index build,
  answering-model version, and run-to-run variance, and never gates either
  outcome.
- The paper's related-work section places SWE-DecisionBench in the
  SWE-bench → SWE-ContextBench lineage.
- The `rac-core` CI grounding eval (`rac-grounding-eval-benchmark`) is
  unchanged: no embeddings, no LLM judge, no network in its gated path.

## Success Metrics

- The paper cites a publication-grade SWE-DecisionBench result answering
  ADR-081's open question — measured deltas (in either direction) between
  deterministic decision grounding, semantic RAG, context-dump, and no
  grounding, on both adherence and executable resolution — on a set a
  reviewer would accept as a SWE-family benchmark.

## Risks

- The SWE- name over-promises if the executable arm stays thin. Mitigation:
  REQ-002/003 make executable resolution co-primary, not optional, so the
  badge is earned rather than borrowed.
- The `naive_rag` baseline looks like a straw man. Mitigation: it already uses
  a current embedding model (Voyage) with asymmetric query/document roles;
  publish its configuration and invite replication via `rac-benchmarks`.
- Construct validity — adherence versus resolution. Mitigation: reporting both
  as co-primary is the design response; adherence measures the thesis cleanly,
  resolution supplies the executable rigor the family expects.
- Scope creep turns a paper study into an open-ended benchmark programme.
  Mitigation: this requirement scopes the SWE-DecisionBench publication
  extension of one existing benchmark; broader external-benchmark tables stay
  in `external-benchmark-evidence`.

## Assumptions

- `decisiongrounding` / SWE-DecisionBench is the right base to extend rather
  than a new harness to build; its four-arm, fixed-answering-model design is
  sound for the claim.
- Decision-conditioned executable tasks can be sourced or curated (GitChameleon
  version pins and comparable) where the correct patch depends on a recorded
  decision an ungrounded or semantically-retrieved agent would miss.
- Running the `naive_rag` and executable arms outside `rac-core` preserves the
  engine's no-embeddings posture (ADR-066, ADR-080).

## Related Decisions

- adr-066
- adr-081
- adr-092
- adr-097

## Related Roadmaps

- decision-grounding-paper
- external-benchmark-evidence

## Related Requirements

- rac-grounding-eval-benchmark
