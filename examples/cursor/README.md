# RAC with Cursor

[Cursor](https://cursor.com) consumes RAC on two surfaces — a generated context
file Cursor reads, and the `asdecided` MCP server it connects to. A stranger can
reproduce this from the file alone.

## Prerequisites

```bash
brew install asdecided/tap/asdecided-core   # the `decided` CLI and the `decided-mcp` server
```

A repository with a RAC corpus under `decisions/` (run `decided quickstart`, or use this
repository's own `decisions/`).

## 1. Context file (the push)

```bash
decided export decisions/ --agent-rules
```

This writes several agent-context files, two of which Cursor reads:

- **`AGENTS.md` (recommended).** Cursor reads `AGENTS.md` at the project root as
  plain instructions — the simplest, glob-free path, and the one to rely on here.
- **`.cursor/rules`.** RAC also writes this legacy single rules file. Modern
  Cursor's rules system is a `.cursor/rules/*.mdc` *directory* with metadata
  (`description`/`globs`/`alwaysApply`), so the single-file form may be ignored by
  current Cursor — prefer `AGENTS.md` above until RAC emits an `.mdc` rule. Either
  way the decisions reach Cursor through `AGENTS.md`.

The managed block keeps your own content intact; re-run on change
(`decided export decisions/ --agent-rules --check` fails CI on drift).

## 2. The `asdecided` MCP server (the pull)

Add `.cursor/mcp.json` in the repo root (project-scoped; a sample is in
[`mcp.example.json`](mcp.example.json)):

```json
{
  "mcpServers": {
    "asdecided": { "command": "decided-mcp", "args": ["--root", "."] }
  }
}
```

- **Project:** `.cursor/mcp.json` (commit it to share with the team).
- **Global:** `~/.cursor/mcp.json` — use an absolute `--root` path.

Enable the server in Cursor's MCP settings if prompted. It exposes the six
read-only `asdecided` tools (`get_summary`, `search_artifacts`, `retrieve_grounding`,
`get_artifact`, `get_related`, `find_decisions`); the server re-reads the corpus on every call
and never writes to the repo.

## 3. Enforcement is separate, and Cursor-agnostic

RAC supplies context and enforces *after* the edit (ADR-067). There is no
platform API to veto a Cursor agent edit before it lands, so Cursor relies on the
post-edit guard: `decided validate` / `decided relationships --validate` and the GitHub
Action / pre-merge gate, the same as any contributor. (The pre-edit veto is
Claude-Code-specific — see [`examples/claude-code/`](../claude-code/README.md).)

## Verify it

Run the bundled grounding demo — same task twice, once unconnected and once with
`asdecided` connected — and watch the connected run respect a recorded decision the
unconnected run violates: [`examples/guide/`](../guide/demo.md).

## Summary

| Surface | Command | What Cursor does with it |
| --- | --- | --- |
| `AGENTS.md` | `decided export decisions/ --agent-rules` | Reads it as project instructions |
| `asdecided` MCP | `.cursor/mcp.json` → `decided-mcp --root .` | Calls `find_decisions` / `get_related` on demand |
| CI gate | `decided validate` · `decided relationships --validate` | Enforces on every PR |
