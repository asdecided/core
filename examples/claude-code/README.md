# RAC with Claude Code

[Claude Code](https://docs.anthropic.com/en/docs/claude-code) is RAC's most
integrated client — it gets the two surfaces every client gets (a generated
context file + the `lore` MCP server), plus two Claude-Code-specific extras: a
bundled authoring **skill** and the only platform seam that allows a real
**pre-edit veto**. A stranger can reproduce this from the file alone.

## Prerequisites

```bash
brew install asdecided/tap/asdecided-core   # the `decided` CLI and the `decided-mcp` server
```

A repository with a RAC corpus under `decisions/` (run `decided quickstart`, or use this
repository's own `decisions/`).

## 1. `CLAUDE.md` — context every session (the push)

```bash
decided export decisions/ --agent-rules
```

Writes the settled decisions into a managed block in `CLAUDE.md` at the repo
root (your own content outside the block is preserved). Claude Code reads
`CLAUDE.md` automatically. Re-run on change; `decided export decisions/ --agent-rules
--check` fails CI if it drifts.

## 2. The `lore` MCP server — query on demand (the pull)

```bash
claude mcp add lore -- decided-mcp --root .
```

…or commit a project-level `.mcp.json` so the team shares it:

```json
{
  "mcpServers": {
    "asdecided": { "command": "decided-mcp", "args": ["--root", "."] }
  }
}
```

Exposes the five read-only tools `get_summary`, `search_artifacts`,
`get_artifact`, `get_related`, `find_decisions`. The server re-reads the corpus
on every call and never writes to the repo.

## 3. The authoring skill (Claude-Code-specific)

```bash
decided skill install rac-artifacts
```

Installs a project skill that teaches Claude Code to create, validate, and
update RAC artifacts with the `rac` CLI (it only touches the `decisions/` subtree).
`decided skill list` shows the bundled skills (`rac-artifacts`, `rac-import`,
`rac-ingest`, `rac-review`).

## 4. Enforcement — two seams

RAC supplies context and enforces *after* the edit (ADR-067); it does not rewrite
Claude Code's loop. Two optional guards:

- **Git hook (any client).** `decided hook install --style pre-commit` validates
  staged artifacts on commit (`--style post-commit` is an advisory cadence
  nudge that never blocks). This is a *git* hook, not a Claude Code hook.
- **Pre-edit veto (Claude-Code-only).** Claude Code's `PreToolUse` hook is the
  one platform seam that can block an edit *before* it lands. The RAC VS Code /
  Cursor extension generates it ("RAC: Enable Claude Code pre-edit hook"), or you
  can register it by hand in `.claude/settings.json` under `hooks.PreToolUse`:
  it pipes the proposed content to `decided validate - --corpus decisions/` and **blocks
  (exit 2)** only on a structural finding — a reference to a retired or missing
  decision, or a malformed artifact — and **fails open** on any internal error.
  All validation stays in `rac`; the hook computes nothing (ADR-063, ADR-067).

Either way, the CI / PR gate (`decided validate`, `decided relationships --validate`)
remains the backstop, regardless of which agent edited.

## Verify it

Run the bundled grounding demo — same task twice, once unconnected and once with
`lore` connected — and watch the connected run respect a recorded decision the
unconnected run violates: [`examples/guide/`](../guide/demo.md).

## Summary

| Surface | Command | What Claude Code does with it |
| --- | --- | --- |
| `CLAUDE.md` | `decided export decisions/ --agent-rules` | Reads it every session |
| `lore` MCP | `claude mcp add lore -- decided-mcp --root .` | Calls `find_decisions` / `get_related` on demand |
| Skill | `decided skill install rac-artifacts` | Authors artifacts with the `rac` CLI |
| Pre-edit veto | `.claude/settings.json` → `PreToolUse` → `decided validate - --corpus decisions/` | Blocks an edit that contradicts a decision |
| CI gate | `decided validate` · `decided relationships --validate` | Enforces on every PR |
