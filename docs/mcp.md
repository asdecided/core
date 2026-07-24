# RAC Guide — MCP Server

AsDecided MCP serves your repository's requirements,
decisions, designs, and roadmaps to coding agents as callable tools. It ships
as the native `decided-mcp` binary.

## 1. Install

```bash
brew install itsthelore/tap/rac-full
```

No Python runtime or extra is needed.

## 2. Configure your client

Replace `/path/to/your/repo` with the absolute path to the directory that
contains your RAC artifacts (or the `decisions/` subdirectory within it). Use the
path you would pass to `decided validate`.

> Adding a client that is not listed here? Every harness connects on the same two
> surfaces (the generated agent-instructions file and the `lore` MCP server), so a
> new integration is a documented recipe, not engine work — follow the
> [integration recipes authoring guide](integration-recipes.md).

### Claude Code

**Command form** (adds the server to your Claude Code session):

```bash
claude mcp add lore -- decided-mcp --root /path/to/your/repo
```

**`.mcp.json` form** — create or edit `.mcp.json` in your project root:

```json
{
  "mcpServers": {
    "asdecided": {
      "command": "decided-mcp",
      "args": ["--root", "/path/to/your/repo"]
    }
  }
}
```

<!-- TODO: verify against Claude Code <version> before release -->

### Claude Desktop

Open `claude_desktop_config.json` (macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`; Windows: `%APPDATA%\Claude\claude_desktop_config.json`) and add an entry under `mcpServers`:

```json
{
  "mcpServers": {
    "asdecided": {
      "command": "decided-mcp",
      "args": ["--root", "/path/to/your/repo"]
    }
  }
}
```

Restart Claude Desktop after saving.

<!-- TODO: verify against Claude Desktop <version> before release -->

### Cursor

Create or edit `.cursor/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "asdecided": {
      "command": "decided-mcp",
      "args": ["--root", "/path/to/your/repo"]
    }
  }
}
```

<!-- TODO: verify against Cursor <version> before release -->

### Omnigent

[Omnigent](https://omnigent.ai) is a meta-harness: its custom agents are defined
in a `config.yaml`, and an MCP server is a first-class tool type. Add a `lore`
entry under the agent's `tools:` section:

```yaml
tools:
  lore:
    type: mcp
    command: decided-mcp
    args: ["--root", "/path/to/your/repo"]
```

The tool travels with the agent definition, so it stays attached whichever
harness Omnigent routes to. A worked setup — including pointing the agent's
`instructions` at the generated `AGENTS.md` — is in
[`examples/omnigent/`](https://github.com/itsthelore/asdecided-core/blob/main/examples/omnigent/README.md).

<!-- TODO: verify against Omnigent <version> before release -->

## 3. Point Guide at a repository

`--root` accepts any directory. It does not have to be the top of a Git
repository — point it at the folder where your RAC Markdown artifacts live.

To check that the path is right before configuring your client:

```bash
decided index /path/to/your/repo
```

That should list your artifacts. If it shows nothing, run
`decided init /path/to/your/repo` to initialize the repository.

To try Guide against a ready-made corpus before using your own, point `--root`
at the included examples:

```bash
decided-mcp --root examples/guide
```

The `examples/guide/` corpus contains one requirement, decision, design, and
roadmap for a fictional user management service — enough to explore all four
tools.

## 4. Your first grounded question

Once the server is connected, ask your agent:

> What decisions has this repository recorded about data deletion?

The agent should call `search_artifacts` with a keyword like "delete" or
"soft-delete", retrieve `ADR-001: Soft-Delete User Records` via `get_artifact`,
and cite the decision ID in its response.

If you are pointing at your own repository, substitute a topic you know
a decision covers.

## 5. The four tools

| Tool | When the agent calls it |
|---|---|
| `get_summary` | Once at session start — counts artifacts, flags health issues |
| `search_artifacts` | Before designing or implementing anything that a recorded decision might cover |
| `get_artifact` | When an artifact ID appears, or before changing anything a decision covers |
| `get_related` | After retrieving an artifact — finds what else the change could affect |

`get_related` takes an optional `depth` (default `1`, capped at `5`): the default
returns immediate neighbours only, while `depth>1` additionally returns a
`neighborhood` array of artifacts two or more hops away, each tagged with its
`hops` distance — for transitive context such as a decision a requirement's
roadmap depends on. The walk is bounded (depth, frontier, visited-set, and a work
budget) and deterministic; a truncation marker is set if any cap stops it.

Each `search_artifacts` match carries an additive **`recency`** object —
`last_committed`, `age_days`, and a `stale` flag derived from git history
(ADR-045) — so an agent choosing between results can see which artifact has
decayed without a follow-up `get_artifact`. `stale` is `true` once a file's age
exceeds the freshness threshold (default 180 days, set per repository under
`freshness.stale_after_days` in `.decided/config.yaml`). It is advisory data beside
its date, never a correctness verdict, and never changes which artifacts match or
their order; outside git the fields degrade to `null`. The join respects the
response budget — matches truncate whole, exactly as before.

The tool descriptions contain the trigger language; well-tuned agents call them
without being told to.

## 6. Team setup: route CLAUDE.md to a RAC prompt (Claude Code)

The tool descriptions are sufficient on their own — the grounding demo proves
that — but teams adopting RAC can raise the call rate by giving every session
standing guidance. Rather than pasting instructions into `CLAUDE.md`, record
the guidance as a RAC prompt artifact and route to it, the same pattern this
repository uses for its own agent guidance:

```bash
decided new prompt decisions/prompts/agent-session-start.md
```

Fill the artifact with your team's standing instructions, for example:

- at session start, call `get_summary` to learn what recorded knowledge exists
- before designing or implementing, call `search_artifacts` for the feature
  area — recorded decisions take precedence over conventions inferred from
  the code
- when an artifact ID is mentioned, call `get_artifact`; call `get_related`
  before changing anything an artifact covers
- cite decisions by ID; if a task conflicts with a recorded decision, say so
  instead of silently overriding it

Then make `CLAUDE.md` a router:

```markdown
# Agent session context

