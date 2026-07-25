# RAC with Omnigent

[Omnigent](https://omnigent.ai) is an open-source meta-harness: a layer that
sits above agent harnesses (Claude Code, Codex, Cursor, Pi, and custom agents)
so you can swap or combine them without rewriting, govern them with policies,
and collaborate on live sessions. Because an Omnigent custom agent is just a
`config.yaml`, it consumes RAC on the same two surfaces every other client
does — an `AGENTS.md` it reads for instructions, and MCP servers it connects to
as tools — so there is no Omnigent-specific RAC code.

This recipe is written against Omnigent's documented custom-agent schema. Smoke-
test it against your Omnigent version before relying on it in production.

## Prerequisites

```bash
brew install asdecided/tap/asdecided-core   # the `decided` CLI and the `decided-mcp` server
```

A repository with a RAC corpus under `decisions/` (run `decided quickstart`, or use this
repository's own `decisions/`). A working Omnigent install with at least one harness
configured.

## 1. Generate `AGENTS.md` (the push)

Omnigent's custom agents take a system prompt inline or from a file. Point the
agent's `instructions` at the `AGENTS.md` RAC generates from your recorded
decisions:

```bash
decided export decisions/ --agent-rules
```

This writes `AGENTS.md` at the repo root, in a managed block (your own content
is preserved). Reference it from the agent's `config.yaml`:

```yaml
instructions: AGENTS.md
```

Re-run the export when decisions change (`decided export decisions/ --agent-rules --check`
fails CI on drift). The same file is read verbatim whichever harness the agent
is bound to, so the grounding survives a one-line harness swap.

## 2. Add the `lore` MCP tool (the pull)

Omnigent declares tools in the agent's `config.yaml`. An MCP server is a
first-class tool type, so add a `lore` entry under `tools:` (a sample is in
[`config.example.yaml`](config.example.yaml)):

```yaml
tools:
  lore:
    type: mcp
    command: decided-mcp
    args: ["--root", "."]
```

This exposes the read-only `lore` tools (`get_summary`, `search_artifacts`,
`get_artifact`, `get_related`, `find_decisions`). Because the tool travels with
the agent definition, it stays attached across every harness the agent runs on —
that is the whole point of the meta-harness layer. The server re-reads the
corpus on every call and never writes to the repo.

Use an absolute `--root` if the agent runs from a working directory other than
the corpus root.

## 3. Enforcement is separate, and harness-agnostic

RAC supplies context and enforces *after* the edit (ADR-067) — it does not
intercept the agent's loop. Whatever the agent writes is checked by
`decided validate` and `decided relationships --validate` (and the GitHub Action /
pre-merge gate) the same as any other contributor; the trust boundary is human
PR review and CI (ADR-065). That holds no matter which harness Omnigent routes
to, and adds no latency to the loop.

Omnigent's distinctive layer is its stateful **policies** — cost budgets,
permissions, and guardrails evaluated server-, agent-, and session-wide. Today
RAC's role stops at supplying grounding through the `lore` tools; a
decisions-aware Omnigent policy (one that consults the corpus to gate an action
at the harness layer) would be *pre-action* interception and so sits outside the
ADR-067 boundary. It is not part of this recipe and would need a recorded
decision before it is built.

## Verify it

Run the bundled grounding demo — same task twice, once unconnected and once with
`lore` connected — and watch the connected run respect a recorded decision the
unconnected run violates: [`examples/guide/`](../guide/demo.md).

## Summary

| Surface | Where it goes | What Omnigent does with it |
| --- | --- | --- |
| `AGENTS.md` | `instructions: AGENTS.md` in `config.yaml` | Reads it as the agent's instructions |
| `lore` MCP | `tools.lore` (`type: mcp`) in `config.yaml` | Calls `find_decisions` / `get_related` on demand |
| CI gate | `decided validate` · `decided relationships --validate` | Enforces on every PR |
