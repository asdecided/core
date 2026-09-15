---
schema_version: 1
id: RAC-KWGHXS2YNMNR
type: roadmap
---
# Deterministic Substrate Programme

## Status

Planned

## Outcomes

Advance rac-core as the deterministic knowledge layer underneath AI coding
agents: every outcome below strengthens what the engine can assert
deterministically, or widens who consumes those assertions, without adding
inference to the engine (ADR-002, ADR-066).

- An agent editing a file can deterministically ask which decisions govern
  that path and get a validated, declared answer at the moment of edit.
- Staleness and drift are loud: git-derived freshness signals and advisory
  "suspect" findings give teams the rot evidence that today's agent-context
  tools structurally cannot show — the number-one documented abandonment
  driver, answered deterministically.
- External memory and RAG layers ground against RAC's validated graph
  through stable, one-way export projections, instead of re-inferring it.
- Extensibility is proven in sequence: the built-in family factory first,
  then the ADR-083 pinned spec-bundle registry, so third parties can model
  their own knowledge families without forking the engine.
- The harness funnel widens at zero engine cost through verified
  integration recipes and a measured, regression-checked agent-facing
  context footprint.

## Initiatives

Sequenced by a council-of-eight scoring pass (method and averages below).
Each member item remains recorded in `rac/roadmaps/future/` and graduates
to its own scoped roadmap when picked up; this programme records the
ranking, the sequence, and the constraints that reviews must hold.

- Tranche A — core differentiators and free wins:
  - Decision-to-code proximity (`decision-to-code-proximity`): declared,
    validated code-scope references plus a deterministic path→decisions
    lookup across CLI and MCP. Precondition for the drift gate below.
    Graduated out of `future/` to its own scoped roadmap; the Tranche A
    constraint pattern is carried in that roadmap's constraints.
  - Freshness and drift detection, phase 1
    (`freshness-and-drift-detection`): additive git-derived staleness
    fields and an advisory drift finding in doctor/review. Graduated out
    of `future/` to its own scoped roadmap, with the phase-1 fence
    (staleness fields plus advisory finding only) recorded there.
  - Integration recipe factory (`integration-recipe-factory`): the
    reusable harness recipe and first verified examples; zero engine diff.
    Graduated out of `future/` to its own scoped roadmap, with the boundary
    constraint (context-supply and post-edit CI only, never pre-edit
    interception) carried there.
  - Lean context delivery (`lean-context-delivery`): measure the MCP
    tool-surface cost and hold it to a regression-checked budget. Graduated
    out of `future/` to its own scoped roadmap, with the deterministic
    accounting method recorded in a paired design.
- Tranche B — reach:
  - Retrieval diagnostics (`retrieval-diagnostics`): explain-miss and the
    floor-ratio bounded-boost gate, golden-pinned. Graduated out of
    `future/` to its own scoped roadmap now its sequencing condition is
    met — the relevance-ranking work it complements is Achieved.
  - Corpus export projections (`corpus-export-to-rag-backends`): stable
    documents and nodes+edges projections; reference adapters live in
    rac-connectors with at least one self-hostable target. Shipped as the
    v0.25.0 export projections (ADR-074) and the `rac-connectors`
    companion (ADR-095); the residual documentation deliverable is
    absorbed into `corpus-sync`, and the artifact is Achieved.
- Tranche C — extensibility, deliberately ordered:
  - Artifact family factory (`artifact-family-factory`): the family
    creation contract proven on one built-in Risk pilot first, with no
    published plugin API commitment. Graduated out of `future/` to its
    own scoped roadmap; each instantiated family lands its own ADR at
    implementation.
  - Third-party artifact types (ADR-083, revised for the native engine):
    the pinned spec-bundle registry — inert registry seam first, bundle
    loading and generic validation second, the hardcoded OKF type map
    closed by the per-type `okf_type` mapping in the same acceptance; the
    public invitation stays behind GATE-2.
- Deferred until their stated triggers:
  - Team-scale serving (`lore-at-team-scale`): its 50+ developer trigger
    has been met by an organisation-scale rollout, and the item has
    graduated out of `future/` to its own scoped roadmap; the entry
    conditions recorded here — mandatory audit-on and cache coherency
    guarantees — are carried into that roadmap's initiatives.
  - Skill trust and surfacing (`skill-trust-and-surfacing`): waits for
    third-party skills becoming real.

