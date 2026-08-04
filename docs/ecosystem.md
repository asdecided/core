# AsDecided ecosystem

Things that consume AsDecided artifacts or the native engine. Every entry is
real and verified at the time of listing: it exists at the cited
location and works against a released engine version. There are no
planned or placeholder entries.

| Name | What it is | Where |
| --- | --- | --- |
| AsDecided dogfood corpus | This repository's own product knowledge — requirements, decisions, roadmaps, prompts, designs — validated in CI by the engine it specifies | [`decisions/`](https://github.com/asdecided/core/tree/main/decisions/) |
| `decided-artifacts` Claude Code skill | A bundled project-level agent skill that teaches Claude Code to create, validate, and update AsDecided artifacts using the `decided` CLI | [`rust/rac-engine/assets/skills/decided-artifacts/`](https://github.com/asdecided/core/blob/main/rust/rac-engine/assets/skills/decided-artifacts/SKILL.md) |
| MCP grounding example | A runnable demo showing an agent connected to AsDecided over MCP respecting a recorded decision that an unconnected agent violates | [`examples/guide/`](https://github.com/asdecided/core/blob/main/examples/guide/demo.md) |
| Amp setup | A worked setup connecting Sourcegraph's Amp to AsDecided — it reads the generated `AGENTS.md` natively and queries the `asdecided` MCP server | [`examples/amp/`](https://github.com/asdecided/core/blob/main/examples/amp/README.md) |
| Claude Code setup | A worked setup connecting Claude Code to AsDecided — the generated `CLAUDE.md`, the `asdecided` MCP server, the `decided-artifacts` skill, and the optional pre-edit veto hook | [`examples/claude-code/`](https://github.com/asdecided/core/blob/main/examples/claude-code/README.md) |
| Codex setup | A worked setup connecting OpenAI Codex to AsDecided — it reads the generated `AGENTS.md` and queries the `asdecided` MCP server via `config.toml` | [`examples/codex/`](https://github.com/asdecided/core/blob/main/examples/codex/README.md) |
| Cursor setup | A worked setup connecting Cursor to AsDecided — it reads the generated `AGENTS.md` and queries the `asdecided` MCP server via `.cursor/mcp.json` | [`examples/cursor/`](https://github.com/asdecided/core/blob/main/examples/cursor/README.md) |
| GitHub Copilot setup | A worked setup connecting Copilot in VS Code to AsDecided — the generated `.github/copilot-instructions.md` plus the `asdecided` MCP server in agent mode | [`examples/copilot/`](https://github.com/asdecided/core/blob/main/examples/copilot/README.md) |

## Adding an entry

An entry is one row in the table above. The criteria:

- The thing exists — on disk in this repository or at a stated
  external location — and can be inspected; intentions and
  works-in-progress are not listed.
- It consumes AsDecided artifacts or the native engine against a released
  version.
- The row states what it is and where it lives, in one line.

Entries are added by a pull request changing the single table row.
Contributions follow [`CONTRIBUTING.md`](https://github.com/asdecided/core/blob/main/CONTRIBUTING.md)
and the Developer Certificate of Origin; external additions are welcome via
pull request.

### Harness integration recipes

A harness integration (a worked `examples/<client>/` setup connecting a
coding agent to AsDecided) follows a template and a verification gate — see the
[integration recipes authoring guide](integration-recipes.md). A recipe is
listed here **only after it is smoke-tested against a released engine
version** by running the grounding demo ([`examples/guide/`](https://github.com/asdecided/core/blob/main/examples/guide/demo.md))
with the harness connected. Until then it ships as documentation carrying a
`verify against <client> <version>` marker and stays off this table — the
real-and-verified rule made a precondition of the row, not a courtesy.
