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

Installing AsDecided through Homebrew or a GitHub Release does not make its MCP
server discoverable through MCP-native catalogues. A public listing must be
verifiably controlled by AsDecided, resolve to the native Rust server, preserve
the local-first trust boundary, and participate in the same immutable release
line as every other distribution channel.

## Requirements

- [REQ-001] `decided-mcp` MUST be published as an installable crates.io package from the same release commit and exact version as the AsDecided workspace.
- [REQ-002] The official MCP Registry record MUST use the canonical name `io.github.asdecided/core`, title `AsDecided`, Cargo package identifier `decided-mcp`, and stdio transport.
- [REQ-003] The published crate README MUST contain the exact ownership token `mcp-name: io.github.asdecided/core`, and Registry authentication MUST use GitHub OIDC rather than a long-lived personal or organisation token.
- [REQ-004] The Registry version, package version, and workspace version MUST be equal. Publication MUST fail before Registry mutation when they disagree.
- [REQ-005] Registry publication MUST wait until the exact crate version is retrievable from crates.io, then run the checksum-pinned official publisher's validation before publishing.
- [REQ-006] Registry publication MUST be independently retryable without republishing or overwriting an immutable crate version.
- [REQ-007] The public metadata MUST describe AsDecided as a local, read-only MCP server and MUST NOT imply a hosted corpus, remote index, model call, or write authority.

## Acceptance Criteria

- `cargo publish --dry-run --locked -p decided-mcp` succeeds from a clean release
  candidate.
- Pull-request CI proves the manifest namespace, Cargo registry, package,
  transport, and versions and finds the exact ownership token in the packaged
  README.
- On a release, crates.io serves the exact `decided-mcp` version before
  `mcp-publisher validate` and `mcp-publisher publish` run.
- Re-running only the Registry workflow can recover from a Registry outage
  without attempting to publish any crate again.
- Installing the listed crate exposes `decided-mcp`, which serves a local corpus
  over stdio with the same current protocol identity tested by ADR-121.

## Success Metrics

- The current AsDecided release is discoverable under exactly one canonical
  official MCP Registry identity.
- Registry metadata and crates.io never advertise different AsDecided versions.
- Registry publication requires no persistent Registry secret.

## Risks

- The preview Registry may reset data or change schema. The crate remains a
  complete independent installation channel, and the pinned workflow makes a
  republish explicit.
- Some clients may not yet install Cargo packages automatically. Homebrew and
  native archives remain documented alternatives; the listing must not claim
  universal client support.

## Assumptions

- The official Registry continues to accept Cargo packages and GitHub
  organisation OIDC namespaces.
- crates.io remains the canonical Rust package registry for the native server.

## Related Decisions

- adr-124
- adr-121
- adr-039

## Related Requirements

- rac-release-versioning
