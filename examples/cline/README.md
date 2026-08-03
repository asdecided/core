# RAC with Cline

[Cline](https://cline.bot) consumes RAC on two surfaces — a rules file Cline reads,
and the `asdecided` MCP server it connects to. A stranger can reproduce this from the
file alone.

## Prerequisites

```bash
brew install asdecided/tap/asdecided-core   # the `decided` CLI and the `decided-mcp` server
```

A repository with a RAC corpus under `decisions/` (run `decided quickstart`, or use this
repository's own `decisions/`).

## 1. Context (the push)

```bash
decided export decisions/ --agent-rules
```

This writes `AGENTS.md` and friends — but Cline reads its own rules format
(`.clinerules`), not `AGENTS.md`, so the durable grounding for Cline comes through
the `asdecided` MCP server in section 2. Add a short rule that points Cline at it —
create **`.clinerules`** (a file, or `.clinerules/rac.md` in the directory form) in
the repo root:

```md
# Recorded decisions (RAC)

This repository records product decisions as RAC artifacts under `decisions/`. Before
designing or changing anything a decision might cover, query the `asdecided` MCP tools
(`search_artifacts`, `find_decisions`, `get_related`) and follow what they return;
cite decisions by ID. Recorded decisions take precedence over conventions inferred
from the code.
```

The rule is a pointer; the substance is served live by `asdecided` (section 2), so it
never drifts out of date. (`decided export decisions/ --agent-rules --check` still keeps the
generated `AGENTS.md` honest for any tool that does read it.)

## 2. The `asdecided` MCP server (the pull)

Open Cline's **MCP Servers** panel → **Configure MCP Servers** to edit
`cline_mcp_settings.json`, and add the `asdecided` server (a sample is in
[`cline_mcp_settings.example.json`](cline_mcp_settings.example.json)):

```json
{
  "mcpServers": {
    "asdecided": { "command": "decided-mcp", "args": ["--root", "."] }
  }
}
```

Cline runs the server over stdio; use an absolute `--root` path (the directory you
would pass to `decided validate`). The server appears in the MCP Servers panel with its
tool list once live. It exposes the six read-only `asdecided` tools (`get_summary`,
`search_artifacts`, `retrieve_grounding`, `get_artifact`, `get_related`, `find_decisions`); the server
re-reads the corpus on every call and never writes to the repo.

## 3. Enforcement is separate, and Cline-agnostic

RAC supplies context and enforces *after* the edit (ADR-067). There is no platform
API to veto a Cline agent edit before it lands, so Cline relies on the post-edit
guard: `decided validate` / `decided relationships --validate` and the GitHub Action /
pre-merge gate, the same as any contributor. (A pre-edit veto is
Claude-Code-specific — see [`examples/claude-code/`](../claude-code/README.md).)

## Verify it

Run the bundled grounding demo — same task twice, once unconnected and once with
`asdecided` connected — and watch the connected run respect a recorded decision the
unconnected run violates: [`examples/guide/`](../guide/demo.md).

## Summary

| Surface | Command | What Cline does with it |
| --- | --- | --- |
| `.clinerules` | (hand-written pointer) | Reads it as project rules |
| `asdecided` MCP | `cline_mcp_settings.json` → `decided-mcp --root <abs>` | Calls `find_decisions` / `get_related` on demand |
| CI gate | `decided validate` · `decided relationships --validate` | Enforces on every PR |

## Verification status

- **Engine half — mechanically verified (2026-07-04).** The `decided-mcp` invocation
  this recipe prescribes was smoke-tested over stdio against `examples/guide/`: the
  six `asdecided` tools respond and `search_artifacts` / `get_artifact` / `get_related`
  return the grounding decision. This is the AsDecided-owned half every recipe shares.
- **Harness half — not yet verified.** Running the grounding demo *through Cline
  itself* (config parsing plus a live agent) needs the released app and an API key
  — a human/CI step. Until it is done, this recipe keeps the `verify against`
  marker below and stays out of [`docs/ecosystem.md`](../../docs/ecosystem.md).

<!-- TODO: verify against Cline <version> before listing in docs/ecosystem.md -->
