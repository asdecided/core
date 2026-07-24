---
schema_version: 1
id: RAC-KVTRP81ZWA57
type: roadmap
---
# RAC — Freshness Signals and Drift Detection

## Status

Planned

Prioritised as the rank-2 Tranche A item of the deterministic-substrate
programme, graduated out of `future/` now the programme's rank-3 and rank-4
items are live. **Phase 1 is fenced hard**: Initiative 1's git-derived
staleness fields plus one advisory drift finding in doctor/review, only.
The CI-gate form of the finding and freshness-biased retrieval are recorded
below as later-phase initiatives behind their own fences. The programme's
constraint pattern is carried verbatim: **proximity references are declared
and validated, never inferred; drift findings are advisory before they are
ever a gate.** Execution is tracked in GitHub (ADR-093): the epic in
`## Related Tickets` carries ordering and task state, with a sub-issue per
initiative.

## Context

The strongest, best-evidenced finding from researching knowledge tools at team
scale: **staleness leading to trust collapse is the number-one cause of
abandonment** — once a reader hits one wrong page, they distrust the whole corpus
and revert to Slack or reading source. And the evidence is empirical, not
folklore: a study of 27,772 pull requests across 714 repositories found only
~0.8% of PRs update the README and ~21.5% of changes that should have updated it
did not; DORA finds high-quality documentation more than doubles the odds of
hitting performance targets.

The sharp, uncomfortable corollary: **git + PR review is necessary but
proven-insufficient to keep knowledge fresh** — the docs-as-code literature itself
concedes there is no evidence drift decreases at scale, and the PR data shows
reviewers routinely miss documentation updates. So Lore's PR-review trust boundary
(ADR-065) alone will not keep the corpus trustworthy.

What every serious tool at the high end *does* ship is a freshness mechanism:
enterprise requirements tools (Jama, IBM DOORS, Siemens Polarion) all flag
downstream items as **"suspect"** when an upstream item changes; team wikis bolt on
owner + expiry-clock verification. By contrast, the AI agent-context tools
(Cursor, Amp, Continue, Augment) ship *no* freshness tooling at all — a gap Lore
can own.

The raw material already exists in the engine: the git-isolated recency
service derives per-artifact provenance — last and first commit, authors,
and status history — per ADR-045; the MCP `get_artifact` response already
embeds that provenance; and `rac review` already carries a stale-corpus
cadence advisory. Phase 1 *widens* these to the read surfaces that lack
them and adds one drift finding over the validated relationship graph
(ADR-074) — no new machinery, no new datastore.

## Outcomes

- Git-derived staleness is loud at the point of use: last-touched recency
  and a staleness indicator across the CLI and MCP read surfaces (phase 1).
- A deterministic advisory "suspect" finding in doctor/review when a
  referenced target changed after the referencing artifact — the git-native
  equivalent of enterprise "suspect links," with no database and no AI
  (phase 1).
