---
schema_version: 1
id: RAC-KWNYX7KT1QN2
type: roadmap
---
# Decision-Grounding Paper

## Status

Planned

Unscheduled — captured as future intent, not yet on a release. **Blocked:
GATE-1** (employer external-communications / IP review), the same gate that
governs the growth-essay work; nothing here is published before it clears.
This records the intent to publish an academic (arXiv, then a peer venue)
position-and-evidence paper naming the layer the AI-coding-agent field is
missing, with Lore (built on the RAC engine, ADR-036) as the reference
implementation. It states positioning already recorded in ADR-036 and
ADR-081; it does not alter it.

Initiative 1 is complete: the `decision-grounding-thesis` design is Accepted
with its open questions resolved. Initiative 2 is run-ready in
`itsthelore/asdecided-benchmarks`: the paired-significance statistics, the
GitChameleon executable co-primary pipeline, the 49-scenario study-grade
roster, and the frozen analysis-plan amendment are landed; only the funded
model runs remain (the refreshed funded-run handoff covers both co-primary
outcomes). Initiative 3 remains blocked on GATE-1.

## Context

The AI-coding-agent stack has converged on two ways to give an agent
context, and both miss the same thing from opposite sides. Spec-driven
development tools (GitHub Spec Kit, AWS Kiro, Tessl, the Spec Growth Engine)
couple specs to *code* — encoding what to build, but code-bound, per-repo,
and mostly ephemeral or generative. Agent-memory and RAG systems (Mem0,
Zep, Cognee, Letta, Cursor rules) retrieve relevant *text* — nondeterministic,
untyped, and unable to signal "this is settled, do not re-open." Neither
treats the durable product *decision* — the why, the rejected alternatives,
the accepted constraints — as a first-class, typed, validated,
deterministically-retrievable artifact.

That absence is the named failure mode: agents confidently re-doing what a
team already ruled out, because the ruling-out was never machine-legible.
ADR-081 records the precise research gap this paper would close — *"no
published proof that curated decision context lifts agent task success."*
The corpus already holds the thesis (ADR-081, `rac-growth-positioning`,
`rac-growth-agent-memory-positioning`) and a deterministic evaluation
apparatus (ADR-066, ADR-097, `rac-grounding-eval-benchmark`); what is
missing is a paper that frames the gap generally and brings the evidence.

## Outcomes

- A general, citable framing of the missing layer — *deterministic,
  typed, human-ratified decision grounding* — with Lore as one reference
  implementation, positioned off the spec-driven axis rather than on it.
- Third-party-credible evidence for the claim ADR-081 flags as unproven:
  that curated, deterministic decision context measurably improves agent
  task outcomes over an ungrounded and a semantic-RAG baseline.
- A published record that strengthens the recorded positioning (ADR-036,
  ADR-081) without repositioning it.

## Initiatives

### Initiative 1 — The thesis and argument (`decision-grounding-thesis` design)

Develop the paper's argument in the corpus: the two-axis gap taxonomy
(code-coupling × determinism, and the empty quadrant), the synthesis of the
recorded positioning, the section outline, and the arXiv abstract. The
design is the durable home; the manuscript is its output.

### Initiative 2 — The evaluation (`rac-grounding-baseline-study`)

Mature the existing `decisiongrounding` head-to-head — published as
**SWE-DecisionBench** — into a citable SWE-family study: two co-primary
deterministic outcomes (decision-adherence and decision-conditioned
executable resolution) across the `no_grounding` / `naive_rag` / `rac` /
`context_dump` arms, extending the `external-benchmark-evidence` programme.
The semantic baseline is reported as evidence, never a CI gate (ADR-066,
ADR-097).

### Initiative 3 — Clearance and submission

Clear GATE-1 (external-communications / IP review), submit to arXiv
(cs.SE), and pursue a peer venue (an LLM4SE / agents-for-software-engineering
workshop) for citation signal. Author and affiliation are the maintainer's
call at pickup.

## Constraints

- Positioning is stated, not altered: ADR-036 (Lore is the product, RAC is
  the engine) and ADR-081 (the deterministic-source-of-truth-for-decisions
  thesis) are the governing record; the paper cites them.
- The reported evaluation honours the eval contract: no embeddings and no
  LLM judge in the *scored/gated* path (ADR-066, ADR-097); any semantic or
  LLM-judged arm is comparative evidence only, version-pinned, never a gate.
- No new engine scope: this is a positioning-and-evidence effort, not a
  product feature; it adds no CLI, schema, or contract behaviour.
- External publication waits on GATE-1.

## Non-Goals

- Repositioning Lore or altering ADR-036 / ADR-081.
- A marketing paper: Lore is a reference implementation and worked example,
  not the subject; the contribution is the framing plus the evidence.
- Embeddings, vector-RAG, or an LLM judge anywhere in the scored eval gate
  (ADR-066) — the semantic arm is a reported baseline only.
- Folding into the growth-essay series (`growth-essay-mapping`): that is a
  personal, non-academic genre with its own GATE-1 track; this is distinct.

## Success Measures

- The thesis design and the study requirement pass `rac validate`,
  `rac relationships --validate`, and `rac review` with no priority 1–2
  findings, so the paper's own argument clears the corpus gates it describes.
- The evaluation produces a defensible head-to-head result (RAC vs RAG vs
  none) on a study-grade query set with a downstream task-success outcome —
  or records honestly that the result did not support the claim.
- A submitted preprint that a reader can trace back, in prose, to the
  recorded decisions and the eval methodology.

## Assumptions

- GATE-1 can be cleared for an academic publication as it is anticipated
  for the essays; the timeline is external and unscheduled here.
- The `external-benchmark-evidence` "none / RAG / rac-corpus" arm and the
  GitChameleon executable-scoring skeleton are the right experimental base
  to extend, rather than a new harness.
- Position-and-evidence papers of this shape are acceptable on arXiv cs.SE
  (the Spec Growth Engine, arXiv:2606.27045, is a recent single-author
  precedent); peer-venue acceptance is a separate, later bar.

## Risks

- Reads as vendor marketing rather than a contribution. Mitigation: lead
  with the general framing and the evaluation; Lore is the worked example,
  not the pitch (the Non-Goals pin this).
- The evaluation fails to show a task-success lift. Mitigation: the study
  is framed to report the honest result; a null result still advances the
  question ADR-081 says is open, and the deterministic-retrieval regression
  evidence stands regardless.
- The paper cannot be a validated corpus edge — there is no relationship
  type for external/unpublished documents (recorded in `growth-essay-mapping`
  and `rac-growth-essay-bridge`). Mitigation: cite the manuscript and the
  preprints in prose; keep the durable argument in the design artifact.

## Related Decisions

- adr-036
- adr-066
- adr-081
- adr-097

## Related Roadmaps

- growth-programme
- external-benchmark-evidence
- commercial-layer-positioning

## Related Designs

- decision-grounding-thesis

## Related Requirements

- rac-grounding-baseline-study

## Related Tickets

- itsthelore/asdecided-core#293
