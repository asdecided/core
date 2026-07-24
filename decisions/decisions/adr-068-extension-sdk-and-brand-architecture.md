---
schema_version: 1
id: RAC-KVA44MVMDXXX
type: decision
tags: [structure, org, distribution, branding]
---
# ADR-068: Extension, SDK, and Brand Architecture

## Status

Accepted

## Category

Architecture

## Context

> **Topology and naming superseded by ADR-092.** The `lore-*` (install) /
> `rac-*` (engine) prefix split and the per-client / per-action repositories
> recorded here are replaced by ADR-092's *one repo per concern, subdir per
> member* model with uniform `rac-*` naming — the brand "Lore" lives at the org
> and the marketplace listings, not in repository slugs. ADR-068's
> non-topology decisions still hold: the single shared-VSIX client surface, and
> the Python SDK staying inside `rac-core` (ADR-062). ADR-092 governs where the
> two differ; execution is re-planned in the `repo-topology` series.

ADR-064 settled the first repository topology for the `itsthelore`
organisation: extract `decisiongrounding` and the GitHub Actions, keep the
engine, its shipped resources, the dogfood corpus, and `examples/` in
`itsthelore/requirements-as-code`, and defer `lore-web`. Since that decision
was recorded the product has grown two surfaces that did not exist when ADR-064
was written: a VS Code / Cursor **extension** (`typescript/rac-vscode`) and a
TypeScript **client SDK** (`typescript/rac-sdk`, `@rac/sdk`), which the
extension consumes today over a `file:../rac-sdk` path. The Actions story has
also sharpened: there are now three composite actions — Watchkeeper (root
`action.yml`), the older single-purpose `validate-action/`, and the real gate
`pr-gate-action/` (`rac gate`) — not the two ADR-064 assumed.

Naming has drifted with the growth. Some surfaces are branded **Lore** (the
installable product), others are named after the **RAC** engine, with no stated
rule for which prefix a new repository takes. Without a principle, every new
repo re-litigates its own name.

Several decisions fence this one and must not be contradicted:

- **ADR-029** freezes the engine and its MCP server (`rac mcp`) into a *single*
  package and release pipeline. No split of engine from server.
- **ADR-036** and **ADR-039** freeze the shipped distribution names: PyPI
  `requirements-as-code`, CLI `rac`, server identity `lore`. Repository renames
  must not rename these.
- **ADR-062** fixes the Python SDK's public surface as `rac.__all__`, shipped
  inside the engine package — the Python SDK is not a separable artifact.
- **ADR-058** governs the validation GitHub Action.
- **ADR-049** — "enforcement is the product"; the gate is the headline surface.
- **ADR-063** — non-Python clients are thin clients over the contract.

This ADR settles the naming principle and the full target topology, including
the surfaces that post-date ADR-064, and amends ADR-064 where the Actions and
viewer picture has changed. The `repo-extraction-programme` roadmap and the
`v0.22.x-housekeeping` series sequence the moves.

## Decision

### Naming principle

A repository's prefix is decided by what it is to a user, not by who wrote it:

- **`lore-*`** — anything a user or team **installs**: the branded product
  surfaces (the editor extension, the CI actions). These are what someone adopts
  when they "use Lore".
- **`rac-*`** — the **engine** and its build-coupled internals: the package, the
  vendored viewer, and the typed client that mirrors the engine contract.

`decisiongrounding` keeps its established name (it is neither, and ADR-064 fixed
it). The shipped names stay frozen regardless of repository name (ADR-036,
ADR-039): PyPI `requirements-as-code`, CLI `rac`, import `rac`, server `lore`.

### Target topology

| Repository | Holds | Prefix rationale |
| --- | --- | --- |
| `rac-core` | The engine `src/rac/` (incl. the Python SDK surface, ADR-062), the dogfood corpus `rac/`, `examples/`, and the vendored viewer dir `rac-localview/` | The engine and its build-coupled internals. PyPI name stays `requirements-as-code` (ADR-036). |
| `rac-localview` | The local Portal / graph viewer, vendored into the engine via a build script + drift-guard | Build-coupled internal of the engine; not separately installed while vendored. |
| `rac-sdk-ts` | The TypeScript client `@rac/sdk`, published to npm as `@itsthelore/asdecided-sdk` | A typed client mirroring the engine contract (ADR-063). |
| `lore-vscode` | The VS Code extension — one VSIX to Marketplace + OpenVSX; runs in VS Code and, as a fork, Cursor | A surface a user installs (the first per-client repo). |
| `lore-watchkeeper` | The Watchkeeper action (root `action.yml`) | A surface a team installs into CI. |
| `lore-gatekeeper` | The gate action = `pr-gate-action` (`rac gate`); the older `validate-action` is folded in / deprecated by it (ADR-058 moves with the gate) | A surface a team installs into CI; the real enforcement gate (ADR-049). |
| `decisiongrounding` | The reproducible benchmark (ADR-064, unchanged) | Established name; neither prefix. |

`rac-core` is the rename of the `requirements-as-code` GitHub repository.
`rac-localview` is the `lore-web/` directory **renamed in-repo**; its
standalone-repo *extraction* stays **deferred** (ADR-064) until a viewer
publish/vendor contract exists. Publishing `@rac/sdk` to npm decouples the
extension from the current `file:../rac-sdk` path and is the foundation for
per-client repositories.

### Per-client integration repositories

Each client integration is **its own `lore-<client>` repository**, consuming the
published `@itsthelore/asdecided-sdk` — never engine internals (ADR-063). One repo per
client is the model; a single `lore-extensions` container is rejected (it fights
independent per-client cadence and ownership). Today there is exactly one:
`lore-vscode`. Planned siblings — created when each is built, and **not part of
this decision's scope or the v0.22.x series** — include:

