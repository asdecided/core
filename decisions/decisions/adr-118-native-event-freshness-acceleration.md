---
schema_version: 1
id: RAC-P55FRE5HNE55
type: decision
tags: [performance, freshness, rust, linux]
---
# ADR-118: Native Event Freshness Acceleration

## Status

Accepted

## Context

ADR-114 deferred the native event rung because the stat-manifest scan cost
about 1 ms on the then-live 427-artifact corpus. P5 graph indexing removed the
other warm `get_related` work and exposed that scan as the limiting phase:
about 17 ms at 5,000 artifacts and 324 ms at 100,000 artifacts on the measured
Apple Silicon host. The cost is linear and is paid even when nothing changed.

ADR-105 already defines the safety boundary: an event source may assert only
that a corpus is clean. Any event, overflow, incomplete watch set, or setup
failure must run the authoritative stat-manifest differ. Events never compute
the changed set and never replace content confirmation.

macOS FSEvents was evaluated with `NoDefer`, `FSEventStreamFlushSync`, and the
system journal watermark. An immediate completed file write can still precede
delivery and watermark advancement. It therefore cannot preserve RAC's
completed-writes-are-visible contract as an O(1) clean oracle. Fixed sleeps
were rejected: even 10 ms missed a pinned immediate-write case and no finite
delay is a correctness proof.

## Decision

Port the ADR-105 event rung to the native tracker on Linux using synchronous
inotify draining.

- Install a watch on every non-hidden real directory before the initial scan.
- At each request boundary, drain the non-blocking kernel queue. A completely
  drained empty queue under a complete watch set may assert clean and skip the
  stat scan.
- Any event, queue overflow, read error, watch-limit failure, or incomplete
  directory walk selects the stat rung.
- Before a dirty scan, rebuild the complete watch set. The authoritative scan
  covers the replacement gap; a post-scan drain brackets mutations racing with
  the scan. If an event arrives during the scan, rebuild and repeat, bounded to
  three attempts while leaving the watcher dirty on continued churn.
- Add `inotify` only on Linux, with default async features disabled.

macOS, Windows, and unsupported filesystems remain on the stat rung. The native
stat implementation probes file metadata in parallel and shards recursive
discovery across root children while preserving the final component-wise walk
order. This improves the safe fallback but does not claim scale-invariant warm
latency.

The active rung is observable as `inotify` or `stat`; timing records state
whether a scan was performed.

## Consequences

Unchanged warm MCP requests on a healthy Linux watch set no longer scale with
corpus size. Changed requests retain exactly the same stat/content truth path
and output bytes. Setup or runtime uncertainty degrades latency, never
correctness.

On the measured macOS 100,000-artifact corpus, parallel fallback scanning
reduced the observed warm p50 from about 324 ms to about 200 ms. It remains the
dominant cost and is explicitly not described as solved on macOS.

This partially supersedes ADR-114: its dependency deferral remains true for
unsupported platforms, but Linux now adopts one target-specific dependency
because measured 100k latency crossed the recorded review threshold.

## Alternatives Considered

### Trust macOS FSEvents after a fixed delay

Rejected. It failed the immediate completed-write test and converts a latency
decision into bounded silent staleness.

### Trust asynchronous cross-platform callback delivery

Rejected. Callback queues do not establish a synchronous request barrier.

### Poll periodically instead of on every request

Rejected. This admits an explicit stale window and weakens the existing MCP
freshness contract.

### Use events to apply precise deltas directly

Rejected. Rename pairing, overflow, new-directory races, and platform-specific
event semantics would become correctness dependencies. Events remain hints;
the stat/content differ remains truth.

## Related Decisions

- ADR-105
- ADR-114
- ADR-116

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "Linux watcher scope, overflow degradation, and platform fallback are explicit native implementation boundaries."
rules:
  - id: watcher-remains-linux-targeted
    kind: require_pattern
    path_glob: "rust/rac-engine/src/freshness_watch.rs"
    pattern: '#\[cfg\(target_os = "linux"\)\]'
    message: "The native event accelerator must remain explicitly Linux-targeted."
  - id: watcher-overflow-never-asserts-clean
    kind: require_pattern
    path_glob: "rust/rac-engine/src/freshness_watch.rs"
    pattern: 'EventMask::Q_OVERFLOW'
    message: "Queue overflow must remain a dirty signal that falls back to authoritative scanning."
  - id: tracker-keeps-mode-and-fallback-regression
    kind: require_pattern
    path_glob: "rust/rac-engine/tests/freshness_tracker.rs"
    pattern: 'watcher_setup_and_runtime_failure_degrade_to_stat'
    message: "Watcher setup and runtime failures must continue to degrade to the stat rung."
```
