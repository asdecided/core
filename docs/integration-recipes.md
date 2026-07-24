# Integration recipes — authoring guide

RAC meets a coding agent on two surfaces it does not own: a generated
agent-instructions file the agent reads (the **push**), and the `lore` MCP server
the agent connects to for live retrieval (the **pull**). Because both are standard
surfaces, connecting a new harness is **documentation, not engine work** — a worked
`examples/<client>/` setup plus a row in [`docs/ecosystem.md`](ecosystem.md).

This guide is the authoring contract for those recipes. The shapes recur — the same
`lore` invocation in three config dialects, the same push/pull/enforcement README
structure, the same "verify with the grounding demo" close — so a new recipe is
filling a template, not reverse-engineering an existing one. Every recipe adds
**zero `asdecided-core` engine diff**: it consumes only the two stable surfaces (the
export and the MCP server) as additive contracts (ADR-007, ADR-008, ADR-063), and
stores and serves nothing new (ADR-024).

## The template

Copy [`examples/_recipe-template/`](../examples/_recipe-template/) to
`examples/<client>/` and fill it in. It carries the README skeleton and the `lore`
invocation in all three config dialects; the inline HTML comments tell you what to
replace and what to leave alone. You should be able to produce a complete,
structurally consistent recipe from the template and this checklist **without
opening another recipe** — the same way `decided new` makes an artifact from its
template (ADR-021).

## The recurring shape

A recipe README has these parts, in this order:

1. **Title and framing** — `# RAC with <Client>`, then one line naming the two
   surfaces (the context file the client reads, and the `lore` MCP server).
2. **Prerequisites** — `brew install itsthelore/tap/rac-full` and a corpus under `decisions/`.
3. **Context file (the push)** — `decided export decisions/ --agent-rules`, and which
   generated file this client reads (`AGENTS.md` is the glob-free default;
   `CLAUDE.md` and `.github/copilot-instructions.md` are the other targets).
4. **The `lore` MCP server (the pull)** — the config path for this client and the
   server invocation in the client's dialect (below). Name the five read-only
   tools and note the server re-reads the corpus per call and never writes.
5. **Enforcement is separate** — the fixed ADR-067 paragraph (below). Do not
   reword it.
6. **Verify it** — the grounding demo, [`examples/guide/`](../examples/guide/demo.md).
7. **Summary** — the three-row table (context file, `lore` MCP, CI gate).

### The `lore` invocation, in three dialects

Every recipe runs the same server — `decided-mcp --root .` — expressed in whichever
config dialect the harness uses. Keep only the one your harness reads.

```json
{ "mcpServers": { "asdecided": { "command": "decided-mcp", "args": ["--root", "."] } } }
```

```toml
[mcp_servers.lore]
command = "decided-mcp"
args = ["--root", "."]
```

```yaml
mcpServers:
  lore:
    command: decided-mcp
    args: [--root, .]
```

Use a project-scoped config path (commit it to share with the team); for a global
config, use an absolute `--root` path.

### The enforcement section is fixed (ADR-067)

Every recipe's enforcement section describes **context-supply plus post-edit CI
only** — never a pre-edit interception hook — restated identically so the boundary
never drifts. Copy this paragraph verbatim, swapping only the client name:

> RAC supplies context and enforces *after* the edit (ADR-067). There is no
> platform API to veto a `<Client>` agent edit before it lands, so `<Client>`
> relies on the post-edit guard: `decided validate` / `decided relationships --validate`
> and the GitHub Action / pre-merge gate, the same as any contributor.

A harness that offers a pre-edit hook is tempting to describe as enforcement — do
not. The single exception is Claude Code's pre-edit veto, which is
Claude-Code-specific and documented only in [`examples/claude-code/`](../examples/claude-code/README.md);
every other recipe points readers there rather than describing a hook of its own.

## Authoring checklist

- [ ] Copied `examples/_recipe-template/` to `examples/<client>/` and removed every
      `<PLACEHOLDER>` and HTML comment.
- [ ] Section 1 names `decided export decisions/ --agent-rules` and the exact context file
      this client reads.
- [ ] Section 2 shows the one config dialect and path this client uses, and names
      the five read-only tools.
- [ ] Section 3 is the fixed ADR-067 paragraph, client name swapped, nothing else
      changed — no pre-edit hook described.
- [ ] The "Verify it" close points at [`examples/guide/`](../examples/guide/demo.md).
- [ ] The recipe adds **no** `asdecided-core` source diff (documentation only).
- [ ] The recipe carries the `verify against <client> <version>` marker and is
      **not** yet added to `docs/ecosystem.md` (see the verification gate below).

