# Changelog

User-visible changes to RAC, by release. Follows the spirit of
[Keep a Changelog](https://keepachangelog.com/): user impact over implementation
details, release history over commit history.

## Unreleased

- Renamed the native engine repository to `itsthelore/asdecided-core`, the
  language-neutral contract repository to `itsthelore/asdecided-spec`, and the
  CI delivery repository to `itsthelore/asdecided-ci`. Active documentation,
  release metadata, CI checkouts, and OCI publication now use the canonical
  AsDecided repository names. Stable `RAC-*` artifact IDs remain unchanged.

## v0.23.1 — 2026-07-24

- Fixed native Windows compilation by using portable filesystem metadata,
  limiting permission changes and symlink inspection to Unix platforms.
- Added a Windows workspace compile check to CI so native Windows support
  remains a release gate.

## v0.23.0 — 2026-07-24

**AsDecided is now the product and runtime surface.** The supported native
executables are `decided` and `decided-mcp`; there is no `rac` executable,
Python CLI fallback, or `RAC_*` environment compatibility layer. New
repositories use `.decided/config.yaml` and `decisions/`. Existing repositories
can preview and apply the explicit migration with `decided migrate layout .
--dry-run` and `decided migrate layout .`; existing `RAC-*` artifact IDs and
published machine-schema keys remain unchanged. Rust is the normal product
engine, Explorer is retired, and ingest leaves core as an ancillary connector.
The Python engine, package, PyPI release path, compatibility oracle, Explorer,
and ingest implementation have been removed from `rac-core`. Python API access
belongs in `itsthelore/asdecided-sdk`; the final former engine remains available only
at the immutable `python-engine-final` Git tag for historical review.

**Decisions on pull requests — the Herald action**
(`pr-decision-surfacing`). A pull request that edits governed code now gets
one advisory comment naming the live decisions whose declared `## Applies
To` scope covers the changed paths — id, title, the matched scope, and a
link — deduplicated, sorted, and updated in place on re-runs. Facts, never
a merge gate (ADR-034/067): the check never fails on findings, and on
forks the comment degrades to the step summary. A thin client over
`rac decisions-for --json` (ADR-063) with no engine change; the action
ships in the CI delivery repo as
`itsthelore/asdecided-ci/herald/github` beside Watchkeeper, Gatekeeper, and
Registrar, and is documented on the new
[Decisions on PRs](https://itsthelore.github.io/asdecided-core/decisions-on-pr/)
page.

**The org grounding plane** (ADR-117). One org-standards corpus behind the
shared HTTP endpoint grounds every repository's agents — including
repositories with no corpus of their own. `rac init --org-endpoint <url>`
wires a repository to it: the `lore-org` streamable-HTTP entry is ensured in
`.mcp.json` and `.cursor/mcp.json`, on fresh **and** already-initialized
repositories, merging into existing files (only the `lore-org` key is
touched, nothing you wrote is removed) and writing nothing on an unchanged
re-run. The new [Org Grounding](https://itsthelore.github.io/asdecided-core/org-grounding/)
page is the operator runbook: the org corpus, the shared-server recipe, fleet
wiring, boundaries, and the federation handoff (ADR-089 stays untouched — no
cross-corpus resolution enters the engine). The `rac init --json` contract
gains an additive `org_endpoint` field. The flag ships in both engines:
`init` stays a covered command, with the native port pinned byte-identical
by new closure parity cases (ADR-116).

**Unhashable frontmatter keys no longer crash the corpus walk.** A YAML
sequence, mapping, or set used as a frontmatter key (`? []`, `[a]: v`,
`? !!set {a}`) — the fuzz campaign's pinned oracle-crash class — now surfaces
as a structured `malformed-frontmatter` finding, so directory walks (`rac new`
id-minting, `rac decisions-for`, root-scoped `rac find`) keep going instead
of dying on one hostile file. Both engines converge on the identical finding:
the native engine's former oracle-crash divergence marker for this class is
retired, pinned by new mainline parity cases.

## v0.23.0 — the "warm by default" release

**The cache is on by default** (ADR-112, supersedes ADR-110). `rac find`,
`rac validate`, and `rac mcp` now reuse the persistent derived-index and
per-file result caches without a flag — repeated queries against an unchanged
corpus are bound by query selectivity, not corpus size, and stay byte-identical
to the uncached walk. Freshness is verified by a stat scan by default (one-shot
`rac find` runs gain a persisted stat manifest, so a warm run reads zero
artifact bytes); the new `--verify` flag on `find`/`validate` forces the full
byte re-hash floor, which catches the one rewrite shape stats cannot see (a
size- and mtime-preserving in-place rewrite). Escapes: `--no-cache` per
invocation, `RAC_NO_CACHE=1` per environment. Environments with no resolvable
home degrade to a temp-dir cache — a missing cache location never fails a
query.

## v0.22.0 — the "scale" release

Two headline movements on top of a large accumulated release. **Search by tags**:
the frontmatter `tags` you already write become a first-class search signal — a
query term matches them, and a `--tag` facet narrows by them. And **single-node
scale**: retrieval stops growing with corpus size — a persistent memory-mapped
index, postings-served search, incremental recompute, and a parallel cold build,
every piece opt-in and byte-identical to the uncached path. Alongside them, the
**decision-to-code-proximity** programme: recorded decisions now know which
code they govern, and you can ask which decisions govern a path — declared and
validated, never inferred; deterministic and offline. Plus the completed
**lore-at-team-scale** programme: `rac mcp` can now serve the whole team over one
always-current HTTP endpoint, with an optional content-addressed cache so
per-call latency stops scaling with corpus size, per-caller audit attribution,
and an operator guide for running it — servers and caches over git, no database.
And the completed **note-tool ingest sources**: `rac ingest` now imports
Obsidian, Logseq, Notion, and Roam exports, carrying each source's link graph in
as candidate relationships.

### Added

- **Tags are part of search.** The frontmatter `tags` you already write were
  validated but invisible to retrieval — now a query term matches an artifact's
  tags (a metadata tier between title and path, tokenised by the same rule as
  every field, so `model` finds a `data-model` tag), and a repeatable `--tag`
  facet on `rac find` plus a `tags` argument on the `search_artifacts` MCP tool
  narrow results to artifacts carrying *every* requested tag (whole-tag,
  case-insensitive, AND semantics). The tier matches tokenised tags; the facet
  matches whole tags — one mechanism for "find things about X," one for "only
  things labelled X." Deterministic and lexical throughout, never an embedding
  (ADR-037/038/109). A tagged search hit now carries its `tags` additively
  (emitted only when non-empty, so an untagged hit is byte-identical to before).
- **Retrieval stops scaling with corpus size.** For large corpora the engine no
  longer re-derives its expensive structures from disk on every read. The derived
  index — the repository index, the resolved relationship graph, and the search
  token vectors — is now a persistent, **memory-mapped segment store**, so the
  working-set memory stays bounded (the index lives on disk, never fully
  resident); search is served from **term-major postings**; the shared server
  tracks freshness **incrementally** instead of re-hashing the whole corpus on
  each call; and the **cold build parallelises** the whole parse-and-derive across
  cores. Every piece is opt-in and disposable — keyed on a corpus content hash,
  byte-identical to the uncached path, and safe to delete (it costs only latency)
  — so enabling it can never change an answer, only its speed
  (ADR-103/104/105/107/108). The measured before/after scale curve lives in the
  `rac-benchmarks` harness, not here.
- **`rac find --cache`.** An opt-in flag that serves a one-shot query from the
  persistent index store instead of a fresh walk, so a benchmark or an agent
  issuing many queries against a stable corpus skips the parse and graph rebuild
  on every warm invocation. Byte-identical to the uncached `rac find` for every
  mode — search, `--decisions`, `--type`, `--tag`, `--explain` — cold and warm;
  off by default (ADR-110).
- **`rac validate --cache`.** Opt-in incremental validation: a per-file result
  cache keyed on each file's content hash × the active config fingerprint, so
  re-validating after a small change is proportional to what changed rather than
  to corpus size. Disposable and byte-identical to the uncached run; a changed
  config or a corrupt cache recomputes from scratch (ADR-106). Off by default.
- **Decisions can declare the code they govern.** A decision may list the paths
  or components it applies to in an optional `## Applies To` section. Literal
  path and directory entries are existence-checked by
  `rac relationships --validate` — a new `applies-to-target-not-found` finding
  flags a declared path that no longer exists; glob patterns and component-name
  labels are recorded without existence-checking.
- **`rac decisions-for <path>`** — a new read-only command listing the live
  decisions whose declared scope governs a file or directory, so you see which
  recorded decisions constrain an edit before you make it. An ungoverned or
  outside-repository path is a valid empty result, never an error.
- **`find_decisions` gains an optional `path` argument (MCP).** The Guide tool
  that answers "what did we decide about X" now also answers "which decisions
  govern this code path," over the same five-tool surface; called with a topic
  it is unchanged.
- **Explorer `/decisions-for <path>`** — the same lookup in the TUI: type a code
  path to list the governing decisions, each openable like any other result.
- **Shared HTTP MCP transport (`rac mcp --transport http`).** Serve one
  always-current endpoint for a whole team instead of a local server per
  developer: `--transport http` (with `--host`, `--port`, `--path`) speaks the
  streamable HTTP transport, stateless per call and payload-identical to stdio.
  stdio stays the default, so every existing `.mcp.json` is unchanged. The
  endpoint is read-only and unauthenticated by design — authentication belongs
  to your deployment proxy — and it is mandatory audit-on: it refuses to start
  without a working read-access audit log (ADR-098).
- **Derived-index cache (`rac mcp --cache`).** For large corpora, an opt-in
  content-addressed cache reuses the expensive derived structures — the
  repository index, the relationship graph, and the search token vectors — so
  repeated reads of an unchanged corpus skip re-indexing and re-tokenising.
  It is disposable and never authoritative: keyed on a corpus content hash so
  any byte change rebuilds it, byte-identical to the uncached path, and deleting
  it costs only latency (ADR-099). Off by default.
- **Per-caller attribution on the shared server.** When serving over HTTP, each
  caller asserts who it is with an `X-Lore-Principal` header, and the read-access
  audit log records that per-request principal instead of the host identity — so
  an auditor can answer "who read what, when" per caller. Records gain additive
  `transport` and `attribution` fields marking asserted-over-HTTP versus locally
  resolved identity. It stays attribution, not authentication (the engine never
  verifies it; your proxy does), the principal never affects tool output, and
  shared HTTP serving blocks a call if the audit sink write fails
  (ADR-098). stdio behaviour is unchanged.
- **Operator guide for the shared server.** A new
  [Shared Server](https://itsthelore.github.io/asdecided-core/shared-server/) doc
  covers when to run one, the container and authenticating-proxy recipe, keeping
  the checkout current with `main`, and where observability lives — the whole
  topology is deployment wrapper around an unchanged, database-free engine.
- **Ingest a note-tool export — Obsidian, Logseq, Notion, or Roam (`rac
  ingest`).** Point `rac ingest` at a note-tool export directory (or a Roam
  `graph.json`) and each note becomes a reviewable RAC-shaped draft, with the
  links you already drew (`[[wikilinks]]`, Logseq/Roam `[[page links]]`, or
  Notion's Markdown links) carried in as **candidate `## Related` references** for
  you to promote — never asserted edges. Deterministic and offline (identical
  export → byte-identical drafts), lossless (frontmatter, block references,
  properties, and inline metadata preserved verbatim), and it never overwrites an
  existing file. Ambiguous and unresolved links are reported for review, never
  guessed; Notion database CSVs are reported and skipped (rows arrive as their
  own pages); a Roam JSON graph is flattened to outliner Markdown per page
  (ADR-079).
- **Freshness on the surfaces where you pick artifacts.** `rac find` results and
  the MCP `search_artifacts` tool now carry a git-derived `recency` object per
  match — `last_committed`, `age_days`, and a `stale` flag — so you can see which
  result has decayed without opening it. `stale` is `true` once an artifact's age
  exceeds a freshness threshold (default 180 days; set `freshness.stale_after_days`
  in `.rac/config.yaml` to change it). In the human `rac find` output a stale match
  is flagged inline with `⚠ stale (Nd)`. It is advisory data beside its date, never
  a correctness verdict, and it never changes which artifacts match or their order;
  outside a git repository the fields degrade to `null`. Derived from git, never a
  stored frontmatter date (ADR-045).
- **`rac doctor` flags suspect-artifact drift.** A new advisory `suspect-artifact`
  finding surfaces the git-native "suspect link": a referring artifact whose
  resolved relationship target was committed **more recently** than the referrer
  itself, so the reference may no longer reflect the target. It names the newer
  target and both commit dates as facts and recommends review — never a verdict,
  never an auto-fix (ADR-034). Derived purely from git history and the validated
  relationship graph (ADR-045, ADR-074); only declared, resolvable references
  participate, so external references (tickets, `verified by`) are excluded
  (ADR-087). It is warning-only (always exits `0`) and silent outside git or where
  history cannot answer. `rac review` surfaces the same finding beside its
  write-cadence advisory.
- **A template and gate for harness integration recipes.** Connecting a coding
  agent to RAC is documentation, not engine work — the same push
  (`rac export --agent-rules`) / pull (`lore` MCP) / post-edit-enforcement shape
  each time. A new [`examples/_recipe-template/`](examples/_recipe-template/) plus
  an [authoring guide](docs/integration-recipes.md) capture that shape (the README
  skeleton, the `lore` invocation in JSON/TOML/YAML, and the `examples/guide/`
  verify close), so a contributor produces a consistent `examples/<client>/` recipe
  by filling the template — with a fixed enforcement section that never drifts into
  pre-edit interception (ADR-067) and zero `rac-core` engine diff. A recipe is
  listed in [`docs/ecosystem.md`](docs/ecosystem.md) only after it is smoke-tested
  against a released engine; until then it ships carrying a
  `verify against <client> <version>` marker and stays off the table. Ships with
  the first four recipes authored from the template —
  [`examples/windsurf/`](examples/windsurf/README.md),
  [`examples/cline/`](examples/cline/README.md),
  [`examples/zed/`](examples/zed/README.md), and
  [`examples/opencode/`](examples/opencode/README.md) — each documented but not yet
  verified, so they carry the marker and are not listed in `docs/ecosystem.md`. The
  guide also carries a dated, adoption-ordered **harness backlog** — honest about
  what is shipped, drafted, or a candidate, and about which candidates are
  superseded (Gemini CLI is being wound down; Roo Code is archived).
- **The MCP surface is measured and budgeted.** A knowledge server justifies
  itself only if it stays lean, so the agent-facing footprint — the five tool
  descriptions and their JSON schemas a client pays for every session — is now a
  regression-checked property. A deterministic, offline token count (no model, no
  network — ADR-066) holds the standing surface under a stated budget: today it
  measures ~915 tokens against a 1000 budget (for context, comparable MCP servers
  have been cited around 23k), so a description or schema edit that inflates the
  surface fails CI rather than quietly taxing every session. Raising the budget
  needs explicit justification and is itself capped; a representative response is
  budgeted too. The served five-tool surface is unchanged — this measures, it does
  not shrink (ADR-033).
- **Lean context delivery documented.** A new
  [Context Cost](docs/context-cost.md) guide explains how Lore keeps the agent-
  facing footprint small: the measured MCP budget, **selective on-demand retrieval
  by default** (search and lookup return the relevant artifacts, never the whole
  corpus — the antidote to context rot; bulk delivery is an explicit `rac export`,
  not a default), and the **CLI as a first-class, lowest-tax delivery path**
  (`find` / `resolve` / `relationships` ground an agent with zero standing token
  cost, with when-to-use-which guidance against the MCP server — both stay
  supported, neither is deprecated). Selectivity is by scoping, never by lossy
  compression (ADR-066); the selective-by-default guarantee is now asserted in the
  test suite.

### Changed

- **Versioning reverted from CalVer back to SemVer (`vX.Y.Z`).** RAC's brief
  CalVer detour (`YYYY.MM.N`) is reverted (ADR-111 supersedes ADR-076): the
  release number communicates release intent again, and the release line
  re-aligns with the `v0.22.x` roadmap series. The three CalVer releases are
  remapped to SemVer — `v0.23.0` (Hardening) → `v0.20.0`, `2026.06.4` →
  `v0.21.0`, `2026.06.5` → `v0.21.1` — this release is `v0.22.0`, and the CalVer
  PyPI releases are yanked so the SemVer line resolves as "latest". Compatibility
  still lives on `schema_version` (ADR-007), not the release number.
- **Search `evidence.tier` integers shifted by one.** Adding the tags tier at
  rank 2 renumbers the lower tiers in the `--explain` / `search_artifacts` tier
  field: path `2 → 3`, heading `3 → 4`, body `4 → 5`. The *ranking order* of
  results is unchanged (it is BM25F-driven, not tier-driven — ADR-078); only the
  numeric tier label moved, and the matched field name (`path`/`heading`/`body`)
  is unchanged (ADR-109). A consumer that reads the field name is unaffected; one
  that pinned the integer should re-read it.
- **The persisted index store format changed (a rebuild, never an answer
  change).** The tags field and the memory-mapped segment layout bump the store's
  format and bundle versions; an index built by an older `rac` fails the version
  gate closed and is transparently rebuilt on next use. The store is a disposable
  derived structure, so this costs a one-time rebuild, never a different result
  (ADR-104/109).

## v0.21.1 — the "rename" release

The release that renames the package to match the project. The PyPI distribution
is now **`rac-core`** (was `requirements-as-code`); the import package `rac` and
the `rac` CLI are unchanged, so existing code and scripts keep working.
`pip install requirements-as-code` continues to work through a transitional
redirect that depends on `rac-core`. This release also lands the first wave of
enterprise-adoption features — all opt-in, all preserving RAC's offline,
deterministic, content-free defaults.

### Changed

- **PyPI package renamed `requirements-as-code` → `rac-core`** (ADR-092). Install
  with `pip install rac-core`. The import name `rac` and the `rac` CLI entry point
  are unchanged — no code or command changes. The old name still installs via a
  one-time redirect package that depends on `rac-core`; pinned
  `requirements-as-code==` installs still resolve to their historical releases.
- **Wayfinder (prompt-complexity routing) was extracted to its own product**
  (`wayfinder-router`, ADR-069/070); its subtree is removed from this repository.

### Added

- **Enterprise telemetry hard-lock** (ADR-086). `rac telemetry off --enterprise`
  forces the anonymous-ping kill state at runtime, refuses `rac telemetry on` while
  locked, and reports `locked (enterprise)` in `rac telemetry status`; reversible
  only with the explicit `rac telemetry off --enterprise --unlock`. The air-gap
  posture is now consolidated and citable.
- **Read-access audit recorder** (ADR-084). An opt-in, local-only, default-absent
  recorder of MCP read-tool calls (who queried what, when, and which artifact IDs
  came back), enabled by an `audit:` stanza in `.rac/config.yaml` (`enabled`,
  `path`, `on_write_error`) with `RAC_AUDIT_PATH` / `RAC_AUDIT_PRINCIPAL`
  overrides. It records IDs and query arguments — never artifact bodies — to local
  JSONL, imports no network code, and is fail-loud. With no `audit:` stanza (the
  default) nothing is written and RAC's content-free guarantee is byte-for-byte
  intact.
- **External ticket references** (ADR-087). A `## Related Tickets` relationship
  section ties artifacts to external tickets. The provider is per-repo config —
  `rac init --ticketing <jira|github|linear|azure-devops|servicenow|none>` writes a
  `ticketing.provider` stanza, and `rac validate` format-lints entries against that
  provider offline. External edges are exempt from in-corpus resolution and surface
  in `rac export --graph` marked external with their provider; existence and state
  checks live in backend connectors, not the engine.
- **Init profiles** (ADR-088). `rac init --profile <default|enterprise>` scaffolds
  a named bundle of repository config (severity overrides, enforcement policy,
  telemetry posture, ticketing) instead of hand-wiring `.rac/config.yaml`; the
  command reports the files it wrote. A profile sets only existing, committed,
  versioned knobs — it adds no runtime mode.
- **`rac-capture` skill.** A guided interview skill that authors RAC artifacts
  through the two-gate capture write model (ADR-077).


## v0.21.0 — the "unlock" release

The release that shipped the user-facing work accumulated across the v0.23–v0.26
scope-fences. It also moved the version string to **CalVer** (`YYYY.MM.N`,
ADR-076) — a scheme later **reverted to SemVer** in v0.22.0 (ADR-111), which is
why this release now carries a `vX.Y.Z` number; compatibility stays on
`schema_version` (ADR-007) throughout. Its user-facing work:

- **A reimagined Explorer (TUI).** Three themes — `lantern` (dark default),
  `parchment` (warm light), and `high-contrast` — switchable in `/settings` and
  persisted, with theme-aware, accessible type tags. A portfolio list view
  (`/list`) gives a dense, sortable table of the whole corpus with live fuzzy
  search (`ctrl+f`), status filtering, and sort cycling. An optional split
  master-detail layout pairs that list with a persistent reading pane so scanning
  and reading happen together.

- **Exportable corpus.** `rac export --documents` emits an ingestion-ready JSONL
  projection (one Markdown-bodied record per artifact) that any RAG/memory backend
  consumes directly, and `rac export --graph` hands a graph backend RAC's real,
  validated relationship topology instead of one inferred from prose.

- **Visible grounding and coverage.** Local, content-free telemetry shows how
  agents actually consult Lore over time; a coverage view lists where the
  knowledge graph is incomplete (requirements no roadmap schedules, decisions
  nothing applies); and `get_related` can return a bounded multi-hop neighbourhood
  within the response budget.

- **Hardening (from the v0.23 line).** Explainable retrieval (search results show
  *why* they matched), `rac doctor` for paste-ready corpus fixes, git-derived
  provenance on `get_artifact`, a documented trust model (`SECURITY.md`), and a
  gated, deterministic grounding benchmark (`rac eval --check`).

- **Release tooling.** A fail-closed publish gate (`python -m rac.release`)
  rejects a malformed or undocumented release before it can reach PyPI. (This
  release's gate checked the CalVer form; v0.22.0 reverted it to check `vX.Y.Z`.)

### Changed

- The TypeScript stack moved out of this repository (v0.22.5). The client SDK
  now lives in `itsthelore/asdecided-sdk-ts`, published to npm as
  `@itsthelore/asdecided-sdk`, and the VS Code extension in `itsthelore/lore-vscode`,
  consuming that published package instead of the former in-repo
  `file:../rac-sdk` path. The in-repo `typescript/` directory and its CI
  (`typescript.yml`, `sdk-release.yml`, `extension-release.yml`) are removed.
  The `rac` engine, CLI, and PyPI package (`requirements-as-code`) are
  unchanged.

### Added

- `rac export --documents` (v0.25.0 WS1). A new additive export mode that writes
  an ingestion-ready JSON Lines projection to stdout — one record per classified
  artifact, carrying the artifact's **Markdown body** (not the viewer's HTML) plus
  `id`/`type`/`status`/`title` and a metadata block (`path`, `aliases`, `tags`,
  `source`). One document per artifact (no chunking), all artifacts included with
  status stamped, deterministic and offline. Feeds external memory/RAG backends
  (the connector itself is companion work, ADR-073); the canonical `id` is the
  verify-in-Lore re-fetch hook. The default viewer JSON payload is unchanged.

- `rac export --graph` (v0.25.0 WS2, ADR-074). A new additive export mode that
  writes a single typed node+edge JSON graph to stdout. Nodes are
  `{id, type, status, title}`; edges carry the real relationship kind
  (`supersedes`, `related_*`) and a `directed` flag from the relationship-type
  registry — so graph/GraphRAG backends (Neo4j, Graphiti, Cognee) receive RAC's
  validated decision graph rather than one inferred from prose. Resolved
  references become canonical-id edges; unresolved ones keep their literal target
  with `resolved: false`. The viewer JSON's flattened `relates-to` edges are
  unchanged.

- Export documentation (v0.25.0 WS3). `docs/cli.md` now documents `rac export`
  and its modes, names the supported memory/RAG/graph backends explicitly, and
  spells out the verify-in-Lore loop by which an agent gets the authoritative
  answer after a fuzzy recall.

- Extension robustness for release (v0.21.6 milestone). The extension now
  **activates only in RAC workspaces** (a `.rac/config.yaml` is present),
  **caches** resolve/export lookups (cleared on save) so hover/completion stay
  responsive, **warns** once on `rac` schema-version skew, logs to a dedicated
  **"RAC" output channel** (no telemetry), and carries Marketplace/OpenVSX
  packaging metadata (`repository`, `bugs`, `keywords`) and an **icon** (the RAC
  lore-explorer mascot). Only the actual publish step remains manual. This
  completes the `v0.21.x-editor` series.

- RAC Explorer in the extension (v0.21.5 milestone). A **RAC: Open Explorer**
  command renders the corpus relationship graph in a webview — the self-contained
  Portal viewer produced by `rac export --html` (the `lore-web` build with the
  corpus injected, offline). Re-run to refresh. The SDK gains `exportHtml(dir,
  path)`. (Graph↔editor click-through is deferred — the standalone viewer doesn't
  message a host.)

- Ambient corpus awareness in the extension (v0.21.4 milestone). A **status-bar
  health score** (`rac review`, click for the Problems panel) and
  **workspace-wide diagnostics** (`rac validate <dir>`) so issues in unopened
  artifacts are visible. The live per-file diagnostics own open files; the
  workspace scan covers the rest, so nothing is double-reported.

- Editor navigation in the extension (v0.21.3 milestone). Hover now shows the
  target's lifecycle status (⚠ for retired) and a snippet; **find-all-references**
  lists every artifact that references the one under the cursor (from the export's
  resolved edges); artifact aliases are **clickable links**; and the corpus is
  navigable via the **Outline** (an artifact's sections) and **workspace symbols**
  (jump to any artifact by title). All from the cached `rac export` (ADR-063).

- Authoring aids in the editor extension (v0.21.2 milestone). Inside a
  relationship section, the extension now offers **artifact-alias completion**
  (human aliases like `adr-007`, from a cached `rac export`); **quick-fixes**
  insert a missing `## Section` to clear a `missing-<section>` finding; and a
  **RAC: New Artifact** command scaffolds an artifact of any type via `rac new`.
  The SDK gains `schema(type)` and `createArtifact(type, path)`.

- Cross-artifact enforcement in the editor extension (v0.21.1 milestone). The
  extension now flags references that don't resolve, and references to **retired**
  (superseded/deprecated) artifacts, at the reference site — drawn distinctly
  (an unresolved reference is an error, a retired one a warning). Findings come
  from `rac relationships --validate`; the engine's `relationship-target-*` codes
  map to editor diagnostics, anchored to the target token inside the source
  artifact's relationship section. This is what makes the extension RAC rather
  than a generic Markdown linter (ADR-049, ADR-051). Relationship diagnostics
  refresh on save/activation (relationship validation reads files from disk).

- TypeScript SDK and editor extension (v0.21.0 milestone — TypeScript-only; the
  Python package is unchanged and versions independently). `@rac/sdk` is a thin
  Node client that shells out to the installed `rac` CLI and returns typed results
  over the stable `--json` contracts (ADR-063): validate a file, a directory, or
  in-memory text (via `rac validate -` on stdin), plus resolve, find,
  relationships, review, stats, and export — all under one `RacError` root, with
  an injectable runner seam for testing. A VS Code / Cursor extension built on it
  validates RAC artifacts in-editor (live as you type, plus on open/save) and
  offers hover and go-to-definition on artifact IDs/aliases. Both packages live
  under `typescript/` at 0.1.0; a TypeScript CI workflow builds and tests them.
  The `v0.21.x` series (`rac/roadmaps/v0.21.x-editor/`) scopes the follow-on
  releases — cross-artifact enforcement, authoring aids, navigation, awareness,
  visualization, and release hardening.

### Changed

- **License: RAC is now under the Apache License 2.0 (previously MIT).** The
  package, CLI, and TypeScript stack carry `Apache-2.0` metadata, and a `NOTICE`
  file ships alongside `LICENSE`. Apache 2.0 adds an express patent grant and an
  explicit trademark non-grant over MIT; it remains a permissive license, so
  existing usage rights are unaffected. Contributions now require a Developer
  Certificate of Origin sign-off (`git commit -s`); there is no CLA. Distribution
  names are unchanged (PyPI `requirements-as-code`, CLI `rac`, server identity
  `lore`). See `rac/decisions/adr-071-apache-2-relicense-and-dco.md`.

## v0.20.0 — Hardening

The release that turns the grounding claim from "trust us" into something proven
and legible. Everything a user feels:

- **Explainable retrieval.** A search result now shows *why* it was retrieved —
  which field matched, which term, which relationship edge — on
  `search_artifacts` / `get_related`, and via `rac find --explain`.
- **`rac doctor`.** One command diagnoses a corpus and emits paste-ready fixes
  for malformed front matter, broken or cyclic relationships, orphans, duplicate
  ids, contradictions, and injection-style content.
- **Provenance on `get_artifact`.** The agent can cite who decided and when:
  git-derived author and dates plus the reconstructed status history, additive
  and backward-compatible.
- **A documented trust model.** `SECURITY.md` records that artifact content is
  untrusted input made authoritative by human PR review — the read-only server
  protects the store, the PR gate protects the agent — and `get_artifact`
  surfaces the reviewed status.
- **Provably works, and stays working.** A gated grounding benchmark
  (`rac eval --check`: deterministic Precision@k / Recall@k with a hard-negative
  check and a committed baseline) and parser/traversal robustness (input caps,
  graceful degradation, a bounded `get_related`) turn the claim into a CI
  regression guard. A reproducible obey-demo (`examples/obey-demo/`) captures a
  real agent declining a forbidden change after consulting Lore.

**What is deferred.** No automated multi-agent CI harness — the obey-demo is a
manual smoke, never a gate (the agent supplies the judgment, Lore the facts). No
schema-migration framework (`schema_version: 1` already exists). No resumability
or crash-safe job machinery. No multi-hop relationship traversal — `get_related`
stays 1-hop, bounded by the response budget.

**Known limits.** The MCP server is **pull-based and read-only**: the agent must
consult it — nothing is pushed — and it never writes to or mutates the
repository. Full CI enforcement of code-vs-decision conflicts is *not* in this
release: the obey-demo demonstrates the behaviour, but a build does not yet fail
when code contradicts a decision. That is a future release.

## v0.19.0 — 2026-06-15

The kitchen-sink release. Everything since v0.7.3 lands at once. Over this
stretch RAC grew from a requirements validator into a product-knowledge
system: the Explorer TUI, canonical frontmatter identity and opaque IDs, the
relationship graph and its integrity checks, repository intelligence
(`rac portfolio`, `rac watchkeeper`), the Portal and OKF interop exports,
per-type standards enforcement, and a GitHub Action that brings all of it to
pull requests. This entry collects the whole arc into one formal release — the
individual `(vX.Y.Z)` markers below trace where each capability landed.

### Added

- Roadmap "Achieved" lifecycle status (v0.19.0): a roadmap whose scope has
  shipped can declare `## Status: Achieved`, a validated *live terminal* state
  (ADR-061) — the intent was delivered, so the roadmap reads as done without
  being treated as retired, and inbound references to it are not flagged as
  pointing at a superseded target. `rac schema roadmap` lists it among the
  allowed statuses; the roadmap enum is now Planned / Achieved / Superseded /
  Abandoned.

- Validate GitHub Action (v0.17.2): a composite action
  (`tcballard/requirements-as-code/validate-action@<ref>`) runs
  `rac validate --sarif` and uploads the result to GitHub Code Scanning, so RAC
  findings annotate a pull request inline. A thin wrapper over the CLI (ADR-058):
  errors fail the check, warnings (including findings downgraded in
  `.rac/config.yaml`) annotate without failing — warnings-first onboarding. Docs
  cover the workflow, the `security-events: write` permission, and the
  custom-types/edges extensibility boundary (deferred, ADR-052/ADR-055).

- Per-type standards enforcement (v0.17.1): `rac validate` now lints requirement
  *quality* against the standards RAC cites — BCP-14 keyword discipline
  (`requirement-normative-keyword`, error: only uppercase MUST/SHALL/SHOULD/MAY are
  normative), ISO 29148 singularity (`requirement-not-singular`, warning), and EARS
  (`requirement-non-ears` / `requirement-ears-clause`, warnings). Roadmaps gain an
  optional, validated `## Horizon` (now/next/later or a quarter) and an
  advancement-linkage warning. All checks are deterministic (no AI in core) and
  overridable per the v0.15.2 model. Severity overrides are now **repository-wide**
  (ADR-053 revised): a rule downgraded in `.rac/config.yaml` is downgraded for
  `rac review`/`watchkeeper`/`portfolio` too, not only `rac validate` — so a
  warnings-first policy is consistent across every surface.

- Relationship-graph integrity (v0.16.0): `rac relationships --validate` now
  validates the corpus as a graph (ADR-055). A `## Related <Type>` reference that
  resolves to the wrong artifact type is reported as
  `relationship-target-type-mismatch` (untyped-document targets are exempt,
  ADR-010), and a cycle in `supersedes` as `relationship-cycle`. Lifecycle status
  is generalized to all five artifact types (ADR-051): each type has an optional,
  validated `## Status` enum and a retired set, so the "nothing live points at a
  retired artifact" rule (`relationship-target-superseded`) now covers
  requirements, designs, roadmaps, and prompts, not just decisions — status stays
  a knowledge lifecycle, never work state (ADR-017). The MCP `get_summary` tool
  (and `rac portfolio`) gain an additive `validation_status` block reporting the
  repository gate; the four read-only MCP tools are unchanged in count. The edge
  vocabulary (`related_*` + `supersedes`) and existing issue codes are unchanged;
  custom relationship types remain deferred (ADR-052).

- Validation severity overrides + SARIF output (v0.15.2): a repository may declare
  an optional `validation` section in its committed `.rac/config.yaml` to downgrade
  or silence findings — per rule code (`error|warning|off`) and per artifact type
  (`error|warning` ceiling), with the per-rule entry winning over the type ceiling.
  This makes warnings-first onboarding possible: a team can point `rac validate` at
  a legacy repo, keep CI green, and tighten the gate over time. Overrides apply to
  `rac validate` only (review/watchkeeper/portfolio are unchanged); an absent
  section is a no-op, so the default gate stays strict. Also adds
  `rac validate <dir> --sarif`, emitting a deterministic, offline SARIF 2.1.0
  document (core validation + OKF conformance findings) for GitHub Code Scanning;
  `--sarif` is mutually exclusive with `--json` and applies to directory validation.

- OKF v0.1 conformance check (v0.15.1): `rac validate <dir>` now enforces OKF
  conformance as a write-time gate, not just on export. It reports, per typed
  artifact, `okf-unmapped-type` (a `type` with no OKF mapping) and
  `okf-reserved-filename-collision` (a typed artifact named `index.md`/`log.md`),
  and fails the run when the corpus could not produce a conformant bundle. Untyped
  documents are excluded (ADR-010); the directory `validate` JSON gains an additive
  `okf` section (no `schema_version` bump). Per ADR-052, `rac-core` stays the
  code-defined OKF-superset envelope — no JSON Schema files, no new dependency.

- OKF frontmatter superset (v0.15.0): RAC artifacts may now declare an optional
  `tags: [...]` list (the OKF-reserved descriptive field ADR-025 anticipated),
  validated for shape and additive — no `schema_version` bump. `rac export --okf`
  carries `tags` from source plus `created`/`updated` derived from git history
  (first and last commit) in each bundle artifact, so the OKF bundle is
  timestamped while the source stays date-free (recency is git-derived, ADR-045).
  RAC deliberately does not add frontmatter `title`/`description` or make `type`
  mandatory (ADR-050). The JSON export contract is unchanged.

- Status-consistency validation (v0.14.1): `rac relationships --validate` now
  reports a relationship from a live artifact to a decision the team has retired
  (`Superseded` or `Deprecated`) as `relationship-target-superseded` — so nothing
  live points at a superseded artifact. The `supersedes` edge by which a replacing
  decision references the one it replaces is exempt, as is a reference from a
  retired decision (historical chains). Decision-only for now, since lifecycle
  status lives on decisions. Existing issue codes are unchanged.

- Edge-legality validation (v0.14.0): `rac relationships --validate` now reports a
  `## Related <Type>` (or `## Supersedes`) section that the artifact's type does
  not support, instead of silently dropping it. Such a section produces no graph
  edge today; the new `relationship-edge-unsupported` issue surfaces it (and fails
  the validate exit code) so authors learn the link did nothing. Opens the
  enforcement series that makes deterministic cross-artifact validation RAC's core
  (ADR-049). Existing relationship issue codes are unchanged.

- OKF bundle export (v0.13.6): `rac export <dir> --okf [--out <dir>]` writes a
  derived Open Knowledge Format (OKF v0.1) bundle — one Markdown file per typed
  artifact with its OKF `type` projected (decision→ADR, requirement→Requirement,
  and so on), plus a generated `index.md` (progressive disclosure) and `log.md`
  (git-derived, date-grouped). Resolved relationships render as `# Citations`
  body links so they survive for permissive OKF consumers, while the typed front
  matter and `## Related` sections stay authoritative. Parallel to `--json` and
  `--html`; existing exports and validation are unchanged. See
  `docs/okf-profile.md`.

- OKF carrier profile recorded (ADR-048 + `rac-okf-carrier-profile`): RAC
  adopts Google's Open Knowledge Format (OKF v0.1 Draft) as an informative
  carrier profile and a derived export target — never a foundation. RAC repos
  are conformant OKF bundles (RAC `type` maps to OKF `type`: decision→ADR,
  requirement→Requirement, and so on), and the derived OKF bundle view
  (`rac export --okf`, above) joins the JSON/Portal export. RAC's normativity is unchanged — `rac validate`
  and `rac relationships --validate` keep rejecting what they reject today. The
  profile is documented in `docs/okf-profile.md`; the dependency is informative
  and pinned to OKF v0.1, with no code or package dependency on OKF tooling.

- Watchkeeper GitHub Action and reusable workflow (v0.12.3): a composite
  `action.yml` at the repository root (`uses:
  tcballard/requirements-as-code@<tag>`) and a callable
  `.github/workflows/watchkeeper.yml` bring product knowledge review to
  pull requests — failed check per the `fail-on` policy, inline
  annotations on the artifacts needing attention, and a step-summary
  report. The action is logic-free (install RAC, resolve the base ref,
  run one `rac watchkeeper --format github`, propagate the exit code) and
  this repository's own PR checks run it from source as the live
  end-to-end test. Pin exact release tags; no moving major tag is
  published (setuptools-scm derives versions from git tags). See
  `docs/watchkeeper.md`.

- Watchkeeper review verdict, GitHub format, and CI policy (v0.12.2):
  `rac watchkeeper` now ends with a deterministic review recommendation —
  validation regressions, broken relationships, and clarity-regression
  findings recommend human review with Core-owned reasons; ambiguity and
  unlinked scope inform but never recommend alone. `--fail-on
  error|warning|none` turns the verdict into CI policy, and `--format
  github` writes a Markdown step-summary report to stdout and
  workflow-command annotations (with repository-relative paths) to stderr
  — no GitHub API involved. JSON gains an additive `review` block.

- Watchkeeper intent analysis (v0.12.1): the `rac watchkeeper` report now
  ends with deterministic intent findings — specificity regressions
  (numbers vanishing from requirements), ambiguous wording arriving,
  mandatory language weakening or disappearing, acceptance criteria or
  success measures being removed, new scope with no relationships, and
  the relationship impact of modified or removed artifacts. Every check
  is token-boundary text matching or parsed-section comparison — no
  semantic scoring — and each finding carries a one-sentence detail plus
  diff-style evidence. JSON gains an additive `findings[]` array.

- Watchkeeper repository comparison (v0.12.0): `rac watchkeeper [directory]
  --base REF [--head REF] [--json]` reviews product knowledge changes
  between two repository states — added/modified/removed artifacts (with
  requirement-level diffs), validation deltas (including newly invalid
  artifacts), relationship deltas (including references broken purely by a
  removal elsewhere), and per-type artifact count deltas. Base and head
  each accept a git revision or a plain directory; revisions are
  materialized read-only via `git archive` (ADR-043) and nothing ever
  mutates the repository. JSON output is a stable contract
  (`schema_version: "1"`) that grows additively across the v0.12.x series
  (intent findings and review recommendations follow).

- Portal export (v0.11.0): `rac export` turns a repository's corpus
  into shareable artifacts. The default mode prints a deterministic
  JSON payload to stdout — artifacts with stable ids, aliases, type,
  status, title, path, and CommonMark-rendered bodies (raw HTML in
  sources arrives escaped), plus relationships as `relates-to` edges
  with unresolved references preserved verbatim — a stable contract for
  anyone building their own viewer. `rac export --html` writes the
  Portal: one self-contained HTML file with search, type/status
  filters, citation cross-links, and a related-artifacts panel, opening
  from `file://` with zero network requests — attach it to a release or
  send it to a stakeholder. The viewer shell is vendored from the
  repository's own `lore-web` source with provenance recorded, and a
  drift-guard test fails the build if the viewer source changes without
  re-vendoring. No timestamps and stable ordering keep two exports of
  the same tree byte-identical; both output modes are pinned by golden
  and round-trip tests. No new dependencies.

- Anonymous usage sharing (v0.10.6, opt-in): `rac telemetry on|off|status`
  and a one-time, TTY-only consent question at `rac init` (default No).
  With consent, `rac mcp` sends at most one anonymous daily ping — a random
  install id, the RAC version, and a 30-day active-repo count; never paths,
  queries, or repository content. The payload is pinned by ADR-041, the
  network surface is a single module enforced by tests, and a build without
  an endpoint key sends nothing at all.

- Bundled agent skills (v0.10.5): the package now carries three
  Claude Code skills as resources — `rac-artifacts` (author and maintain
  artifacts with the CLI), `rac-review` (work `rac review` findings
  worst-first until validation passes), and `rac-ingest` (convert DOCX,
  PDF, HTML, PPTX, XLSX, or Markdown documents into valid, linked
  artifacts). `rac skill install` drops all three into a project's
  `.claude/skills/` discovery path in one command — all-or-nothing, never
  overwriting; `rac skill install` with a skill name adds just that one,
  and `rac skill list` shows what is bundled. Installation works from the
  installed wheel alone: no repository checkout, no network, no AI. Human
  and `--json` output are pinned by golden tests, and each packaged skill
  is kept byte-identical to the repository's own dogfood copy by test.

- Growth programme corpus and comparison (growth-programme): the
  repository's own growth plan now lives in the corpus as seven
  requirements, two designs, and an umbrella roadmap — including the
  spec-driven-development comparison that backs the README's new "How
  this relates to spec-driven development" section (GitHub Spec Kit and
  OpenSpec, every claim cited to the tool's own documentation), a
  `docs/ecosystem.md` seed list, and a consolidated traceability gap
  report written from real authoring friction
  (`.agent-context/GAPS_TRACEABILITY.md`).

- Opt-in Guide telemetry (v0.10.4): `rac mcp --telemetry` records tool-call
  counts and metadata — never arguments or repository content — to a local
  log under `$XDG_STATE_HOME/rac/`; off by default and announced on stderr
  when on. Tool responses are byte-identical with telemetry on and off. A
  new `rac mcp-stats` command summarizes the log (`--json` is the shareable
  export; `--share` prints a prefilled GitHub usage-report issue URL you
  review and submit yourself — RAC contains no network code).

- Explorer knowledge-graph grammar (v0.8.13): the Explorer's **Links** tab
  now renders relationships in the designed terminal grammar — a vertical
  dependency chain from the artifact to what it relates to (each `↓` carrying
  the relationship kind), an **Impact Analysis** block that frames a change
  ("Changing: … / May affect: …"), and a `↓`-joined lineage chain for
  supersession. The relationships are unchanged — only their presentation —
  so "why does this exist?", "what depends on this?", and "what happens if
  this changes?" read directly. Presentation only; no Core, adapter, or
  state change.

- Explorer mascot interaction (v0.8.12): selecting the mascot in the
  Explorer — a click, or keyboard focus then Enter — returns a small
  response beneath the figure: a default acknowledgement, occasional
  reminders of why product knowledge is worth keeping, gentle guidance
  toward existing commands, and one rare line on repeated selection.
  Responses appear inline with no popup, dialog, or notification, and
  nothing is hidden behind them — the mascot surfaces functionality, it
  does not contain it. A new `mascot_interaction` preference (default on,
  cycled in `/settings`) turns it off independently of the mascot and
  animation toggles, and selection works with animations off. No Core or
  service changes.

- Review impact and the first-run editor (v0.8.11): every `rac review`
  finding now carries an `impact` sentence — why it matters — owned by Core
  and present in the JSON contract (additive field; `schema_version`
  unchanged), so the CLI, automation, and the Explorer all read identical
  text. Explorer onboarding gains one optional editor step after the
  welcome: Enter accepts (an empty value keeps the `$VISUAL`/`$EDITOR`
  fallback), typing persists the `editor` preference, Esc skips — and
  returning users never see it.

- Explorer creation, stats, and the directory view (v0.8.10): the sidebar
  now mirrors the repository's actual directory structure by default —
  directories as collapsible nodes (name, trailing `/`, artifact count),
  nested exactly as on disk, with expansion and cursor surviving reloads at
  any depth and `/open` revealing a nested artifact along its filesystem
  path; the `artifact_grouping` setting cycles `folders` | `type` | `flat`.
  `/new <type> <path>` creates an artifact from its canonical template:
  preview first, `y` confirms, the ID is minted by the same Core service as
  `rac new`, and nothing ever overwrites — on success the Explorer reloads
  and opens the new file, ready for `e`. `/stats` opens a portfolio
  dashboard (per-type validity, requirement and quality totals, decision
  status and category breakdowns, relationship counts), collected off the
  UI thread. `/browse <type>` now lists that type in the filterable results
  view in every grouping mode; bare `/browse` focuses the sidebar.

- Explorer live workspace and validation depth (v0.8.9): the Explorer now
  watches the repository and reloads itself when artifacts change on disk —
  a cheap path/mtime comparison every two seconds, with the sidebar keeping
  its expansion, the open artifact keeping its tab and scroll position, and
  the health chip updating; the watcher holds while a terminal editor owns
  the screen and rescans the moment the Explorer resumes, so a saved edit
  shows immediately. Invalid artifacts now explain themselves where they
  are shown: a health attention item opens the artifact on its Inspection
  tab — whose badge counts the validation diagnostics — and a
  recommendation opens the artifact's Findings tab, which also gains an
  Improvement group from the improve service (one suggestion per missing
  section, with the schema's guidance question as the action; rendered,
  never applied). The command surface deepens: `/schema` lists the
  registered artifact types and `/schema <type>` renders the expected
  structure; the palette offers the last artifacts you opened in this
  repository before you type a character (Enter reopens one); and artifact
  results can be narrowed by type with `f` — all → each type present → all.

- Explorer command palette and settings (v0.8.8): pressing `/` summons a
  command palette — an input with a live, navigable menu below it that lists
  every command when empty, filters and completes them as you type, and
  quick-opens matching artifacts for any other text; `Esc` dismisses it and
  `?` opens help. `/settings` (alias `preferences`) changes everything in
  place — theme with live preview, mascot, animations, artifact grouping,
  and a new default-editor command; terminal editors (vim, nvim, emacs,
  nano, …) now run with the Explorer suspended and resume it on exit.
  Reading is first-class: the Content tab takes the keyboard (`j`/`k`
  scrolls, capped reading width), artifact references inside the rendered
  document open in place so the corpus reads like a wiki, and the Links and
  Findings tabs carry count badges. The mascot animates through per-state
  frame sequences (searching plays while loading; static with animations
  off), the sidebar leads with artifact titles and marks invalid artifacts
  `✗`, keeps its expansion across reloads, and opens the highlighted
  artifact in your editor with `e`; resume restores the last view as well as
  the last artifact, and `Esc` always has somewhere to go (home, at worst).

- Explorer visual overhaul (v0.8.7): one persistent workspace frame replaces
  the screen-per-view shell — a navigation sidebar of type-tagged artifacts
  (`REQ` `ADR` `RMP` `PRM` `DSG`, grouped with counts or flat by preference),
  a context panel whose views swap in place with `Esc` unwinding history, an
  always-visible `/` command bar, and a status line of key chips with the
  health score. Opening an artifact now shows the document itself: a tabbed
  context view with the rendered Markdown first (read-only), then Inspection,
  Links (the relationship traversal moved in here), and Findings. Ships the
  rac-lantern theme — lantern amber on near-black, from the Explorer mascot's
  palette — as the default; the `theme` preference selects any Textual theme,
  and every state keeps its text label under any palette. Key meanings are
  unchanged (`/`, `Enter`, `Esc`, `h`, `r`, `.`, `g`, `e`, `x`, `y`, `q`),
  and the sidebar hides below 80 columns so narrow terminals keep reading
  room.

- Explorer maturity (v0.8.6): workspace continuity — Explorer remembers recently
  opened repositories and the last artifact per repository, and `.` / `/resume`
  reopens it; optional file-based preferences (`theme`, `mascot`, `animations`,
  `artifact_grouping`) under XDG config with `/preferences` to view them; and a
  lantern-carrying mascot in the welcome and empty states. Disabling the mascot
  or animations loses no information (every state carries text), and nothing
  requires login, cloud, or sync.
- Explorer relationship navigation (v0.8.5): `g` from a context view (or
  `/relationships <ref>`) opens a knowledge-graph view — the artifact's
  outgoing relationships, its impact ("what depends on this?"), and its lineage
  (Supersedes / Superseded By). Connected artifacts are selectable, so the graph
  can be traversed one hop at a time. Rendered from Core's relationship model;
  Explorer infers nothing.
- Explorer action workflows (v0.8.4): open the current artifact in your editor
  (`e`, via `$VISUAL`/`$EDITOR`; Explorer never edits — ADR-024); a guided
  `/import <source> [target]` that converts a document through the ingest
  service, previews the Markdown, and writes only on confirmation (never
  overwriting); and `x` to export recommendations to a Markdown file with the
  same preview-and-confirm flow. Conversions report progress.
- Explorer recommendations (v0.8.3): `/recommendations` (or `r` from the health
  view) presents RAC Core's review findings grouped by category (Validation,
  Relationships, Repository Health, Quality), each with its impact, a suggested
  `rac` command, and navigation to the affected artifact. Severities map to
  Critical / Warning / Suggestion. Advisory only — Explorer applies nothing and
  invents no findings.
- Explorer health view (v0.8.2): `h` or `/health` opens a repository health
  screen — Core's score with a text label, the four health areas
  (Completeness, Relationships, Validation, Coverage), and a prioritized
  attention list whose items open the affected artifact's context view.
  Explorer adds no scoring; every value comes from existing Core results.
- Explorer navigation (v0.8.1): browse every artifact grouped by type, open
  any artifact's context view (identity, validation state, completeness,
  relationships, diagnostics), and reach anything through the `/` command
  surface — `open`, `find`, `browse`, `home`, `help`, `quit`, with bare text
  treated as a search using `rac resolve` / `rac find` semantics.
- Explorer first-run onboarding (v0.8.1): launch states derive from
  repository content (existing, empty, or invalid repository); returning
  users skip onboarding via a marker under the XDG state directory — the
  only state Explorer persists.
- `rac explorer` now defaults to the `rac/` root when present (ADR-018),
  falling back to the current directory (v0.8.1).
- `rac explorer [directory]` — interactive terminal Explorer application
  shell (Textual): loads a repository without blocking the interface, shows
  live progress and a repository summary (artifact counts, relationships,
  diagnostics, health score), and recovers from failures in place with
  reload. Ships as the optional `explorer` extra
  (`pip install 'requirements-as-code[explorer]'`); without it the command
  prints an install hint (v0.8.0).
- First-class repository model in the service layer: `load_repository`
  composes index, validation, relationships, and portfolio over a single
  corpus walk into one navigable object (artifacts, relationships with
  resolution outcomes, unified diagnostics) for Explorer and future
  consumers; no CLI or JSON output changes (v0.8.0).
- Operation primitives for long-lived consumers: progress reporting and
  cooperative cancellation across repository loading, validated against
  1000+ artifact corpora (v0.8.0).
- CI battery integrity (v0.7.14): eight test files (~1,300 lines, including
  all coverage for `rac new` and `rac migrate`) were missing from the CI
  battery matrix and never ran; they are restored, and a new guard test
  fails the suite if any test file is ever orphaned again.
- Static quality gates (v0.7.14): ruff (lint + format) and mypy now gate CI;
  pull requests run the gates plus a fast smoke battery (ADR-027 amended),
  while the full battery grid stays merge-gated on `main`. CLI output is
  unchanged — all golden files are byte-identical.
- Test coverage is reported on every CI run (report-only, currently 97%)
  (v0.7.14).

### Changed

- The Explorer's persistent bottom command bar is gone (v0.8.8): `/` summons
  the palette instead, returning two rows to the content. The `/preferences`
  command became `/settings` (the old spelling still routes), status chips
  use one casing everywhere (`✓ Valid`, `! Warning`, `✗ Error`), key hints
  live only in the status-line chips, sidebar rows show artifact titles
  rather than opaque IDs, and the app bar shows the short version with
  `~`-contracted paths.

- Explorer command results, lookups, help, and preferences now render inside
  the context panel instead of a modal overlay, so the layout never jumps
  (v0.8.7). The default Explorer theme preference is `rac-lantern` (was
  `textual-dark`); set `theme` in `$XDG_CONFIG_HOME/rac/explorer.json` to
  keep a different one.

- Repository corpus traversal is defined once in core (`walk_corpus`) and
  consumed by every repository command — behavior and output unchanged
  (v0.7.14).

- `rac migrate metadata <directory>` — migrate existing recognized artifacts
  onto canonical frontmatter identity: idempotent, byte-preserving, with
  `--dry-run` preview; unrecognized documents are reported, never guessed at
  (v0.7.13).

- `rac resolve <ID>` — resolve any artifact ID (canonical or legacy alias) to
  its type, title, and path; duplicates are reported with every path, never
  silently resolved (v0.7.12).
- `rac find <query>` — deterministic artifact search by ID, title, filename,
  or path, with `--type` filtering and JSON output (v0.7.12).
- `rac relationships` human output resolves references to human-friendly
  labels — `Title (type · ID)` — while JSON keeps stored references unchanged
  (v0.7.12).
- `rac index` entries gain an additive `aliases` field: every identifier an
  artifact answers to, canonical first (v0.7.12).

- `rac init` — establish the repository identity namespace
  (`.rac/config.yaml` with a `repository_key`); idempotent, and an
  established key is never silently changed (v0.7.11).
- Hybrid artifact metadata: a leading YAML frontmatter block
  (`schema_version`, `id`, `type`, `relationships`) is parsed, strictly
  schema-validated, and exposed as canonical machine-operational metadata;
  artifacts without frontmatter remain fully supported (v0.7.11).
- System-assigned opaque artifact IDs (e.g. `RAC-01JY4M8X2QZ7`): branch-safe,
  offline, stable across renames, moves, and type changes; `rac new` assigns
  one automatically and `rac index` reports it (v0.7.11).
- Identity validation: conflicting frontmatter/legacy identity and duplicate
  canonical IDs are deterministic errors — RAC never silently picks one
  (v0.7.11).
- Relationship references resolve against legacy identity aliases (`## ID`
  values, filename prefixes, stems), so adopting canonical IDs does not break
  existing human-readable references; RAC's own corpus now carries canonical
  frontmatter identity (v0.7.11).
- `rac new <type> <output-path>` — create a valid artifact from its canonical
  bundled template; deterministic, AI-free, and never overwrites an existing
  file (v0.7.10).
- `rac templates` — list the canonical artifact templates available to
  `rac new`, with `--json` for tools (v0.7.10).
- `rac validate <directory>` — validate every recognized artifact in a tree in
  one command; unrecognized documents are skipped, not failed.
- `rac review <directory>` — full repository review: validation, relationship
  integrity, and completeness as one prioritized worklist (invalid artifacts
  first, then broken relationships, then advisory findings), each finding with
  a concrete suggested action. Exits `1` only on blocking issues.
- CI trust gates: RAC's own `rac/` corpus must pass `rac validate`,
  `rac relationships --validate`, and `rac review` (dogfood battery), and CLI
  output is pinned byte-for-byte by golden tests.
- README build badge, "How RAC earns trust" section, CHANGELOG.md, and
  CONTRIBUTING.md.
- `rac portfolio --json` now lists `artifacts.unknown_paths` (additive).
- `rac index` — flat artifact inventory (id, type, title, path) for tools and
  agents (v0.7.5).

- Documentation restructured around task-focused guides under `docs/`
  (quickstart, CLI reference, artifacts, relationships, repository workflow,
  testing); README simplified to an overview (v0.7.6–v0.7.7).

### Fixed

- RAC's own planning corpus now passes its own validation: one invalid roadmap
  repaired and all cross-artifact references resolve.

## v0.7.3 — 2026-06-06

### Added

- `rac portfolio` — one-screen repository intelligence: counts by type,
  validity, completeness, relationship coverage, attention list, health score.

## v0.7.2 — 2026-06-06

### Added

- `rac relationships --validate` — resolve every cross-artifact reference and
  report broken, ambiguous, self-referencing, or duplicate-identifier findings.

## v0.7.1 — 2026-06-06

### Added

- `rac relationships` — discover and report the explicit references artifacts
  declare to each other.

## v0.7.0 — 2026-06-06

### Added

- Relationship metadata: artifacts can declare `## Related Requirements`,
  `## Related Decisions`, and similar sections that RAC recognizes and counts.

## v0.6.3 — 2026-06-05

### Added

- Design artifact type: validate and inspect product-design documents.

## v0.6.2 — 2026-06-05

### Added

- Prompt artifact type: validate and inspect reusable AI prompts.

## v0.6.1 — 2026-06-05

### Added

- Guided improvement for roadmaps (`rac improve` understands roadmap sections).

## v0.6.0 — 2026-06-05

### Added

- Roadmap artifact type: validate and inspect roadmap documents.

## v0.5.2 — 2026-06-05

### Added

- `rac schema` — show the expected structure of any artifact type, with
  `--template` to emit a starting document.

## v0.5.0 — 2026-06-05

### Added

- `rac improve` — actionable suggestions (and templates) for incomplete
  artifacts.

## v0.4.2 — 2026-06-05

### Added

- Decision (ADR) artifact type with status/category metadata.
- `rac inspect` — classify a document and report its completeness.

## v0.3.1 — 2026-06-04

### Added

- More ingest formats (HTML, PPTX, XLSX).

## v0.3.0 — 2026-06-03

### Added

- `rac ingest` — convert DOCX/PDF documents into RAC-compatible Markdown.

## v0.2.0 — 2026-06-02

### Added

- `rac stats` — summarize a directory of artifacts: counts, quality signals,
  missing recommended sections.

## v0.1.0 – v0.1.3 — 2026-06-01

### Added

- Initial release: `rac validate` and `rac diff` for requirement documents,
  human and `--json` output, stable exit codes (`0` ok, `1` validation failed,
  `2` usage error).
