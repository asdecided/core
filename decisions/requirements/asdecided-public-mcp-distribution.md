---
schema_version: 1
id: RAC-KYVTHR3TXXHC
type: requirement
---
# REQ-AsDecided-Public-MCP-Distribution

> The key words MUST, MUST NOT, SHOULD, SHOULD NOT, and MAY in this document are
> to be interpreted as described in BCP 14 (RFC 2119, RFC 8174) when, and only
> when, they appear in all capitals.

## Status

Accepted

## Problem

Installing AsDecided through Homebrew, Cargo, or a GitHub Release does not make
its MCP server discoverable through MCP-native catalogues. A public listing
must be verifiably controlled by AsDecided, use a package type supported by the
live official Registry, resolve to the native Rust server, preserve the
local-first trust boundary, and participate in the same immutable release line
as every other distribution channel.

## Requirements

- [REQ-001] `decided-mcp` MUST remain an installable crates.io package from the same release commit and exact version as the AsDecided workspace, independently of official Registry packaging.
- [REQ-002] The official MCP Registry record MUST use the canonical name `io.github.asdecided/core`, title `AsDecided`, an OCI package under `ghcr.io/asdecided/core`, and stdio transport.
- [REQ-003] The registered OCI image MUST carry `io.modelcontextprotocol.server.name=io.github.asdecided/core`, MUST launch `decided-mcp`, and Registry authentication MUST use GitHub OIDC rather than a long-lived personal or organisation token.
- [REQ-004] The Registry version, versioned MCP image tag, and workspace version MUST be equal. Publication MUST fail before Registry mutation when they disagree.
- [REQ-005] Registry publication MUST check out the exact release tag, wait until its MCP image is retrievable from GHCR, verify the ownership label and entrypoint, then run the checksum-pinned official publisher's validation before publishing.
- [REQ-006] Registry publication MUST be independently retryable without rebuilding, republishing, or overwriting an immutable release image or crate.
- [REQ-007] The public metadata MUST describe AsDecided as a local, read-only MCP server and MUST NOT imply a hosted corpus, remote index, model call, write authority, or universal filesystem-mount support.
- [REQ-008] The root Dockerfile MUST expose a dedicated `asdecided-mcp` build target that launches the native `decided-mcp` binary while preserving the existing CLI as the default target.
- [REQ-009] Native releases MUST publish the dedicated target under an immutable `mcp-v<version>` tag in the existing Core GHCR package; movable `mcp-latest` MUST NOT be used by the Registry record.
- [REQ-010] A Docker MCP Catalog entry MUST select the named build target, pin an immutable Core commit, mount only a user-selected repository, pass its container path through `--root`, and disable network access.
- [REQ-011] Secondary catalog metadata MUST retain `io.github.asdecided/core` as the canonical MCP identity and MUST remain downstream of the Core release and official Registry record rather than becoming a second release authority.

## Acceptance Criteria

- `cargo publish --dry-run --locked -p decided-mcp` succeeds from a clean release
  candidate, preserving the independent Rust-native installation path.
- Pull-request CI proves the Registry namespace, OCI package type, exact image
  tag convention, stdio transport, workspace version, Docker ownership label,
  and native MCP target agree.
- Publishing a release builds the CLI image and the dedicated MCP image from
  the same commit, then a current-protocol `server/discover` smoke request
  reports `decided-mcp` and the release version.
- The Registry workflow checks out `v<version>` and refuses to publish until
  `ghcr.io/asdecided/core:mcp-v<version>` is available with the canonical label
  and entrypoint.
- Re-running only the Registry workflow can recover from a Registry outage
  without attempting to republish an image or crate.
- The checksum-pinned `mcp-publisher validate` and `publish` commands succeed,
  and the live Registry API returns the exact canonical name and version.
- Docker's catalog validation and image build pass for metadata that pins a Core
  commit containing the dedicated target.

## Success Metrics

- The current AsDecided release is discoverable under exactly one canonical
  official MCP Registry identity.
- Registry metadata and the immutable GHCR image never advertise different
  AsDecided versions or entrypoints.
- Registry publication requires no persistent Registry secret.

## Risks

- The preview Registry may reset data or change schema. Cargo, Homebrew, native
  archives, and the published image remain independent installation channels.
- Some clients may discover OCI packages but lack a portable way to grant the
  local repository mount. Product copy and setup guidance must retain that
  limitation rather than claiming universal one-click installation.
- GHCR package visibility or image-label drift could block verification. The
  Registry workflow fails closed before publication.

## Assumptions

- The official Registry continues to accept OCI packages and GitHub
  organisation OIDC namespaces.
- GHCR continues to serve the public Core image package.
- crates.io remains the canonical Rust package registry for direct native
  installation, not for official Registry ownership verification.

## Related Decisions

- adr-124
- adr-126
- adr-121
- adr-039

## Related Requirements

- rac-release-versioning
