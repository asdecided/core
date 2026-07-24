---
schema_version: 1
id: RAC-KWPC5P5APA9G
type: roadmap
---
# Artifact-Completeness Benchmark

## Status

Planned

Unscheduled — captured as future intent, not yet on a release. Records the
intent to build **SWE-CompletenessBench**, a self-authored benchmark that
measures how completely a recorded corpus specifies a system: an external
agent reconstructs a target component from its Lore artifacts alone, and the
*residual* — the fraction that cannot be recovered from the record — is the
reported result. Sibling to SWE-DecisionBench (`decisiongrounding`); the
harness executes in `itsthelore/asdecided-benchmarks` (ADR-092), never in
`rac-core`. Framed as a completeness diagnostic, not a reconstruction
scoreboard, so it stays clear of the spec-as-source line the corpus refuses
(ADR-081: "Lore is not an SDD/codegen tool").

## Context

The recorded thesis is that durable, typed, human-ratified decision
knowledge is the layer coding agents are missing — but the corpus also
records its own frontier honestly: tacit product knowledge is built from
*unrecorded* observable decisions, and capture-at-the-moment is the one
capability Lore flags as absent (`growth-essay-mapping`, Row 6). No benchmark
measures that frontier. SWE-CompletenessBench does: give an agent only a
system's recorded artifacts and ask it to rebuild a component; whatever it
*cannot* recover is a direct measurement of what was never written down. The
result is a knowledge-completeness number and a residual — "the record fully
specified X% of the observable component; (1−X) stayed tacit" — which turns
the corpus's own honest premise into evidence.

This is deliberately *not* a claim that Lore generates systems from specs.
Lore only serves the recorded corpus deterministically (ADR-002, ADR-036); a
bring-your-own external agent does any building, in `rac-benchmarks`. The
residual is expected to be non-trivial — the thesis predicts an incomplete
rebuild — so a bounded ceiling is a confirmation, not a failure.

## Outcomes

- A published completeness diagnostic: for a system with a recorded Lore
  corpus, the fraction of its tested behaviour recoverable from the
  artifacts alone, with the residual (unrecorded/tacit) gap reported as the
  headline finding.
- Evidence for which artifact families carry the most reconstruction signal
  — a per-family ablation that tells teams what is worth recording.
- A second evidence pillar for the `decision-grounding-paper`: where
  SWE-DecisionBench measures the *lift* recorded decisions give an agent,
  SWE-CompletenessBench measures the *completeness* of the record itself.

## Initiatives

### Initiative 1 — Dataset construction

Assemble targets that have both a recorded artifact corpus and executable
tests. Sequence by honesty: (a) `rac-core` dogfoods itself — rebuild a
`rac-core` component from `rac-core`'s own `rac/` artifacts, scored by the
engine's tests (available now, N=1); (b) retrofit open-source projects that
carry ADRs plus tests; (c) synthetic artifact bundles for known components.
Dataset construction, not the harness, is the make-or-break.

### Initiative 2 — Harness, arms, and the residual metric

A `rac-benchmarks` subdir on the shared harness (ADR-097): arms
`none` / `RAG` / `rac-corpus`, plus a **full-source oracle** ceiling arm,
each feeding the same external answering agent. Scoring is the target's
executable tests — deterministic, offline, no LLM judge in the scored path
(ADR-066). The reported hero metric is the **residual** (oracle − rac);
reconstruction pass-rate is a proxy, not the headline.

### Initiative 3 — Evidence tie-in

Fold the result into the `decision-grounding-paper` as the completeness
pillar alongside SWE-DecisionBench's adherence pillar, and into the
`external-benchmark-evidence` programme. The residual finding is reported
honestly whichever way it falls.

## Constraints

- Lore serves the corpus; it never generates. The answering agent is
  external and bring-your-own (ADR-002, ADR-035); all building lives in
  `rac-benchmarks`, never `rac-core` (ADR-092).
- Scoring is deterministic and executable, no embeddings or LLM judge in the
  scored path; byte-stable metrics; human-gated baseline (ADR-066, ADR-097).
- The benchmark is a completeness diagnostic; the residual is the headline,
  reconstruction pass-rate a proxy — never a "Lore rebuilds systems" claim.

## Non-Goals

- Positioning Lore as a spec-as-source / codegen / MDA tool (ADR-081); the
  benchmark's own local record states the "not the assembler" discipline
  (`commercial-layer-positioning`) up front.
- Reconstruction success-rate as the hero number.
- Any generation, embedding, or LLM-judged scoring inside `rac-core`
  (ADR-066, ADR-092).
- Duplicating SWE-DecisionBench: that measures grounding lift on a per-patch
  task; this measures corpus completeness on a whole-component rebuild.

## Success Measures

- SWE-CompletenessBench reports, per arm, a reconstruction pass-rate and the
  residual (oracle − rac) on at least the `rac-core` dogfood target, with a
  per-artifact-family ablation.
- The result is reproducible and deterministic where scored (ADR-066/097),
  built entirely in `rac-benchmarks` with zero `rac-core` engine imports.
- The residual is reported honestly, including a large or inconvenient gap.

## Assumptions

- Targets with both a recorded artifact corpus and executable tests can be
  sourced — starting with `rac-core` itself.
- Executable test pass-rate is a defensible proxy for recoverable behaviour;
  the residual is a defensible proxy for the tacit frontier.
- A completeness diagnostic strengthens the recorded positioning (ADR-036,
  ADR-081) rather than straining it, because the thesis predicts a residual.

## Risks

- External evaluators misread "rebuild from artifacts" as a spec-as-source
  claim (ADR-081: "evaluators misfile Lore against whichever category they
  arrived from"). Mitigation: lead every artifact with residual/completeness,
  keep the agent external and BYO, and record the codegen-boundary discipline
  in the benchmark's own local ADR.
- The dataset is too thin for a credible result (N=1). Mitigation: the
  `rac-core` dogfood is an honest MVP; OSS-with-ADRs retrofit scales it.
- The name reads generative. Mitigation: the completeness-forward name
  SWE-CompletenessBench; the design records SWE-ArtifactRecoverability as an
  alternative to settle before publication.

## Related Decisions

- adr-002
- adr-036
- adr-066
- adr-081
- adr-092
- adr-097

## Related Roadmaps

- decision-grounding-paper
- external-benchmark-evidence
- growth-programme

## Related Designs

- artifact-completeness-diagnostic

## Related Requirements

- rac-artifact-completeness-study

## Related Tickets

- itsthelore/asdecided-core#299
