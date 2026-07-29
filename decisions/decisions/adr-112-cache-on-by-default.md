---
schema_version: 1
id: RAC-KX2WTHEMDEY0
type: decision
---
# ADR-112: Cache On by Default, Stat-Proxy Freshness as the Floor

## Context

Every surface that can serve from the persistent memory-mapped index store
ships opt-in: `rac find --cache` (ADR-110), `rac validate --cache` (ADR-106),
`rac mcp --cache` (ADR-099/ADR-105). ADR-110 recorded the two honest costs
that justified the opt-in posture for one-shot runs: a cold `--cache` run is
heavier than the plain walk (fork + store write), and a warm one-shot still
pays an Ω(bytes) full re-hash of the corpus to detect change before reusing
anything — only the long-lived server escapes the hash via its event watcher.
ADR-110 also considered and rejected two alternatives: making `--cache` the
default, and a per-invocation stat-only changeset scan, deferring the latter
to the single-node-scale residuals.

Both premises have shifted. First, the stat-proxy scan is no longer
speculative: it ships as the authoritative differ inside the server's
freshness ladder (ADR-105) and as the default detection inside incremental
validation (ADR-106) — `(size, mtime_ns)` as a prefilter that selects which
files to content-confirm, content hashing as the truth, with one named,
test-pinned accepted miss (S5: a size- and mtime-preserving in-place
rewrite). The only cached surface still paying the byte-hash toll per call is
one-shot `rac find`, and only because no manifest survives a one-shot
process — the machinery exists; the persistence does not. Second, the
adoption evidence points the other way from opt-in: the callers who benefit
most from the warm path — agents, benchmark harnesses, editor integrations
issuing many one-shot queries against a stable corpus — are precisely the
callers least able to discover or pass a flag, so the engineered fast path
lies dormant on exactly the workload it was built for, and the tool's
effective performance is its default's.

The `warm-by-default` roadmap schedules the flip; this decision records it.

## Decision

The persistent cache becomes the default, and stat-proxy freshness becomes
the default verification, on every surface that supports them.

- **Default-on with first-class escapes.** `--cache` defaults to on for
  `rac find`, `rac validate`, and `rac mcp` (the flag remains parseable as an
  explicit affirmation). `--no-cache` restores the zero-state walk for one
  invocation; a non-empty `RAC_NO_CACHE` environment variable restores it
  environment-wide for callers that cannot pass flags (CI, hooks, generated
  integrations). The cache remains disposable and content-addressed under
  `$XDG_CACHE_HOME` / `RAC_CACHE_DIR` (ADR-099/ADR-104); deleting it costs
  only latency.
- **A persisted one-shot freshness manifest.** `rac find`'s cache path gains
  a per-root, recursion-mode-keyed stat manifest
  (`manifest/v1/{root_key}.fseg`, the `.vseg` store discipline: checksummed
  framing, fail-closed decode, atomic replace), written best-effort after
  every `load_or_build`. A later one-shot process runs the shared
  stat-manifest scan against it — stats every enumerated file, reuses the
  prior content hash when `(size, mtime_ns)` is unchanged, content-confirms
  only stat-changed or new files — and recomposes the corpus key from the
  manifest at O(files) enumeration cost, byte-identical to the full re-hash
  for every non-S5 state. A missing, corrupt, or unreadable manifest fails
  closed into the content-confirm-all scan and is rewritten (self-healing);
  a failed manifest write is ignored — the manifest can change only latency,
  never an answer.
- **The full re-hash becomes the opt-in floor.** A `--verify` flag on
  `rac find` and `rac validate` forces the content-confirm-all scan — byte
  for byte the legacy `corpus_content_hash` check — catching the S5 rewrite
  the stat rung accepts, and repairing the manifest as it does. The server's
  ladder is unchanged (its `verify` rung stays an internal surface; inotify
  still only asserts *clean*, ADR-105).
- **The S5 acceptance extends to one-shot runs, on the record.** The single
  missable case — an in-place rewrite preserving both `size` and `mtime_ns`
  between two cached one-shot invocations — is accepted by this decision
  exactly as ADR-105 accepted it for the server, pinned by a test that
  documents the stale serve and proves `--verify` returns fresh output. Add,
  remove, and rename are never at risk: enumeration detects them from the
  path set.
- **Degrade-never-fail hardens for default-on.** A homeless environment
  (unresolvable home directory with no `RAC_CACHE_DIR`/`XDG_CACHE_HOME`)
  falls back to a temp-directory cache location; that failing too, the
  existing chain holds — an unwritable store or cache directory degrades to
  the fresh walk. No query fails, and no output changes, because a cache
  could not be used.

