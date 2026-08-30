---
schema_version: 1
id: RAC-KZ0F0RG3N5XT
type: decision
---
# ADR-149: Git Repository Truth Is Forge-Agnostic

## Status

Accepted

## Category

Architecture

## Context

ADR-080 correctly rejected a database as a second source of truth, but tied the
remaining contract too narrowly to a Git host's branch named `main`. AsDecided
now supports local-only operation, shared servers, and pinned corpus federation.
The same repository may be hosted on GitHub, Cursor Origin, GitLab, Forgejo, a
private bare remote, or nowhere at all.

Forge-specific repository, pull-request, and checks APIs are useful workflow
integrations. They are not corpus storage semantics. If the engine derives
identity, resolution, or authority from GitHub, a remote named `origin`, or a
branch named `main`, moving the exact same reviewed bytes to another standard
Git forge would change the product's answer.

## Decision

The authoritative AsDecided state is the reviewed corpus committed to its
owning Git repository. Git is the versioned system of record; no particular
forge, remote name, default branch spelling, or hosted API is part of corpus
identity or resolution.

- Core read, validation, federation, retrieval, enforcement, export, cache, and
  MCP behavior MUST derive from the selected local tree and explicit AsDecided
  configuration. These paths MUST NOT require a `.git` directory or network.
- Teams choose the branch or protected reference that represents reviewed
  truth. Documentation may use `main` in examples, but engine semantics MUST
  NOT infer authority from that spelling. Commands comparing revisions accept
  an explicit ref; a compatibility default is convenience, not identity.
- A remote is optional. If present, its name and URL are operational data. A
  clone from Cursor Origin, GitHub, GitLab, Forgejo, another standard Git
  server, or a local bare repository yields the same AsDecided answer from the
  same bytes.
- Forge adapters MAY create repositories, push branches, open reviews, publish
  checks, or format host-native output. They MUST remain optional integration
  layers outside the deterministic corpus engine and MUST NOT become a second
  source of truth.
- Shared-server freshness is deployment policy: the operator advances its
  selected reviewed checkout. The server remains a read-only consumer and does
  not make one forge or branch name canonical for every installation.
- Release hosting, issue links, and GitHub Actions in this repository remain
  project delivery choices. Their presence does not make GitHub a runtime or
  storage dependency of AsDecided corpora.

This decision amends ADR-080's hard-coded Git-host `main` language while
preserving its central conclusion: Git-backed Markdown, not a database or
hosted AsDecided control plane, is authoritative.

## Consequences

Organizations can move or mirror a corpus between standard Git forges without
migration of its AsDecided identity. Origin-native repositories need no core
adapter for clone, pull, push, validation, federation, or MCP; only optional
Origin-native review and checks operations need an integration.

Documentation and deployment templates must distinguish examples such as
`origin/main` from normative product semantics. Tests must prove version-2
federation and its stable status output work in a plain materialised directory
without `.git` and without checkout paths entering identity.

The trade-off is that AsDecided does not declare which branch is authoritative
for a team. Repository protections, review policy, and deployment configuration
make that choice, exactly as they do for application code.

## Alternatives Considered

### Make GitHub the canonical storage platform

Rejected. It would couple deterministic local reads to one commercial forge,
exclude Origin-native and self-hosted repositories, and confuse optional review
automation with corpus truth.

### Make Cursor Origin the new canonical storage platform

Rejected for the same reason. Origin is a compatible standard Git forge and a
valuable optional integration, not a replacement control plane for AsDecided.

### Treat a hosted AsDecided service as canonical

Rejected. It recreates the second mutable representation and reconciliation
problem that ADR-080 ruled out.

### Require the branch name `main`

Rejected. Branch spelling is repository policy and carries no semantic content.
An explicit comparison or deployment ref is sufficient.

## Related Decisions

- adr-001
- adr-002
- adr-016
- adr-018
- adr-032
- adr-055
- adr-065
- adr-080
- adr-089
- adr-098
- adr-144
- adr-145

## Related Roadmaps

- corpus-federation
- lore-at-team-scale

## Related Requirements

- parent-corpus-inheritance
- federated-resolution-provenance
