---
schema_version: 1
id: RAC-KXE0M2QBF2MP
type: decision
---
# ADR-114: Native Index Workspace Dependencies — memmap2 In, inotify Deferred

## Context

The native derived-index port (roadmap:native-derived-index) brings the
ADR-099/103/104/105/106/107/108/112 cache and store stack to `rac-engine`.
ADR-104 mandates a memory-mapped read path: the store is a directory of
binary segment files, mapped and read by point access, so `get_artifact`
touches only the identity rows it needs. The Rust standard library has no
mmap surface, so honouring ADR-104 natively requires a new workspace
dependency. Until now the Rust workspace has held its dependency set to
serde/serde_json (plus the harness), and every addition is a supply-chain
and audit surface the maintainer must carry, so new workspace dependencies
require a recorded decision rather than an incidental `Cargo.toml` edit.

The same stack's freshness ladder (ADR-105) names inotify as its fastest
rung. inotify is only ever trusted to assert *clean* — the moment it
reports anything, the authoritative stat-manifest scan runs — so skipping
it is behaviour-neutral by construction: the stat-scan rung answers every
correctness question the watcher would, at O(files) stat cost instead of
O(1). The `notify`/`inotify` crates would be a second new dependency,
carried only for warm-serving latency on the long-lived MCP server.

## Decision

Adopt **`memmap2`** as a workspace dependency of `rac-engine`, pinned at a
reviewed version, used only to map read-only index-store segments under
the fail-closed open gates the store format defines (magic, format
version, scoring fingerprint, hash echo, exact-length truncation check).
No other new dependency lands with the index port.

**Defer inotify.** The native `FreshnessTracker` rests on the
stat-manifest scan as its fastest rung; the inotify accelerator is not
ported in this series. The fallback-ladder seam stays (the tracker's
detection is a rung selection), so a later decision can add a watcher
without reshaping the tracker.

Parallelism for the cold build (ADR-107/108) reuses the workspace's
existing `rayon` dependency — already adopted for the engine's parallel
corpus walk (PORT-CONTRACT decision 5) — so the fan-out adds no new
dependency; the pickling boundary the Python oracle fans across does
not exist in-process.

## Consequences

- ADR-104's point-access RSS posture holds natively: warm reads map
  segment pages on demand instead of rehydrating whole structures.
- One new audited dependency (`memmap2`, no transitive dependencies)
  instead of two; the supply-chain surface stays minimal.
- Native warm MCP serving detects change by stat-scan every call: warm
  latency scales O(files) with corpus size rather than O(1). This is the
  recorded trade — the native stat-scan floor is fast enough that the
  inotify rung is a latency optimisation, not a correctness need, and it
  can be added later behind the existing ladder seam.
- The `RAC_TIMING`/degrade contracts of the stack port unchanged: cache
  failure of any kind still degrades silently to a fresh build.

## Status

Accepted

Partially superseded by ADR-118: Linux now adopts the target-specific inotify
rung after the measured 100k stat floor reached 324 ms. Other platforms retain
this decision's stat fallback.

## Category

Technical

## Alternatives Considered

- **Plain `read()` into heap buffers for v0, mmap deferred.** Honours the
  byte contracts (the store bytes are identical either way) but defeats
  ADR-104's stated point: per-call peak allocation would again scale with
  the corpus, and a second pass would have to re-plumb the reader. The
  escape hatch remains available if `memmap2` were ever unacceptable, but
  it is the fallback, not the default.
- **Porting the inotify rung now (`notify` or raw `inotify` bindings).**
  Behaviour-neutral to skip (it only ever asserts clean), a second new
  dependency, and Linux-only value; deferred until warm-serving latency
  at scale demands it.
- **Standard-library scoped threads for the cold build.** Viable, but
  `rayon` is already in the workspace for the parallel corpus walk, so
  hand-rolling a second fan-out mechanism would add code without
  removing a dependency.

## Related Decisions

- ADR-099
- ADR-104
- ADR-105
- ADR-107
- ADR-108
- ADR-112

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The audited mapped-store dependency and target-specific watcher dependency are exact manifest boundaries."
rules:
  - id: engine-keeps-reviewed-memmap-dependency
    kind: require_pattern
    path_glob: "rust/rac-engine/Cargo.toml"
    pattern: '(?m)^memmap2 = "0\.9"$'
    message: "The mapped store must retain the reviewed memmap2 dependency line."
  - id: inotify-remains-linux-only-and-minimal
    kind: require_pattern
    path_glob: "rust/rac-engine/Cargo.toml"
    pattern: '(?s)\[target\.\x27cfg\(target_os = "linux"\)\x27\.dependencies\].*inotify = \{ version = "0\.11", default-features = false \}'
    message: "The event accelerator must remain Linux-only with default async features disabled."
```
