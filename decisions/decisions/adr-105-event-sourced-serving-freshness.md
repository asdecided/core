---
schema_version: 1
id: RAC-KWSDFYW7PCW6
type: decision
---
# ADR-105: Event-Sourced Serving Freshness

## Context

ADR-099 gave the derived read-model a content-addressed cache keyed on
`corpus_content_hash`, and ADR-104 replaced its serialized-blob representation
with a memory-mapped base + delta fold — but both kept ADR-032's freshness
posture on the serving path: the key is recomputed on **every** tool call, and
`corpus_content_hash` is an Ω(bytes) read of every artifact in the corpus. So
even a warm cache hit that answers a single point query first re-reads and
re-hashes the whole repository. On the `rebuild-scale` reference node the warm
line therefore slopes with corpus size (measured ~0.22 ms/artifact of freshness
cost alone), and the flat-latency gate (warm p99 < 100 ms independent of N)
cannot be met while every call pays that Ω(bytes) toll.

The `rebuild-scale` performance lens (v2 §2) isolates the fix. The Ω(N) freshness
floor splits into an Ω(files) **enumeration** cost (readdir — detects add /
remove / rename from the path set, staleness-free under pure content addressing)
and an Ω(bytes) **content-read** cost (needed only for in-place edits). Only an
event source or an explicit trust assertion collapses the content-read term to
O(changed) and yields a flat warm line; a per-call proxy (stat, dir-mtime) at
best reduces the slope. ADR-104 built the base + delta fold seam precisely so
this decision could populate the delta from an event-sourced changed set without
re-deriving the corpus, and left the freshness decision — how the delta is kept
current — explicitly to this ADR.

This revises ADR-032 narrowly and only for the opt-in cache path. ADR-032's
correctness contract — an agent must never act on stale repository state; a wrong
answer is worse than a slow one — is non-negotiable and is preserved by
construction here. The default path (no `--cache`) is untouched: it still
re-reads and rebuilds from disk on every call, exactly ADR-032.

## Decision

On the opt-in cache path the `rac mcp` server maintains a **server-lifetime
freshness tracker** (`services/freshness.py`) that replaces the per-call
`corpus_content_hash` re-hash. The tracker owns the current corpus manifest
(relpath → content hash + `(size, mtime_ns)` proxy), an incrementally-maintained
parsed snapshot, and the last served read-model with its corpus hash. Every MCP
tool reads through the tracker instead of re-hashing the corpus, and the served
bytes remain byte-identical to a fresh whole-corpus walk of the current corpus
state.

**Change detection is a fallback ladder, and correctness never depends on the
fast rung.** At the top of every call the tracker drains pending events to a
barrier and, when the corpus changed, content-confirms each flagged path (reads
its bytes) before mutating state — events are triggers, content is truth. The
rungs, best-latency first:

1. **inotify** (Linux, ctypes, stdlib only) — a watch set over every directory
   the walk descends, established following the correctness protocol
   (watch-before-scan at setup; on directory creation the new directory is
   watched and then rescanned; on queue overflow the watch set is rebuilt before
   the ensuing verify). The watcher is trusted **only to assert *clean***: a drain
   that yields zero events under a known-complete watch set skips detection
   entirely and returns the cached read-model, giving warm latency independent of
   corpus size — the flat line. The instant it reports anything (an event, an
   overflow, a directory it could not watch) the authoritative stat-manifest scan
   runs. It is an accelerator, never the arbiter.
2. **stat-manifest scan** — the primary, always-available differ, and the shipped
   default when inotify is unavailable. It enumerates the walk's files (the exact
   `find_markdown_files` scope), stats each for `(size, mtime_ns)`, and
   content-confirms only the files whose stat changed or that are new.
   Enumeration makes add / remove / rename staleness-free; the corpus hash is
   recomposed from the manifest at O(files) enumeration cost with no O(bytes)
   reads of unchanged files.
3. **full re-hash** — the floor, always correct: it reads and hashes every file,
   byte for byte the legacy `corpus_content_hash`, and is the `verify` path.

When the corpus is unchanged the tracker returns the cached read-model with no
re-derive; when it changed, only the changed files are re-parsed and the whole
read-model is re-derived over the snapshot through the `build_derived_index`
from-entries seam, so the result is byte-identical to a fresh walk. The
memory-mapped base (ADR-104) is written for a corpus hash and, while the corpus
drifts within a bounded window of changed files (the **delta**), reads are served
from the re-derived snapshot without rewriting the base; when the window crosses
the compaction threshold (v2 §1.2: delta docs > max(10k, 1% of base)) a fresh
base is written for the current hash via the store writer's atomic `os.replace`
and the window resets — the LSM-style base + delta + compaction shape.

