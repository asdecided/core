---
schema_version: 1
id: RAC-KWPC5TNY8VKA
type: requirement
---
# Requirement: Artifact-Completeness Study

## Status

Proposed

Classification: `[internal]` — the testable contract for
**SWE-CompletenessBench**. Initiative 2 of the
`artifact-completeness-benchmark` roadmap; the harness executes in
`itsthelore/asdecided-benchmarks` (ADR-092), never in `rac-core`. Referenced in
prose because there is no relationship type for an external-repository
artifact.

## Problem

The corpus records a tacit frontier it cannot yet measure: tacit product
knowledge is built from unrecorded observable decisions, and
capture-at-the-moment is the one capability Lore flags as absent
(`growth-essay-mapping`, Row 6). SWE-CompletenessBench measures it by
reconstructing a component from its recorded artifacts and reporting the
residual. To be credible and on-thesis it needs a testable contract: the
residual is the reported outcome (not reconstruction success), scoring is
deterministic and executable (no LLM judge), Lore serves but never generates,
and the whole thing lives outside `rac-core`.

## Requirements

- [REQ-001] The study MUST reconstruct a target component from its recorded Lore artifact bundle using an external, bring-your-own answering agent; Lore's role is limited to serving the recorded corpus deterministically (ADR-002, ADR-036), and no generation, embedding, or LLM-judged scoring occurs inside `rac-core` (ADR-092).
- [REQ-002] The study MUST run four arms feeding the same fixed answering agent and scaffold, varying only the knowledge source: `none`, `RAG` (semantic retrieval over the same artifacts), `rac-corpus` (deterministic Lore serving), and a full-source `oracle` ceiling.
- [REQ-003] Scoring MUST be the target's own executable tests — deterministic, offline, no embeddings and no LLM judge in the scored path (ADR-066, ADR-097) — with a byte-stable `metrics` block and a human-gated baseline.
- [REQ-004] The reported hero outcome MUST be the **residual** (oracle − rac): the fraction of tested behaviour recoverable from full source but not from the recorded artifacts. Reconstruction pass-rate MUST be reported as a proxy only, never as a "Lore rebuilds systems" claim (ADR-081).
- [REQ-005] The study MUST report the grounding **lift** (rac vs none, rac vs RAG) and a **per-artifact-family ablation** (requirements vs decisions vs designs) attributing reconstruction signal.
- [REQ-006] The benchmark's own local record MUST state the spec-as-source / codegen boundary up front (ADR-081; the `commercial-layer-positioning` discipline), so external evaluators do not misfile the study as a generation claim.
- [REQ-007] The study MUST report its residual faithfully, including a large or inconvenient gap: a substantial residual is a valid finding about the tacit frontier, not a result to suppress.
- [REQ-008] The harness MUST be a `rac-benchmarks` subdir on the shared family harness, driving `rac` as an external CLI with zero engine imports (ADR-097).

## Acceptance Criteria

- On at least the `rac-core` dogfood target, the study reports per-arm
  reconstruction pass-rate and the residual (oracle − rac), reproducibly.
- The `rac` arm's deterministic serving reproduces exactly on re-run; the
  `RAG` arm reports its pinned embedding model, index build, and answering
  agent/model version.
- Scoring is executable tests only; no embeddings or LLM judge appears in the
  scored path; the `metrics` block is byte-identical across runs on an
  unchanged target.
- The per-family ablation attributes reconstruction signal across
  requirements / decisions / designs.
- Nothing in `rac-core` is modified; the harness imports no engine code
  (asserted by a test).
- The reported artifacts lead with residual/completeness; the codegen
  boundary is stated in the benchmark's local record.

## Success Metrics

- SWE-CompletenessBench produces a defensible completeness number and
  residual for a system with a recorded Lore corpus — a measured tacit
  frontier a reviewer would accept — composing with SWE-DecisionBench as the
  paper's second evidence pillar.

## Risks

- External misread as spec-as-source. Mitigation: REQ-004/006 make the
  residual the headline and record the codegen boundary; the agent is
  external and BYO.
- Thin dataset (N=1 dogfood). Mitigation: the `rac-core` dogfood is an honest
  MVP; the roadmap sequences OSS-with-ADRs retrofit to scale it.
- The RAG arm is a straw man. Mitigation: pin a current embedding retriever
  and publish its configuration, mirroring `rac-grounding-baseline-study`.

## Assumptions

- A target with both a recorded artifact corpus and executable tests exists —
  starting with `rac-core` itself.
- Executable pass-rate is a defensible proxy for recoverable behaviour and
  the residual for the tacit frontier.
- Running the reconstruction and the RAG arm outside `rac-core` preserves the
  engine's serve-not-generate posture (ADR-002, ADR-092).

## Related Decisions

- adr-002
- adr-036
- adr-066
- adr-081
- adr-092
- adr-097

## Related Roadmaps

- artifact-completeness-benchmark
- external-benchmark-evidence

## Related Requirements

- rac-grounding-baseline-study
