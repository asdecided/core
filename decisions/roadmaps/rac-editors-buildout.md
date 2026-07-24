---
schema_version: 1
id: RAC-KWGQKAXMB16T
type: roadmap
tags: [editor, distribution, release, structure]
---
# rac-editors Build-Out

## Status

Planned

## Context

The repo-topology `rac-editors` item delivered the family repository, but as a
shell: the `lore-vscode` seed it renamed was a LICENSE-only stub, so
`rac-editors/vscode/` holds no extension source. The real VS Code / Cursor
extension lived in rac-core at `typescript/rac-vscode` and was removed by
v0.22.5 after the seed repos were created — the removal commit is
`eed463e`, and the last pre-removal tree (`eed463e^`) carries the complete
extension: source, test harness (v0.21.9), fixture corpus, esbuild bundling,
packaging config, and the `extension-release.yml` / `typescript.yml` workflows.

Meanwhile v0.21.10 (Marketplace & OpenVSX publish) remains the one open item of
the v0.21.x editor series: the extension is packaged but unpublished, so its
reach is zero. This item fences the work to close both gaps: recover the source
into its ADR-092 home and make it shippable from there. v0.21.10 stays the
publish contract — this item executes it from the new home rather than
rewriting it.

## Outcomes

- The extension source lives, builds, and tests in `rac-editors/vscode/`,
  consuming the published `@itsthelore/asdecided-sdk` — a thin client over the
  contract, never engine internals (ADR-063).
- rac-editors has a merge-gated CI battery (ADR-027, ADR-075) and a
  tag-triggered release pipeline in which publishing is the last step,
  reachable only on green (v0.21.10).
- The Explorer webview ships publish-ready: strict CSP and validated message
  handling before the first public release.
- First publish to the VS Code Marketplace and OpenVSX is unblocked on exactly
  one human decision (publisher id) and the human-owned secrets — nothing else.

## Initiatives

### Initiative 1 — Source recovery with provenance

Snapshot-import `typescript/rac-vscode` from rac-core `eed463e^` into
`rac-editors/vscode/`, recording the source SHA in the commit body rather than
replaying engine-scoped history onto the family trunk. Repoint the SDK
dependency from `file:../rac-sdk` to the published `@itsthelore/asdecided-sdk`
(finishing the v0.22.5 repoint contract, which the removed tree never
received). Prove the result: typecheck, test battery, and a packaged VSIX.

### Initiative 2 — CI battery and release pipeline

A `pr-checks` battery in rac-editors (install, typecheck, bundle, the v0.21.9
integration tests under a virtual display, VSIX packaging smoke) intended as a
required merge gate per ADR-075, and a `vscode-v*` tag-triggered release
workflow adapted from the recovered `extension-release.yml`: build → gate →
package → publish last, only on green, with a tokenless `workflow_dispatch`
dry-run path (v0.21.10 initiative 1).

### Initiative 3 — Webview hardening and listing

Before the first tag: strict `Content-Security-Policy` on the Explorer webview
and validated message envelopes (v0.21.10 initiative 2), with regression tests
in the battery; then the Marketplace listing content — metadata, README as the
listing page, and the no-telemetry / fully-offline posture stated as a selling
point (v0.21.10 initiative 3).

### Initiative 4 — Publish readiness, human-owned steps fenced

The publisher id decision, the Marketplace and OpenVSX accounts, and the
`VSCE_PAT` / `OVSX_PAT` secrets (now landing in rac-editors, not the archived
seed repo) stay explicit human-owned steps, called out and never automated.
The recovered manifest still carries the pre-ADR-092 identity (`publisher:
"rac"`, "RAC — Requirements as Code"); the final listing identity is applied
once the publisher decision lands, and the first publish waits for it.

## Success Measures

- `npm ci && npm run check-types && npm test && vsce package` succeeds in
  `rac-editors/vscode/` against the published SDK, with no `file:` dependency.
- The pr-checks battery is green on its own pull request and catches a removed
  CSP tag or a forged webview message (mutation-checked regression tests).
- A `workflow_dispatch` run of the release workflow produces a VSIX artifact
  with no publish and no secrets.
- After the human-triggered first tag: the Marketplace and OpenVSX listings
  resolve, and install and auto-update work (v0.21.10 success measures).
- v0.21.10 and this item flip to Achieved together after the first publish.

## Assumptions

- `@itsthelore/asdecided-sdk` is published on npm (verified: 0.1.0) and exposes the
  API surface the extension consumes; if not, the fix is a new SDK release via
  the v0.22.3 flow, never vendored SDK code (ADR-063).
- The Explorer webview keeps consuming `rac export --html` output at runtime
  (self-contained Portal, `localResourceRoots: []`), so no build-time viewer
  coupling follows the source into rac-editors.
- rac-core `eed463e` remains reachable as the provenance anchor for the
  snapshot import.
- The publisher id decision may land as a brand re-decision (a new ADR
  superseding the listing-identity parts of ADR-092/ADR-036); this item does
  not pre-empt it and publishes only after it.

## Risks

- **Publishing under an identity that is later re-branded.** The maintainer is
  weighing a pivot of the listing brand; a Marketplace publisher id cannot be
  renamed. Mitigation: the id is a fenced human decision and the first publish
  blocks on it; everything else lands independently.
- **The battery passes locally but not in CI** (display, `rac` provisioning).
  Mitigation: the recovered workflows already solved both (`xvfb-run`, pip
  install); adapt rather than reinvent, and install `rac` from PyPI in
  rac-editors CI.
- **Partial publish** (Marketplace succeeds, OpenVSX fails). Mitigation: one
  VSIX built once, published to both registries in the same job, secrets
  checked loudly before either publish step runs.

## Related Decisions

- adr-092
- adr-093
- adr-094
- adr-063
- adr-068
- adr-027
- adr-075

## Related Roadmaps

- v0.21.10-marketplace-publish
- v0.21.9-extension-test-harness
- v0.22.5-extract-typescript-stack
- rac-editors

## Related Tickets

- itsthelore/asdecided-editors#2