- **`lore-cursor`** — a Cursor-*native* integration (its rules and MCP surfaces).
  Cursor can install the `lore-vscode` VSIX as a fork, but a dedicated repo gives
  the richer native experience, so the two are separate from the start.
- **`lore-codex`, `lore-claude`, `lore-jetbrains`, …** — one per agent or editor.

The first repo is named for the editor (`lore-vscode`) because a VS Code
extension's VSIX format is shared across VS Code and its forks; the rest are named
for the agent or tool they integrate. The asymmetry is intentional.

### Release-management model

Per-repo, tag-driven; consumers pin a major where they consume an action:

- `rac-core` → PyPI on a version tag.
- `rac-sdk-ts` → npm on `sdk-v*`.
- `lore-vscode` → Marketplace + OpenVSX on `vscode-v*`.
- `lore-watchkeeper` / `lore-gatekeeper` → `@v1` (consumers pin major).
- `rac-localview` → vendored; no publish while it lives in `rac-core`.

### Amendments to ADR-064

This ADR **amends** ADR-064 on three points and is recorded as a sibling, not a
supersession — most of ADR-064 holds:

1. **Actions topology.** The GitHub Actions split into **two product-branded
   repos**, `lore-watchkeeper` + `lore-gatekeeper`, rather than the single
   `rac-actions` repo ADR-064 named, and now cover **three** actions including
   `pr-gate` / `rac gate` — the real gatekeeper. ADR-058 moves with the gate.
2. **New surfaces.** The VS Code / Cursor extension and the TypeScript SDK,
   which post-date ADR-064, are added to the topology: the extension as
   `lore-vscode`, the SDK as `rac-sdk-ts` published to npm.
3. **Viewer rename.** `lore-web` is renamed `rac-localview` **in-repo**
   (standalone extraction remains deferred per ADR-064).

This ADR **does not change** ADR-064's decisions that the dogfood corpus and
`examples/` stay in the engine repo, that `decisiongrounding` extracts, or that
standalone viewer extraction is deferred until a publish/vendor contract exists.

## Consequences

### Positive

- Every future repository name is decided by one rule (install vs engine), so
  new surfaces no longer re-litigate their prefix.
- The full surface — engine, viewer, SDK, extension, two actions, benchmark —
  has a recorded home and a per-repo release trigger, removing ambiguity about
  where a change lands and how it ships.
- Publishing `@rac/sdk` to npm makes the extension consume a versioned package
  instead of a relative path, hardening the thin-client contract (ADR-063) and
  unlocking per-client repositories.

### Negative

- More repositories to create, permission, and maintain than ADR-064 foresaw
  (two action repos instead of one, plus extension and SDK repos).
- Splitting the actions and repointing the extension are consumer-breaking
  cutovers that require versioned references and documentation updates, not
  silent moves.

### Risks

- **Cross-repo drift.** Surfaces that leave the engine repo can lag its
  contract. Mitigation: each consumes the *published* package, the public CLI,
  or the published `@rac/sdk` — never engine internals — and pins a version.
- **Stranded references.** Consumers pinned to the old action `uses:` path or to
  `file:../rac-sdk` break on cutover. Mitigation: the series sequences the
  breaking moves last, behind versioned references and deprecation notes.
- **Brand confusion during transition.** Mid-migration, some repos carry the old
  name. Mitigation: the principle is recorded here and the rename of the engine
  repo to `rac-core` is an explicit, documented org action.

## Alternatives Considered

### One `rac-actions` repository (ADR-064's original)

Keep the Actions in a single repo with per-action subdirectories.

#### Disadvantages

- The actions are what a team *installs*, so they are product surfaces and read
  more clearly as `lore-watchkeeper` and `lore-gatekeeper`. A single
  engine-prefixed repo buries the gate (the headline enforcement surface,
  ADR-049) in a subdirectory and mixes the install brand with the engine brand.
  Rejected in favour of per-action product-branded repos.

### Unify every repository under one prefix

Name everything `rac-*` (or everything `lore-*`).

#### Disadvantages

- Loses the signal that distinguishes an installable product surface from the
  engine and its internals. A single prefix tells a reader nothing about whether
  a repo is a thing they adopt or a thing the engine builds from. Rejected for
  brand/engine clarity.

### The Python SDK as its own repository

Lift the Python SDK out of the engine package into a `rac-sdk-py` repo.

#### Disadvantages

- The Python SDK's public surface *is* `rac.__all__`, shipped inside the engine
  package (ADR-062), and the engine and its server are a single package
  (ADR-029). Separating the Python SDK would fragment that package. Rejected;
  only the TypeScript client — a thin client over the contract (ADR-063) — gets
  its own repo.

## Related Decisions

- adr-064
- adr-029
- adr-036
- adr-039
- adr-049
- adr-058
- adr-062
- adr-063
- adr-092

## Related Roadmaps

- repo-extraction-programme
- v0.22.0-topology-and-release-decision

## Success Measures

- Every repository in the target topology exists under `itsthelore` with the
  prefix the naming principle dictates, and no new surface's name is decided
  ad hoc.
- The extension consumes the published `@rac/sdk`, and no consumer reference
  traces to a stranded action `uses:` path or a `file:../rac-sdk` path.
- The frozen shipped names (PyPI `requirements-as-code`, CLI `rac`, server
  `lore`) are unchanged after every rename.

## Review Date

Revisit when the `rac-localview` viewer gains a publish/vendor contract (then
decide its standalone extraction, the trigger ADR-064 set), or when a per-client
SDK ecosystem beyond TypeScript begins and the prefix principle needs hardening.
