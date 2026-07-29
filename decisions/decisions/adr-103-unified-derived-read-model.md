---
schema_version: 1
id: RAC-KWS4Y9KCTD90
type: decision
---
# ADR-103: Unified Derived Read-Model

## Context

ADR-099 introduced the content-addressed derived-index cache: the expensive
derived structures (the repository index, the resolved relationship graph, and
the tokenised BM25 field vectors) built once behind the corpus-snapshot seam and
reused, byte-identically, under an unchanged corpus content hash. The cache is
wired into the MCP tools `get_artifact`, `search_artifacts`, `get_related`, and
the topic mode of `find_decisions`, which all reach the same `DerivedIndex`.

Two served surfaces bypass it. `get_summary` calls `build_portfolio_summary`
directly, re-walking and re-validating the whole corpus on every call — the
heaviest read tool, and the cache gives it nothing. `find_decisions` in path
mode returns `decisions_for_path`, which does its own fresh `walk_corpus` and
per-entry relationship extraction before the cache branch is ever consulted. So
the same corpus is walked, parsed, and derived through three construction paths
per session — the cached `DerivedIndex`, the portfolio walk, and the scope walk —
and only one of them is content-addressed.

This is a construction-path fragmentation, not a freshness or contract problem.
The portfolio summary and the path-mode scope answer are pure functions of the
same corpus snapshot the `DerivedIndex` already captures; they are simply
computed on their own separate walks. A read-model that carries what those two
surfaces need lets every serving-path derived structure build through one
composer and inherit ADR-099's cache — without changing a single output byte and
without touching the freshness model.

## Decision

The `DerivedIndex` (`services/derived_cache.py`) is the canonical serving-path
read-model, and every derived structure a read tool serves builds through the
one composer that produces it (`build_derived_index`). The cached bundle ADR-099
defined is extended to carry the two structures the bypassing surfaces need:

- the **portfolio summary** the `get_summary` tool serves, computed once through
  the existing `portfolio_from_corpus` from-parts seam over the same walked
  snapshot, and
- the **per-decision scope rows** the path mode of `find_decisions` serves — for
  each live decision, its identity and its declared `## Applies To` entries — so
  the path-to-decisions answer is matched over precomputed rows instead of a
  fresh walk.

Both `get_summary` and `find_decisions` path mode are wired through the same
read-model build in both cache modes: with the cache enabled they reuse the
content-addressed bundle; with the cache disabled (the ADR-032 default) they
build the read-model fresh per call. The served bytes are identical either way,
and identical to the pre-existing `build_portfolio_summary` and
`decisions_for_path` output — including the portfolio health score's Python
banker's-rounding boundary and path mode's distinct payload shape (no `filter`
key).

Extending the cache's reach changes the shape of the cached bundle on disk, so
the derived-cache `SCHEMA_VERSION` is bumped. A cache file written under the old
version fails the version gate and is treated as a miss — rebuilt fresh, never
rehydrated into the new shape — exactly the pinned-schema discipline ADR-099 and
ADR-007 already require.

This decision extends ADR-099; it does not revise it. ADR-099's non-negotiables
carry forward unchanged: content addressing on the corpus content hash (any byte,
add, remove, or rename changes the key), byte-parity to the uncached path as the
coherency guarantee, disposability (the bundle is a rebuildable index, never
authoritative; a corrupt, unreadable, or wrong-version file degrades to a fresh
build), and the opt-in, off-by-default posture. It does not supersede ADR-032:
freshness is unchanged. The key is still recomputed every call, no call can
observe stale state, and the default serving path is still a fresh build per
call. Only the number of construction paths changes — from three to one.

## Consequences

One construction path now serves the read-model. `get_summary` and
`find_decisions` path mode stop re-walking the corpus on their own and reach the
same content-addressed bundle every other tool uses. `get_summary` — previously
uncacheable and the heaviest tool, a full re-validate per call — becomes
cache-backed: on an unchanged corpus its validation and relationship work is
reused, not repeated. The cache's coherency guarantee now covers two more
surfaces: byte-parity cache-on versus cache-off is asserted for `get_summary`
and path mode across a mid-session corpus edit, closing the two bypasses the
parity battery could not previously reach.

The trade-offs are accepted on the record. The cached bundle carries two more
structures, so it is larger on disk and the serialisation surface that must
round-trip losslessly grows. In the cache-off default, `get_summary` and path
mode now build the full read-model per call rather than only the slice they
consume — marginally more work for the freshness the default preserves; the
cache-on path removes the repetition entirely.

The risks are the two ADR-099 already names, extended. A serialisation change
could silently rehydrate a stale bundle shape — mitigated by the bumped
`SCHEMA_VERSION`, which fails a wrong-version file closed to a rebuild, and by
the round-trip identity and cache-on/cache-off parity assertions that hold the
serialisation honest. The unified path could drift from the legacy
`build_portfolio_summary` / `decisions_for_path` output — mitigated by keeping
both as the reference implementations and pinning byte-parity against them across
a mid-session mutation.

## Alternatives Considered

### Leave `get_summary` and path mode uncached (ADR-099 unchanged)

Keep both surfaces on their own fresh walks. This perpetuates three construction
paths for one snapshot and leaves the heaviest tool paying a full re-validate on
every call, warm or cold — the exact scaling cost ADR-099 was raised to remove,
left unremoved for these two surfaces.

### Cache the walked corpus entries and recompute both answers per call

Store the parsed corpus snapshot in the bundle and re-run the portfolio and scope
computations at serve time. Serialising parsed products is a far larger,
parity-fragile surface than storing the two computed structures, and it would
repeat the validation and scope work on every call rather than reusing it —
losing most of the win.

### A new, separate cache for the two surfaces

Stand up a second content-addressed cache dedicated to the portfolio summary and
scope rows. Two caches keyed on the same corpus hash is redundant machinery and a
second disposability and freshness surface to keep coherent, when one read-model
already captures the snapshot they both derive from.

## Status

Accepted

## Category

Architecture

## Relationship to Other Decisions

- ADR-099: this decision extends the derived-index cache's reach to the two
  serving-path surfaces that bypassed it, bumping the cache schema version, and
  preserves its content-addressing, byte-parity, and disposability pins
  unchanged. It does not revise ADR-099.
- ADR-032: not superseded. Freshness is unchanged — the key is recomputed every
  call and the default serving path is still a fresh build per call; only the
  number of construction paths behind that path changes.
- ADR-007: the change is additive and off by default; no wire contract field
  changes, and the cache schema version is a pinned schema whose bump discards
  stale-shaped files.
- ADR-067: path-mode `find_decisions` keeps its distinct payload shape and its
  live-decision scope semantics; only its data-supply path is unified.
- ADR-002: content addressing and lossless serialisation keep the two added
  structures deterministic and byte-identical across runs and platforms.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The unified serving model and its persisted projections have concrete native source and parity-test anchors."
rules:
  - id: derived-index-keeps-summary-and-scope
    kind: require_pattern
    path_glob: "rust/rac-engine/src/derived.rs"
    pattern: '(?s)pub struct DerivedIndex.*pub portfolio_summary: Value.*pub scope_rows: Vec<ScopeRow>'
    message: "The canonical derived read model must retain portfolio and decision-scope projections."
  - id: mapped-store-keeps-summary-and-scope-parity
    kind: require_pattern
    path_glob: "rust/rac-engine/tests/index_store_vectors.rs"
    pattern: '(?s)reader\.scope_rows\(\).*reader\.portfolio_summary\(\)'
    message: "The mapped-store parity suite must continue to cover both unified projections."
```
