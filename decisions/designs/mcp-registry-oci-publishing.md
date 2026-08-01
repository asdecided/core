---
schema_version: 1
id: RAC-KYVTHR4WNT8T
type: design
---
# MCP Registry OCI Publishing

## Context

ADR-124 uses OCI as the supported package authority behind AsDecided's official
MCP Registry record. Cargo remains a native installation surface, but the live
Registry does not accept Cargo package records. The release design must
coordinate the workspace, GHCR image, and Registry version without making a
Registry outage rebuild or overwrite an immutable release.

## User Need

An MCP user needs to find the canonical AsDecided server, understand that it
runs locally over their repository, and receive the actual Rust implementation.
A maintainer needs an inspectable, secretless publication path that is safe to
retry after partial external failure.

## Design

The repository contains four connected surfaces:

1. The root Dockerfile builds both native executables and exposes a dedicated
   `asdecided-mcp` target with the canonical ownership label and
   `decided-mcp` entrypoint.
2. A release publishes that target to the existing public Core GHCR package as
   `ghcr.io/asdecided/core:mcp-v<version>` and advances `mcp-latest`. Existing
   CLI tags retain the default `decided` image.
3. `server.json` points `io.github.asdecided/core` at the exact versioned OCI
   tag using stdio transport.
4. A separate Registry workflow checks out `v<version>`, proves the workspace
   and manifest agree, waits for the image, verifies its ownership label and
   entrypoint, installs the checksum-pinned publisher, validates the listing,
   authenticates through GitHub OIDC, and publishes.

The first publication uses v0.26.2. Each later release updates the workspace,
exact engine dependencies, lockfile, changelog, Registry version, and image tag
together before the immutable tag is created.

## Constraints

- Registry metadata cannot publish before its exact public OCI image exists.
- The registered image must launch `decided-mcp`, not the default CLI target.
- The ownership label and Registry name must both remain
  `io.github.asdecided/core`.
- Git tags and Registry versions are immutable; recovery retries metadata
  publication from the release tag and never overwrites an old version tag.
- The Registry is metadata-only and in preview. Cargo, Homebrew, and native
  archives must remain usable if it is unavailable.
- Filesystem access stays explicit and local; the record must not imply a hosted
  corpus or universal client-side mount support.

## Rationale

OCI is supported by the live Registry and can carry the exact native binary
without a language wrapper. Reusing the established Core GHCR package avoids a
new repository and visibility policy. Distinct `mcp-v<version>` tags prevent the
MCP entrypoint from changing existing CLI image behavior.

Separating image publication from Registry publication creates a safe recovery
point. Once GHCR has accepted the immutable release tag, Registry publication
can be retried without rebuilding or republishing the image.

## Alternatives

- **Cargo record:** rejected because the live Registry does not support Cargo.
- **One monolithic workflow:** rejected because a Registry failure would make
  the already-published release harder to retry safely.
- **Separate `core-mcp` GHCR package:** rejected because the existing Core
  package can carry explicit MCP tags without another visibility or ownership
  surface.
- **MCPB package:** deferred; it adds platform-specific artifacts and metadata
  while OCI already satisfies the supported package and ownership contract.
- **Hosted remote entry:** rejected because AsDecided has not decided to host
  user corpora or introduce the associated trust boundary.

## Accessibility

The title and description use plain product language rather than internal
component names. Installation and configuration remain text-based and copyable.

## Style Guidance

Describe capability and authority precisely: “read-only,” “local repository,”
and “deterministic” are useful; “hosted,” “AI-powered,” or universal client
compatibility are unsupported claims. Keep AsDecided as the display name and
reserve `decided-mcp` for the executable.

## Open Questions

- Which downstream clients can express the required repository mount directly
  from OCI package metadata?
- When the Registry reaches general availability, should publication move from
  a manually approved dispatch to an automatically gated release job?

## Related Decisions

- adr-124
- adr-126
- adr-121

## Related Requirements

- asdecided-public-mcp-distribution
