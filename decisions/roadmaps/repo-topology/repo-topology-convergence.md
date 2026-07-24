---
schema_version: 1
id: RAC-KW6F43XVSPCX
type: roadmap
tags: [structure, org, distribution, branding]
---
# Repository Topology Convergence

## Status

Planned

## Context

ADR-092 set the current repository topology for the `itsthelore` organisation:
**one repo per concern, subdir per member, uniform `rac-*` naming**, with the
brand "Lore" living at the org and the marketplace listings rather than in
repository slugs. It supersedes the first topology (ADR-064/ADR-068, the
`lore-*` / `rac-*` split and per-member repos) and generalises the connector
consolidation (ADR-073).

The first extraction phase — the `repo-extraction-programme` and the
`v0.22.x-housekeeping` series — already moved the standalone components out of
the engine repo: `decisiongrounding` (v0.22.4) and the TypeScript stack
(`rac-sdk-ts` + `lore-vscode`, v0.22.5) now live in their own repositories, and
the GitHub Actions split was deferred (v0.22.6). Those moves landed under the
*old* topology, so the org now carries a mix of `rac-*`, `lore-*`, and own-brand
slugs.

This series converges the live org onto ADR-092's target. It is a new series —
not a rewrite of the v0.22.x items, which stand as the record of the first phase
— with one item per family repo, each an independently-executable org action.
The work is repository topology only: no engine behaviour, public surface,
package, CLI, or server-identity change (ADR-029/036/039 hold).

## Outcomes

- The `itsthelore` org matches ADR-092's target table: a bounded set of `rac-*`
  family repos — `rac-core`, `rac-ci`, `rac-connectors`, `rac-sdk`,
  `rac-benchmarks`, `rac-editors` — one per concern, subdir per member.
- No contradictory `lore-*` or per-member slug remains in the RAC/Lore
  constellation; the separate products keep their own brands
  (`wayfinder-router`, `proofkeeper`).
- Every cross-repo consumer reference (action `uses:` paths, the npm package,
  install docs) resolves after the renames, with no stranded reference.

## Initiatives

Each initiative is its own item in this series, executed independently:

- **`rac-ci`** — consolidate `rac-actions`, `lore-watchkeeper`, and
  `lore-gatekeeper` into one CI-delivery repo, subdir per capability.
- **`rac-connectors`** — rename `lore-connectors`; subdir per
  integration, inbound and outbound.
- **`rac-sdk`** — consolidate `rac-sdk-ts` into `rac-sdk/ts/`; the
  Python SDK stays in `rac-core` (ADR-062).
- **`rac-benchmarks`** — restructure `decisiongrounding` into
  `rac-benchmarks/decisiongrounding/`.
- **`rac-editors`** — rename `lore-vscode` into `rac-editors/vscode/`.
- **Naming/brand cutover** — apply the `rac-*` slug rule across the
  constellation as each repo lands; the "Lore" brand stays at the org and the
  marketplace listings (ADR-092).

## Success Measures

- Every repository in ADR-092's target table exists under its `rac-*` name; no
  `lore-*` slug remains except the separate-product brands.
- Consumers of the actions, the npm SDK, and the install docs resolve to the new
  locations; no issue traces to a stranded `uses:` path, npm name, or repo URL.
- The `rac-core` corpus gates (`rac validate`, `rac relationships --validate`,
  `rac review`) stay green throughout; no engine or contract change ships with
  the topology move.

## Assumptions

- The maintainer can create, rename, permission, and archive repositories under
  `itsthelore`; the GitHub-side moves are org actions outside `rac-core`.
- The components being consolidated already exist as their own repos (the first
  phase's output) or in-repo (`rac-actions`), so this phase renames and groups
  rather than extracts from the engine.
- ADR-092 stands as the governing topology decision for the duration of the
  series.

## Risks

- **Stranded references on rename.** Renaming a repo or npm package breaks
  pinned consumers. Mitigation: GitHub redirects on rename, a versioned cutover
  for the actions, and a deprecation note on the old npm name.
- **Brand confusion mid-migration.** Some repos carry the old slug until their
  item lands. Mitigation: the rule is recorded (ADR-092) and each rename is an
  explicit, documented org action.
- **Scope creep into engine changes.** Mitigation: this series is topology only;
  any engine-side support (for example a Bitbucket renderer for `rac-ci`) is
  scoped in its own item, not assumed here.

## Related Decisions

- adr-092
- adr-064
- adr-068
- adr-073

## Related Roadmaps

- rac-ci
- rac-connectors
- rac-sdk
- rac-benchmarks
- rac-editors
- repo-extraction-programme

## Related Requirements

- rac-growth-extensibility

## Related Tickets

- itsthelore/asdecided-core#228
