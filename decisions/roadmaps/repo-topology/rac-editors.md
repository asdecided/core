---
schema_version: 1
id: RAC-KW6F4D6J27WS
type: roadmap
tags: [structure, org, editor, distribution]
---
# Rename lore-vscode into rac-editors

## Status

Achieved

## Context

ADR-092 places the IDE clients in a single `rac-editors` repository, subdir per
client. The existing `lore-vscode` (the VS Code / Cursor extension, extracted in
v0.22.5) becomes `rac-editors/vscode/`; future editors (for example
`jetbrains/`) join as sibling subdirs. The repo takes the uniform `rac-*` slug,
while the published extension still **lists** as "Lore for VS Code" — the brand
lives at the marketplace listing, not the repository name (ADR-092).

## Outcomes

- `itsthelore/asdecided-editors` exists with the VS Code / Cursor extension under
  `vscode/`; further IDE clients land as sibling subdirs.
- The Marketplace / OpenVSX listing identity and the published extension id are
  unchanged, so installed users are unaffected — only the source repo moves.
- `rac-core` references to the editor client resolve to `rac-editors`.

## Initiatives

- **Seed `rac-editors`** with the `lore-vscode` history under `vscode/`; archive
  `lore-vscode` with a redirect note.
- **Preserve the listing identity**: continue publishing the same VSIX to
  Marketplace + OpenVSX from `rac-editors/vscode/`, listed as "Lore for VS Code".
- **Update references** in `rac-core` docs/corpus from `lore-vscode` to
  `rac-editors`.

## Success Measures

- `itsthelore/asdecided-editors` exists with `vscode/`; `lore-vscode` is archived with
  a redirect.
- The extension continues to publish and update for existing users with no
  identity break.
- No `rac-core` reference points at `lore-vscode`.

## Assumptions

- The extension consumes the published `@itsthelore/asdecided-sdk` and the public CLI,
  never engine internals (ADR-063), so the repo move needs no contract change.
- A shared VSIX serves VS Code and its Cursor fork (ADR-068's surviving client
  decision), so `vscode/` covers both today.
- The maintainer can create `rac-editors` and archive `lore-vscode`.

## Risks

- **Listing/identity break** if the publish flow changes. Mitigation: keep the
  Marketplace/OpenVSX identity and VSIX id; move only the source repo, behind a
  GitHub redirect.
- **Premature single-client repo.** Mitigation: the family form makes the next
  editor a subdir, not a new repo decision.

## Related Decisions

- adr-092
- adr-068
- adr-063

## Related Roadmaps

- repo-topology-convergence
- v0.22.5-extract-typescript-stack