Canonical agent guidance lives in `decisions/prompts/` as validated RAC artifacts.

@decisions/prompts/agent-session-start.md
```

Claude Code inlines the referenced artifact at session start, so the effect is
identical to pasting the text — but the guidance is now a governed artifact:
`decided validate` checks it in CI, it is versioned and diffable like any other
decision, and Guide itself can serve it (`get_artifact` retrieves your usage
instructions — the system is self-describing).

Two caveats:

- The import inlines the artifact verbatim, YAML frontmatter included. That
  is harmless, and the agent then knows the artifact's own ID.
- `@import` syntax is Claude Code-specific. For Cursor or Claude Desktop,
  carry the same pointer in their native convention (for example
  `.cursor/rules`); the prompt artifact remains the single source of truth.

## 7. Telemetry (opt-in)

Guide records nothing by default. If you want to see whether it is actually
being used — and help decide where Guide investment goes — opt in with an
explicit flag in your client's server configuration:

```json
{
  "mcpServers": {
    "asdecided": {
      "command": "decided-mcp",
      "args": ["--root", "/path/to/repo", "--telemetry"]
    }
  }
}
```

When enabled, each tool call appends one JSON line to a local log
(`~/.local/state/decisions/guide-telemetry.jsonl`, or under `$XDG_STATE_HOME`):

```json
{"schema_version": "1", "ts": "2026-06-12T14:03:22.512Z", "session": "a3f29c1b",
 "tool": "search_artifacts", "outcome": "ok", "duration_ms": 12, "truncated": false}
