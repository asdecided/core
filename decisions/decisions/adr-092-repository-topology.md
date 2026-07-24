---
schema_version: 1
id: RAC-KW5A03ZHXN9F
type: decision
tags: [structure, org, distribution, branding]
---
# ADR-092: Repository Topology — One Repo Per Concern, rac-* Naming

## Status

Accepted

## Category

Architecture

## Context

> **Extended by ADR-101.** ADR-101 adds `itsthelore.github.io` to the target
> topology below as the org's documentation-aggregation site — its own repo
> per GitHub's root-Pages naming rule, not a family-pattern member. That ADR
> governs the site's hosting and vendoring model; this ADR governs its place
> in the topology and naming.

ADR-064 and ADR-068 set the first repository topology for the `itsthelore`
organisation: a `lore-*` / `rac-*` split (install-surface vs engine), per-action
repos (`lore-watchkeeper`, `lore-gatekeeper`), a per-client repo rule
(`lore-vscode`, `lore-cursor`, …), and a per-language SDK repo (`rac-sdk-ts`).
ADR-073 then consolidated export connectors into `lore-connectors`, and ADR-090
(Proposed) added enterprise satellites.

Enumerating the live org surfaced two problems. First, the naming is already
inconsistent in practice — `rac-*` (rac-core, rac-actions, rac-sdk-ts), `lore-*`
(connectors, watchkeeper, gatekeeper, vscode, proofkeeper), and own-brand
(wayfinder-router, decisiongrounding) coexist with no rule a reader can apply.
Second, the topology had drifted toward **one repo per member** — separate repos
per action, per client, per SDK language — which trends toward many small repos to
permission and maintain. The maintainer's stated preference is **a few
high-quality repos over many small ones.**

A single organising idea resolves both: a repository is defined by the **concern**
it serves, with one subdir per **member** of that concern. The existing
`lore-connectors` (one repo, subdir per backend) and the maintainer's request to
treat benchmarks the same way (`rac-benchmarks`, subdir per benchmark) are the
pattern; this ADR generalises it.

Several decisions are touched and are reconciled in the Relationship section:
ADR-036 freezes the PyPI distribution name; ADR-062/063 fix the SDK surfaces;
ADR-064/068 set the prior topology; ADR-073 consolidates connectors; ADR-090 the
satellites; ADR-069 makes Wayfinder a separate product. ADR-029 (engine + server
one package) and the frozen CLI `rac` / server `lore` identities (ADR-036/039) are
**not** changed.

## Decision

### Governing principle

**One repo per concern, subdir per member; a separate repo only for a distinct
product or an independently-owned/released surface.** Consolidate *within* a
concern; separate *across* concerns. Not one repo per member (sprawl); not one
mega-repo (a grab-bag that erodes quality).

- **Consolidated family repo, subdir per member** — for thin contract-consumers
  that version with the engine contract: integrations, language SDKs, benchmarks.
- **Own repo** — for surfaces with genuinely independent versioning/ownership: the
  IDE-client surface and the CI surface (each a published artifact the maintainer
  versions on its own line).
- **Separate product, own brand** — for distinct products: Wayfinder, Proofkeeper.

### Naming

Every repository in the RAC/Lore constellation is **`rac-*`**, consistent with
`rac-core` and the `rac` CLI. The **brand "Lore" lives at the org (`itsthelore`)
and the marketplace listings**, not in repository slugs — a `rac-vscode` repo
still *lists* as "Lore for VS Code", exactly as `rac-core` is already the engine
behind the product "Lore". This replaces ADR-068's `lore-*` / `rac-*` split.

Distinct products keep their own brand and naming (`wayfinder-router`,
`proofkeeper`); they are siblings of the constellation, not satellites of it.

### Target topology

| Repo | Concern / kind | Members (subdirs) |
| --- | --- | --- |
| `rac-core` | engine + Python SDK (ADR-062) | — |
| `rac-ci` | CI delivery | `watchkeeper/`, `gatekeeper/`, `audit/` — each × platform (`github/`, `bitbucket/`, `jenkins/`) |
| `rac-connectors` | integrations (inbound + outbound) | `atlassian/`, `supermemory/`, … |
| `rac-sdk` | non-Python language SDKs (thin contract clients, ADR-063) | `ts/`, `go/`, … |
| `rac-benchmarks` | benchmarks | `decisiongrounding/`, … |
| `rac-editors` | IDE clients | `vscode/` (VS Code/Cursor) (+ `jetbrains/`, …) |
| `wayfinder-router` | separate product (ADR-069) | — |
| `proofkeeper` | separate product (BYO-model QA agent) | — |
| `itsthelore.github.io` | org presence + docs aggregation (ADR-101) | — |

`rac-connectors` is the rename of the existing `lore-connectors`; `rac-ci` is the
rename/absorption of `rac-actions` plus the standalone `lore-watchkeeper` /
`lore-gatekeeper`; `rac-benchmarks` is `decisiongrounding` restructured with the
benchmark as a subdir; `rac-editors` is the rename of `lore-vscode`; `rac-sdk` is
the consolidation of `rac-sdk-ts` (→ `ts/`). The GitHub renames/archives are
maintainer-run org actions, sequenced in the deferred roadmap rewrite.

### Specific consequences of the principle

- **CI delivery is one `rac-ci` repo**, subdir per capability, platform nested
  within — restoring ADR-064's consolidation intent (which ADR-068 had split) and
  retiring the standalone action repos. A capability is never split across repos
  by platform, so the ADR-090 "capability-first" intent holds; `lore-pipelines` is
  not created. Accepted tradeoffs: coupled version tags (one suite version, which
  is simpler for consumers) and no per-action Marketplace listing (consumed via
  `uses: itsthelore/asdecided-ci/<capability>@<ref>`).
