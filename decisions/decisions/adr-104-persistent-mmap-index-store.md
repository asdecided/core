---
schema_version: 1
id: RAC-KWS7QCT10Q5A
type: decision
---
# ADR-104: Persistent Memory-Mapped Index Store

## Context

ADR-099 gave the derived read-model a content-addressed cache, and ADR-103
unified every serving-path structure behind one composer. Both kept ADR-099's
on-disk representation: a single `{corpus_hash}.json` document, `json.dumps`d on a
miss and rehydrated whole on every hit. That representation has a measured cost
the `rebuild-scale` performance work isolates. A warm cache hit still pays an
O(N) whole-corpus rehydration — `json.loads` of a multi-megabyte string plus
reconstruction of every index row, every relationship, and every field's token
vector — even to answer a point query that needs one artifact. The transient
peak (source string, parsed dict, and dataclasses co-resident during
rehydration) is the ~22 MB/1k per-call allocation spike that walls the server at
scale on a fixed-memory node, and it scales with the corpus, not the query.

The performance lens's answer is a persistent, memory-mapped index: an
immutable directory of binary segment files, mapped and read by point access, so
`get_artifact` and `get_related` touch only the identity rows they need and
`search` — Θ(N) by contract (ADR-078) — reads field vectors from mapped pages
rather than from a JSON blob co-resident in the heap. The same lens requires this
substrate to be *mutable* in a later bundle (an in-memory delta overlay folded
over the immutable base) so an event-sourced freshness model can absorb an
O(changed) update without re-deriving the corpus; that freshness decision is
recorded separately (ADR-105). This decision records the substrate itself and the
constraints it must satisfy, so mutation can be added later over a format that was
built to fold.

The quality lens fixes seven properties any persistent index must hold to avoid
becoming a poisoning or staleness surface (ADR-065, ADR-002, ADR-032). They are
accepted here as constraints on the format, not aspirations.

## Decision

The derived read-model's on-disk representation is a **persistent,
memory-mapped, content-addressed index store**: a directory per corpus content
hash, holding versioned binary segment files — a sorted term dictionary with
binary-searched prefix ranges (ADR-037: a query term matches every indexed term
it prefixes, and both document frequency and term frequency flow from that one
range), per-document forward token vectors, integer global accumulators (per-field
Σ token count and the live-document count, from which `avglen` is one division per
query — never a stored float), the identity and alias rows, the resolved
relationship edges, the per-live-decision scope rows and the portfolio summary
ADR-103 added, and the searchable section text. A positional docid indexes the
segments compactly, but document identity and the search tie-break are the
artifact's real path string, exactly as a fresh walk uses it (ADR-078) — never the
positional id. This store **replaces ADR-099's serialized-blob representation**:
the JSON document is retired as the cache's data form.

Every read goes through a **base + delta fold**: an immutable mapped base
combined with a small in-memory delta overlay under `live = (base − tombstones) ∪
delta`. In this bundle the delta is always empty — the base is the whole answer —
but the read API is the fold, not the reader, and the seams (tombstones, added
rows, integer stat adjustments, term deltas) are declared, so the freshness
decision (ADR-105) can populate the delta without touching a single consumer. The
consuming decision that this bundle does not make — how the delta is kept fresh —
is deferred to ADR-105 by construction.

This **extends ADR-099 and supersedes only its serialized-blob mechanism**;
ADR-099's non-negotiables carry forward unchanged and are the store's acceptance
criteria:

- **Content addressing doubles as the integrity check.** The store lives under a
  directory named by the corpus content hash; a mismatched or hand-forged store
  cannot be served for the wrong corpus, because the key is the checksum. Any
  byte change, add, remove, or rename changes the key and forces a rebuild.
- **The schema and scoring-constant gates fail closed.** A segment carries a magic
  number and a format version; the header echoes the bundle schema version, the
  scoring-constant fingerprint, and the corpus hash. Any mismatch — wrong version,
  a changed field boost or BM25 constant, a hash that does not match — is rejected
  on open and rebuilt, never partially parsed.
- **No code-bearing deserialisation.** The format is fixed length-prefixed binary
  read by struct with bounds checks — no `pickle`, `eval`, `marshal`, or
  `yaml.load` anywhere in the read path. The one leaf that survives as JSON is the
  portfolio summary, which is itself a JSON wire payload decoded as data. A
  hostile or truncated file can at worst raise a miss, never execute.
