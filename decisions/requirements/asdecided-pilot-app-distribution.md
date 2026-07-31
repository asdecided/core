---
schema_version: 1
id: RAC-01K8P7A3M6QZ
type: requirement
---
# REQ-AsDecided-Pilot-App-Distribution

> The key words MUST, MUST NOT, SHOULD, SHOULD NOT, and MAY in this document are
> to be interpreted as described in BCP 14 (RFC 2119, RFC 8174) when, and only
> when, they appear in all capitals.

## Status

Accepted

## Problem

Pilot users cannot discover and install AsDecided through Pilot's local App
Store, while its service-agent model would require AsDecided to host private
repository knowledge. Distribution must preserve local execution and ship a
complete, verifiable native asset set.

## Requirements

- [REQ-001] AsDecided's Pilot identity MUST be `io.pilot.asdecided` and MUST be distributed as a local App Store app rather than an always-on service agent.
- [REQ-002] One immutable AsDecided release MUST provide native `decided` assets for macOS and Linux on both amd64 and arm64 before a catalogue submission is made.
- [REQ-003] Every submitted asset and generated adapter bundle MUST be pinned by SHA-256 and covered by Pilot's signed manifest and catalogue verification flow.
- [REQ-004] The Pilot method surface MUST be curated and read-only and MUST NOT provide arbitrary CLI passthrough, scaffolding, migration, rename, sharing, or other mutation commands.
- [REQ-005] Calls MUST identify the repository they read, and the generated manifest MUST declare the filesystem and `proc.exec` authority required to service those calls.
- [REQ-006] A release candidate MUST pass Pilot template validation, bundle verification, catalogue validation, and a real-node smoke test covering install, help, and deterministic retrieval.
- [REQ-007] Product copy MUST state that the app executes locally and MUST NOT imply that Pilot or AsDecided receives, hosts, or indexes the user's corpus.

## Acceptance Criteria

- A GitHub Release contains `asdecided-x86_64-unknown-linux-gnu.tar.gz`,
  `asdecided-aarch64-unknown-linux-gnu.tar.gz`,
  `asdecided-x86_64-apple-darwin.tar.gz`, and
  `asdecided-aarch64-apple-darwin.tar.gz`, each containing `decided`.
- The generated app exposes only its documented help, retrieval, validation,
  and relationship-inspection methods.
- `pilot-app validate` and bundle verification pass with exact release asset
  hashes.
- A clean Pilot node installs the catalogue candidate and returns a valid
  retrieval response from a local fixture corpus without a network corpus
  dependency.

## Success Metrics

- `pilotctl appstore install io.pilot.asdecided` installs and starts on all four
  supported Pilot OS/architecture combinations.
- Repeated retrieval against unchanged local input returns equal output through
  the Pilot adapter and direct `decided` CLI.

## Risks

- Pilot's App Store is young and may change its manifest or IPC contracts.
- The generated adapter could broaden authority beyond the curated surface.
  Manifest and generated-source review are mandatory before signing.

## Assumptions

- Pilot deploys its current `proc.exec` support to the nodes used for release
  verification.
- Pilot's catalogue accepts publisher-provided Rust binaries as native assets
  behind its generated CLI adapter.

## Related Decisions

- adr-125
- adr-098

## Related Requirements

- rac-release-versioning
