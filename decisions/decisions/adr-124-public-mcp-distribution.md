---
schema_version: 1
id: RAC-KYVTHFQD44BP
type: decision
---
# ADR-124: Publish the Native MCP Server Through OCI and the Official Registry

## Context

AsDecided ships `decided-mcp` through Homebrew, Cargo, and native GitHub
Release archives, but those channels do not make the server discoverable to MCP
clients. The official MCP Registry is a metadata registry: a local-server record
must point at a supported public package and prove control of both its namespace
and that package.

The original decision selected Cargo after concluding that the Registry
accepted Cargo packages. The live Registry contract does not: its supported
local package types are npm, PyPI, NuGet, OCI, and MCPB. Publishing the existing
Cargo manifest would therefore fail. Core now has a dedicated
`asdecided-mcp` OCI target whose ownership label, native entrypoint, and local
trust boundary meet the Registry's supported verification path.

ADR-039 reserved `io.github.tcballard/lore` before the AsDecided rename and
Rust cutover. ADR-121 has since made `decided-mcp` the current protocol identity
while retaining `lore` only in frozen legacy responses.

## Decision

The public MCP Registry identity remains `io.github.asdecided/core`.

- Cargo remains a supported native installation channel for `decided-mcp`, but
  it is not the package authority used by the official Registry.
- Each release publishes the dedicated `asdecided-mcp` Docker target as
  `ghcr.io/asdecided/core:mcp-v<version>`. The existing CLI image keeps its
  `v<version>` and `latest` tags; the MCP image also advances `mcp-latest`.
- `server.json` identifies the package as OCI, pins the exact versioned MCP
  image tag, and declares stdio transport. The image carries
  `io.modelcontextprotocol.server.name=io.github.asdecided/core` and starts the
  native `decided-mcp` binary.
- Registry namespace ownership is proven through GitHub OIDC for the
  `asdecided` organisation. The publisher binary remains version- and
  checksum-pinned and no long-lived Registry credential is introduced.
- Registry publication checks out the immutable release tag, proves the
  workspace version and image tag agree, waits for the public image, verifies
  its ownership label and entrypoint, validates the manifest, then publishes.
- The listing describes a local, read-only server. It does not imply that
  AsDecided hosts, uploads, or remotely indexes a user's corpus.
- v0.26.2 is the first release eligible for this corrected publication path.
  Previously published artifacts are not rebuilt or relabelled.

This decision corrects ADR-124's package-type premise and supersedes only
ADR-039's public Registry identity. ADR-121's legacy `lore` bytes remain
unchanged.

## Consequences

### Positive

- MCP clients and downstream catalogues can discover a package type the live
  official Registry actually supports.
- The image contains the same workspace-built Rust binary as the native
  release; no Python, npm wrapper, hosted corpus, or second engine is added.
- The CLI and MCP images share one GHCR package while retaining distinct,
  explicit entrypoints and tags.
- A Registry retry is anchored to an immutable release tag and cannot silently
  pick up a later `main` version.

### Negative

- The release publishes and verifies a second OCI target in addition to the
  established CLI image.
- Generic Registry clients may still require a client-specific filesystem mount
  configuration before the local server can read a repository.
- The Registry remains in preview, so its schema or package support can change.

### Risks

- A Registry record could point at a missing, private, or incorrectly labelled
  image. Mitigation: publication waits for the exact public tag and verifies the
  ownership label and native entrypoint before mutation.
- A mutable tag could drift after publication. Mitigation: release tags are
  version-specific, built from the release commit, and never overwritten by the
  release workflow; only `mcp-latest` is movable and it is not registered.
- Package support could change again. Mitigation: PR CI asserts the current OCI
  contract and the checksum-pinned publisher performs live validation before
  every publication.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

### Keep Cargo as the Registry package type

Rejected because Cargo is not a supported package type in the live official
Registry. Cargo remains valuable as an independent Rust-native install surface.

### Publish an MCPB archive

Rejected for now. It would add a new packaging format and per-platform manifest
entries when Core already has a supported OCI target with an ownership label.

### Publish an npm or Python wrapper

Rejected. A wrapper would restore the duplicate-runtime maintenance burden the
Rust cutover removed and would add no product capability.

### Host a remote MCP endpoint

Rejected because it would require corpus upload, tenancy, authentication, and
storage decisions outside AsDecided's local-first boundary.

### Keep `io.github.tcballard/lore`

Rejected. It represents a retired owner and product name and is retained only
where ADR-121 requires historical wire compatibility.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The public identity and OCI publication path have stable source anchors."
rules:
  - id: registry-keeps-canonical-name
    kind: require_pattern
    path_glob: "server.json"
    pattern: '"name": "io.github.asdecided/core"'
    message: "The MCP Registry manifest must retain the canonical AsDecided identity."
  - id: registry-keeps-supported-oci-package
    kind: require_pattern
    path_glob: "server.json"
    pattern: '"registryType": "oci"'
    message: "The official Registry record must use its supported OCI package type."
  - id: registry-publish-keeps-oidc
    kind: require_pattern
    path_glob: ".github/workflows/mcp-registry-publish.yml"
    pattern: 'login github-oidc'
    message: "Registry publication must authenticate without a long-lived token."
  - id: release-keeps-versioned-mcp-image
    kind: require_pattern
    path_glob: ".github/workflows/native-publish.yml"
    pattern: 'mcp-\$TAG'
    message: "Native releases must publish the versioned MCP image used by the Registry."
```

## Related Decisions

- adr-039
- adr-111
- adr-121
- adr-126

## Related Requirements

- asdecided-public-mcp-distribution
