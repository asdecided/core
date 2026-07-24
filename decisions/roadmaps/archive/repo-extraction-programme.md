---
schema_version: 1
id: RAC-KV68XS3J7P6C
type: roadmap
tags: [structure, org, distribution]
---
# Repo Extraction Programme

## Status

Planned

## Context

> **Superseded by ADR-092; re-planned as the `repo-topology` series.**
> This programme sequenced the *first* (`lore-*` / one-repo-per-component)
> extraction topology, and its low-risk moves landed (`decisiongrounding`,
> v0.22.4; the TypeScript stack → `rac-sdk-ts` + `lore-vscode`, v0.22.5).
> ADR-092 then replaced that topology with *one repo per concern* under uniform
> `rac-*` naming. The remaining work — consolidating those already-extracted
> repos into `rac-connectors` / `rac-sdk` / `rac-benchmarks` / `rac-editors` and
> the actions into `rac-ci` — is carried by the `repo-topology` series.
> This item is retained as the record of the original programme.

ADR-064 records the decision to adopt a small multi-repo topology under the
`itsthelore` organisation: extract the standalone components and keep the
engine, its shipped resources, and its governing corpus in `itsthelore/asdecided-core`
(the renamed `requirements-as-code` repository). ADR-068 extends that topology
for the surfaces that post-date ADR-064 — the editor extension and the
TypeScript SDK — and sharpens the Actions picture. This programme sequences the
moves. It is a non-versioned `future/` item because the work is structural and
cross-cutting, not a feature release on the current series. The
`v0.22.x-housekeeping` series schedules the execution.

The extraction targets differ in risk. `decisiongrounding` imports no engine
code and can move with no consumer impact. The TypeScript stack (extension +
SDK) depends on the SDK being published to npm first. The GitHub Actions are the
consumer-breaking target: extraction changes the `uses:` path that downstream
workflows reference, so they come last and behind a versioned cutover.
`examples/` is explicitly kept (ADR-064), since `examples/guide` is the
grounding demo and the dashboards are test fixtures. `lore-web` is renamed
`rac-localview` in-repo (ADR-068); its standalone extraction stays deferred
(ADR-064) until a publish/vendor contract exists.

## Outcomes

- Each target component lives in its own `itsthelore` repository, depending
  only on the published `requirements-as-code` package or the public `rac`
  CLI — never on engine internals — with no coupling regression.
- The engine repository carries only the engine, its shipped resources
  (skills, templates, hooks), and the dogfood corpus.
- Every consumer-facing reference (action `uses:` paths, docs, CI workflows)
  is updated, and no stranded reference is left behind.

## Initiatives

### Initiative 1 — Extract `decisiongrounding` (reference migration)

