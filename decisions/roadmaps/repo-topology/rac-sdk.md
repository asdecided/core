---
schema_version: 1
id: RAC-KW6F49KET6TB
type: roadmap
tags: [structure, org, sdk, distribution]
---
# Consolidate Language SDKs into rac-sdk

## Status

Planned

## Context

ADR-092 places the non-Python language SDKs in a single `rac-sdk` repository,
subdir per language (`ts/`, `go/`, …). The existing `rac-sdk-ts` (extracted in
v0.22.5, published to npm as `@itsthelore/asdecided-sdk`) becomes `rac-sdk/ts/`. The
**Python SDK stays in `rac-core`** — its public surface *is* `rac.__all__`
shipped inside the engine package (ADR-062), and the engine plus its server are
one package (ADR-029). Non-Python clients remain thin contract clients over the
stable `--json` surface (ADR-063).

## Outcomes

- `itsthelore/asdecided-sdk` exists with `ts/` holding the former `rac-sdk-ts`; further
  languages land as sibling subdirs.
- The published npm package name is unchanged (`@itsthelore/asdecided-sdk`), so no
  consumer break — only the source repository moves.
- The Python SDK remains in `rac-core`; no Python SDK repo is created.

## Initiatives

- **Seed `rac-sdk`** with the `rac-sdk-ts` history under `ts/`; archive
  `rac-sdk-ts` with a redirect note.
- **Preserve the npm identity**: continue publishing `@itsthelore/asdecided-sdk` from
  `rac-sdk/ts/`, with per-subdir release tags (polyglot-monorepo cost accepted,
  ADR-092).
- **Update references** in `rac-core` docs/corpus that point at `rac-sdk-ts` to
  the `rac-sdk` repo (the npm package name itself does not change).

## Success Measures

- `itsthelore/asdecided-sdk` exists with `ts/`; `rac-sdk-ts` is archived with a
  redirect.
- `@itsthelore/asdecided-sdk` continues to publish and resolve for existing consumers
  with no version break.
- The Python SDK surface (`rac.__all__`) stays in `rac-core`; no `rac-sdk-py`
  repo appears.

## Assumptions

- Non-Python SDKs are thin clients over the published contract, so they belong
  outside `rac-core` and consolidate cleanly (ADR-063).
- The mixed-toolchain monorepo cost (per-subdir release tooling) is acceptable; a
  language SDK graduates to its own repo only if it grows an independent
  community/cadence (the ADR-092 escape hatch).
- The maintainer can create `rac-sdk` and archive `rac-sdk-ts`.

## Risks

- **npm-consumer break** if the package name or publish flow changes. Mitigation:
  keep `@itsthelore/asdecided-sdk` exactly; move only the source repo, behind a GitHub
  redirect.
- **Toolchain friction** across languages in one repo. Mitigation: isolate each
  language in its subdir with its own build and release tag.

## Related Decisions

- adr-092
- adr-062
- adr-063
- adr-029

## Related Roadmaps

- repo-topology-convergence
- v0.22.5-extract-typescript-stack
- v0.20.1-python-sdk-docs
