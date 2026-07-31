---
schema_version: 1
id: RAC-KYVTHR4WNT8T
type: design
---
# MCP Registry Cargo Publishing

## Context

ADR-124 selects Cargo as the package authority behind AsDecided's official MCP
Registry record. The release design must coordinate three immutable values—the
workspace version, crates.io version, and Registry version—without making a
Registry outage force a duplicate crate publication.

## User Need

An MCP user needs to find the canonical AsDecided server, understand that it
runs locally over their repository, and install the actual Rust implementation.
A maintainer needs a release path that is inspectable, secretless at the
Registry boundary, and safe to retry after partial external failure.

## Design

The repository contains four connected surfaces:

1. `rust/decided-mcp/Cargo.toml` defines a publishable crates.io package whose
   README contains `mcp-name: io.github.asdecided/core`.
2. `server.json` points `io.github.asdecided/core` at that exact Cargo package
   version using stdio transport.
3. The crates.io workflow publishes `asdecided-core`, waits for it to reach the
   index, then publishes both executable crates, `decided` and `decided-mcp`.
4. A separate Registry workflow accepts an exact version, proves it matches the
   workspace and both manifest version fields, waits for that crate on
   crates.io, installs an exact checksum-pinned `mcp-publisher`, validates the
   listing, authenticates with GitHub OIDC, and publishes.

The first publication is made in the next patch release. v0.26.0 is not rebuilt
after its immutable release tag. Each subsequent release updates the workspace,
exact engine dependencies, lockfile, changelog, and both `server.json` version
fields together before the tag is created.

## Constraints

- Registry metadata cannot publish before its package exists and carries the
  ownership token.
- crates.io versions and Git tags are immutable; recovery must move forward or
  retry metadata publication, never overwrite an artifact.
- The Registry is metadata-only and in preview. Local installation and use must
  remain viable if it is unavailable.
- Current identity is `decided-mcp`; ADR-121's legacy `lore` response bytes must
  not change.

## Rationale

A dedicated Cargo package makes the Registry record resolve directly to the
binary users run. Splitting Registry publication from crate publication gives
the non-transactional release chain a safe recovery point: once crates.io has
accepted a version, the Registry step can be retried without touching it.

## Alternatives

- **One monolithic workflow:** rejected because a Registry failure leaves no
  safe way to rerun after crates.io has accepted immutable versions.
- **OCI-only package:** rejected because the current image launches the CLI by
  default and Cargo is the native distribution surface for this Rust server.
- **Hosted remote entry:** rejected because AsDecided has not decided to host
  user corpora or introduce the associated tenancy, authentication, and trust
  boundary.

## Accessibility

The title and description use plain product language rather than internal
component names. Installation and configuration examples remain text, are
copyable, and do not depend on images or colour.

## Style Guidance

Describe capability and authority precisely: “read-only,” “local repository,”
and “deterministic” are useful; “hosted,” “AI-powered,” or universal client
compatibility are unsupported claims. Keep AsDecided as the display name and
reserve `decided-mcp` for the executable and package.

## Open Questions

- Which MCP clients will consume Cargo package records automatically while the
  Registry remains in preview?
- When the Registry reaches general availability, should publication move from
  manual dispatch to an automatically gated release job?

## Related Decisions

- adr-124
- adr-121

## Related Requirements

- asdecided-public-mcp-distribution