The cleanest target and the pattern for the rest. Move `decisiongrounding/`
to `itsthelore/decisiongrounding` with history preserved, carrying its own
`pyproject.toml`, tests, LICENSE, Makefile, and its `decisions/` ADRs
(DG-ADR-0001 onward). It already treats `rac` as an external CLI on `PATH`, so
no code changes are needed; remove the directory from the monorepo and update
any references to it (including the v0.20.1 roadmap's "mirrors
`decisiongrounding/`" note, which becomes a cross-repo reference).

### Initiative 2 — Split the GitHub Actions into two product-branded repos

ADR-068 amends ADR-064's single `rac-actions` repo here. The Actions are
what a team *installs*, so they take the `lore-*` prefix and split into two
repositories covering **three** actions:

- `itsthelore/lore-gatekeeper` — the real gate, `pr-gate-action` (`rac gate`).
  The older single-purpose `validate-action` is folded in and **deprecated** by
  it. ADR-058 governs the gate and moves with it. The gate is the headline
  enforcement surface (ADR-049).
- `itsthelore/lore-watchkeeper` — the Watchkeeper action (root `action.yml`).

This changes the `uses:` path consumers reference, so it ships last, behind a
versioned cutover: publish the new locations, document the new `uses:`
references (`itsthelore/lore-gatekeeper@v1`, `itsthelore/lore-watchkeeper@v1`),
leave a deprecation note for the old paths, and repoint this repository's own
`.github/workflows/` to the new locations.

### Initiative 3 — Extract the TypeScript stack (extension + SDK)

The surfaces that post-date ADR-064. The TypeScript SDK (`@rac/sdk`) is a typed
thin client over the engine contract (ADR-063), so it takes the `rac-*` prefix
and moves to `itsthelore/asdecided-sdk-ts`, **published to npm** as
`@itsthelore/asdecided-sdk`. The VS Code / Cursor extension is a surface a user
installs, so it takes the `lore-*` prefix and moves to
`itsthelore/lore-vscode` (one VSIX to Marketplace + OpenVSX). Sequencing:
publish the SDK first, then repoint the extension from `file:../rac-sdk` to the
published package and remove the in-repo `typescript/` stack and its CI.

### Initiative 4 — Rename the viewer `rac-localview` in-repo

`lore-web` is the local Portal / graph viewer, vendored into `src/rac/` — a
build-coupled internal of the engine, so it takes the `rac-*` prefix and is
renamed `rac-localview/` **in-repo** (ADR-068). Standalone-repo extraction stays
deferred (ADR-064) until a publish/vendor contract exists.

### Initiative 5 — Keep `examples/`; adopt the per-repo examples convention

`examples/` is **not** extracted. `examples/guide` is the grounding demo and
the dashboards are test fixtures, so they stay in the engine repo (ADR-064).
The convention going forward: each repo carries its own `examples/`
subdirectory where useful, rather than a central examples repository. The
separately planned liftable SDK examples sub-project (v0.20.1) is unaffected by
this programme.

## Constraints

- No engine behaviour change and no change to the public surface: this is a
  topology move, not a feature.
- Distribution names stay frozen (ADR-036, ADR-039): PyPI
  `requirements-as-code`, CLI `rac`, server identity `lore`.
- The engine and MCP server stay one package (ADR-029); they are not part of
  any extraction.
- Each extraction preserves history (`git filter-repo` or `git subtree`,
  decided per repo) rather than copying files flat.

## Non-Goals

- Extracting `examples/` or creating a `rac-examples` repo — examples stays in
  the engine repo (ADR-064).
- Extracting `rac-localview` (the renamed `lore-web`) to its own repo — deferred
  by ADR-064 until its Portal-shell vendoring has a publish/vendor contract. The
  in-repo rename (ADR-068) is in scope; the standalone extraction is not.
- Renaming the package, CLI, or server.
- Creating the org repositories as part of this corpus task; the repos are
  created when each initiative is executed.

## Implementation Contract

**Scheduling.** The `v0.22.x-housekeeping` series sequences the execution: the
in-repo viewer rename (v0.22.1) and `rac-core` identity updates (v0.22.2), the
SDK publish prerequisite (v0.22.3), then the extractions — `decisiongrounding`
(v0.22.4), the TypeScript stack (v0.22.5), and the consumer-breaking Actions
split last (v0.22.6).

**Safety sequencing.** Populate each destination repo with preserved history
and confirm it *before* removing anything from the engine repo. Never delete
from `rac-core` until the content lives elsewhere.

**Seed `itsthelore/decisiongrounding`** (history-preserving):

```bash
git clone <engine-url> dg && cd dg
git filter-repo --path decisiongrounding/ --path-rename decisiongrounding/:
git remote add origin <decisiongrounding-url> && git push -u origin main
```

**Seed the two action repos** (`itsthelore/lore-gatekeeper`,
`itsthelore/lore-watchkeeper`), history-preserving: filter `pr-gate-action/`
(folding in / deprecating `validate-action/`) into `lore-gatekeeper`, and the
root `action.yml` into `lore-watchkeeper`. ADR-058 moves with the gatekeeper.
Add a README documenting the new `uses:` references, tag each `v1`, and push.

**Seed the TypeScript repos** (`itsthelore/asdecided-sdk-ts`,
`itsthelore/lore-vscode`) after the SDK is npm-publishable (v0.22.3): publish
`@itsthelore/asdecided-sdk` from `rac-sdk-ts`, then seed `lore-vscode` and repoint
it to the published package.

**Removal + rewire PRs on `rac-core`**: remove `decisiongrounding/`, the
`typescript/` stack, the root `action.yml`, `validate-action/`, and
`pr-gate-action/` once each lives elsewhere; repoint the engine's own self-tests
in `.github/workflows/pr-checks.yml` (`uses: ./` → `…/lore-watchkeeper@v1`;
`uses: ./pr-gate-action` → `…/lore-gatekeeper@v1`); update or remove
`tests/test_*_action.py`; update `docs/watchkeeper.md`, `docs/validation.md`
(grep for others) and add a deprecation note for the old paths; keep `examples/`
untouched.

After the removals the engine repository's `rac validate rac/`,
`rac relationships rac/ --validate`, and `rac review rac/` gates stay green,
and `pytest` passes.

## Success Measures

- Every extraction target (`decisiongrounding`, `rac-sdk-ts`, `lore-vscode`,
  `lore-gatekeeper`, `lore-watchkeeper`) lives in its own repo and consumes only
  the published package, the published `@rac/sdk`, or the public CLI; none
  imports engine internals.
- No issue traces to a stranded `uses:` path, a `file:../rac-sdk` path, or a
  missing relocated component.
- The engine repository's corpus gates remain green throughout, and the dogfood
  corpus and `examples/` stay in `rac-core`.

## Assumptions

- The `itsthelore` organisation can host the sibling repositories and the
  maintainer can create and permission them when each initiative runs.
- `decisiongrounding` continues to consume `rac` only as an external CLI on
  `PATH`, so its move needs no code change (DG-ADR-0001 holds).
- The extracted actions consume only the public `rac` CLI, not engine
  internals, so they run from any repo once published.
- The extracted TypeScript SDK and extension consume the published
  `@itsthelore/asdecided-sdk` and the public CLI, never engine internals.
- `rac-localview` (the renamed `lore-web`) stays in this repository until its
  vendoring contract exists, so no Portal-shell drift-guard breaks during this
  programme.

## Risks

- **Stranded action references.** Consumers pinned to the old `uses:` path
  break. Mitigation: actions move last, behind a versioned cutover with a
  deprecation note.
- **Cross-repo drift.** Extracted components lag the engine contract.
  Mitigation: each pins a published-package or CLI version and depends on no
  internals.
- **History loss.** A flat copy discards provenance. Mitigation: the contract
  mandates a history-preserving move.

## Related Decisions

- adr-064
- adr-068
- adr-058
- adr-063

## Related Roadmaps

- v0.20.1-python-sdk-docs
- v0.22.0-topology-and-release-decision

## Related Requirements

- rac-growth-extensibility
