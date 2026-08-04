<!--
RECIPE TEMPLATE — copy this directory to examples/<client>/ and fill it in.

How to author a new harness recipe from this template alone (no need to read an
existing recipe): see docs/integration-recipes.md for the full checklist. Replace
every <PLACEHOLDER>, keep the three numbered sections and the "Verify it" close in
this order, and delete the config dialect(s) your harness does not use. Do NOT
change the wording of section 3 (Enforcement) — it is fixed by ADR-067 so the
boundary reads identically across every recipe. Delete these HTML comments as you
go; a finished recipe carries none of them.
-->
# AsDecided with <Client>

[<Client>](<client-url>) consumes AsDecided on two surfaces — a generated context file
<Client> reads, and the `asdecided` MCP server it connects to. A stranger can reproduce
this from the file alone.

## Prerequisites

```bash
brew install asdecided/tap/asdecided-core   # the `decided` CLI and the `decided-mcp` server
```

A repository with an AsDecided corpus under `decisions/` (run `decided quickstart`, or use this
repository's own `decisions/`).

## 1. Context file (the push)

```bash
decided export decisions/ --agent-rules
```

This writes several agent-context files; <Client> reads **`<CONTEXT-FILE>`**
<!-- e.g. AGENTS.md (the glob-free default), CLAUDE.md, or .github/copilot-instructions.md -->
as plain instructions. The managed block keeps your own content intact; re-run on
change (`decided export decisions/ --agent-rules --check` fails CI on drift).

## 2. The `asdecided` MCP server (the pull)

Add **`<CONFIG-PATH>`** <!-- e.g. .cursor/mcp.json, ~/.codex/config.toml --> with the
`asdecided` server invocation (a sample is in [`<SAMPLE-FILE>`](<SAMPLE-FILE>)):

<!-- Keep only the dialect your harness uses; delete the others. -->
```json
{
  "mcpServers": {
    "asdecided": { "command": "decided-mcp", "args": ["--root", "."] }
  }
}
```

```toml
[mcp_servers.asdecided]
command = "decided-mcp"
args = ["--root", "."]
```

```yaml
mcpServers:
  asdecided:
    command: decided-mcp
    args: [--root, .]
```

- **Project:** commit `<CONFIG-PATH>` to share it with the team.
- **Global:** `<GLOBAL-CONFIG-PATH>` — use an absolute `--root` path.

It exposes the six read-only `asdecided` tools (`get_summary`, `search_artifacts`,
`retrieve_grounding`, `get_artifact`, `get_related`, `find_decisions`); the server re-reads the corpus on
every call and never writes to the repo.

## 3. Enforcement is separate, and <Client>-agnostic

<!-- FIXED WORDING (ADR-067). Only swap <Client> for the harness name; change nothing
else in this section so the boundary reads identically across every recipe. -->
AsDecided supplies context and enforces *after* the edit (ADR-067). There is no platform
API to veto a <Client> agent edit before it lands, so <Client> relies on the
post-edit guard: `decided validate` / `decided relationships --validate` and the GitHub
Action / pre-merge gate, the same as any contributor. (A pre-edit veto is
Claude-Code-specific — see [`examples/claude-code/`](../claude-code/README.md).)

## Verify it

Run the bundled grounding demo — same task twice, once unconnected and once with
`asdecided` connected — and watch the connected run respect a recorded decision the
unconnected run violates: [`examples/guide/`](../guide/demo.md).

## Summary

| Surface | Command | What <Client> does with it |
| --- | --- | --- |
| `<CONTEXT-FILE>` | `decided export decisions/ --agent-rules` | Reads it as project instructions |
| `asdecided` MCP | `<CONFIG-PATH>` → `decided-mcp --root .` | Calls `find_decisions` / `get_related` on demand |
| CI gate | `decided validate` · `decided relationships --validate` | Enforces on every PR |

<!-- Before this recipe is listed in docs/ecosystem.md it MUST be smoke-tested against
a released AsDecided core version (docs/integration-recipes.md, the verification gate).
Until then, keep the marker below and stay off the ecosystem table. -->
<!-- TODO: verify against <Client> <version> before listing in docs/ecosystem.md -->
