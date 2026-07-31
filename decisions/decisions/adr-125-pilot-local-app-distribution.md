---
schema_version: 1
id: RAC-01K8P7A3M5QZ
type: decision
---
# ADR-125: Distribute AsDecided as a Local Pilot App

## Context

Pilot Protocol has two materially different discovery surfaces. Service agents
are always-on HTTP services reached across Pilot's encrypted overlay; App Store
apps are signed local adapters installed and supervised on the user's own node.
AsDecided reads repository-local engineering knowledge and deliberately avoids a
hosted corpus, so presenting it as a service agent would introduce tenancy,
authentication, storage, and trust decisions that the product has not made.

Pilot's App Store now supports generated CLI adapters, `proc.exec` consent, and
per-platform native assets. A valid native submission must cover macOS and Linux
on both amd64 and arm64. AsDecided's current GitHub Release matrix omits macOS
amd64 and Linux arm64, which makes distribution incomplete before the adapter is
even considered.

## Decision

AsDecided will target Pilot's local App Store as `io.pilot.asdecided`, not the
Pilot service-agent directory.

- The adapter is generated from Pilot's official `pilot-app` template and
  invokes a release-pinned native `decided` binary delivered as a signed app
  asset.
- The public method surface is curated and read-only. It exposes deterministic
  retrieval, validation, relationship inspection, and help; it does not expose
  arbitrary CLI passthrough, mutation, migration, scaffolding, or sharing.
- Repository access is explicit in each call and declared in the app's install
  grants. Submission must not imply that a corpus is uploaded to Pilot or
  AsDecided.
- Native releases cover `darwin/{amd64,arm64}` and
  `linux/{amd64,arm64}`. Windows remains a supported AsDecided release target
  but is not part of Pilot's required native asset set.
- Catalogue submission happens only after the four assets exist for one
  immutable AsDecided release, their hashes are pinned, the generated bundle
  verifies locally, and a real Pilot node proves install, help, and one retrieval
  call against a fixture repository.

## Consequences

### Positive

- Pilot users install AsDecided without creating a remote copy of their corpus.
- The Pilot surface inherits the Rust engine's deterministic behavior instead
  of creating another implementation.
- Four-target native releases improve direct GitHub distribution independently
  of Pilot.

### Negative

- The Pilot adapter is a second executable layer and must track its upstream IPC
  and manifest contracts.
- Native release CI adds two runners and therefore more release time.
- Pilot's declared `proc.exec` and filesystem grants require higher scrutiny
  than a remote HTTP adapter.

### Risks

- Source support for `proc.exec` may precede deployment on every Pilot host.
  Mitigation: the catalogue submission is gated on a real-node install test.
- A broad filesystem grant could overstate required authority. Mitigation: no
  arbitrary passthrough, explicit repository input, and review of the generated
  manifest before signing or submission.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

### Publish an always-on Pilot service agent

Rejected. It would require hosting user knowledge and would change AsDecided's
local-first trust boundary rather than merely distribute the existing product.

### Front a hosted HTTP shim

Rejected for the same reason and because the local CLI adapter is now supported
by Pilot's current source.

### Expose the whole CLI through passthrough

Rejected. It would grant agents mutation and migration commands unrelated to
the read-only discovery use case and make meaningful permission review harder.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "Pilot's required native target set has a stable release-workflow anchor."
rules:
  - id: pilot-keeps-linux-arm64-release
    kind: require_pattern
    path_glob: ".github/workflows/native-publish.yml"
    pattern: 'target: aarch64-unknown-linux-gnu'
    message: "Pilot distribution requires a Linux arm64 native asset."
  - id: pilot-keeps-macos-amd64-release
    kind: require_pattern
    path_glob: ".github/workflows/native-publish.yml"
    pattern: 'target: x86_64-apple-darwin'
    message: "Pilot distribution requires a macOS amd64 native asset."
```

## Related Decisions

- adr-098
- adr-121

## Related Requirements

- asdecided-pilot-app-distribution