```

What is recorded: timestamp, a random per-session id, the tool name, whether
the call succeeded, how long it took, and whether the response was truncated.
What is **never** recorded: tool arguments, artifact IDs, search queries,
file paths, or any repository content. The server announces the log path on
stderr at startup, so enablement is never silent.

Read the log back any time:

```bash
decided-mcp-stats          # human summary: events, sessions, per-tool usage
decided-mcp-stats --json   # the same summary as JSON — this is the export
```

If you want to share your usage with the project (early reports directly
shape Guide's roadmap):

```bash
decided-mcp-stats --share
```

This prints a prefilled GitHub issue URL. Open it, review the report — counts
and timestamps only — and submit it with your own account. RAC never sends
anything anywhere; building a URL is string formatting, and transmission
belongs to you and your browser. Submitted reports are public issues.

### Share anonymously (optional)

If you'd rather contribute a signal without writing anything, opt in to an
anonymous daily ping:

```bash
decided telemetry on       # opt in (decided init also asks once, on a real terminal)
decided telemetry status   # exactly what is shared, and whether sending is possible
decided telemetry off      # stop; nothing else changes
```

With consent on, `decided-mcp` sends at most one ping per 24 hours. This is the
entire transmission — adding a field requires a new recorded decision
(ADR-041):

```json
{
  "api_key": "<public project write key>",
  "event": "lore-daily-ping",
  "timestamp": "<ISO 8601 UTC>",
  "properties": {
    "distinct_id": "<random install id>",
    "$process_person_profile": false,
    "schema_version": "1",
    "rac_version": "<version>",
    "active_repos": 2
  }
}
```

What the fields are: the install id is a random token minted when you opt in
(derived from nothing, so it identifies nothing); `$process_person_profile:
false` tells PostHog to create no person profile, so the event stays
anonymous on the receiving side as well; `active_repos` counts the
distinct repositories Guide served in the last 30 days, tracked locally as
salted digests in `~/.local/state/decisions/active-repos.json` — the salt never
leaves your machine and only the count is sent. The last-ping marker lives at
`~/.local/state/decisions/last-ping`; your consent record at
`~/.config/decisions/telemetry.json`.

Sharing is independent of `--telemetry` — each is its own opt-in. When
sharing is on, the server announces it on stderr at startup; it is never
silent. The sender is one readable module (`decisions/mcp/ping.py`, the only
network code in RAC): failures are dropped without retries, the socket
timeout is three seconds, and a build with no endpoint key configured sends
nothing at all — `decided telemetry status` will say so.

## 8. Read-access audit log (enterprise, opt-in)

For regulated installs that must record *who consulted which decision, when, and
which artifact IDs came back* — the audit trail telemetry deliberately does not
keep — the server can append one JSON line per read-tool call to a local file
([ADR-084](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-084-read-access-audit-recorder.md)).

It is **content-bearing by design and off by default**: with no `audit:` stanza
nothing is written and responses are byte-identical to a server with no recorder.
It is **local-only** — the engine never transmits it; shipping the log to a sink
(Loki, S3, Elastic) is a separate collector's job. The log records the query and
the returned artifact IDs, **never artifact bodies**.

Enable it in `.decided/config.yaml` (committed and team-wide, so an auditor has one
git-diffable artifact to point at):

```yaml
audit:
  enabled: true
  # path: /var/log/lore/audit.jsonl   # optional; default: $XDG_STATE_HOME/decisions/audit.jsonl
  # on_write_error: warn              # warn (default) | block
