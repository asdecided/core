---
schema_version: 1
id: RAC-KW6F45V3NGFF
type: roadmap
tags: [structure, org, ci, distribution]
---
# Consolidate CI Delivery into rac-ci

## Status

Planned

## Context

ADR-092 places all CI delivery in a single `rac-ci` repository — one subdir per
capability (`watchkeeper/`, `gatekeeper/`, `audit/`), with delivery platforms
nested inside each (`github/`, `bitbucket/`, `jenkins/`). This restores ADR-064's
single-`rac-actions` consolidation intent, which ADR-068 had split into the
per-action `lore-watchkeeper` / `lore-gatekeeper` repos, and supersedes the
deferred two-repo plan in `v0.22.6-split-actions`. A capability is never split
across repos by platform, so ADR-090's capability-first intent holds and no
`lore-pipelines` is created.

The engine is already platform-neutral — `rac gate` and `rac watchkeeper` emit
SARIF and JSON, and the GitHub annotations are one output flavour — so the
GitHub→other-platform work is in the thin wrappers, not the engine.

## Outcomes

- A single `itsthelore/asdecided-ci` repository holds every CI wrapper, a subdir per
  capability, consumed as `uses: itsthelore/asdecided-ci/<capability>@<ref>`.
- `rac-actions`, `lore-watchkeeper`, and `lore-gatekeeper` are consolidated into
  it and archived; the in-repo `action.yml`, `pr-gate-action/`, and
  `validate-action/` sources are removed from `rac-core` once `rac-ci` carries
  them.
- This repository's own dogfood CI consumes the `rac-ci` capabilities (or a
  documented source-install equivalent), with no stranded `uses:` path.

## Initiatives

- **Seed `rac-ci`** with the watchkeeper and gatekeeper wrappers under
  `watchkeeper/` and `gatekeeper/` (`github/` first), history preserved, plus an
  `audit/` placeholder for the ADR-084 recorder's future wrapper; tag a suite
  version consumers can pin.
- **Repoint and remove** the in-repo action sources: switch `rac-core`'s own
  workflows to the `rac-ci` capabilities (or keep the source-install dogfood and
  document it), then remove the in-repo action dirs and update
  `tests/test_*_action.py` and `docs/`.
- **Archive predecessors** `rac-actions`, `lore-watchkeeper`, `lore-gatekeeper`
  with a redirect note to `rac-ci`.

## Success Measures

- `itsthelore/asdecided-ci` exists with the capability subdirs and a pinnable suite
  tag; the three predecessor action repos are archived with redirects.
- No consumer reference traces to an in-repo action path or a `lore-*` action
  repo; `rac-core`'s dogfood CI is green against the new locations.
- The `rac-core` corpus gates stay green after the in-repo action sources are
  removed.

## Assumptions

- The actions consume only the public `rac` CLI, never engine internals, so they
  run from any repo once published.
- Coupled suite version tags (one `rac-ci` version) and consumption via
  `uses: itsthelore/asdecided-ci/<capability>@<ref>` rather than per-action
  Marketplace listings are acceptable (ADR-092).
- The maintainer can create `rac-ci` and archive the predecessor repos.

## Risks

- **Stranded `uses:` references** break consumers pinned to the in-repo or
  `lore-*` paths. Mitigation: seed and tag `rac-ci` first, then cut over behind a
  documented version, leaving redirect notes on the archived repos.
- **Non-GitHub wrappers underspecified.** Mitigation: ship `github/` first; add
  `bitbucket/` (with any engine-side Code-Insights renderer scoped separately)
  and `jenkins/` when demanded.

## Related Decisions

- adr-092
- adr-064
- adr-068
- adr-049
- adr-058
- adr-090

## Related Roadmaps

- repo-topology-convergence
- v0.22.6-split-actions
- repo-extraction-programme