## Council Ranking

Eight scoring lenses (product strategy, core engineering, developer
experience, agent integration, enterprise/security, open-source community,
QA/release, competitive analysis) each scored all ten candidates 1–10 on
product impact, technical challenge (a cost; 10 is hardest), and adoption
drive. VALUE is the mean of impact and adoption averaged across the eight
seats; V/E divides VALUE by challenge.

| Rank | Item | Impact | Challenge | Adoption | VALUE | V/E |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | decision-to-code-proximity | 7.88 | 5.62 | 6.62 | 7.25 | 1.29 |
| 2 | freshness-and-drift-detection | 7.50 | 5.88 | 6.25 | 6.88 | 1.17 |
| 3 | corpus-export-to-rag-backends | 6.62 | 5.38 | 6.88 | 6.75 | 1.26 |
| 4 | lore-at-team-scale | 6.50 | 8.12 | 6.88 | 6.69 | 0.82 |
| 5 | third-party artifact types (ADR-083) | 6.88 | 6.50 | 6.38 | 6.62 | 1.02 |
| 6 | integration-recipe-factory | 4.12 | 2.12 | 7.50 | 5.81 | 2.74 |
| 7 | artifact-family-factory | 6.38 | 5.00 | 4.75 | 5.56 | 1.11 |
| 8 | retrieval-diagnostics | 6.25 | 4.12 | 4.38 | 5.31 | 1.29 |
| 9 | lean-context-delivery | 5.38 | 2.62 | 4.62 | 5.00 | 1.90 |
| 10 | skill-trust-and-surfacing | 5.12 | 4.75 | 4.38 | 4.75 | 1.00 |

## Success Measures

- Each tranche item graduates from `rac/roadmaps/future/` as its own scoped
  roadmap before implementation begins, and its execution is tracked in a
  GitHub issue per ADR-093.
- `rac validate rac/`, `rac relationships rac/ --validate`, and
  `rac review rac/` stay clean across the programme's output.
- Every initiative traces back to its source artifact in this corpus; no
  scope enters the programme without a recorded home.
- The Tranche A pair ships with the constraint pattern intact: proximity
  references are declared and validated, never inferred; drift findings are
  advisory before they are ever a gate.

## Assumptions

- ADR-083's split holds: the plugin mechanism may ship while the public
  ecosystem invitation stays gated behind GATE-2 (CLA).
- The deferred items' triggers remain as recorded in their artifacts
  (team-scale: 50+ developer signal; skill trust: third-party skills).
- ADR-036 positioning is unchanged by this programme; the ranking sharpens
  the substrate story, it does not reposition the product.
- The council averages are a planning input, not a contract; re-ranking is
  cheap if the landscape shifts, and the method is recorded here so it can
  be re-run.

## Risks

- A SaaS export adapter is egress that undercuts the ADR-086 air-gap
  posture that wins security reviews; mitigated by keeping adapters in
  rac-connectors, off by default, with at least one self-hostable
  reference target.
- Git-derived drift findings could destabilise the deliberately
  git-state-independent golden outputs; mitigated by keeping drift findings
  out of byte-pinned goldens or fully controlling git state in fixtures.
- The plugin registry converts a load-bearing closed type set into dynamic
  discovery — the highest structural-debt item scored; mitigated by the
  provably-inert first seam and by closing the hardcoded OKF type map in
  the same change.
- Integration recipes rot silently because real-harness verification is
  not CI-automatable; mitigated by the ecosystem-listing verification gate
  and dated verification markers.

## Related Decisions

- adr-083
- adr-093
- adr-094
- adr-019
- adr-045
- adr-066
- adr-067
- adr-086
- adr-033
- adr-074
- adr-036

## Related Designs

- third-party-artifact-extensibility

## Related Roadmaps

- decision-to-code-proximity
- freshness-and-drift-detection
- integration-recipe-factory
- lean-context-delivery
- retrieval-diagnostics
- corpus-export-to-rag-backends
- artifact-family-factory
- lore-at-team-scale
- skill-trust-and-surfacing
- lore-supermemory-grounding
- growth-programme
- relationship-vocabulary
- corpus-sync
- corpus-federation
