# The Org Grounding Plane

One organisation-wide standards corpus, served from one shared endpoint, read
by every engineer's agent — including in repositories that have no corpus of
their own. This page is the recipe for standing that up on surfaces that
already ship: an ordinary corpus, the [shared server](shared-server.md), and
one line of client wiring
([ADR-117](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-117-org-grounding-plane-topology.md)).

It is a deployment topology, not a new engine capability: no cross-corpus
resolution, no federation semantics, no authentication code. Those boundaries
are the point — see [§7](#7-boundaries-what-this-is-not).

## 1. The problem it solves

At organisation scale the corpus unit and the decision unit diverge. A large
engineering organisation holds hundreds of repositories, but the decisions its
agents most need — platform standards, golden paths, firm-wide ADRs — are
org-wide. A per-repository corpus gives those decisions a reach of one.

Corpus federation
([ADR-089](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-089-corpus-federation.md))
is the eventual mechanism, and it is deliberately paced. The org grounding
plane is the day-1 answer: put the firm-wide knowledge in **one** corpus,
serve it from **one** endpoint, and point **every** agent at it. Rollout cost
stops scaling with repository count.

## 2. The shape

```
                       org-standards repo (main)
                                 │
                     shared AsDecided server (shared-server.md:
                     container + proxy + audit + keep-current)
                                 │
        ┌────────────────────────┼────────────────────────┐
   agent in repo A          agent in repo B          agent in repo C
   (lore-org only)          (lore-org only)          (lore + lore-org)
```

Agents **co-mount, they do not merge**: the org endpoint appears as a second
MCP server (`lore-org`) beside any local `lore` server. Each corpus stays its
own canonical truth; an answer from `lore-org` *is* the org's knowledge, and
the endpoint identity carries that provenance. In a repository with no corpus
of its own, `lore-org` is simply the only mount.

## 3. The org corpus

The org corpus is an ordinary single-root corpus in its own repository —
`decided quickstart`, artifacts under `decisions/`, validated and gated like any other.
Nothing about it is special to the engine. What makes it work is editorial:

- **Curate the live constraints, not the archive.** A few hundred binding,
  current decisions ground agents better than ten thousand imported pages.
  Seed it with [`decided ingest`](cli.md#ingest) from what already exists, and
  promote only what an owner ratifies as live.
- **Govern it like code.** Changes land by pull request behind required
  review; use `CODEOWNERS` on `decisions/decisions/` so every standard has an
  accountable owner.
- **Commit the audit stanza.** The shared server refuses to start without a
  working audit sink, and committing the stanza gives the auditor one
  git-diffable artifact (see [the container](shared-server.md#3-the-container)).

## 4. Serve it

Operationally the org endpoint **is** a shared AsDecided server — follow
[Operating a Shared AsDecided Server](shared-server.md) as written: the container,
the authenticating proxy that overwrites `X-AsDecided-Principal`, the keep-current
step, and wrapper-side observability. This page adds no serving machinery.

## 5. Wire the fleet

Three paths, cheapest first:

- **Repository templates.** Put the wiring in the org's repo scaffold so new
  repositories are born grounded:

  ```json
  {
    "mcpServers": {
      "lore-org": { "type": "http", "url": "https://lore.example.com/mcp" }
    }
  }
  ```

- **One command per existing repository:**

  ```bash
  decided init --org-endpoint https://lore.example.com/mcp
  ```

  This ensures the `lore-org` entry in `.mcp.json` and `.cursor/mcp.json` on
  fresh **and** already-initialized repositories. It merges into an existing
  file, touches only the `lore-org` key, never removes what you wrote, and a
  second run with the same URL writes nothing. It composes with `--profile`,
  so a repository with its own corpus gets the local `lore` server and the
  org endpoint side by side.

- **Managed rules blocks for non-MCP clients.** Generate agent-rules blocks
  from the org corpus and commit them into repo templates, so clients that
  read `CLAUDE.md` / `AGENTS.md` files rather than MCP still see the org's
  decisions:

  ```bash
  decided export /path/to/org-corpus --agent-rules
  ```

## 6. Verify

From any wired repository, ask the agent a question only the org corpus can
answer — "which decision governs service health endpoints?" — and check two
things: the answer cites an org artifact id, and the org endpoint's audit log
attributes the read to the caller the proxy authenticated:

```bash
tail -1 /var/log/lore/audit.jsonl
# … "principal": "jane@example.com", "tool": "search_artifacts", "returned": [ … ] …
```

That pair — a cited decision and an attributed read — is the whole loop
working: grounding at the agent, accountability at the endpoint.

## 7. Boundaries (what this is not)

- **Not federation.** There is no cross-corpus resolution, validation,
  identity, or precedence: a repository's artifacts cannot yet cite an org
  ADR as a validated relationship, and duplicate ids across the two corpora
  are not detected. Those semantics arrive with the
  [federation programme](https://github.com/itsthelore/asdecided-core/blob/main/decisions/roadmaps/corpus-federation.md),
  under its recorded constraints.
- **Endpoint reach is corpus visibility.** Anyone the proxy admits can read
  the whole org corpus. Partition sensitive knowledge by corpus topology —
  separate corpora behind separate endpoints — never by expecting
  per-artifact access control from the engine (ADR-085).
- **Two mounted surfaces until federation.** A repository with its own corpus
  carries both tool surfaces in the agent's context. Both are budgeted and
  lean; in repositories without a local corpus, mount only `lore-org`.

## 8. The federation handoff

When the ADR-089 mechanism ships, the org corpus you stood up here becomes
the pinned, materialised **parent**: references resolve across the boundary,
collisions surface as findings, overrides become explicit and attributable,
and the enterprise profile gains its parent declaration
([ADR-088](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-088-enterprise-profile-scaffold.md)).
Nothing about this topology needs to be undone — starting here is starting
early, not starting over.