**The drain barrier is no weaker than a fresh walk (the race equivalence).** On a
local filesystem the kernel queues the inotify event, and the stat reflects the
completed write, before the mutating `write()` returns. So any mutation that
*completes before a call* is observed by that call — the no-stale-read contract
ADR-032 achieved by full re-read is preserved by construction for completed
writes. A write racing *concurrently* with a call is unordered against it exactly
as it is against a fresh whole-corpus walk: the walk-based path has the identical
race window, so this decision claims no more than parity with it, and no less.

**The admitted staleness cases are enumerated and each is pinned by a test that
documents the accepted behaviour**, not hidden:

- **S1 — an inotify-incapable filesystem** (NFS, some overlay / FUSE / bind
  mounts, or the watch limit). Detected at setup; the tracker records the
  degraded mode and every call runs the stat-manifest scan (correct, O(files)
  stat, not flat) — the flat-latency property is explicitly forfeited there, not
  silently claimed. Pinned by `test_inotify_setup_failure_degrades_to_stat` and
  `test_degraded_stat_mode_is_parity_correct`.
- **S5 — an in-place rewrite preserving both `size` and `mtime_ns`** (a backdated
  same-length rewrite, a byte-restore). The stat rung diffs on `(size, mtime_ns)`
  and so does not re-read the file: this is the single missable case, and the
  full re-hash floor catches it. Add / remove / rename are never at risk —
  enumeration detects them from the path set. Pinned by
  `test_size_and_mtime_preserving_rewrite_is_the_accepted_stat_miss`.
- **S2 — a symlinked `.md` file whose out-of-tree target's bytes change.**
  inotify on the containing directory reports nothing when the link target's own
  bytes change, and the stat rung's `mtime_ns` follows the target through the
  link, so a target rewrite that also preserves `(size, mtime_ns)` reduces to S5;
  a target edit the stat observes is caught. The residual — a symlinked file
  whose target the tracker cannot watch and whose stat is unchanged — is the same
  S5 miss, caught by the full re-hash floor.

The inotify rung is the flat-line mechanism; the stat-manifest scan is the
shipped primary, so on a filesystem where inotify's clean signal cannot be
trusted the honest outcome is a reduced-slope stat line, and the scorecard names
the active rung. The full re-hash floor is always available and always correct.

This **supersedes ADR-099's per-call full re-hash key recomputation** and
**revises ADR-032's absolute per-call re-read for the opt-in cache mode**; it does
not touch the default path. ADR-099's and ADR-104's non-negotiables carry
forward: content-addressed integrity on the corpus hash, byte-parity to a fresh
build as the coherency guarantee, disposability (a corrupt or unwritable store
degrades to a fresh build, never a wrong answer), fail-closed schema and
scoring-constant gates, no code-bearing deserialisation, and atomic writes.

## Consequences

Warm serving latency stops scaling with corpus size on inotify-capable local
filesystems: an unchanged corpus is answered from the cached read-model with no
byte reads and no re-derive, and a change re-parses only the changed files. The
per-call Ω(bytes) `corpus_content_hash` toll — paid on every call under ADR-099,
warm or cold — is removed from the steady-state path. The delta window serves the
common single-edit case without rewriting the base, and compaction bounds the
window so the resident overlay cannot grow without limit.

The trade-offs are accepted on the record. The tracker introduces
**server-lifetime state**, which ADR-032 deliberately avoided — this is the
paradigm shift the decision records, and it is why correctness is anchored to the
drain-barrier race equivalence and the byte-parity-vs-fresh-build battery rather
than to statelessness. The tracker holds the parsed snapshot resident to re-parse
only changed files on a mutation; that resident cost is a named residual — the
memory-mapped base still bounds the on-disk index and is the compaction target,
and shedding the resident parse (serving even changed states purely from the
mapped base + a declared-reference delta) waits on the cross-file incremental
mechanism a later bundle owns. `get_summary`'s portfolio aggregate is re-derived
over the snapshot on change (an O(N) recompute, not flat) — a stated residual
consistent with the performance lens's treatment of it — while point, search, and
graph reads ride the flat path. The stat rung's S5 miss and the inotify
operational preconditions (`fs.inotify.max_user_watches` must cover the directory
count; a bulk changeset can overflow the queue and trigger a rebuild-then-verify
spike) are named, not hidden.