The carried non-negotiables are restated, not renegotiated: byte-parity to
the fresh walk for every non-S5 corpus state (search, `--decisions`,
`--type`, `--tag`, `--explain`, validate's outputs), content-addressed
integrity on the corpus key, disposability, fail-closed schema and
scoring-constant gates, no code-bearing deserialisation, and atomic writes.

## Consequences

A caller who types nothing now gets the engineered path: repeated one-shot
queries against an unchanged corpus read zero artifact bytes — enumeration
and stats only — and the warm line the store bought reaches the CLI's
default, not just its flag. The two costs ADR-110 recorded are re-priced
rather than denied: the Ω(bytes) warm toll is replaced by an O(files) stat
scan (the sub-stat git/fsmonitor fast path remains a single-node-scale
residual), and the cold-build penalty still exists but is now paid once per
corpus rather than guarded by a flag nobody finds — with `--no-cache` and
`RAC_NO_CACHE` restoring the old posture for genuinely one-off invocations
and policy-constrained environments.

The trade-offs are accepted on the record. Default paths are no longer
zero-state: commands that previously wrote nothing now write a disposable
cache, which test harnesses and sandboxes must isolate (the suite's own
hermeticity fix ships with the flip). The S5 window, previously confined to
opt-in surfaces, is now a property of the default — that is the substance of
this decision, and it is bounded (one enumerated case, stat-preserving
rewrites only), pinned by test, and floored by `--verify`. And ADR-032's
zero-state posture, already revised narrowly for the opt-in cache path by
ADR-105, is now revised for the default path too: what is preserved is not
statelessness but the contract statelessness served — no query acts on stale
state beyond the named S5 window, and a wrong answer is still worse than a
slow one.

## Status

Accepted

## Category

Technical

## Alternatives Considered

### Keep the opt-in posture (ADR-110 unchanged)

The flags exist; adoption does not follow. The callers the warm path was
built for cannot discover it, every integrator decides the default
independently, and the measured product is the slow path. Rejected: a fast
path that ships dark is a cost without a constituency.

### Default-on, but keep the full re-hash as the warm check

Flip the default and retain the per-call `corpus_content_hash`. Simpler — no
manifest — but every warm one-shot keeps the Ω(bytes) toll, so the default
buys parse/derive skips while still reading the whole corpus per query;
ADR-110's cost objection stood against exactly this variant. Rejected in
favour of persisting the already-proven stat rung.

### Sub-stat changed-set detection (git/fsmonitor)

Detect change below the O(files) stat floor. Remains deferred to the
single-node-scale residuals: it needs a service mode or an external event
source, and the stat floor passes the current gates at the corpus sizes the
one-shot path serves.

## Supersedes

- ADR-110

## Relationship to Other Decisions

- ADR-110: superseded. Its store-reuse mechanics (`load_or_build` on the
  one-shot path, cross-process content-addressed reuse, byte-parity,
  recency joined per-match after ranking per ADR-045) carry forward
  unchanged; its opt-in default, its accepted per-call hash floor, and its
  two rejected alternatives — which this decision adopts — do not.
- ADR-099: its "opt-in, off by default" enablement clause is amended to
  default-on. The content-addressed key contract is unchanged: the key is
  now recomposed from the persisted manifest, byte-identical to the full
  re-hash for every non-S5 state.
- ADR-104: the opt-in posture it established is amended; the store format,
  schema and scoring-fingerprint gates, and disposability are untouched.
- ADR-105: extended, not amended. Its fallback ladder and S5 acceptance
  become the default posture and reach one-shot processes through the
  persisted manifest; the server's tracker, drain barrier, and rungs are
  unchanged.
- ADR-106: "opt-in behind the same `--cache` flag" is amended to default-on,
  and its existing internal `verify` parameter is surfaced as the
  `--verify` flag; the incremental validation machinery is unchanged.
- ADR-032: revised for the default path. ADR-105 revised it narrowly for the
  opt-in cache mode; this decision makes that revision the default posture
  for `find`, `validate`, and `mcp`, preserving the correctness contract
  (never act on stale state; the S5 window is the named, floored exception)
  rather than the mechanism (per-call zero-state re-reads). `--no-cache` and
  `RAC_NO_CACHE` keep the pure ADR-032 posture reachable everywhere.
- ADR-080: disposability is load-bearing for default-on — a corrupt,
  unwritable, or absent store or manifest degrades to a fresh build, never
  a wrong answer or a failed query.

## Related Roadmaps

- warm-by-default
- candidate-discovery
- single-node-scale-residuals

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The default cache posture and explicit rollback flags are deterministic CLI properties."
rules:
  - id: find-and-validate-default-cache-on
    kind: require_pattern
    path_glob: "rust/rac-engine/src/cli.rs"
    pattern: '(?s)let mut cache = true;.*let mut cache = true;'
    message: "The native validate and find paths must continue to default to the engineered cache."
  - id: cli-keeps-no-cache-escape-hatch
    kind: require_pattern
    path_glob: "rust/rac-engine/src/cli.rs"
    pattern: '"--no-cache" => cache = false'
    message: "The CLI must retain the explicit no-cache rollback path."
```
