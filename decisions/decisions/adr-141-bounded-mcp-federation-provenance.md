---
schema_version: 1
id: RAC-KZKMJA3YK5Y1
type: decision
---
# ADR-141: Add Bounded Federation Provenance to the Existing MCP Surface

## Context

> **Amended by ADR-146 and ADR-147.** Version-2 ID arguments also accept global
> source qualification. Version-2 response provenance carries the complete
> ordered override chain atomically or returns ADR-128's hard budget error;
> audit records remain the smaller fixed identity and never copy mappings.

MCP clients need to distinguish local and inherited results and identify the
verified parent pin. Adding federation-specific tools would expand the standing
surface and force agents to choose between local and federated retrieval.
Dropping provenance when a response approaches its character budget would make
the most constrained responses the least attributable.

ADR-127 fixes a path-only returned-identity audit object, and ADR-128 fixes hard
response budgets. Federation must amend provenance without reopening either
contract broadly.

## Decision

The six-tool MCP surface remains unchanged when federation ships.

- Existing ID arguments accept the `alias::canonical-id` form from ADR-136.
- Read tools operate on ADR-138's effective composed corpus; MCP exposes no
  local-only federation bypass.
- Artifact-bearing results add fixed source, layer, and verified-pin provenance.
  Those fields are identity metadata and are never removed to make content fit.
- Existing content, excerpt, collection-tail, and error behavior remains under
  ADR-128's measured character budgets. No federation response may exceed the
  configured hard limit.
- No federation-specific MCP tool, source-boost argument, or hidden local-first
  mode is introduced.
- MCP audit records extend ADR-127's bounded returned-identity object only with
  the fixed source, layer, and pin fields needed to identify an inherited
  result. They do not copy artifact bodies, excerpts, override mappings, or the
  full response provenance.
- Tool descriptions and schemas retain the existing measured standing-surface
  budget; provenance is additive within that envelope.

This ADR explicitly amends ADR-127's returned-identity shape for federated
results while leaving its principal, collection coverage, and
content-exclusion rules intact. ADR-128 remains unamended.

## Consequences

Agents use the same tools in local and federated repositories, and every
inherited result remains attributable even when artifact content is truncated.
Audit files can identify the corpus layer and pin without becoming a second
response archive.

Fixed provenance consumes part of the existing response budget, so less body
content may fit in some federated responses. That cost is accepted because
source identity cannot be optional metadata.

## Status

Accepted

## Category

Product

## Alternatives Considered

### Add federation-specific MCP tools

Rejected. It expands the protocol, duplicates existing read behavior, and asks
agents to understand an implementation topology.

### Remove provenance before content

Rejected. An unattributable inherited excerpt is unsafe grounding, especially
when the response is already constrained.

### Copy complete response provenance into the audit log

Rejected. ADR-127 deliberately records bounded returned identities rather than
artifact content or full response payloads.

### Raise the MCP character budget for federated repositories

Rejected. Federation does not get a context-flooding exception to ADR-128.

## Related Decisions

- adr-033
- adr-089
- adr-121
- adr-127
- adr-128
- adr-135
- adr-138
- adr-139
- adr-140
- adr-146
- adr-147

## Related Designs

- corpus-federation-mechanism
- corpus-federation-graph-composition

## Related Requirements

- federated-resolution-provenance
