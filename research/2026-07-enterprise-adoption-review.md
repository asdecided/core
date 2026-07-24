# Lore / RAC — Enterprise Adoption Architecture Review (2,500-Engineer Lens)

> Method: principal-engineer adoption review against the corpus (ADRs, roadmaps,
> requirements), the shipped engine at `21b8143` (v0.23.0 line), hands-on CLI
> runs, and two code-level inventories of `src/rac/` and `rust/`. Recorded
> decisions were treated as authoritative over inferences from code. Prepared as
> portable session context for rac-core roadmap thinking, alongside
> `2026-07-agentic-tooling-demand.md`. Question under review: *what would make
> Lore 10× more effective for adoption inside a single ~2,500-engineer
> enterprise?*

## 1. Premise verdict

The bet — agents cite the team's recorded decisions instead of re-litigating
them, served deterministically and read-only from git — survives contact with
2,500 engineers, but only after a reframe of who it is for.

The demand is real and independently confirmed: "company-wide AGENTS.md
delivered over MCP" is the community's own phrasing of Lore's wedge
(`research/2026-07-agentic-tooling-demand.md`, item 6), and DORA names decision
logs an AI-effectiveness lever (ADR-081). The 95%-never-author objection is
weaker than it looks: knowledge corpora are always produced by few and consumed
by many. An org this size has perhaps 50–150 people whose decisions bind
everyone — platform leads, staff engineers, architects. The value ceiling is not
authorship breadth; it is **reach**: decisions × repos-they-ground. Today reach
is hard-capped at one repo, because the corpus unit is a repo and federation has
zero engine code (ADR-089 accepted-in-principle; `corpus-federation` Planned).

So the enterprise wedge is not "a team's agents respect the team's ADRs." It is
**architecture governance for the agent fleet**: a platform organisation
authoring a few hundred live constraints that 2,500 engineers' agents obey,
with audit as the procurement unlock and onboarding as the visible quick win.
The premise holds; the product's unit of deployment does not yet match it.
Everything below follows from that mismatch.

## 2. Current-state map

Verified against code and corpus; "planned" means a recorded artifact exists
with no implementation in `src/`.