- **A miss or corruption is never fatal.** A missing directory, a truncated or
  corrupt segment, a wrong version, or a hash mismatch degrades to a fresh build;
  the error never reaches a tool caller. Enabling the store can only change
  latency, not answers.
- **Writes are atomic.** Segments are built in a temporary directory, fsynced, and
  `os.replace`d into the content-addressed name in one step, so a concurrent
  reader never observes a half-written store.
- **Byte-parity is asserted against a fresh build, not a self round-trip.** The
  store's materialised read-model equals `build_derived_index` for every corpus
  state and every tool — the serialisation-drift trap (a change that preserves
  structural equality but alters rendered output) is caught because the oracle is a
  fresh cold build, never the store's own encode/decode.
- **Rehydrated strings are data, never filesystem targets.** The store never opens
  a stored path; path strings flow only to resolution, ranking, and display.

Freshness is untouched (ADR-032/ADR-099): the corpus content hash is still the
key, recomputed on every call, so no call can observe stale state, and the store
is opt-in and off by default (ADR-099). The store is disposable and never
authoritative (ADR-080): the files in git are the truth, and deleting the cache
directory costs only latency.

## Consequences

The whole-corpus JSON rehydration is retired. On a warm hit, `get_artifact` and
`get_related` read only the identity rows they need from mapped pages and never
materialise the section text or token vectors; `search`, still Θ(N) by contract,
reconstructs its field vectors from mapped pages rather than from a JSON string
and dict held live in the heap. The per-call peak-allocation spike ADR-099 paid is
removed — this bundle's own attributable win — while every served byte stays
identical to the fresh path.

The format is built to fold before it needs to. Carrying the empty-delta base
through the fold API now means the freshness decision (ADR-105) adds mutation as a
delta overlay without reopening any consumer — the ordering objection that a later
bundle cannot make mutable a format that was never built to fold is resolved here.

The trade-offs are accepted on the record. There are now two on-disk artifacts per
corpus state — a small schema-gate marker and the segment directory — and the
serialisation surface that must round-trip losslessly is larger and binary rather
than a single JSON document; both are held honest by the parity-against-fresh-build
battery. Stores for superseded corpus states are not garbage-collected in this
bundle; they are disposable and bounded by the cache directory, and a later bundle
may prune them. Of the two structures the format anticipated, one is now built and
one stays deferred. Term-major postings — term id → the docids holding it in any
field — are added by the postings-served-search bundle once the flat line (ADR-105)
made a store-fed search path worth its keep; the earlier persistence bundle rightly
did not build them, because its serving path re-hashed the whole corpus per call and
so gained nothing from selective postings. Per-file validation blobs remain deferred
to the incremental-validate bundle that computes them. The store persists the sorted
term dictionary, the per-document forward token index, and the term-major postings,
which together support the prefix-range df and tf mechanism, byte-identical
field-vector reconstruction, and O(matches) candidate discovery.

### Postings-served search

When the cached read-model is served from the memory-mapped base — the delta empty,
the corpus unchanged since the base was written or last compacted — a search reads
only the query terms' binary-searched prefix ranges in the term dictionary, the
term-major postings rows those ranges cover, and the identity/section/token rows of
the docs that match at least one term. Non-matching docs contribute solely through
the global integer accumulators the header already carries (n and the per-field Σ
token counts; avglen is one division per query) and the postings-derived df, so
their rows are never touched. Candidate discovery is the union of the query terms'
prefix-range postings; `resolve._match_entry` then applies the AND predicate and
snippet selection per candidate, and the shared BM25F+RRF scoring code ranks them —
the same terms × fields summation order, the same `(-round(fused, 12), path)` sort,
the same `match_count` over every match — so the served bytes equal a fresh
whole-corpus walk's. When the delta is non-empty the read-model is the re-derived
in-memory snapshot and search is the whole-corpus scan over it (correct, already
resident); the postings fast path is taken only in the delta-empty base-served
state, the common unchanged-corpus case. Adding the postings segment bumps the
segment format version, so a store written before it fails the version gate closed
and is rebuilt — no half-old layout is ever read.

