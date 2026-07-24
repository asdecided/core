---
schema_version: 1
id: RAC-KTYB8RVVQ1HX
type: roadmap
---
# Growth Programme

## Status

Planned

## Outcomes

Grow adoption of Lore and the RAC engine without altering the recorded
product positioning (ADR-036), and produce empirical evidence for future
traceability work from real corpus usage.

- Evaluators understand within one README read how Lore relates to
  spec-driven development tools (Spec Kit, OpenSpec, Kiro): a complement
  that owns the durable requirements layer, not a competitor.
- A new user reaches a first validated artifact in under five minutes
  from a clean machine.
- A coding agent can author and maintain RAC artifacts in a host project
  through a shipped Claude Code skill.
- Derivative work (schemas, templates, examples) is visibly possible and
  invited, at minimum viable scale.
- Every place the corpus or this programme needed a relationship RAC
  cannot express is recorded as a concrete, designable gap.

## Initiatives

- Positioning and comparison: a verifiable, source-cited comparison with
  spec-driven development tools, below the README fold, governed by a
  requirement artifact.
- Adoption surface: zero-configuration install paths, a timed cold-start
  contract, a demo shot list, and the Claude Code skill.
- Traceability gap audit: one record per missing relationship type, each
  evidenced by at least three concrete instances in the existing corpus.
- Essay–artifact bridge: structural mapping between the product-knowledge
  essay series and corpus artifacts, with no prose written on the
  author's behalf.
- Ecosystem seed: extension-mechanism requirements, a minimal ecosystem
  list with only real entries, and a third-party contribution convention.

## Success Measures

- `rac validate rac/`, `rac relationships rac/ --validate`, and
  `rac review rac/` remain clean over the full programme output.
- Every produced artifact (README section, skill, lists) traces back to
  at least one requirement in the corpus.
- Every competitor claim carries a recorded source; unverified claims do
  not survive integration review.
- The consolidated gap report is specific enough to design schema
  features from.

## Assumptions

- ADR-036 (Lore product identity) remains the governing positioning; the
  README first screen is not repositioned by this programme.
- Items intended for public posting remain blocked behind GATE-1
  (employer external-communications and IP review), and contribution
  policy changes behind GATE-2 (CLA), until the maintainer releases them.

## Risks

- Comparison content drifts out of date as competitor tools change;
  mitigated by citing dated sources rather than asserting evergreen
  claims.
- Two-name positioning (Lore and RAC) confuses comparison framing;
  mitigated by stating the ADR-036 relationship sentence once per
  surface.

## Related Decisions

- adr-036

## Related Requirements

- rac-growth-positioning
- rac-growth-adoption
- rac-growth-agent-skill
- rac-growth-contribution-policy
- rac-growth-essay-bridge
- rac-growth-extensibility
- rac-growth-ecosystem-list
- rac-traceability-self-relationships

## Related Designs

- growth-demo-gif
- growth-essay-mapping

## Related Roadmaps

- relationship-vocabulary

## Related Tickets

- itsthelore/asdecided-core#230