| Dimension | Exists today | Planned / absent |
|---|---|---|
| **Serving** | Stdio MCP (5 read-only tools, ~915-token surface, 10k-char response budget); streamable HTTP `rac mcp --transport http`, stateless, mandatory-audit-on (ADR-098); derived-index cache on by default with persistent mmap store (ADR-099/104/112) — 412-artifact dogfood corpus validates in ~1.8 s | No auth/TLS in engine *by decision* (ADR-085/098 — proxy's job); Rust native engine + stdio MCP server is a parity-proven **spike** (130/130 + 56/56 cases, ~57–87× on hot paths), not mainline |
| **Cross-repo grounding** | Nothing. Single root per corpus; `## inherits` deliberately unrecognised; cross-repo references do not resolve | Whole `corpus-federation` programme (ADR-089, `rac-parent-corpus-inheritance`, `rac-federated-resolution-provenance`, design partner live, epic #267) |
| **Decision↔code proximity** | Shipped: `## Applies To` + `rac decisions-for` + `find_decisions(path=…)` (`decision-to-code-proximity` Achieved) | PR surfacing of governing decisions (`decisions-on-pr` — Planned, unscheduled, all dependencies shipped); drift gate (`freshness-and-drift-detection` phase 1 fenced) |
| **Cold start / supply** | `rac ingest`: DOCX/PDF/PPTX/XLSX/HTML via markitdown (ADR-072) + Obsidian/Logseq/Notion/Roam normalisation (ADR-079) → reviewable drafts; `rac quickstart`; import/capture/ingest agent skills; enterprise profile scaffold (config-only, ADR-088) | Confluence **inbound** ingest explicitly deferred behind outbound publish/verify (`atlassian` roadmap); mega-doc split + setup conventions (`corpus-setup-guidance`, which itself records the ~2,500-person org's inconsistent-setup problem); no scaled curation path |
| **Write path** | `rac new`/`quickstart` scaffolds; `rac-capture` skill; two-gate PR model (ADR-077) | ADR-113 capture sibling surface (draft-only `propose_artifact`): decided days ago, **zero code**; Slack bot / overlay are future items |
| **Governance** | Supersession + status-consistency + edge-legality enforcement (`rac-cross-artifact-enforcement`); git-derived recency (ADR-045); stale-corpus advisory in `review` | Ownership model: none (no owner field, no CODEOWNERS story); drift "suspect" findings, freshness on read surfaces — planned |
| **Deployment & security review** | `docs/security.md` (no-egress test, committed SBOM, 3 runtime deps); telemetry hard-lock `rac telemetry off --enterprise` (ADR-086); audit JSONL with per-request `X-Lore-Principal` (ADR-084/098); `docs/shared-server.md` nginx recipe; OCI image `ghcr.io/itsthelore/asdecided-core` | Audit sink shippers (Loki/S3/Elastic — `rac-ci/audit/` satellite), Helm/compose, Bitbucket/Jenkins/GitLab CI wrappers (ADR-090, `agnostic-surfaces`) — all planned-only |
| **Integrations** | GitHub Actions ×3 (watchkeeper, validate→SARIF, pr-gate); typed `--graph`/`--documents`/`--okf`/`--html` exports; external Jira edges format-linted + marked in graph (ADR-087) | `rac-connect atlassian verify/publish` (rac-connectors#4); export schemas, `--at`, `--since` change feed, source identity (`corpus-sync` — all planned) |
| **Agent-fleet coverage** | MCP (Claude Code/Desktop, Cursor); `rac export --agent-rules` managed blocks for CLAUDE.md/AGENTS.md/Cursor/Copilot; client recipes in `examples/` for codex, cline, copilot, windsurf, zed, amp, opencode; CLI-first path documented (`lean-context-delivery` Achieved) | Nothing verifies non-MCP agents actually consume the blocks; TS SDK still requires the Python CLI (ADR-063) |
| **Proof of value** | `rac eval`: deterministic P@k/R@k + hard-negative gate (ADR-066), CI-gated; obey-demo with measurement protocol | Eval is 12 queries scoring a non-discriminating 1.0 (admitted in `corpus-sync`); no violation/usage reporting over the audit log; paraphrase gap is real — I reproduced it: `rac find "who decided we don't use embeddings"` misses ADR-066 entirely (`paraphrase-recall-response` design owns it) |

Also observed hands-on: `rac decisions-for <path>` with the default corpus
directory crashes on a non-corpus markdown file (unhashable frontmatter key,
`core/frontmatter.py:52`). A robustness paper cut in the flagship proximity
feature — exactly the kind of first-touch failure that costs pilots.

## 3. The 10× bets, ranked

### Bet 1 — Stand up the org-wide grounding plane *now*, on shipped code; land federation underneath it

**The bet.** One org-standards corpus repo + the shipped shared HTTP endpoint
(ADR-098/099/084) + a documented co-mount (`lore` local + `lore-org` HTTP in the
same `.mcp.json`) + `--agent-rules` blocks in repo templates — as the *day-1
deployment recipe*, with the `corpus-federation` programme then making
cross-corpus references resolve, collide loudly, and carry provenance.

**Why 10× at 2,500.** Value is decisions × repos reached. Hundreds of repos ×
reach-of-one is the ceiling on everything else. The federation programme is
correctly designed (materialised bytes, explicit overrides, provenance — ADR-089's
five constraints) but its consumption default is child-declares-parent: at this
scale that is hundreds of PRs to add `## inherits`, plus a permanent pin-bump
tax, before anyone sees value. Inverting the default consumption path to the
serving topology — every agent reads the org endpoint with **zero per-repo
setup** — collapses rollout cost from O(repos) to O(1). Nothing needs to change
in any repo's truth: the endpoint fronts pinned checkouts, determinism holds
(ADR-032/080), and per-repo `main` stays canonical (ADR-018). Federation then
upgrades the same topology from "two answers side by side" to one resolution
space with collision findings and provenance (`rac-federated-resolution-provenance`).

**Builds on.** ADR-098, ADR-099, ADR-084 (all shipped); ADR-088 profile
unhollowing; ADR-089 + `corpus-federation` (epic #267, design partner live).
**Effort.** S for the recipe + profile emitting the org endpoint; XL for
federation proper (already committed). **Primary risk.** Two co-mounted servers
double the tool surface an agent sees; mitigate by serving the org corpus as the
*only* endpoint for repos with no local corpus, and by federation's overlay
eventually collapsing it to one. **Constraint posture.** Respects every
load-bearing decision; it is a sequencing-and-topology recommendation, not a
mechanism change. Grade on the existing plan: right constraints, right partner,
wrong implied rollout order — serve-side reach should not wait for declare-side
mechanism.

### Bet 2 — Ship `decisions-on-pr` next; it is the distribution loop

**The bet.** The advisory PR comment naming the live decisions whose
`## Applies To` scope covers the changed paths — already specced
(`decisions-on-pr`, `pr-decision-surfacing` design), all dependencies shipped
(`decision-to-code-proximity` Achieved, three reusable Actions in-repo).

**Why 10× at 2,500.** Every pull-based surface (MCP, CLI) requires an engineer
to opt in; realistic penetration in a big org is single-digit percent for
quarters. A PR comment reaches **100% of engineers passively**, at exactly the
moment a settled constraint is about to be violated, and every appearance is an
advertisement carrying a decision ID. It converts the corpus from
pull-infrastructure into a visible, daily, org-wide surface — and it is the
on-ramp for the later drift gate (advisory → gate is the recorded pattern,
ADR-075). This was already identified as the top-ranked net-new adoption lever;
it is Planned-unscheduled. The grade here is sequencing: schedule it ahead of
everything except Bet 1's recipe. **Builds on.** `rac decisions-for`, ADR-034/067
(advisory, post-edit), pr-gate-action packaging. **Effort.** S–M. **Primary
risk.** Noise → mute; mitigated by live-status filtering and scope-precision
already specced (Initiative 3). **Constraint posture.** Fully inside ADR-067's
context-supply + post-edit boundary.

### Bet 3 — Make cold start an ingestion problem, not an authorship problem: Confluence inbound + agent-triage curation

**The bet.** Resequence Confluence **inbound** ingest ahead of outbound
publish/verify in the Atlassian roadmap, and pair it with a scaled curation
flow: agents triage the imported mass into candidate live constraints
(drafts, candidate relationships — exactly what `rac ingest` emits today),
owners ratify by PR (ADR-065/077 intact).

**Why 10× at 2,500.** Day 1 the corpus is empty and empty Lore is worthless —
but this org's decisions already exist, in thousands of Confluence pages and
scattered ADR directories. The bottleneck is not conversion (markitdown +
note-tool normalisation shipped, ADR-072/079); it is **curation bandwidth**: no
platform team will hand-read 10,000 drafts. The reframe that makes it tractable:
the target is not 10,000 artifacts, it is the ~200 *live, binding* constraints
(the corpus-setup research from the 2,500-person org shows teams default to
mega-docs precisely because nobody curates). Agent-assisted triage with human
ratification turns ten years of Confluence into a valuable corpus in weeks, and
the two-gate model means AI-drafted knowledge never enters the record without an
independent human merge — the trust story survives unchanged. **Builds on.**
ADR-006, ADR-072, ADR-079, `rac-import`/`rac-ingest` skills, ADR-090's
Atlassian surface, `corpus-setup-guidance` (mega-doc split). **Effort.** M
(connector is a thin export-contract consumer per ADR-073; triage flow is a
skill + recipe, not engine code). **Primary risk.** Garbage in — a corpus of
stale imports is worse than empty; mitigate by importing *only* what an owner
ratifies live, and letting the rest stay in Confluence. **Constraint posture.**
Respects everything; the network code lives in `rac-connectors` (ADR-002 clean).

### Bet 4 — Build the proof surface: a discriminating eval on *your* corpus + ROI reporting over the audit log

**The bet.** (a) Grow `rac eval` from 12 perfect-scoring queries into a
discriminating, paraphrase-family, hard-negative benchmark **generatable against
an adopter's own corpus** — already partially planned (`corpus-sync` "scale and
retrieval evidence", `rac-grounding-eval-benchmark`). (b) Ship a reporting
surface over the audit JSONL (which already records principal, tool, query, and
returned IDs per read): decisions-consulted per team/repo/week, top-cited
decisions, never-cited decisions, grounded-session share.

**Why 10× at 2,500.** A platform team buys tools it can defend in a budget
review. Lore is uniquely positioned to *prove* grounding because retrieval is
deterministic (ADR-066) and reads are attributed (ADR-084) — competitors
structurally cannot show either (ADR-081 names both differentiators, and admits
both are currently unproven). "Agents consulted ADR-014 3,100 times this
quarter; PR comments flagged 240 governed changes; hard-negative violations:
zero" is the sentence that renews a rollout. Without it, Lore's value is
anecdote. **Builds on.** ADR-066, ADR-084, `rac usage` precedent, the obey-demo
measurement protocol. **Effort.** S–M, all local and deterministic. **Primary
risk.** Metrics theatre — citation counts ≠ avoided violations; pair with the
eval's hard-negative gate (never-serve-superseded is the countable core claim).
**Constraint posture.** Clean; the audit log stays local, reporting is a CLI
read over it.

### Bet 5 — Governance primitives before the corpus rots: ownership join + freshness phase 1

**The bet.** Pull `freshness-and-drift-detection` phase 1 forward (staleness on
read surfaces + advisory "suspect" finding), and add the missing org-scale
primitive: an **ownership join** — document and validate the CODEOWNERS
composition so every decision path has an accountable owner, surfaced on reads
next to recency (git-native, no new frontmatter, consistent with ADR-045's
derive-don't-store rule).

**Why 10× at 2,500.** The project's own research says staleness-driven trust
collapse is the #1 abandonment cause and that PR review alone is
proven-insufficient. At 2,500 seats the corpus will accumulate conflicting and
dead decisions across hundreds of authors within a year; "who owns this, is it
still true, what supersedes it" is what separates a system of record from a
graveyard. Enterprise RM tools charge six figures largely for suspect-link
machinery Lore can derive from git for free — it is the recorded competitive
claim (ADR-081), still unbuilt. This bet is mostly a sequencing grade
(phase 1 is well-scoped; schedule it), plus one cheap new piece (ownership).
**Builds on.** ADR-045, ADR-074, `rac-drift-advisory-finding`,
`rac-freshness-read-surfaces`, ADR-075 for the eventual gate. **Effort.** M.
**Primary risk.** Advisory fatigue; the roadmap's advisory-before-gate fence is
the right mitigation — keep it. **Constraint posture.** Clean.

**Deliberately not a bet, but protect it:** the Rust native engine spike. Its
strategic payoff at 2,500 seats is not raw speed — it is a **single static
binary** that deletes the Python-toolchain barrier for every non-Python team and
CI platform (`agnostic-surfaces`' real blocker, ADR-063's admission). Keep it
aimed at binary distribution + serve-path latency; do not let it become an SDK
programme.

## 4. The single biggest adoption risk

**Org-level emptiness: the unit-is-a-repo mismatch and the empty-corpus cold
start compound into a failed pilot before governance, security, or performance
are ever tested.** Ninety days in: the platform team's repo has a good corpus;
the other several hundred repos have nothing (federation unbuilt, Confluence
inbound deferred, per-repo setup cost real); an engineer's agent queries Lore,
misses — sometimes on a paraphrase even when the knowledge exists — and the
verdict "nice for the platform repo, unproven for us" hardens into the rollout's
epitaph. Every strength (determinism, budget discipline, audit) is invisible if
the first hundred queries return nothing.

**Cheapest credible mitigation** (weeks, no engine code): the Bet-1 day-1
topology — one org-standards corpus behind the shipped shared endpoint, wired
into the org's agent baseline (`.mcp.json` template + `--agent-rules` blocks in
repo scaffolds) so *every* engineer's agent grounds against org constraints from
the first session, whether or not their repo has a corpus — plus scheduling
Confluence inbound ingest (Bet 3) so the org corpus is seeded from what already
exists, and miss-payload vocabulary hints (`paraphrase-recall-response`) so a
near-miss teaches instead of silently failing.

## 5. What NOT to build

- **SSO, RBAC, or tool-level authorization in the engine.** The standing red
  line (ADR-085/098) is correct, and not only philosophically: regulated
  enterprises *prefer* auth at their own proxy (their IdP, their mTLS, their
  WAF), and an engine that handles credentials forks the trust story and the
  test matrix. What is missing is distribution, not capability: the Helm/compose
  reference deployment with proxy + audit shipper (ADR-090's `rac-ci` scope).
  Ship the recipe, never the auth. One consequence to document honestly:
  no RBAC means endpoint reach = whole-corpus read; sensitivity partitioning is
  corpus topology (separate repos/endpoints), not per-artifact ACLs.
- **A database, hosted control plane, or multi-tenant service.** ADR-080's
  argument is the differentiator: files-in-git is why the security review is
  short and why there is zero infrastructure to run. Any "sync service" that
  holds corpus state quietly reintroduces the second representation ADR-080
  exists to forbid. The commercial layer, if pursued, sits *on* exports
  (ADR-012, `commercial-layer-positioning`), never under the read path.
- **Embeddings or an LLM judge in the serving/scoring path.** The paraphrase
  gap is real (demonstrated above) and the pressure to "just add vectors" will
  recur — resist it in core. Determinism is the audit story, the eval story,
  and the air-gap story at once (ADR-002/066; Sourcegraph's public reversal is
  the market evidence, ADR-081). The recorded answer is right: miss payloads +
  explain diagnostics in core; semantic recall as a composing sidecar the team
  runs (`lore-supermemory-grounding`), which cites Lore rather than replacing it.
- **A web editing UI / Confluence competitor.** ADR-024's line holds. The wedge
  corpus is a few hundred curated constraints, not a wiki; competing for
  long-tail prose head-on against Confluence loses and destroys the
  small-and-governed property that makes grounding work.
- **More MCP tools.** The five-tool, ~915-token surface is a competitive asset
  (`lean-context-delivery`; the context-tax critique is aimed at 23k-token
  servers). Growth pressure — capture, decomposition, diagnostics — belongs on
  sibling surfaces (ADR-113's pattern) or the CLI, not on Guide's budget.
- **Per-language SDKs and more capture hosts, this cycle.** Thin clients cannot
  remove the Python barrier (ADR-063's own finding) — the OCI image and the Rust
  binary do. And a second capture surface (Slack bot, overlay) grows the write
  path while the read side still reaches one repo; at 2,500 seats, reach beats
  capture. Sequence both behind Bets 1–3.
- **An "enterprise mode."** Already rightly refused (ADR-085), recorded here
  because the pressure will return with every procurement conversation. The
  moment a capability is enterprise-gated, the OSS corpus of trust — the thing
  the security review actually reads — stops covering the thing being sold.

---

*Rough edges worth fixing regardless of strategy: the `decisions-for`
frontmatter crash above, and `rac find`'s argument order (`query` then
`directory`) silently treating a multi-word query as a directory — both are
first-touch failures on the exact commands a pilot demos.*