The risks are ADR-099's, extended to a binary format. A serialisation change could
silently rehydrate a stale shape — mitigated by the segment format version and the
header's bundle-schema and scoring-constant gates, each failing closed to a
rebuild, and by the round-trip-against-fresh-build parity assertion. A corrupt
store could serve wrong bytes — mitigated by the content-addressed directory name
(a store under a hash it no longer matches is rejected) and by structural bounds
checks that turn truncation or a bad offset into a miss; the residual (an
in-bounds byte flip under a still-matching hash) requires local write access to the
cache directory, ADR-065's out-of-scope local trust boundary, exactly as the JSON
cache's was.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

### Keep the serialized JSON blob (ADR-099 unchanged)

Leave the derived bundle as one `{corpus_hash}.json` document. This keeps the O(N)
whole-corpus rehydration and its per-call peak-allocation spike on every warm hit,
the exact cost the performance work exists to remove, and it offers no mutable
substrate for the event-sourced freshness a later bundle needs — a format that
cannot fold cannot later be made incremental without a second rewrite.

### A binary blob rehydrated whole (no point access)

Replace JSON with a compact binary encoding but still parse the whole file into
memory on open. This removes the JSON parse cost but keeps the O(N) rehydration and
the RSS wall for point queries, and still offers no fold seam. Point access and the
delta seam are the reasons to map segments rather than parse a blob.

### An embedded key-value or SQL datastore

Hold the derived index in SQLite or a similar embedded store. This crosses ADR-080's
red line — a second authoritative representation with its own transactional and
schema semantics to reconcile with git — for a capability the mapped segment
directory already provides: point reads, prefix ranges, and disposability, with no
datastore to keep coherent and no dependency added (stdlib `mmap`/`struct` only).

### Persist term-major postings and per-file validation now

Build the full inverted index (term-major postings for selective queries) and the
per-file validation blobs in this bundle. This bundle's serving path is O(N)
because freshness is unchanged, so selective postings deliver no win here, and the
per-file validation blobs belong to the incremental-validate bundle that computes
them. Building them now is unattributable work ahead of the bundle that needs it;
the sorted term dictionary and forward token index this bundle does persist are
sufficient for its prefix-range mechanism and its byte-parity reconstruction.

## Relationship to Other Decisions

- ADR-099: this decision extends the derived-index cache and supersedes its
  serialized-blob representation, replacing the rehydrated JSON document with a
  memory-mapped segment store while preserving every one of its pins unchanged —
  content addressing, byte-parity to the fresh path, disposability, and the opt-in,
  off-by-default posture.
- ADR-103: the unified read-model bundle (portfolio summary and per-decision scope
  rows) is what the store persists; its shape and schema version are unchanged, so
  the store is a new encoding of the same bundle, not a new bundle.
- ADR-105: the base + delta fold seam this decision builds is the substrate the
  freshness decision populates; how the delta is kept fresh is deferred there, not
  decided here.
- ADR-032: not superseded. Freshness is unchanged — the corpus content hash is
  still recomputed every call, no call can observe stale state, and the default
  serving path is still a fresh build per call.
- ADR-037: the sorted term dictionary's binary-searched prefix ranges implement the
  token-boundary prefix-matching predicate for both document and term frequency, so
  the store's statistics are byte-identical to a walk's.
- ADR-078: scoring is unchanged and byte-identical; the store supplies the scorer's
  integer inputs (df, tf, per-field lengths, Σ, n) in the same iteration order, and
  the artifact path string remains the identity and the final tie-break.
- ADR-065: artifact content stays untrusted; the store adds no new injection
  surface — no code-bearing deserialisation, and stored strings are never used as
  filesystem targets.
- ADR-080: the store is the rebuildable derived index behind the snapshot seam —
  disposable, never authoritative; deleting it costs only latency.
- ADR-002: content addressing and lossless, deterministic serialisation keep the
  store byte-identical across runs and platforms.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The read-only mapped-store dependency and fail-closed corruption battery are deterministic repository properties."
rules:
  - id: index-store-remains-memory-mapped
    kind: require_pattern
    path_glob: "rust/rac-engine/src/index_store.rs"
    pattern: 'use memmap2::Mmap;'
    message: "The persistent native index must retain its memory-mapped read path."
  - id: index-store-keeps-fail-closed-open-tests
    kind: require_pattern
    path_glob: "rust/rac-engine/tests/index_store_vectors.rs"
    pattern: '(?s)open_store\(cache_dir, "0"\.repeat\(64\).*open_store\(cache_dir, corpus_hash, SCHEMA_VERSION\)\.is_some\(\)'
    message: "The store suite must continue to reject invalid stores before accepting a valid one."
```