The risk is a subtly incomplete watch set silently absorbed into a *clean*
verdict — mitigated structurally: the watcher latches to always-dirty on any
unwatchable directory, overflow, or failed rescan, so a *clean* answer requires a
provably complete watch set, and the authoritative differ is always the
stat-manifest scan (or the full re-hash floor), never inotify. The new-directory
race is pinned by a shard-boundary parity test that also edits a file inside the
freshly created subdirectory, so a watch-management gap fails the battery loudly
rather than serving stale.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

### Keep the per-call full re-hash (ADR-099 / ADR-032 unchanged)

Recompute `corpus_content_hash` on every call. This is correct but keeps the
Ω(bytes) whole-corpus read on the steady-state warm path — the exact cost the
performance work exists to remove — so the flat-latency gate can never be met at
scale. It is retained only as the always-correct floor (`verify`) and as the
default (cache-off) posture ADR-032 mandates.

### A modification-time cache (ADR-032's rejected option)

Key the parsed corpus on file mtimes and invalidate on change. ADR-032 rejected
this because mtime alone is an unreliable invalidation signal — editor
save-in-place and same-second rewrites make it silently stale. This decision does
not trust mtime as the invalidation arbiter: `(size, mtime_ns)` is only a cheap
*prefilter* that selects which files to content-confirm, and content hashing
remains the truth. The one case the prefilter alone can miss (S5) is named,
pinned, and caught by the full re-hash floor — it is an accepted, bounded residual,
not silent staleness.

### A pure inotify model with no authoritative differ

Trust the inotify event stream to compute the precise changed set (cookie-paired
renames, per-event changed-set application) and never scan. This is the flattest
design but makes correctness depend on never missing an event — a large, hard-to-
verify surface in pure ctypes. This decision instead makes inotify assert only
*clean* and routes every dirty or uncertain signal through the authoritative
stat-manifest scan, so an inotify bug degrades latency, never correctness.

### A background watcher thread

Refresh the read-model on a background thread as changes arrive. This adds a
thread, its synchronisation, and a change-to-refresh race window between the
event and the next call — complexity the synchronous drain-at-call-barrier avoids
while giving the same completed-writes-are-visible guarantee.

## Relationship to Other Decisions

- ADR-032: revised, narrowly and only for the opt-in cache path. The default
  serving path still re-reads from disk on every call; the cache path replaces
  the per-call re-read with event-sourced detection whose completed-writes-are-
  visible guarantee equals ADR-032's, with the residual race proven equal to a
  fresh walk's.
- ADR-099: supersedes its per-call full re-hash key recomputation. Content
  addressing, byte-parity to the fresh path, and disposability are preserved; only
  the re-hash-every-call mechanism is replaced.
- ADR-104: this decision populates the base + delta fold seam ADR-104 built and
  deferred here. The memory-mapped base is the compaction target; the delta window
  is the changed set the tracker maintains.
- ADR-103: the read-model the tracker keeps fresh is the unified `DerivedIndex`;
  its shape and schema version are unchanged, so freshness changes the supply, not
  the bundle.
- ADR-080: the store and its delta stay disposable and never authoritative; a
  corrupt, unwritable, or unreadable store degrades to a fresh build, so enabling
  the tracker can change only latency, never an answer.
- ADR-002: content confirmation on every flagged path keeps the freshness key a
  pure function of the corpus bytes, byte-identical across runs and platforms.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The request-boundary detection ladder and its authoritative fallback are directly testable."
rules:
  - id: tracker-keeps-bounded-stable-scan-barrier
    kind: require_pattern
    path_glob: "rust/rac-engine/src/freshness.rs"
    pattern: '(?s)for _ in 0\.\.3.*prepare_scan\(\).*acknowledge_if_stable'
    message: "Freshness detection must retain its bounded scan-and-event barrier."
  - id: tracker-keeps-stat-and-verify-floor
    kind: require_pattern
    path_glob: "rust/rac-engine/tests/freshness_tracker.rs"
    pattern: 'stat_fallback_and_verify_never_trust_clean_events'
    message: "The freshness suite must retain the authoritative stat and verify fallback contract."
```