```

- **`path`** — where the JSONL is written. Default `$XDG_STATE_HOME/decisions/audit.jsonl`;
  override per machine with the `DECIDED_AUDIT_PATH` environment variable (for data
  residency).
- **`on_write_error`** — `warn` (the default) reports a write failure on stderr
  and keeps serving; `block` refuses the call with an `audit-unavailable` error
  rather than returning un-audited content.

Each line records `ts`, a per-process `session`, the `principal`, the `transport`
(`stdio` or `http`), the `attribution` (`asserted` or `local`), the `tool`, the
`query`, the `returned` artifact IDs, `outcome`, and `duration_ms`. The
**principal is attributable, not authenticated** (ADR-084): it defaults to the git
`user.name`/`user.email` in the served repository and can be overridden with the
`DECIDED_AUDIT_PRINCIPAL` environment variable. The enforced access boundary stays the
repository ACL plus human pull-request review — the log records who *claimed* to
query, not a verified identity. When enabled, the server announces it on stderr at
startup; it is never silent.

### Per-caller attribution on a shared server

On a shared HTTP endpoint (`--transport http`) one process serves the whole team,
so a single construction-time principal would record every caller as the host.
Each request instead asserts who it is with the **`X-AsDecided-Principal`** header, and
the audit line records that per-request principal with `transport: http` and
`attribution: asserted`
([ADR-098](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-098-shared-http-mcp-serving.md)):

```
X-AsDecided-Principal: Alice Ng <alice@example.com>
```

This stays **attribution, not authentication**: the engine records what the caller
claimed and never verifies it, and the principal is never an access-control input —
tool responses are identical whatever the header says. If you need the assertion to
be trustworthy, your fronting proxy authenticates the caller and sets the header it
trusts (ADR-085). A request that asserts nothing is recorded with `attribution:
local`, and the shared server's fallback **skips the host's git identity** (it
resolves `DECIDED_AUDIT_PRINCIPAL`, else `unattributed`) so a caller's read is never
mislabelled as the host. Shared HTTP serving is also mandatory audit-on: it refuses
to start without a working sink, and a sink write failure blocks the call.

## 9. Shared HTTP endpoint (team scale)

By default `decided-mcp` speaks **stdio**: one server process per developer, against
that developer's own checkout. At team scale you may instead want **one
always-current endpoint** every agent points at, so reads come from a single
`main`-backed source of truth rather than checkouts that lag between pulls. The
server gains a streamable **HTTP transport** for exactly this
([ADR-098](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-098-shared-http-mcp-serving.md)):

```bash
decided-mcp --root /path/to/your/repo --transport http --host 127.0.0.1 --port 8000 --path /mcp
```

- **`--transport`** — `stdio` (default) or `http`. Bare `decided-mcp` is unchanged,
  so every existing `.mcp.json` keeps working.
- **`--host` / `--port` / `--path`** — where the HTTP server binds and serves
  (defaults `127.0.0.1`, `8000`, `/mcp`). It binds to loopback by default;
  exposing it to a network is a deliberate deployment choice.

The HTTP transport is **serving-layer only**: the five tools are unchanged, the
server re-reads the repository per call (no cache, no session state), and an
HTTP response is **payload-identical to stdio** for the same corpus bytes.

**Authentication is your proxy's job, not the engine's.** The endpoint is
read-only and unauthenticated by design — the engine grows no SSO, RBAC, or
credential handling (ADR-085). Front it with a reverse proxy that authenticates
callers and terminates TLS; identity in the audit log stays *attributable, not
authenticated* (ADR-084).

**HTTP serving is mandatory audit-on.** Because a shared endpoint serves reads
no single developer's git identity can attribute, the HTTP transport **refuses
to start without a working audit log** — enable the `audit:` stanza (see
[section 8](#8-read-access-audit-log-enterprise-opt-in)) first, or the server
exits with an actionable error. stdio is unaffected.

Keeping the fronted checkout current with `main` — a merge webhook or a periodic
`git pull` — is a deployment concern outside the engine. The full recipe —
container, authenticating proxy, keep-current step, and observability — is on the
[Shared Server](shared-server.md) page.

### Derived-index cache

By default the server reuses the derived structures — the repository index, the
relationship graph, and the search token vectors — across calls, kept fresh by
an event-sourced watcher
([ADR-099](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-099-derived-index-cache.md),
default-on per ADR-112):

```bash
decided-mcp --root /path/to/your/repo --transport http
```

The cache is **content-addressed and disposable**. It is keyed on a hash of the
corpus bytes, so any change to any artifact — an edit, add, remove, or rename —
changes the key and forces a rebuild; freshness is confirmed before every call,
so no call ever serves stale state. Output with the cache is **byte-identical**
to the uncached path. The cache lives at `$XDG_CACHE_HOME/decisions/derived`
(override with `DECIDED_CACHE_DIR`); deleting it costs only latency, never
correctness — the files in git remain the single source of truth. Pass
`--no-cache` (or set `DECIDED_NO_CACHE=1`) to restore the zero-state posture where
every tool call re-reads the repository from disk.

## 10. Troubleshooting

### Server not listed in the client

- Confirm `rac` is on the PATH the client uses. Test with:
  ```bash
  which rac
  decided --version
  ```
- If you installed with `uv tool install`, the tool binary may be in
  `~/.local/bin/` — add that to PATH or use the full path in the config.
- Check the client's MCP server log for startup errors.

### Wrong root (Guide answers from the wrong repository)

- Verify the `--root` path in your config matches the directory you intend.
- Run `decided index /path/to/your/repo` to confirm the right artifacts are visible.
- In Claude Code, run `/mcp` to inspect the server configuration.

### Empty corpus (Guide says no artifacts found)

When the server starts against a root with no RAC artifacts it prints a
diagnostic to stderr:

```
decided-mcp: no RAC artifacts found under '/path/to/your/repo'. Point --root at a
directory containing RAC Markdown artifacts, or run 'decided init' to initialize
a new repository. The server is running; get_summary will report the empty state.
```

This is not a fatal error — the server runs and `get_summary` reports zero
artifacts. To fix it:

1. Check that `--root` points at the right directory.
2. Run `decided index /path/to/your/repo` to confirm artifacts are visible.
3. If the directory has no RAC artifacts yet, run `decided init /path/to/your/repo`
   and start creating artifacts with `decided new`.

### get_summary returns all zeros

Same cause as the empty corpus diagnostic above. Either `--root` is wrong or
the repository has not been initialized. See the troubleshooting steps above.

## Further reading

- [CLI reference](cli.md) — every `rac` command including `decided-mcp`
- [Artifact types](artifacts.md) — what requirements, decisions, designs, roadmaps, and prompts look like
- [Repository workflow](repo-workflow.md) — how to organize a RAC repository
- [Examples corpus](https://github.com/itsthelore/asdecided-core/tree/main/examples/guide/) — the ready-made guide corpus
