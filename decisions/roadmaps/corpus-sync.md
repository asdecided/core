---
schema_version: 1
id: RAC-KWJ4VCFJ9Y46
type: roadmap
---
# Corpus Sync Programme

## Status

Planned

Extends Tranche B of the deterministic-substrate programme: where
`corpus-export-to-rag-backends` gave the corpus its consumption projections,
this programme makes those projections a versioned consumption plane —
schema-checked, addressable at any revision, syncable by delta, chunkable at
recorded boundaries, and collision-free across corpora. Execution is tracked
in GitHub per ADR-093: the epic in `## Related Tickets` carries ordering
and task state, with a sub-issue per initiative.

## Context

RAC already emits three deterministic projections — the viewer JSON, the
`--documents` JSONL for memory and RAG backends, and the `--graph` typed
nodes-and-edges projection (ADR-074) — and downstream adapters consume them
one-way per ADR-073. That surface is a snapshot exporter: correct, but
missing what an enterprise RAG platform needs to *build on* the corpus
rather than merely read it once. Five gaps, none recorded elsewhere in this
corpus:

- No machine-readable contract. ADR-007 promises additive stability, but no
  schema artifact exists for any projection; `--documents` and `--graph` are
  absent even from the prose viewer contract. A platform cannot validate an
  export without reverse-engineering the emitting code.
- No point-in-time consumption. Watchkeeper materialises any revision
  read-only through the ADR-043 seam, yet export cannot use it: there is no
  way to reproduce the corpus's projections as of a named commit.
- No incremental feed. Every downstream sync is a full re-export and a full
  re-embed, however small the change between two revisions.
- No chunk boundaries. The artifact is atomic and RAC never chunks
  (ADR-004, ADR-010) — but consumers do chunk, today at arbitrary
  boundaries the corpus cannot see or make deterministic.
- No multi-corpus identity. Every repository exports as the same directory
  basename source value, so aggregating two corpora into one backend
  collides immediately, even though artifact ids already carry a per-repo
  key prefix (ADR-026).

Closing these five gaps is one coherent capability: deterministic corpus
sync — the versioned consumption plane underneath RAG graphs and knowledge
maps, delivered without adding inference, state, or a network surface to
the engine.

## Outcomes

- A platform validates every RAC export against a published JSON Schema in
  its own CI, so ADR-007's additive promise becomes checkable instead of
  asserted.
- Any historical corpus state is reproducible from a commit SHA: the
  projections become a pure function of (repository content at revision,
  corpus path, mode).
- A backend re-embeds only what changed between two SHAs, with a cursor the
  consumer owns; RAC persists nothing (ADR-080).
- Consumers chunk at recorded, deterministic section boundaries while the
  artifact remains the atomic knowledge unit — RAC records anchors, never
  chunks (ADR-004, ADR-010).
- N corpora merge into one backend with zero identity collisions, without
  federation semantics entering the engine (ADR-089 stays deferred).
- The claims above are evidenced: exports are schema-checked, byte-stable
  across clones, and the grounding eval discriminates instead of scoring a
  perfect 1.0 on twelve queries.

## Initiatives

### Export contract schemas (`rac-export-contract-schemas`)

Ship JSON Schema files for the three projections as packaged resources,
expose them via `rac export --schema <mode>`, validate every golden and
dogfood export against them in CI with a bidirectional drift guard, and
publish a contracts page reconciled with the viewer contract. Turns the
ADR-007 contract from asserted into machine-checkable — the cheapest,
highest-leverage enterprise consumability win in the programme.

### Point-in-time export (`rac-point-in-time-export`)

`rac export --at <rev>` for the three JSON payload modes, composing the
ADR-043 revision-materialisation seam exactly as watchkeeper does. Output is
a pure function of the repository content at the revision — byte-identical
across runs, working directories, and clones — with paths and corpus
identity derived from the requested directory, never the materialisation
location.

### Incremental change feed (`rac-export-change-feed`)

`rac export --documents|--graph --since <rev>` emits added, modified, and
removed records — and edge deltas for the graph — between a base revision
and the working tree or a second revision, keyed on canonical id, with
resolved-SHA cursor metadata. The replay law is CI-asserted: applying the
feed to the base export reproduces the head export byte-for-byte. The
consumer owns the cursor; RAC persists no sync state (ADR-080).

### Section anchors and ingest filters (`rac-export-section-anchors`)

Additive `metadata.sections` anchors on documents records — heading, level,
ordinal, slug, and exact offsets into the record's text — so consumers
chunk at recorded deterministic boundaries while the artifact stays the
atomic unit (ADR-004, ADR-010). Settles two open questions from the export
shape design additively: a `--live-only` ingest filter and outgoing typed
edges in documents metadata.

### Multi-corpus source identity (`rac-export-source-identity`)

One shared derivation for corpus identity across all three projections —
explicit configuration first, repository key next, directory basename as
the compatibility fallback — plus a documented consumer-side aggregation
recipe keyed on `(source, id)`. Explicitly not federation: no inheritance,
no cross-corpus resolution or validation in the engine (ADR-089 untouched).

### Scale and retrieval evidence

