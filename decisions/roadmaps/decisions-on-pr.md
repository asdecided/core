---
schema_version: 1
id: RAC-KWQCQEMSM601
type: roadmap
---
# Decisions on Pull Requests

## Status

Achieved

Delivered (epic itsthelore/asdecided-core#351; itsthelore/asdecided-ci#3 merged) under
the `pr-decision-surfacing` design's contract. The **Herald** capability —
`itsthelore/asdecided-ci/herald/github` — computes the merge-base diff, runs
`rac decisions-for --json` per changed path (thin client, ADR-063; the
engine owns matching and liveness), and posts one deterministic advisory
comment: id, title, matched `## Applies To` scope, and a head-pinned link
per governing decision, deduplicated, sorted, updated in place by marker on
re-runs, with a fork-safe step-summary fallback. Facts, never a verdict or
merge gate (ADR-034, ADR-067). Its structural contract and behavioral
renderer battery (join, dedup, empty state, noise cap, byte-determinism)
run in rac-ci CI, the behavioral tier against the published engine. The
docs pointer lives here (the Decisions on PRs page); the `v1` tag predates
Herald, so consumers reference `@main` until a tag that includes it is cut.

The design's three Open Questions closed as delivered: **the action ships
in `rac-ci`** — the CI wrappers' extracted home — beside Watchkeeper,
Gatekeeper, and Registrar, while the corpus record and the docs pointer
stay here; **changed paths are computed action-side** — no engine change
and no dual-engine port, since `decisions-for` is already a covered command
under the ADR-116 dispatcher; **noise is tuned** by the engine's live-only
scoping plus dedupe-by-id, sorted output, an inline cap with a details
expander, no comment when nothing governs, and update-in-place always
(including to the empty state, so a stale comment can never outlive its
diff). The claim check the survey required came back clean.

This roadmap is the graduation of the top-ranked net-new adoption lever from
`adoption-opportunity-survey` (Opportunity 1); the implementation contract —
the *how* — lives in the `pr-decision-surfacing` design. It states
positioning already recorded in ADR-036 and ADR-081 and honours
ADR-034/ADR-067, and does not alter them.

## Context

Lore's core promise — a coding agent or reviewer stops re-doing what a team
already ruled out — is realised today only if someone thinks to query the
corpus. At the moment it matters most, a pull request editing code a decision
governs, nothing surfaces the governing decision; the shipped CI actions
annotate corpus *errors*, never the *decisions that govern the code under
review*. Two capabilities that just shipped make closing this cheap: the
`rac decisions` path→decisions lookup and the `## Applies To` code-scope
declaration (`decision-to-code-proximity`). This roadmap turns that into a
visible surface at the review — an adoption lever (visible on every PR in every
adopting repo) that is also on-thesis (the promise, delivered where it is felt).

## Outcomes

- A pull request that edits governed code carries one advisory comment naming
  the live decisions that govern the changed paths — id, title, a one-line
  summary, and a "review recommended" prompt — so a settled constraint is seen
  before it is violated.
- The surface is advisory and post-edit: it reports decisions as facts, never a
  verdict or a merge gate (ADR-034, ADR-067); the human PR review stays the
  trust boundary (ADR-065).
- Every adopting repo's PRs become passive proof the tool is working — the
  distribution loop the CLI has no equivalent of today.

## Initiatives

### Initiative 1 — Diff-aware path→decisions surfacing

Compute a PR's changed paths and collect, per path, the live decisions whose
declared `## Applies To` scope covers it — reusing the `rac decisions` service
(no new engine logic). Decide whether the engine gains a `rac decisions
--changed <base>..<head>` convenience mode (additive, ADR-007) or the surface
computes changed paths itself.

### Initiative 2 — The PR comment surface

A `lore-*` GitHub Action (or a documented `integration-recipe-factory` recipe
any CI can adopt) that posts one deduplicated comment and updates it in place on
re-runs. Facts only; links each cited decision to a rendered, shareable view —
which absorbs the survey's "public decision pages" idea rather than building it
standalone.

### Initiative 3 — Advisory quality

Tune relevance so the comment stays trusted: scope to live decisions with a
matching declared scope, keep it terse, and guard against the over-flagging the
`freshness-and-drift-detection` work records. Compose with the drift signal —
a governed decision that is also suspect can be noted.

## Success Measures

- On a PR touching a path with a governing decision, the comment appears, names
  the decision by id, and recommends review; on a PR touching nothing governed,
  no comment appears.
- The surface never changes a PR's merge status (advisory only; ADR-034,
  ADR-067, ADR-075).
- The comment is reproducible from the corpus and the changed paths — a pure
  function of `rac decisions` output, no wall-clock or model input.
- `rac validate rac/`, `rac relationships rac/ --validate`, and `rac review
  rac/` stay clean.

## Assumptions

- `decision-to-code-proximity` has shipped `rac decisions` and the `## Applies
  To` scope (it has), so this is a surface over existing capability, not new
  engine logic.
- Teams that adopt Lore run CI on their PRs, so a CI-hosted comment reaches the
  review.

## Risks

- Over-flagging trains reviewers to ignore the comment (the recorded
  freshness-and-drift lesson). Mitigation: scope tightly to declared governing
  decisions; keep the comment terse; advisory-first.
- Read as a gate rather than context. Mitigation: facts-only wording, no merge
  status change, "review recommended" never "you must" (ADR-034, ADR-067).
- Parallel-thread duplication in this repository. Mitigation: re-run the claim
  check (open PRs, active branches) before build, per the survey's discipline.

## Related Decisions

- adr-007
- adr-034
- adr-063
- adr-065
- adr-067
- adr-068
- adr-081

## Related Roadmaps

- growth-programme
- decision-to-code-proximity
- freshness-and-drift-detection
- integration-recipe-factory

## Related Designs

- pr-decision-surfacing

## Related Requirements

- rac-growth-adoption

## Related Tickets

- itsthelore/asdecided-core#351
