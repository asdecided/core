---
template: home.html
hide:
  - navigation
  - toc
---

## What it is

Your agent reintroduces an approach you rejected months ago. It rebuilds something you deliberately removed. The decision was written down — in an ADR nobody, human or agent, ever reopened.

AsDecided stores your requirements, decisions, designs, and roadmaps as typed Markdown in your repo, and serves them to Claude Code, Cursor, and Claude Desktop over MCP. The agent cites your decisions instead of violating them.

No AI in the core. No inference. No guessing. Just your team's recorded knowledge, in your Git, handed to the agent that needs it.

AsDecided is built on **RAC — Requirements as Code**. The native CLI and MCP
server are `decided` and `decided-mcp`.

Point your agent at your repo and ask:

> *"Should I add a hard delete to the user model?"*

The agent calls AsDecided, finds your soft-delete decision, cites it by ID, and proposes the compliant change — instead of reintroducing the thing you removed on purpose.

The server exposes four read-only tools: `get_artifact`, `search_artifacts`, `get_related`, `get_summary`. It never writes to your repo.

▶ **Full walkthrough + runnable example: [examples/guide/](https://github.com/itsthelore/asdecided-core/tree/main/examples/guide)**

## Why this works

The code is structured, the tests are automated, the infrastructure is versioned — but the *reasoning* behind what you build is scattered across tickets, chats, and dead docs. Agents can't act on what they can't read, so they re-litigate settled decisions.

AsDecided puts that reasoning back in the repo as typed, connected artifacts, then serves it to the agent through a deterministic interface. You write the decision once, in Markdown; RAC validates it, links it, and makes it retrievable — durable context for both humans and AI, with no proprietary format and no hosted platform.

## How this relates to spec-driven development

Spec-driven development (SDD) tools — GitHub Spec Kit, OpenSpec, Kiro — manage the *change*: proposal, design, tasks, carried through to implementation. They treat requirements as ephemeral inputs that are consumed and archived. RAC manages the *requirements*: a durable, versioned, governed corpus that persists across changes and is served to your agent over MCP. RAC is the layer above SDD tools, not a competitor to them — an SDD tool drives each change, while AsDecided holds the decisions and requirements those changes draw on.

| Dimension | AsDecided / RAC | GitHub Spec Kit | OpenSpec |
| --- | --- | --- | --- |
| Requirement persistence | Requirements, decisions, designs, and roadmaps are long-lived artifacts that persist across changes | Spec, plan, and tasks are created per feature under `specs/<feature>/` | Change folders are archived on completion under `openspec/changes/archive/`; the specs directory is updated |
| Change management | None — RAC does not manage the change cycle; pair it with an SDD tool | Slash-command workflow: specify, clarify, plan, tasks, implement | Slash-command workflow: propose, apply, archive |
| Traceability | Typed `Related` links between artifacts; `decided relationships --validate` checks them in CI | `/speckit.analyze` runs cross-artifact consistency and coverage analysis | `openspec validate` checks changes and specs for structural issues |
| Tool coupling | Read-only MCP server; works with any MCP client | Slash commands or skills installed per agent at init (GitHub Copilot, Claude Code, Cursor, and others) | Slash commands for 20+ AI assistants |
| Install footprint | `brew install itsthelore/tap/rac-full` (native) | `uv tool install specify-cli` from the Git repository (Python 3.11+) | `npm install -g @fission-ai/openspec` (Node.js 20.19+) |

<!--
Comparison sources, verified 2026-06-12:
- GitHub Spec Kit — https://github.com/github/spec-kit (README): per-feature
  artifacts specs/<feature>/spec.md, plan.md, tasks.md; workflow
  /speckit.specify → /speckit.clarify → /speckit.plan → /speckit.tasks →
  /speckit.implement; /speckit.analyze described as "cross-artifact
  consistency & coverage analysis"; agents selected at init, Copilot
  default, Claude Code/Cursor/Gemini CLI and others listed; install via
  `uv tool install specify-cli --from git+https://github.com/github/spec-kit.git`,
  Python 3.11+.
- OpenSpec — https://github.com/Fission-AI/OpenSpec (README): workflow
  /opsx:propose → /opsx:apply → /opsx:archive; archive output
  "Archived to openspec/changes/archive/2025-01-23-add-dark-mode/ Specs
  updated."; "works with 20+ AI assistants via slash commands"; install
  `npm install -g @fission-ai/openspec`, Node.js 20.19.0+.
  https://github.com/Fission-AI/OpenSpec/blob/main/docs/cli.md:
  `openspec validate` checks changes and specs for structural issues.
- Kiro is named in prose as part of the SDD category but excluded from
  the table: its documentation site (https://kiro.dev/docs/specs/)
  returned HTTP 403 to automated fetches at verification time, so its
  cells could not be verified against the primary source.
- AsDecided / RAC cells: this repository (README, docs/, decisions/).
-->

## How this relates to OKF

Google's Open Knowledge Format (OKF) standardises the *carrier* — a Git tree of Markdown with YAML front matter — and is deliberately permissive: consumers must not reject a bundle for missing fields, unknown types, or broken links. OKF says *"if you can `cat` it, you can read it."* RAC says *"and CI guarantees the file is well-formed, the decision is consistent, and nothing points at a superseded artifact."* OKF is read-time interchange; RAC is **write-time enforcement** — deterministic, cross-artifact validation (referential integrity, status-consistency, illegal-edge detection) that fails your build before bad knowledge lands. A RAC repo *is* a conformant OKF bundle (`decided export --okf`), so you get the interchange for free and keep the enforcement OKF leaves out (ADR-048, ADR-049).

| Dimension | AsDecided / RAC | OKF (v0.1 Draft) |
| --- | --- | --- |
| Validation time | Write-time: `decided validate` and `decided relationships --validate` fail CI before the knowledge lands | Read-time: consumers "MUST NOT reject a bundle" for missing optional fields or unknown `type`, and "MUST tolerate broken links" |
| Links | Typed `## Related` structural references, resolved and validated — broken, ambiguous, superseded-target, and unsupported edges are errors | Untyped: the relationship kind "is conveyed by the surrounding prose, not by the link itself" |
| `type` field | Five enumerated types that drive classification and validation | A free string — "Type values are not registered centrally" |
| Cross-artifact checks | Referential integrity, status-consistency, edge-legality — enforced deterministically in CI | None defined; consumption is permissive by design |
| Maturity | Governed corpus; CLI and MCP output is a stability-tested contract | "Version 0.1 — Draft"; a single-vendor (Google Cloud) initiative |
| Interoperability | `decided export --okf` emits a conformant OKF bundle | The shared carrier RAC writes |

<!--
OKF comparison source, verified 2026-06-14:
- OKF — https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md
  (SPEC.md, "Version 0.1 — Draft"): consumers "MUST NOT reject a bundle because
  of: Missing optional frontmatter fields / Unknown `type` values / Unknown
  additional frontmatter keys" and "MUST tolerate broken links"; the relationship
  kind "is conveyed by the surrounding prose, not by the link itself"; "Type
  values are not registered centrally". A Google Cloud Platform initiative
  (single-vendor), openly published on GitHub.
- AsDecided / RAC cells: this repository — `decided validate`, `decided relationships
  --validate`, `decided export --okf`, and ADR-016 / ADR-048 / ADR-049.
-->

## How AsDecided earns trust

AsDecided asks you to trust it with your product knowledge, so it holds itself to the same standard it applies to your repository:

- **The MCP server is read-only by construction.** It cannot create, modify, or delete files in your repo — enforced in code and verified by tests, not by convention.
- **No AI in the core.** Retrieval is deterministic: the same repo state and the same query always return the same result. The reasoning is your agent's job; AsDecided's job is to hand it the facts.
- **It dogfoods itself.** AsDecided's own planning corpus under [`decisions/`](https://github.com/itsthelore/asdecided-core/tree/main/rac) is validated by RAC in CI — if the tool's rules break the tool's own artifacts, the build fails.
- **Output is a contract.** Golden tests pin CLI and MCP output; any change to what the tools return is reviewed as a product change.
- **Telemetry is opt-in twice over.** Local recording needs an explicit `--telemetry` flag and never includes your arguments or repository content. Remote sharing is a separate, explicit consent (`decided telemetry on`, or one honest question at `decided init`): one anonymous daily ping — a random install id, the version, and an active-repo count — never paths, queries, or content. `decided telemetry status` shows exactly what is shared, the network surface is a single readable module, and ADR-041 records the decision.

## Sharing the corpus (the Portal)

Agents read your lore over MCP; people get the Portal — a single self-contained HTML file of the whole corpus that opens from `file://` with zero network requests. Attach it to a release, send it to a stakeholder, open it on a plane.

```bash
decided export decisions/                                  # canonical JSON to stdout
decided export decisions/ --html --out lore-export.html    # the Portal, one file
```

The JSON payload is a stable contract (artifacts with ids, aliases, status, rendered bodies; relationships as edges) for anyone building their own viewer. The Portal ships search, type/status filters, and citation cross-links out of the box.

---

AsDecided is early and evolving quickly. The MCP server ships today; feedback from teams running agents in anger is exactly what shapes what comes next. Contributions, ideas, and experiments welcome — see [CONTRIBUTING.md](https://github.com/itsthelore/asdecided-core/blob/main/CONTRIBUTING.md).

[GitHub repository](https://github.com/itsthelore/asdecided-core) · [MIT license](https://github.com/itsthelore/asdecided-core/blob/main/LICENSE)