A deterministic synthetic large-corpus fixture with a documented
performance floor for export and find; expansion of the ADR-066 grounding
eval from twelve queries to a discriminating set with hard negatives; and
optional `rac find` pagination whose defaults keep the golden outputs
byte-identical. In-repo scope stays inside the grounding eval — the
per-tool benchmark families remain external per ADR-097. This initiative
links the existing `rac-grounding-eval-benchmark` requirement and the
`retrieval-diagnostics` and `external-benchmark-evidence` future items
rather than duplicating them.

## Constraints

- Deterministic and offline throughout: no embeddings, vectors, LLM calls,
  or network in any scored or served path (ADR-002, ADR-066).
- Every projection change is additive within its `schema_version`; removing
  or retyping a consumer-visible field is a breaking change requiring a
  version bump (ADR-007, ADR-063).
- The artifact remains the atomic knowledge unit: anchors are metadata, and
  RAC never emits chunk records (ADR-004, ADR-010).
- Git enters only through the existing revision-materialisation seam,
  read-only, never mutating `.git` (ADR-043).
- No datastore and no persisted sync state: files-in-git stay canonical and
  the consumer owns its cursor (ADR-080).
- The MCP surface is unchanged: point-in-time and feed capabilities are
  CLI-only until a decision revisits stateless reads (ADR-032, ADR-033).
- Enterprise posture is configuration plus published contracts, never an
  operating mode (ADR-085); adapters and collectors consuming these
  contracts live in the connector and CI repos (ADR-073, ADR-092, ADR-095).
- Outputs remain file-first and stdout-pipeable (ADR-011); release framing
  is CalVer (ADR-076).

## Non-Goals

- Corpus federation, `## inherits`, or any cross-corpus resolution in the
  engine — deferred and sequenced last per ADR-089.
- Chunk emission or per-section export records (ADR-004, ADR-010).
- Embeddings, semantic scoring, or LLM judging anywhere (ADR-002, ADR-066).
- MCP server-side caching, push sync, webhooks, or any serving-path change
  (ADR-032); RAC storing or re-importing a backend's copy (ADR-024).
- Any new database or state file (ADR-080).
- SSO, RBAC, or a hosted multi-tenant service (ADR-085, ADR-086).
- Adapter implementations — reference connectors are rac-connectors' scope
  (ADR-073, ADR-095).

## Success Measures

- Every export mode is schema-validated in CI, and a drift test fails when
  the emitted shape and the packaged schema diverge in either direction.
- `--at` and `--since` outputs are byte-stable across runs and clones of the
  same commit, asserted by test; an `--at HEAD` export over a clean tree is
  byte-identical to the plain export.
- The replay law holds in CI: base export plus feed reproduces the head
  export byte-for-byte.
- A two-corpus merge fixture shows zero `(source, id)` collisions and zero
  bare-id collisions.
- After expansion the grounding eval discriminates: overall mean below 1.0
  with hard negatives present, and a documented floor replaces the
  twelve-query perfect score.
- `rac validate rac/`, `rac relationships rac/ --validate`, and
  `rac review rac/` stay clean across the programme's output.

## Assumptions

- The ADR-043 seam is sufficient for read-only revision materialisation at
  export scale; no new git machinery is needed.
- The documents and graph shapes fixed in `corpus-export-shape-contract`
  remain the wire baseline these features extend additively.
- rac-connectors consumes the feed and anchors without contract changes
  beyond this programme.
- The team-scale trigger has since been met and `lore-at-team-scale` has
  graduated to its own scoped roadmap; this programme still does not pull
  its serving work forward — the two proceed independently.

## Risks

- Schema files drift from emitted shapes. Mitigation: the round-trip CI
  validation is itself the feature; drift fails the build in both
  directions.
- Point-in-time export leaks materialisation paths or a tempdir-derived
  corpus name, breaking byte-parity. Mitigation: parity with the plain
  export is a pinned acceptance criterion, not an implementation detail.
- Changing the default source value surprises consumers keyed on the
  directory basename. Mitigation: explicit-config-first precedence with a
  basename fallback and a documented migration note.
- Anchors tempt chunk-emission scope creep. Mitigation: the Non-Goal and a
  requirement stating RAC records anchors and never emits chunks.
- Eval expansion overfits to dogfood phrasing. Mitigation: hard negatives
  and category balance in the expanded query set.

## Related Decisions

- adr-002
- adr-004
- adr-007
- adr-010
- adr-011
- adr-026
- adr-032
- adr-033
- adr-043
- adr-055
- adr-063
- adr-066
- adr-073
- adr-074
- adr-111
- adr-080
- adr-085
- adr-089
- adr-092
- adr-093
- adr-094
- adr-095
- adr-097

## Related Designs

- corpus-export-shape-contract

## Related Roadmaps

- deterministic-substrate
- corpus-export-to-rag-backends
- lore-supermemory-grounding
- retrieval-diagnostics
- external-benchmark-evidence
- lean-context-delivery

## Related Requirements

- rac-export-contract-schemas
- rac-point-in-time-export
- rac-export-change-feed
- rac-export-section-anchors
- rac-export-source-identity
- rac-corpus-documents-export
- rac-corpus-graph-export
- rac-grounding-eval-benchmark

## Related Tickets

- itsthelore/asdecided-core#255
