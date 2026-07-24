# Context cost & lean delivery

A knowledge server justifies itself only if it stays lean — otherwise it becomes
the noise it was meant to cure. Two findings shape how AsDecided delivers knowledge to
an agent:

- **Context rot.** Every frontier model degrades as input grows, well before the
  window fills — so dumping a corpus at an agent *actively worsens* output.
- **The MCP "context tax."** A tool surface spends token real estate on
  descriptions and schemas before it answers anything; oversized surfaces have
  been cited around ~23k tokens.

AsDecided's answer is three deliberate properties: a **measured, budgeted** tool
surface, **selective on-demand** retrieval by default, and a **CLI delivery path**
that spends no standing tokens at all. None of them uses AI summarisation or
compression — payloads stay small because they are *scoped*, not lossily shrunk
(ADR-066).

## 1. The MCP surface is measured and budgeted

The standing cost of the MCP server — the five tool descriptions and their JSON
schemas a client loads every session — is measured deterministically and offline
(no model, no network) and held under a budget as a regression check. Today it
measures ~915 tokens against a 1000 budget — roughly 25× under the ~23k figure the
context-tax critique cited. A description or schema edit that inflates the surface
fails CI rather than quietly taxing every session (see the
[`decided-mcp-surface-budget`](https://github.com/itsthelore/asdecided-core/blob/main/decisions/requirements/decided-mcp-surface-budget.md)
requirement and `decisions/mcp/surface.py`).

## 2. Retrieval is selective and on-demand by default

Both delivery surfaces return the **relevant** artifacts for a query, never the
whole corpus:

- `search_artifacts` / `decided find` return the matches for a query — a small, scoped
  result set, ranked and bounded by the response budget (ADR-033).
- `get_artifact` / `decided resolve` return **one** artifact by id.
- `get_related` / `decided relationships` return an artifact's immediate neighbours,
  not a transitive dump.

This is the antidote to context rot: an agent receives small, relevant payloads by
construction, and pulls more only when it asks. **Bulk, whole-corpus delivery is an
explicit, opt-in action** — `decided export` — never a retrieval default. No default
path hands an agent the entire corpus; it must request it.

## 3. The CLI is a first-class, lowest-tax delivery path

The MCP server is not the only way to ground an agent, and it is not always the
leanest. A CLI spends **no** standing token tax — it costs nothing until invoked —
which is why the context-tax critique steers toward CLI utilities. AsDecided's
CLI-first posture (ADR-005) makes this a supported choice, not an afterthought:
`find`, `resolve`, and `relationships` deliver the same grounding the MCP tools do.

Ground an agent through the CLI by having it shell out and read JSON:

```bash
# What did we already decide about deleting users?
decided find "delete user" decisions/ --json

# Read the specific decision the search surfaced.
decided resolve RAC-01JY4M8X2QZ7 decisions/ --json

# Which recorded decisions govern the file I'm about to edit?
decided decisions-for src/users/repository.py decisions/ --json

# What else would this change affect?
decided relationships decisions/ --json
```

Each returns a small, scoped, JSON payload the agent can act on — the same
selective-by-default retrieval as the MCP path, with zero standing surface cost.

### When to use which

Both surfaces are first-class and supported; the choice is a context-cost
trade-off, not a deprecation.

| Prefer the **CLI path** when… | Prefer the **MCP server** when… |
| --- | --- |
| The agent can shell out (CI jobs, scripted agents, terminal tools) | The agent calls tools autonomously mid-conversation |
| You want zero standing token cost — nothing is paid until a command runs | You want the model to decide *when* to retrieve, from the tool descriptions alone |
| Grounding is a discrete step in a pipeline | Grounding is interactive and continuous |

The MCP server stays fully documented and supported — see
[MCP Server](mcp.md). The CLI commands are in the [CLI Reference](cli.md). Use
whichever spends the context you can afford; neither is deprecated in favour of the
other.
