---
schema_version: 1
id: RAC-KX2WNYNH8Z5X
type: roadmap
---
# Warm by Default

## Status

Planned

Codename: `warm-by-default`. Reverses the opt-in persistent-cache posture:
the cache becomes the default on every surface that supports it, and the
stat-proxy freshness scan becomes the default verification, with the full
byte re-hash demoted to an opt-in floor.

## Context

The persistent memory-mapped index store made warm retrieval scale-invariant,
but every surface that can use it — `rac find --cache`, `rac validate
--cache`, `rac mcp --cache` — ships opt-in, so the default paths still walk,
parse, and rebuild on every invocation. ADR-110 recorded the two honest costs
that justified opt-in at the time: a cold `--cache` run is heavier than the
plain walk, and the warm path still pays an O(bytes) full re-hash to detect
corpus change. The second cost is no longer structural: the stat-proxy scan
(size + mtime_ns per file, content-confirm on stat mismatch) already ships as
the default freshness rung on the server (ADR-105) and inside incremental
validation (ADR-106) — only the one-shot `rac find` path lacks it, because
its freshness key is recomputed from file bytes on every call and no manifest
survives the process. Meanwhile the callers who benefit most from the warm
path — agents, benchmark harnesses, editor integrations issuing many
one-shot queries — are exactly the callers least likely to discover a flag.

The residual "changed-set detection below the stat floor" initiative in the
single-node-scale record pointed at this gap; this roadmap promotes the
stat-rung part of it to scheduled work and flips the posture, per ADR-112.

## Outcomes

- A caller who types nothing gets the warm path: repeated `rac find`,
  `rac validate`, and Lore MCP queries against a stable corpus are bound by
  query selectivity, not corpus size, with no flag knowledge required.
- Freshness verification on cached surfaces costs stats, not bytes: an
  unchanged corpus is confirmed unchanged without reading artifact contents,
  and the full byte re-hash remains available on demand as the always-correct
  floor.
- The escape hatches are first-class: `--no-cache` restores the zero-state
  walk per invocation, and `RAC_NO_CACHE` restores it environment-wide for
  callers who cannot pass flags (CI, hooks, third-party harnesses).

## Initiatives

- Flip the three cache defaults: `--cache` becomes the default on `rac find`,
  `rac validate`, and `rac mcp`; add `--no-cache` and the `RAC_NO_CACHE`
  environment escape; keep `--cache` parseable as an explicit affirmation.
- Persist the one-shot freshness manifest: a per-root, recursion-mode-keyed
  stat manifest beside the store, written best-effort after every
  `load_or_build`, so a later one-shot process verifies freshness with the
  shared stat scan and reconstructs the corpus key byte-identically from
  cached per-file hashes instead of re-reading every file.
- Expose the verify floor: a `--verify` flag on `rac find` and `rac validate`
  that forces the content-confirm-all scan (the pre-flip full-hash
  behaviour), catching the stat-preserving rewrites the default scan accepts.
- Degrade-never-fail hardening for default-on: a homeless environment (no
  resolvable home, no cache-dir override) or an unwritable cache directory
  must fall back to the fresh walk, never fail the query.
- Flip the documentation posture across the CLI, MCP, shared-server, and
  scale docs: the cache is the default; the flags documented are the escapes.
- (Follow-on, unscheduled) Seed the MCP server tracker's cold start from the
  persisted manifest, so a server restart pays stat cost instead of byte
  cost before the watcher takes over.

## Success Measures

- A warm default `rac find` against an unchanged corpus reads zero artifact
  bytes: enumeration and stats only, confirmed by test.
- Default output is byte-identical to `--no-cache` for search, the
  `--decisions` query, `--type`, `--tag`, and `--explain`, and for validate's
  human and JSON outputs, across cold, warm, and every non-S5 corpus
  transition (edit, add, remove, rename).
- The S5 accepted miss (a size- and mtime-preserving rewrite) is pinned by
  test on the one-shot path, and `--verify` both returns fresh output and
  repairs the manifest so subsequent default runs are fresh.
- `RAC_NO_CACHE=1` leaves the cache directory untouched; an unwritable or
  homeless cache environment never fails a query or changes its output.

## Assumptions

- The stat-proxy model's accepted-miss window (ADR-105's S5) is acceptable as
  a default for one-shot runs exactly as it is for the long-lived server; the
  `--verify` floor and the pinning test keep the acceptance honest.
- The persisted manifest, like the store, is a disposable derived structure:
  deleting it costs one full-confirm scan, never a wrong answer.

## Risks

- A default-on cache writes to disk from commands that previously wrote
  nothing; environments with read-only or unusual home layouts must hit the
  degrade path, not an error — hardening is in scope, and the escape hatches
  exist for policy-constrained environments.
- The first-ever run against a corpus pays the cold build (parallel parse +
  store write) that ADR-110 recorded as heavier than the plain walk; the cost
  amortises from the second query onward and `--no-cache`/`RAC_NO_CACHE`
  restore the old behaviour for genuinely one-off invocations.
- Suite hermeticity: with the cache on by default, tests that drive the CLI
  must isolate the cache directory or they pollute the developer's real
  cache; the test harness change is part of the work, not an afterthought.

## Related Decisions

- ADR-112
- ADR-099
- ADR-104
- ADR-105
- ADR-106

## Related Roadmaps

- candidate-discovery
- single-node-scale-residuals
- rebuild-scale

## Related Tickets

- itsthelore/asdecided-core#340
