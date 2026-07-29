---
schema_version: 1
id: RAC-P61BA5EDE7A0
type: decision
tags: [performance, freshness, rust, incremental]
---
# ADR-119: Base-Plus-Delta Serving Generations

## Status

Accepted

## Context

The native freshness tracker parses changed files incrementally, but it still
rebuilds every derived structure for each change. After compaction it also
sheds the parsed snapshot, so the first later edit reparses the entire corpus.
Those choices cap resident memory, but make mutation latency scale with corpus
size even when one file changed.

P6 needs a stable correctness boundary before search postings, graph indexes,
scope rows, and summaries can become incremental independently. Without that
boundary, partially updated structures could leak into serving or disagree
about which corpus generation they represent.

## Decision

The native tracker adopts an immutable base-plus-delta generation model:

`live documents = (base documents - tombstoned base docids) + delta upserts`

- A generation contains one immutable base, cumulative upserts, cumulative
  tombstones, a base generation number, a serving generation number, and a
  canonical sorted changed-path set.
- A rename is a tombstone for the old path plus an upsert for the new path.
- A candidate generation is staged away from the served generation. It is
  published only after every structure required by that slice has completed.
- Canonical manifest order remains docid order and therefore preserves the
  existing path tie-breaks and byte-parity contract.
- Compaction writes a complete replacement base, opens it successfully, then
  atomically promotes the overlay. Failure keeps serving the prior complete
  in-memory generation; incomplete incremental parsing falls back to a fresh
  full parse and derive.
- The parsed base remains resident after P6 compaction, removing snapshot shed
  from the P6 path so the first later edit stays change-bound.

P6 is delivered in referee-gated slices. P6.1 implements the document overlay
and generation publication boundary behind an explicit preview constructor. It
still derives the complete read model from live documents and is not used by
the default CLI or MCP server. Later slices may incrementally maintain:

1. identity, path, aliases, status, and point resolution;
2. token vectors, postings, and exact global search statistics;
3. graph edges, adjacency, and inbound counts;
4. scope rows, live-decision state, and portfolio summary counters; and
5. durable compaction without snapshot shedding.

P6.2 implements item 1 behind the same preview boundary. Identity and status
rows are shared across generations, changed rows and tombstones are staged in
the overlay, and exact point resolution reads the published identity generation.
The complete derived model remains a referee and the default serving path is
still unchanged.

P6.3 implements item 2. Token rows and compacted postings are immutable shared
bases; changed rows and tombstones form the search overlay. Candidate discovery,
filters, field vectors, and exact corpus-global BM25 statistics read that
overlay.

P6.4 implements item 3. Validation rows, resolved edge lists, reverse raw-target
buckets, and inbound-count bases are immutable and shared. Source changes
replace only that source's rows and edges. Identity or alias changes use the
raw-target buckets to re-resolve the otherwise unchanged sources that mention
an affected identifier. Inbound counts are adjusted by subtracting replaced
edges and adding their replacements. Preview graph reads and search graph
scoring now consume this generation rather than the complete referee.

P6.5 implements item 4. Live-decision membership and declared scope rows use
immutable bases with changed-row overlays. Portfolio parsing, structural
validation, completeness classification, and attention projections are stored
as immutable per-document rows; only changed rows are rebuilt. Exact portfolio
JSON is reduced from those compact rows at publication/read time because
relationship integrity, orphan counts, global ordering, and health scoring are
corpus-global. Preview scope lookup, topic-mode live-decision filtering, and
summary reads consume these generations rather than the complete referee.

P6.6 implements item 5. The served delta generation no longer owns or builds a
fresh whole-corpus `DerivedIndex`. Compaction assembles the persisted bundle
directly from the published search rows/field vectors, graph edges and inbound
counts, scope/live-decision rows, and validated portfolio projections. Fresh
whole-corpus derivation remains only in bounded certification tests. Failed
writes or failed reopen still leave the complete in-memory delta generation
served unchanged.

P6.7 certifies a recommended production envelope of 5,000 artifacts. It does
not impose a hard limit: larger corpora remain available without the S1
interactive-latency promise. Scale releases promote measured tiers at 10,000,
25,000, 50,000, and 100,000 artifacts as demand or reusable architecture
justifies them. The measured gates and commands are recorded in
`rust/P6-SCALE-CERTIFICATION.md`.

No slice becomes the default until mutation referees show byte-identical output
against a fresh whole-corpus rebuild and scale tests show bounded regression.

## Consequences

