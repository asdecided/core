# RAC with Codex

[OpenAI Codex](https://developers.openai.com/codex) (the Codex CLI) consumes RAC
on the same two surfaces it already supports natively — an `AGENTS.md` it reads
for project instructions, and MCP servers it connects to — so there is no
Codex-specific RAC code. A stranger can reproduce this from the file alone.

## Prerequisites

```bash
brew install asdecided/tap/asdecided-core   # the `decided` CLI and the `decided-mcp` server
```

A repository with a RAC corpus under `decisions/` (run `decided quickstart`, or use this
repository's own `decisions/`).

## 1. Generate `AGENTS.md` (the push)

Codex reads `AGENTS.md` for project instructions. RAC generates one from your
recorded decisions:

```bash
decided export decisions/ --agent-rules
```

Writes `AGENTS.md` at the repo root, in a managed block (your own content is
preserved). Codex discovers it from the project root; re-run the export when
decisions change (`decided export decisions/ --agent-rules --check` fails CI on drift).

## 2. Add the `lore` MCP server (the pull)

Either use the CLI:

```bash
codex mcp add lore -- decided-mcp --root .
```

…or add a `[mcp_servers.lore]` table to Codex's `config.toml` (a sample is in
[`config.example.toml`](config.example.toml)):

```toml
[mcp_servers.lore]
command = "decided-mcp"
args = ["--root", "."]
```

- **Global config** (most reliable): `~/.codex/config.toml` — use an absolute
  `--root` path there.
- **Project config:** `.codex/config.toml` in the repo root is honoured for
  *trusted* projects; some Codex surfaces only load the global file, so if the
  server does not appear, fall back to `~/.codex/config.toml`.

This exposes the five read-only `lore` tools (`get_summary`, `search_artifacts`,
`get_artifact`, `get_related`, `find_decisions`). The server re-reads the corpus
on every call and never writes to the repo.

## 3. Enforcement is separate, and Codex-agnostic

RAC supplies context and enforces *after* the edit (ADR-067) — it does not
intercept Codex's loop. Whatever Codex writes is checked by `decided validate` and
`decided relationships --validate` (and the GitHub Action / pre-merge gate) the same
as any other contributor; the trust boundary is human PR review and CI. The
per-edit pre-edit veto is Claude-Code-specific (see
[`examples/claude-code/`](../claude-code/README.md)); with Codex you rely on the
CI gate, which keeps Codex's loop untouched and adds no latency.

## Verify it

Run the bundled grounding demo — same task twice, once unconnected and once with
`lore` connected — and watch the connected run respect a recorded decision the
unconnected run violates: [`examples/guide/`](../guide/demo.md).

## Summary

| Surface | Command | What Codex does with it |
| --- | --- | --- |
| `AGENTS.md` | `decided export decisions/ --agent-rules` | Reads it as project instructions |
| `lore` MCP | `codex mcp add lore -- decided-mcp --root .` | Calls `find_decisions` / `get_related` on demand |
| CI gate | `decided validate` · `decided relationships --validate` | Enforces on every PR |