- Later, in order and behind their fences: the same finding as an optional
  CI gate (ADR-075, the team's choice), then freshness-biased retrieval
  surfacing (ADR-078 territory).
- No new datastore: everything is a pure function of git history plus the
  relationship graph.

## Initiatives

### Initiative 1 — Loud staleness on read surfaces (`rac-freshness-read-surfaces`) — phase 1

Widen the existing git-derived recency (ADR-045) additively (ADR-007) to
the read surfaces that lack it — CLI find/search output and MCP
`search_artifacts` results — joining the provenance `get_artifact` already
embeds, plus a documented deterministic staleness indicator. Derived from
git, never frontmatter; degrades to null outside git.

### Initiative 2 — Advisory drift finding (`rac-drift-advisory-finding`) — phase 1

Artifact-graph drift first: a validated relationship target changed in git
after the referencing artifact last changed → an advisory "suspect" finding
in `rac doctor` (stable code, warning severity, exit 0) and surfaced by
`rac review`. Computed from the recency service plus the validated
relationship graph; external-reference sections (format-linted, never
resolved, ADR-087) are excluded. **Recorded dependency**: code-scope drift
(governed code changed) joins additively after `decision-to-code-proximity`
ships its `## Applies To` join — an extension of this finding, not a new
mechanism.

### Initiative 3 — Drift finding as an optional CI gate (later phase)

The same finding in gate form, opt-in per ADR-075's required-merge-gate
posture, and only after the phase-1 advisory has run in anger — drift
findings are advisory before they are ever a gate. No requirement is minted
yet; a later batch schedules it under its own decision if needed.

### Initiative 4 — Freshness biases retrieval surfacing (later phase)

De-prioritising suspect artifacts in retrieval changes ranking, which
touches the byte-pinned ranking goldens and the ADR-078 deterministic
relevance contract and its eval gate — so it rides its own scoped work with
benchmark evidence, not phase 1. No requirement is minted yet.

## Constraints

- Staleness is git-derived, never a stored frontmatter flag (ADR-045);
  outside git the signal degrades to null and no findings, never an error.
- Deterministic given fixed git state (ADR-002); no database, no
  AI/embeddings (ADR-066).
- **Golden stability (council constraint, carried from the programme)**:
  git-derived findings and fields must not destabilise byte-pinned golden
  outputs — they stay out of byte-pinned goldens, or the fixture fully
  controls git state.
- Advisory before ever a gate; phase 1 surfaces findings in doctor/review
  only.
- Additive contracts (ADR-007); the MCP budget holds (ADR-033); findings
  and fields are reported facts, never verdicts (ADR-034).
- PR review remains the human attestation (ADR-065); this *augments* it
  with the machine-checkable signal the evidence says review alone lacks.

## Non-Goals

- Auto-fixing staleness: decision rationale cannot be regenerated, so Lore
  detects and surfaces drift, it does not silently rewrite an artifact.
- A frontmatter "last verified" checkbox: that duplicates state git already
  holds and invites the self-clicked-verify theatre wikis rely on.
- A freshness *guarantee* — the signal means "review recommended," never
  proof of correctness.
- Drift over external references (related tickets, verified by): those are
  format-linted, never resolved (ADR-087), so they are never dated.
- In phase 1: any CI-failing mode, and any change to retrieval matching or
  ordering.

## Success Measures

- A reader or agent sees last-touched recency and suspect status at the
  point of use on the widened surfaces.
- Changing a target without updating a referencing artifact produces a
  deterministic suspect finding in doctor and review, reproducibly;
  warning-only runs exit 0.
- The full byte-pinned golden suite passes unchanged wherever git state is
  uncontrolled.
- No new datastore is introduced; the signal is a pure function of git
  history and the relationship graph.
- `rac validate rac/`, `rac relationships rac/ --validate`, and
  `rac review rac/` stay clean.

## Assumptions

- Trust collapse from staleness is the dominant abandonment driver at 20+,
  so a visible, automated freshness signal is the single highest-leverage
  adoption lever — and one no agent-context competitor ships.
- Phase-1 drift is computable from the recency service plus the
  relationship graph alone; where per-artifact change sets between
  revisions are needed, the Watchkeeper revision-materialisation seam
  (ADR-043) supplies them.
- Advisory-first is how signal quality is proven; over-flagging is tuned
  during phase 1, before any gate form is scheduled.

## Risks

- Over-flagging (every upstream edit marks everything suspect) trains
  people to ignore it. Mitigation: scope "suspect" to declared, resolvable
  references and meaningful target changes; advisory-first with a stable
  code so consumers can filter.
- A freshness signal could imply a freshness *guarantee*. Mitigation: frame
  it as "review recommended," an input to human judgement (ADR-065), never
  proof of correctness — data, not verdicts (ADR-034).
- Git-derived findings destabilise the deliberately git-state-independent
  goldens. Mitigation: the golden-stability constraint above, asserted in
  CI.
- Shallow clones in CI under-report history, silently weakening the signal.
  Mitigation: ADR-045's degrade-to-none posture — the finding is absent
  rather than wrong — with the limitation documented.

## Related Decisions

- adr-002
- adr-007
- adr-033
- adr-034
- adr-043
- adr-045
- adr-065
- adr-066
- adr-074
- adr-075
- adr-078
- adr-087
- adr-093
- adr-094

## Related Roadmaps

- decision-to-code-proximity
- deterministic-substrate
- relevance-ranking

## Related Requirements

- rac-freshness-read-surfaces
- rac-drift-advisory-finding
- rac-traceability-coverage-report
- rac-doctor-diagnostic-validator

## Related Tickets

- itsthelore/asdecided-core#277