P6.1 makes the publication and lifecycle rules executable without changing
production behavior. Add, edit, delete, rename, compaction, and first-edit-after-
compaction tests compare the preview's persisted segments byte-for-byte with a
fresh derivation.

P6.2 adds identity/status and exact-resolution referees for those mutation
classes, including casefolded aliases and duplicate identities. Staging shares
the compacted base maps and clones only the bounded overlay; compaction reuses
unchanged identity rows.

P6.3 extends the same mutation referee to complete search payloads, including
prefix/AND candidates, duplicate query tokens, type/tag/live filters, snippets,
scores, ranks, and ordering. Staging shares the compacted token and posting
bases and clones only the bounded overlay.

P6.4 extends the referee to canonical resolved-edge payloads and inbound counts
across source edits, deletion, identity changes, unresolved-to-resolved and
resolved-to-ambiguous transitions, and compaction. Staging shares the compacted
row, edge, reverse-target, and inbound-count bases; its work is bounded by the
changed documents plus sources in affected raw-target buckets.

P6.5 extends it to scope rows, live-decision membership, and the complete
portfolio JSON payload across scope/status/type/validity changes, add, delete,
rename, and compaction. Staging shares validated portfolio and scope bases and
rebuilds only changed document projections. The final compact-row portfolio
reduction remains corpus-linear; scale gates must measure it before the full
referee can be removed or preview serving becomes the default.

P6.6 removes that referee from runtime mutation publication. Certification now
serializes a delta-native materialization beside a fresh rebuild and compares
every persisted segment byte-for-byte across edit, delete, rename, compaction,
and first-edit-after-compaction transitions. Below-threshold mutations perform
no full corpus parse, validation, or derived-index build.

The first slice does not claim mutation-latency improvement for derivation: it
still materializes live documents and rebuilds all derived structures. Its
performance win is limited to keeping parsing change-bound after compaction.
The intended latency gains arrive as the later derived overlays replace that
full referee one structure family at a time.

Keeping the parsed base resident trades memory for predictable mutation
latency. Compaction thresholds and scale measurements must bound both the
overlay lookup cost and the retained memory before default adoption.

P6.7 closes latency certification for the 5,000-artifact S1 envelope. Default
adoption requires S1 peak-RSS evidence and a bounded lifecycle soak; it does
not require satisfying speculative 100,000-artifact latency targets. Higher
tiers must repeat the same correctness, lifecycle, and memory evidence before
their performance promises are published.

P6.8 completes S1 adoption. The complete lifecycle peaked at 593 MiB RSS for
delta versus 466 MiB for snapshot, passing the S1 limits of 768 MiB and 1.5
times snapshot. A three-round release-mode soak performed 100 unchanged reads
and 21 certified lifecycle transitions with zero validity, determinism,
freshness, cache/no-cache, or persisted-segment divergence. The repeated valid
corpus matrix measured 17.60 ms warm p95, 104.76-140.08 ms mutation p95, and
1.44 s threshold compaction versus 1.50 s for snapshot. The normal constructor
therefore selects delta; `new_snapshot` remains the explicit rollback path for
the initial soak release.

## Alternatives Considered

### Mutate the currently served structures in place

Rejected. A failure could expose a generation where identity, search, graph,
and summary disagree.

### Make P6.1 the default immediately

Rejected. The first slice establishes architecture and correctness but still
pays full derivation cost and retains more memory.

### Keep snapshot shedding and reparse after every compaction

Rejected for the P6 path. It makes the first edit after compaction depend on
total corpus size, contradicting the change-bound target.

## Related Decisions

- ADR-099
- ADR-103
- ADR-104
- ADR-105
- ADR-107
- ADR-116
- ADR-118

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The production constructor, immutable generation type, and lifecycle parity test make the publication boundary enforceable."
rules:
  - id: production-tracker-defaults-to-delta
    kind: require_pattern
    path_glob: "rust/rac-engine/src/freshness.rs"
    pattern: '(?s)pub fn new\(.*Self::new_delta\(cache_dir, root, threshold, true\)'
    message: "The production tracker must continue to select base-plus-delta serving."
  - id: delta-lifecycle-keeps-parity-coverage
    kind: require_pattern
    path_glob: "rust/rac-engine/tests/freshness_tracker.rs"
    pattern: 'default_delta_stages_mutations_compacts_and_keeps_parsed_base'
    message: "The delta lifecycle must remain covered across mutation, compaction, and retained-base behavior."
```
