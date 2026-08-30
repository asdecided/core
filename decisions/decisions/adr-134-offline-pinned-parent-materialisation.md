---
schema_version: 1
id: RAC-KZKMJ8WSMFA1
type: decision
---
# ADR-134: Declare and Verify an Offline Materialised Parent

## Context

> **Amended by ADR-145.** Version 1 retains this ADR's exact single-parent
> carrier and digest. Manifest version 2 permits a strict parent sequence and
> uses a topology-binding digest that authenticates exact nested-manifest
> presence and bytes. Materialised-only, offline, containment, exact-byte,
> source-verification, and pre-overlay failure rules remain authoritative.

ADR-089 requires federation to remain offline, deterministic, Git-native, and
human-readable. Those constraints still leave materially different mechanisms:
a configuration key, a live repository URL, an adjacent checkout, a Git commit
pin, or a content digest over bytes already present in the child repository.

The resolver must know exactly which bytes govern a run without turning
AsDecided into a clone, sync, or package-management service.

## Decision

The child declares its parent in the fixed operational Markdown manifest
`.decided/corpus.md`.

- The manifest is not an AsDecided artifact and does not enter the artifact
  walk, search index, relationship graph, or exports.
- Its federation headings are exactly lowercase `## inherits` and
  `## overrides`.
- `## inherits` contains one versioned YAML mapping with the parent alias,
  source identity, repository-relative materialisation root, corpus path below
  that root, and a full lowercase SHA-256 digest.
- The parent is already materialised inside the child repository as a Git
  submodule or vendored directory. AsDecided performs no clone, fetch, pull,
  refresh, or other network operation.
- Absolute paths, `..` components, canonical path escape, and symlink traversal
  to the parent config or corpus artifacts are rejected before loading.
- Verification requires the materialisation, governing parent
  `.decided/config.yaml`, declared parent source, and parent corpus to exist and
  agree with the manifest.
- The versioned digest covers a domain separator, the parent source identity,
  the governing parent config bytes, and sorted corpus-relative Markdown paths
  and bytes. Checkout location and timestamps are excluded.
- No parent artifact enters the composition until every verification step and
  the digest comparison succeed.

Refreshing parent bytes and updating the recorded digest are explicit Git
operations outside AsDecided.

## Consequences

The effective parent is a pure function of reviewable child-repository bytes.
Two clones with the same committed state verify the same parent, and a stale or
tampered materialisation fails before it can influence retrieval or
enforcement.

Operators must materialise and repin parents themselves. Adjacent checkouts and
remote references are intentionally unsupported, and vendored copies increase
repository size when submodules are not suitable.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

### Fetch a repository URL during validation or serving

Rejected. Network state would make grounding depend on availability, credentials,
and mutable remote state, violating ADR-002 and ADR-089.

### Accept an adjacent checkout or absolute path

Rejected. The same child commit would resolve differently across machines, and
the parent would escape the child repository's reviewable state.

### Pin only a Git commit

Rejected. Vendored parents have no gitlink, and a commit identifier does not by
itself define the governing config and corpus bytes consumed by the engine.

### Put inheritance in `.decided/config.yaml`

Rejected. ADR-089 requires a human-readable Markdown declaration, and the
operational manifest keeps the mechanism visible without making it an artifact.

## Related Decisions

- adr-001
- adr-002
- adr-016
- adr-018
- adr-055
- adr-065
- adr-080
- adr-089
- adr-145

## Related Designs

- corpus-federation-mechanism
- corpus-federation-graph-composition

## Related Requirements

- parent-corpus-inheritance
