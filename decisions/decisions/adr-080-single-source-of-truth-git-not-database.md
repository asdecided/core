---
schema_version: 1
id: RAC-KVSTYDARXKYW
type: decision
---
# ADR-080: The Single Source of Truth Is Git, Not a Database

## Context

A recurring question as Lore targets teams of 50+ developers: if every developer
runs Lore against their own clone, won't those copies drift out of sync, and
shouldn't there be a central database everyone queries instead?

Lore's architecture answers the first half already. The corpus is Markdown in a
git repository (ADR-001, ADR-018), served read-only and re-read per call
(ADR-032), deterministically and offline (ADR-002), with no embeddings (ADR-066).
It is governed exactly as code is: a change becomes authoritative only when it
lands on `main` through human PR review (ADR-065) behind the merge gate
(ADR-075). The question is whether "everyone runs their own copy" is a
consistency problem a database would solve — and it is the opposite.

## Decision

Lore's single source of truth is the corpus on the git host's `main` branch.
RAC does not adopt a database as a system of record, and a developer's local
clone is a working copy that converges on `main`, never a competing truth.

- **`main` is the one canonical state**, exactly as a codebase has one. A local
  checkout is either current with `main` or a `git pull` behind it. Only PR
  review writes to `main` (ADR-065), so a local copy can be *stale* but never
  *authoritative-and-divergent*. Git already keeps a 50-developer team consistent
  on a shared codebase with no database to synchronise it; the corpus is governed
  identically.
- **A database would add drift, not remove it.** It is a second representation of
  the knowledge that must be continuously reconciled with `main` — two stores
  that can disagree, where files-in-git is one. The forces that justify a
  database for agent-memory tools do not apply here: Lore is read-only (no write
  contention), has no embeddings (ADR-066), and stores text git versions natively.
- **Determinism makes "in sync" precise.** Identical corpus bytes yield identical
  answers (ADR-032), so two agents reading the same commit hold the same knowledge
  by construction; consistency reduces to "same commit," which git manages.
- **A hard central guarantee is a server, not a database.** A team that wants every
  agent to call one always-current endpoint — rather than its own checkout — runs
  a single shared `rac mcp` server fronting an auto-updated `main` checkout. The
  corpus stays the source of truth; the shared server is one reader of it. That
  topology is recorded as future intent (`lore-at-team-scale`) and needs HTTP
  serving, which is a transport feature, not a datastore.
- **Database-grade querying lives downstream.** If a team wants graph or vector
  queries over the knowledge, `rac export --graph` / `--documents` feed a database
  the team runs (ADR-073). The engine stays pure; the corpus stays canonical.

## Consequences

Zero infrastructure: no database to run, migrate, back up, or secure — git
provides durability, history, access control, and audit, and "your knowledge is
Markdown in git" is both the architecture and a differentiator against tools that
make you stand up a datastore. Trust and freshness come from git + PR review +
determinism, not from a server holding mutable state.

Trade-off accepted: in the default local-clone topology a developer on a stale
checkout queries stale knowledge until they pull — the same staleness as an
un-pulled codebase, resolved the same way. Teams that find this unacceptable
adopt the shared-server topology (still a server, still no database). The cost of
*not* adding a database is that there is no single live process all reads pass
through by default; the benefit is that there is exactly one representation of the
truth, and it is the one the team already reviews and versions.

## Status

Accepted

ADR-149 preserves this decision's no-database, Git-backed source-of-truth
contract while amending its hard-coded Git-host `main` branch language with a
forge-, remote-, and branch-name-neutral repository contract.

## Category

Architecture

## Alternatives Considered

- **A central database as the system of record.** Rejected: it is a second
  representation to reconcile with `main`, loses git's diffability, history, and
  offline operation, and re-introduces the drift it claims to cure; none of the
  forces that justify a database (concurrent writes, vectors, embedding-scale
  storage) apply to a read-only, no-embedding, text corpus.
- **A central database as a shared cache (not the source of truth).** Deferred,
  not adopted: a *persistent derived index* may be warranted at scale, but it is a
  rebuildable cache behind ADR-032's snapshot seam (recorded in
  `lore-at-team-scale`), not a database and not authoritative.
- **Local-clone only, with no shared option.** Rejected: teams that want a single
  live endpoint have a legitimate need; a shared `rac mcp` server meets it without
  a database, so the topology is recorded as intent rather than refused.

## Related Decisions

- adr-001
- adr-018
- adr-032
- adr-065
- adr-066
- adr-073
- adr-149

## Related Roadmaps

- lore-at-team-scale
