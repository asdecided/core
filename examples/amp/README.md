# RAC with Amp

This shows how to connect [Amp](https://ampcode.com) — Sourcegraph's coding agent
— to RAC, so Amp respects the decisions your team has already recorded. A
stranger can reproduce the setup from this file alone.

RAC meets Amp on two surfaces Amp already supports natively, so there is no
Amp-specific RAC code: a generated **`AGENTS.md`** (context Amp reads every
session) and the **`lore` MCP server** (tools Amp queries on demand). They are
complementary — one pushes the decisions into context, the other lets Amp pull
the full text and relationships when it needs them.

## Prerequisites

```bash
brew install asdecided/tap/asdecided-core   # the `decided` CLI and the `decided-mcp` server
```

You also need a repository with a RAC corpus under `decisions/` (run `decided quickstart`
in a fresh project, or point at this repository's own `decisions/`).

## 1. Generate `AGENTS.md` (the push)

Amp reads `AGENTS.md` for project conventions and guidance. RAC generates one
from your recorded decisions:

```bash
decided export decisions/ --agent-rules
```

This writes (or updates) `AGENTS.md` at the repository root, inside a managed
block — anything you keep outside the block is preserved. Amp discovers it
automatically: it reads `AGENTS.md` from the working directory up through the
parent directories, and from subtrees as it opens files there. Re-run the export
whenever decisions change (a pre-commit hook or CI step keeps it current); the
`agent-rules` drift check (`decided export decisions/ --agent-rules --check`) fails if it
falls out of sync.

> Amp falls back to `CLAUDE.md` when no `AGENTS.md` is present — RAC generates
> that target too, so either way Amp sees the decisions.

## 2. Add the `lore` MCP server (the pull)

Amp configures MCP servers under the `amp.mcpServers` key. Add a project-level
`.amp/settings.json` at the repository root (a sample is in
[`settings.example.json`](settings.example.json)):

```json
{
  "amp.mcpServers": {
    "asdecided": {
      "command": "decided-mcp",
      "args": ["--root", "."]
    }
  }
}
```

- **Project config:** `.amp/settings.json` in the repo root (commit it so the
  team shares it). Use `"."` for `--root` so it resolves to the repository.
- **User config:** `~/.config/amp/settings.json` — use an absolute `--root`
  path there.

This exposes RAC's five read-only `lore` tools to Amp: `get_summary`,
`search_artifacts`, `get_artifact`, `get_related`, and `find_decisions`. The
server re-reads the corpus from disk on every call, returns structured JSON
errors rather than exceptions, and never writes to the repository.

## 3. Verify it

Use the bundled grounding demo to see the difference a connection makes. The
[`examples/guide/`](../guide/demo.md) demo runs the same coding task twice —
once with no MCP server and once with `lore` connected — and shows the
unconnected agent violating a recorded decision that the connected one respects.
Point Amp at it the same way (its prompt and corpus are client-agnostic), or run
`decided-mcp --root examples/guide` and ask Amp to implement the task in
`examples/guide/task/`.

## Enforcement is separate, and Amp-agnostic

RAC supplies context and enforces *after* the edit — it does not intercept Amp's
edit loop (ADR-067). Whatever Amp writes is checked by `decided validate` and
`decided relationships --validate` (and the GitHub Action / pre-merge gate) the same
as any other contributor; the trust boundary is human PR review and CI, not the
agent. The per-edit `decided hook` is specific to Claude Code, so with Amp you rely
on the CI gate rather than a live pre-edit block — Amp's loop stays untouched and
adds no latency.

## Summary

| Surface | Command | What Amp does with it |
| --- | --- | --- |
| `AGENTS.md` | `decided export decisions/ --agent-rules` | Reads it every session — decisions are always in context |
| `lore` MCP | `.amp/settings.json` → `decided-mcp --root .` | Calls `find_decisions` / `get_related` to consult the full corpus on demand |
| CI gate | `decided validate` · `decided relationships --validate` | Enforces the corpus on every PR, regardless of which agent edited |
