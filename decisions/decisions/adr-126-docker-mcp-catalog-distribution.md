---
schema_version: 1
id: RAC-KYYC7HBFMRBA
type: decision
---
# ADR-126: Package the Native MCP Server for Docker's MCP Catalog

## Context

ADR-124 publishes the native MCP target through Core's versioned GHCR image and
the official MCP Registry. That record is the canonical public identity, but
Docker's curated MCP Catalog is a separate downstream surface. Local entries in
that catalog are built from source and must start an MCP server when the catalog
probes the selected image.

Core already has one Dockerfile, but its default image starts the `decided` CLI.
Changing that default would silently break existing container users. Publishing
another implementation or a hosted endpoint would also contradict the Rust
cutover and the local-first trust boundary.

## Decision

AsDecided will add a dedicated `asdecided-mcp` build target to the existing
root Dockerfile and submit that target to Docker's official MCP Catalog.

- The target contains the same workspace-built `decided-mcp` binary as the
  native release. It adds no Python, npm, wrapper, or hosted service.
- The existing default Docker target remains the `decided` CLI. Docker's catalog
  metadata selects `asdecided-mcp` explicitly.
- The MCP image carries the canonical
  `io.modelcontextprotocol.server.name=io.github.asdecided/core` label and starts
  `decided-mcp` over stdio.
- A user grants one repository directory as a volume. The catalog passes that
  container path through `--root`; network access is disabled.
- Docker may build, sign, scan, and publish the catalog image in its `mcp`
  namespace. Core's versioned release image and the official MCP Registry remain
  the release and identity authorities; the Docker listing is a downstream
  packaging surface.
- GitHub's MCP Registry requires no second manifest: its published guidance says
  downstream GitHub discovery consumes the official Registry record. A curated
  GitHub feature request is outreach, not another source-controlled package.

## Consequences

Docker Desktop users gain a discoverable, isolated local installation without a
second engine. The Docker-built option also adds Docker's image signing,
provenance, SBOM, and update process.

The root Dockerfile now has two public targets, and Docker's catalog pins a Core
commit that its maintainers must review and periodically update. Filesystem
access remains explicit, but users must understand that the selected repository
is mounted into the container. Docker acceptance is external and cannot be
treated as shipped until its maintainers merge the listing.

## Status

Accepted

<!-- Choose one: Proposed | Accepted | Superseded | Deprecated -->
<!-- Is this Proposed, Accepted, Superseded, or Deprecated? -->

## Category

Architecture

<!-- Choose one: Architecture | Product | Process | Technical | Other -->
<!-- Which area: Architecture, Product, Process, Technical, or Other? -->

## Alternatives Considered

### Replace the CLI container's default entrypoint

Rejected because it would break an existing distribution surface merely to
satisfy a catalog probe.

### Publish a separately maintained image or wrapper repository

Rejected because it creates a second release implementation and unnecessary
repository surface. One multi-target Dockerfile keeps both binaries tied to the
same workspace and commit.

### Host a remote MCP endpoint

Rejected because it would require corpus upload, tenancy, authentication, and
storage decisions that AsDecided has not made.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The Docker MCP target and canonical identity have stable source anchors."
rules:
  - id: docker-keeps-dedicated-mcp-target
    kind: require_pattern
    path_glob: "Dockerfile"
    pattern: 'FROM runtime AS asdecided-mcp'
    message: "Docker catalog packaging must keep a dedicated MCP build target."
  - id: docker-keeps-canonical-mcp-name
    kind: require_pattern
    path_glob: "Dockerfile"
    pattern: 'io.modelcontextprotocol.server.name="io.github.asdecided/core"'
    message: "The MCP image must retain the canonical public Registry identity."
  - id: docker-keeps-native-mcp-entrypoint
    kind: require_pattern
    path_glob: "Dockerfile"
    pattern: 'ENTRYPOINT \["decided-mcp"\]'
    message: "The Docker catalog target must launch the native Rust MCP server."
```

## Related Decisions

- adr-124
- adr-121

## Related Requirements

- asdecided-public-mcp-distribution