## The verification gate

A recipe that is *written* is not yet a recipe that is *proven*. The line between
"documented" and "verified against a released engine" is hard and explicit, so a
reader can trust every harness [`docs/ecosystem.md`](ecosystem.md) lists.

- **Documented (unverified).** A freshly authored recipe ships carrying the
  `verify against <client> <version>` marker (the convention already used in
  [`docs/mcp.md`](mcp.md)) and **stays off** the `docs/ecosystem.md` table. It is
  useful documentation; it makes no verified-integration claim.
- **Verified.** Smoke-test the recipe against a **released** `asdecided-core` version by
  running the grounding demo ([`examples/guide/`](../examples/guide/demo.md)) with
  the harness connected over the recipe's config — the same engine behaviour every
  recipe is proven against. Only then remove the marker and add a
  `docs/ecosystem.md` row, dated against the version it was smoke-tested on.

Each `docs/ecosystem.md` row is **real and verified** — a named harness, a verified
recipe, dated against its engine version — with no row added before smoke-test.
This keeps the ecosystem table trustworthy and stops it drifting into a vague
"works with any MCP client" claim. The gate is documentation and process
discipline over the existing surfaces; it requires no engine change.

## Harness backlog

The harnesses worth a recipe, ordered by adoption signal — not completeness
(chasing every MCP client is unbounded). The list is honest about what is
*shipped*, *drafted*, or a *candidate*, and it is **dated**, because the landscape
moves fast: a harness can be archived or superseded between releases, so re-check
before starting one. Priority follows real adoption, not this table's row order.

_Last reviewed: 2026-07-04._

| Harness | Status | Notes |
| --- | --- | --- |
| Claude Code | Verified — listed | [`examples/claude-code/`](../examples/claude-code/README.md); reads `CLAUDE.md`, plus the skill and the pre-edit veto hook |
| Cursor | Verified — listed | [`examples/cursor/`](../examples/cursor/README.md); reads `AGENTS.md` |
| GitHub Copilot | Verified — listed | [`examples/copilot/`](../examples/copilot/README.md); reads `.github/copilot-instructions.md` |
| OpenAI Codex | Verified — listed | [`examples/codex/`](../examples/codex/README.md); reads `AGENTS.md` |
| Amp | Verified — listed | [`examples/amp/`](../examples/amp/README.md); reads `AGENTS.md` |
| Omnigent | Documented | [`examples/omnigent/`](../examples/omnigent/README.md); in `docs/mcp.md`, not yet an ecosystem row |
| Zed | Drafted — unverified | [`examples/zed/`](../examples/zed/README.md); reads `AGENTS.md` natively (clean); engine half smoke-tested |
| Windsurf | Drafted — unverified | [`examples/windsurf/`](../examples/windsurf/README.md); own rules file + MCP pull |
| Cline | Drafted — unverified | [`examples/cline/`](../examples/cline/README.md); own rules file + MCP pull |
| opencode | Drafted — unverified | [`examples/opencode/`](../examples/opencode/README.md); reads `AGENTS.md` natively (clean); engine half smoke-tested |
| Goose | Candidate | Block's MCP-native agent; instruction surface to confirm on authoring |
| Continue | Candidate | Open-source, VS Code + JetBrains; MCP + a rules file |
| Kilo Code | Candidate | VS Code agent; MCP; the migration target for former Roo Code users |
| JetBrains AI | Candidate | JetBrains IDE assistant; MCP + a guidelines file |

**Superseded / not worth a recipe (as of 2026-07-04):**

- **Gemini CLI** — Google is winding down request serving (mid-2026) in favour of
  the closed-source **Antigravity CLI**; hold until the successor's integration
  surfaces settle before drafting one.
- **Roo Code** — archived and read-only (final release May 2026). Do not start a
  recipe; its users are migrating to Cline or Kilo Code, both already listed above.

To pick up a candidate: follow [the template](#the-template), draft
`examples/<client>/`, ship it with its `verify against <client> <version>` marker,
and move it to *Drafted* here. It reaches *Verified — listed* only after the
harness-connected smoke test clears [the verification gate](#the-verification-gate).

## Related

- [`docs/ecosystem.md`](ecosystem.md) — the verified-integration table these
  recipes graduate into.
- [`docs/mcp.md`](mcp.md) — per-client MCP setup and the `verify against` marker
  convention.
- Existing recipes to model against once you have the shape:
  [`examples/cursor/`](../examples/cursor/README.md),
  [`examples/codex/`](../examples/codex/README.md),
  [`examples/copilot/`](../examples/copilot/README.md).
