---
schema_version: 1
id: RAC-KTY0D0DFTCJA
type: decision
---
# ADR-039: Lore Server Identity

> Current server and configuration identity is superseded by ADR-132, and the
> public Registry identity is superseded by ADR-124. The `lore` handshake name
> remains frozen only for the legacy MCP compatibility path in ADR-121.

## Status

Accepted

## Category

Product

## Context

ADR-036 named Lore as the product and froze distribution names — the
PyPI package, the `rac` CLI, and the `rac mcp` command — reserving any
rename as a separate decision. It did not name the agent-facing server
identity: the label users type into client configuration, the name the
server reports in the MCP handshake, the namespace agents see on every
tool call, and the identity a registry listing carries.

That surface shipped as `rac-guide`, leaking the internal consumer
codename ("Guide", from ADR-029 through ADR-034) into the product's most
visible seam — the exact ad-hoc naming drift ADR-036 warns against. The
mismatch surfaced concretely when preparing the MCP registry submission:
registering `rac-guide` for a product launched as Lore.

The registry identity is a new surface, not a rename of a shipped one,
and the installed base is days old: the demo is unrecorded, client
configurations unverified, and no registry submission exists. The label
will never be cheaper to settle than now.

## Decision

The agent-facing server identity is `lore`.

- Client configuration label: `claude mcp add lore -- rac mcp`, and
  `"lore"` as the `mcpServers` key in every documented config block.
- Server handshake name: the FastMCP server name is `lore`, so client
  UIs and tool namespaces show `lore`.
- Registry identity: `io.github.tcballard/lore`, with the matching
  `mcp-name` ownership marker in the package README.
- Unchanged, per ADR-036: the `rac mcp` command, the `rac` CLI, the
  `requirements-as-code` package, all tool names, descriptions, and
  response contracts. "Guide" remains the internal component name in
  code, tests, and the corpus.
- `lore-guide` is not used anywhere: it would introduce a third name
  when the problem being solved is having two.

## Consequences

### Positive

- One name on every surface an agent or user reads; the registry
  listing, config blocks, and brand agree.
- The internal codename stays internal; engine and contract naming stay
  stable (ADR-036 holds for everything it froze).

### Negative

- Documentation published with v0.10.3 shows `rac-guide`; anyone who
  configured it keeps a working but differently-labelled server until
  they re-add it.

### Risks

- Label drift recurs on future surfaces. Mitigated the same way as
  ADR-036: this decision is the naming reference for agent-facing
  surfaces.

## Alternatives Considered

### lore-guide

Rejected: carries the internal codename into public view and creates a
third name to explain alongside Lore and RAC.

### Keep rac-guide

Rejected: registers and advertises an internal codename for a product
launched as Lore, days before outreach, with nothing depending on the
old label.

## Related Decisions

- ADR-036
- ADR-029