- **`rac-connectors` is one repo** covering inbound (`rac ingest`) + outbound
  (`rac export`) + provider suites; **Atlassian is a subdir**, not its own repo —
  deployment and multi-direction do not force a repo boundary.
- **`rac-sdk` is one repo**, subdir per language; the **Python SDK stays in
  `rac-core`** (its surface *is* `rac.__all__`, ADR-062). Polyglot-monorepo cost
  (mixed toolchains, per-subdir release tags) is accepted; a language SDK
  graduates to its own repo only if it grows an independent community/cadence.
- **`rac-benchmarks` and `rac-editors`** follow the same family pattern.
- **PyPI distribution renames `requirements-as-code` → `rac-core`** (verified
  available; bare `rac` is taken), aligning the distribution name with the repo
  and the `rac-*` scheme. The **import package `rac` and the `rac` CLI entry point
  are unchanged** (zero code churn). A transitional `requirements-as-code` release
  depending on `rac-core` keeps existing installs working through a deprecation
  window. This lifts ADR-036's freeze on the PyPI name only; CLI `rac` and server
  `lore` stay frozen.

### Relationship to ADR-068 (amend-as-sibling)

This ADR is the current authority on repository topology and naming and records
what it overturns, **amending ADR-068 as a sibling rather than flipping its
status** — exactly the mechanism ADR-068 used for ADR-064. ADR-068 remains in the
record as the prior topology; where the two differ, ADR-092 governs. ADR-068's
status-supersession and the rewrite of the `v0.22.x-housekeeping` extraction
roadmaps (which encode the prior topology) are sequenced as deliberate follow-up,
not done here.

## Consequences

### Positive

- One rule decides every repository's shape and name; no surface re-litigates it.
- A small, bounded, high-quality repo set (~6 `rac-*` repos) instead of a
  per-member sprawl — fewer repos to permission, secure, and maintain.
- The PyPI name matches the repo and CLI; the `rac-*` scheme is consistent end to
  end, with the brand carried by the org and listings.

### Negative

- Reverses recently-recorded structure (ADR-068's split, the per-client rule) and
  the in-flight `v0.22.x` extraction roadmaps, which must be rewritten as
  follow-up.
- Polyglot monorepos (`rac-sdk`, `rac-editors`) carry mixed toolchains and
  per-subdir release tooling; `rac-ci` couples action version tags.
- The PyPI rename needs a transitional shim and a documentation sweep, and lifts a
  deliberately-frozen name (ADR-036).

### Risks

- A consolidated repo grows into a grab-bag. Mitigation: the line is *concern*, not
  convenience; unlike concerns get unlike repos.
- A consumer pinned to the old action paths or the old PyPI name breaks.
  Mitigation: the shim + a sequenced, versioned cutover in the follow-up.

## Alternatives Considered

### Keep ADR-068's lore-/rac- split and per-member repos

Retain the install-vs-engine prefix rule and a repo per action/client/SDK.

#### Disadvantages

- Trends toward many small repos and a naming rule already applied
  inconsistently. The maintainer's explicit preference is fewer high-quality
  repos; the org already carries the brand, so the slug split earns little.

### One mega-repo for everything

Collapse engine, CI, SDKs, benchmarks, and clients into a single repository.

#### Disadvantages

- Mixes unlike concerns with incompatible toolchains and release models into one
  grab-bag — the opposite failure of sprawl, and it erodes quality.

### Status-supersede ADR-068 now

Flip ADR-068 to Superseded in this change.

#### Disadvantages

- It cascades `relationship-target-superseded` across the 12 artifacts that
  reference ADR-068 in resolving sections — 8 of which are the `v0.22.x` roadmaps
  slated for rewrite anyway. Amend-as-sibling now, and retire ADR-068 cleanly in
  the deferred roadmap rewrite.

## Relationship to Other Decisions

- ADR-068: amended as a sibling (this ADR is the current topology/naming
  authority); its status-supersession is deferred to the roadmap rewrite.
- ADR-064: the `rac-actions` consolidation intent is restored as `rac-ci`.
- ADR-036: the PyPI-name freeze is lifted for `requirements-as-code` → `rac-core`;
  CLI `rac` and server `lore` stay frozen.
- ADR-073: generalised — `rac-connectors` is the one integrations repo (inbound +
  outbound + provider suites), the same family pattern as `rac-sdk` /
  `rac-benchmarks`.
- ADR-090: capability-first satellite topology realised as `rac-ci` capability
  subdirs; no `lore-pipelines`.
- ADR-062, ADR-063: the Python SDK stays in `rac-core`; non-Python SDKs are thin
  contract clients consolidated in `rac-sdk`.
- ADR-066: the grounding benchmark lives in `rac-benchmarks`.
- ADR-069: Wayfinder is a separate product; Proofkeeper is its sibling.
- ADR-029, ADR-039: engine + server stay one package; CLI `rac` and server `lore`
  identities unchanged.
- ADR-101: adds `itsthelore.github.io` to this table and governs its docs
  hosting and vendoring model, including retiring `rac-core`'s project page.

## Related Decisions

- adr-064
- adr-068
- adr-036
- adr-073
- adr-090
- adr-062
- adr-063
- adr-066
- adr-069
- adr-101

## Related Roadmaps

- repo-topology-convergence

## Review Date

Revisit if a consolidated family repo's members diverge enough in cadence or
ownership to warrant graduating one out (the escape hatch), or when the deferred
`v0.22.x` roadmap rewrite lands and ADR-068 is formally retired.
