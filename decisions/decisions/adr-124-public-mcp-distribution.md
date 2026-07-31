---
schema_version: 1
id: RAC-KYVTHFQD44BP
type: decision
---
# ADR-124: Publish the Native MCP Server Through Cargo and the Official Registry

## Context

AsDecided already ships `decided-mcp` in Homebrew and native GitHub Release
archives, but those channels do not make the server discoverable to MCP clients.
The official MCP Registry is a metadata registry: a listing must point at an
independently published package and prove control of both its namespace and that
package. Its live validator supports Cargo packages and verifies ownership by
reading an exact `mcp-name` token from the crate README.

ADR-039 reserved `io.github.tcballard/lore` before the AsDecided rename and
before the Rust cutover. ADR-121 has since made `decided-mcp` the current
protocol identity while retaining `lore` only in frozen legacy responses. A
fresh public distribution identity is therefore required; reviving the retired
name would make discovery disagree with the product, repository, and current
wire protocol.

## Decision

The public MCP Registry identity is `io.github.asdecided/core`.

- The native server is published as the dedicated `decided-mcp` crate on
  crates.io. It is not hidden inside the `decided` CLI package and is not
  wrapped in Python, npm, or an OCI-only launcher.
- The crate README carries the exact ownership marker
  `mcp-name: io.github.asdecided/core`. Registry namespace ownership is proven
  independently through GitHub OIDC for the `asdecided` organisation.
- `server.json` identifies the package as Cargo, pins an exact release version,
  and declares stdio transport. Its title is AsDecided; its executable and
  current MCP handshake identity remain `decided-mcp`.
- Registry publication happens only after the exact `decided-mcp` version is
  visible on crates.io. The publisher binary is version- and checksum-pinned,
  validates the manifest before publishing, and uses no long-lived Registry
  credential.
- The listing describes a local server over a local repository. It does not
  imply that AsDecided hosts, uploads, or remotely indexes a user's corpus.
- The first Registry publication must come from a new immutable release commit
  and tag. Existing v0.26.0 artifacts are not retroactively rebuilt.

This decision supersedes only ADR-039's public Registry identity. ADR-121's
legacy `lore` bytes remain unchanged.

## Consequences

### Positive

- MCP clients and catalogues can discover the canonical Rust server through the
  ecosystem's official metadata surface.
- Cargo is both the installation source and the Registry verification source,
  so there is no second runtime or repackaged implementation to drift.
- GitHub OIDC and a checksum-pinned publisher preserve the release chain without
  introducing a reusable organisation-wide Registry token.

### Negative

- A release now has another ordered publication step and can be partially
  complete if crates.io succeeds but Registry publication fails.
- `server.json`, the workspace, and the package record must carry exactly the
  same version at each release.
- The Registry is in preview, so its schema or availability can change.

### Risks

- A Registry retry could accidentally rebuild an already-published crate.
  Mitigation: crate publication and Registry publication are separate manually
  dispatched workflows; the latter first verifies that the immutable crate is
  already available.
- Namespace or package verification could silently weaken. Mitigation: pin the
  publisher checksum and test the exact namespace, package, transport, version,
  and README token in pull-request CI.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

### Register the GitHub Release archive or existing OCI image

Rejected as the primary Registry package. Cargo is natively supported and maps
directly to the Rust executable, while archive installation is not a Registry
package type and the existing image defaults to the CLI rather than the MCP
server.

### Publish an npm or Python wrapper

Rejected. A wrapper would restore the duplicate-runtime maintenance burden the
Rust cutover removed and would add no product capability.

### Keep `io.github.tcballard/lore`

Rejected. It represents a retired owner and product name and is retained only
where ADR-121 requires historical wire compatibility.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The public identity and publication path have stable source anchors."
rules:
  - id: registry-keeps-canonical-name
    kind: require_pattern
    path_glob: "server.json"
    pattern: '"name": "io.github.asdecided/core"'
    message: "The MCP Registry manifest must retain the canonical AsDecided identity."
  - id: crate-keeps-registry-ownership-token
    kind: require_pattern
    path_glob: "rust/decided-mcp/README.md"
    pattern: 'mcp-name: io.github.asdecided/core'
    message: "The published crate must retain the Registry ownership token."
  - id: registry-publish-keeps-oidc
    kind: require_pattern
    path_glob: ".github/workflows/mcp-registry-publish.yml"
    pattern: 'login github-oidc'
    message: "Registry publication must authenticate without a long-lived token."
```

## Related Decisions

- adr-039
- adr-111
- adr-121

## Related Requirements

- asdecided-public-mcp-distribution
