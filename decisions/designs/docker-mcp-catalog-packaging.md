---
schema_version: 1
id: RAC-KYYC7NNZE2ZR
type: design
---
# Docker MCP Catalog Packaging

## Context

ADR-126 adds Docker's curated MCP Catalog as a downstream discovery surface for
the native server selected by ADR-124. The catalog builds local servers from
their source repository, probes them over stdio, and exposes configurable volume
grants in Docker Desktop.

## User Need

A Docker Desktop user needs to select AsDecided, grant one local repository,
and let an MCP client retrieve its recorded engineering decisions without
installing Rust tooling or uploading the corpus.

## Design

The root Dockerfile builds both native executables once in a Rust builder stage,
copies them into one minimal shared runtime, and exposes two final targets:

1. `asdecided-cli` remains last and therefore preserves the existing default
   `decided` entrypoint.
2. `asdecided-mcp` adds the canonical MCP ownership label and replaces only the
   entrypoint with `decided-mcp`.

Docker catalog metadata selects `source.buildTarget: asdecided-mcp`. Its single
`repository` parameter becomes a read-only-intent volume grant and the server is
started with `--root` pointing at the mounted container path. The catalog entry
sets `disableNetwork: true`, because repository retrieval requires no network.

The catalog description says exactly what the server does: read-only access to
deterministic engineering decisions in a local repository. It links to Core and
uses the AsDecided organisation icon. It does not claim Docker acceptance until
the upstream PR is merged.

## Constraints

- The default Dockerfile target must remain compatible with existing CLI use.
- The catalog image must answer MCP `initialize` and `tools/list` over stdio.
- The catalog must pin an immutable Core commit and select the named MCP target.
- Repository access must be user-configured; no home-directory-wide implicit
  mount is allowed.
- The container must not require credentials, outbound network access, Python,
  Node.js, or a model call.

## Rationale

A named final target is the smallest compatibility-preserving seam. Docker can
build and inspect the MCP process it expects, while direct `docker build .`
continues to produce the CLI image. Both derive from identical locked Rust
sources and one runtime layer.

## Alternatives

- **Override the command in catalog metadata:** Docker's `run.command` appends
  arguments to the image entrypoint; it does not replace `decided` with
  `decided-mcp`.
- **Make MCP the default image:** rejected as an avoidable CLI compatibility
  break.
- **Point Docker's catalog at Core's pre-built MCP image:** rejected for this
  surface. Core publishes that image for the official Registry, while Docker's
  catalog source-build path supplies its own signing, provenance, SBOMs, and
  update process from an immutable Core commit.

## Accessibility

The configuration is one clearly labelled path field and does not depend on an
image, colour, or pointer-only interaction. Errors from an absent or invalid
corpus remain textual MCP errors from the native server.

## Style Guidance

Use AsDecided as the product title and `decided-mcp` only for the executable.
Prefer “local,” “read-only,” and “deterministic”; avoid “hosted,” “AI-powered,”
or claims of enforcement beyond the server's retrieval tools.

## Open Questions

- Whether Docker's generated image exposes an independently useful version
  label beyond the pinned Core commit.
- Whether the catalog supports a read-only mount flag in addition to the server's
  read-only MCP behavior; if it does, the upstream entry should adopt it.

## Related Decisions

- adr-126
- adr-124

## Related Requirements

- asdecided-public-mcp-distribution
