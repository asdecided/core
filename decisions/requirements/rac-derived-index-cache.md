---
schema_version: 1
id: RAC-KWJ8S0Z89X5B
type: requirement
---
# Requirement: Derived-Index Cache

## Status

Accepted

Classification: `[internal]` — per-call work stops scaling with corpus
size. Initiative 2 of the `lore-at-team-scale` roadmap: a
content-addressed, rebuild-on-change cache behind the ADR-032 seam,
shipped under its own ADR. Delivered (itsthelore/asdecided-core#264): the
`DerivedIndexCache` persists the repository index, resolved relationship
graph, and tokenised field vectors keyed on `corpus_content_hash`, enabled
opt-in via `rac mcp --cache`, byte-identical to the uncached path — under
ADR-099, which answers ADR-032's review clause and revises its
no-persistent-cache pin.

## Problem

Every tool call rebuilds everything from disk: the corpus walk, the
repository index, and — per query — the search statistics that re-tokenize
every field of every entry. The existing `CorpusCache` is in-memory and
per-invocation, and is deliberately kept off the MCP serving path per
ADR-032. That decision also recorded its own review trigger: a real user
reporting latency at scale. An organisation-scale rollout is that report
arriving. The fix ADR-032 anticipated — reuse behind the corpus-snapshot
seam without breaking the determinism contract — needs a recorded
contract: content-addressed, disposable, and byte-identical to the
uncached path.

## Requirements

- [REQ-001] A derived-index cache MUST persist the expensive derived structures — tokenized field vectors, search statistics, and the relationship graph — keyed on a corpus-level content hash extending the existing per-file `content_hash` primitive, so calls over an unchanged corpus skip re-tokenization and re-indexing.
- [REQ-002] Byte-parity is the coherency guarantee and the recorded entry condition: with the cache enabled, every CLI and MCP output MUST be byte-identical to the uncached path for any corpus state, asserted over golden fixtures in CI (ADR-002, ADR-032).
- [REQ-003] The cache MUST be disposable and never authoritative (ADR-080): deleting it changes nothing but latency; files-in-git stay canonical; no daemon, lockfile protocol, or datastore semantics are introduced.
- [REQ-004] Invalidation MUST be purely content-addressed: any byte change to the corpus changes the key and forces a rebuild; no time-based or event-based invalidation exists.
- [REQ-005] The capability MUST ship under its own ADR that answers ADR-032's recorded review clause and explicitly revises the "not used by the MCP serving path" pin by decision, not code drift; this requirement does not pre-decide that ADR.
- [REQ-006] Freshness MUST hold on the serving path: every call still detects any corpus change since the previous call — re-hashing per call is permitted; derived structures are reused only under an unchanged key.
- [REQ-007] The latency claim MUST be evidenced on a deterministic large-corpus fixture with a recorded before-and-after floor for representative calls, composing with the corpus-sync evidence initiative rather than duplicating it.

## Acceptance Criteria

- Golden-fixture outputs with the cache enabled are byte-identical to the
  uncached path across all CLI modes and MCP tools.
- A one-byte artifact change produces a new key and a rebuild, and the
  next call reflects the change — asserted by a test that interleaves
  edits with calls, mirroring the existing ADR-032 contract tests.
- Deleting the cache directory mid-session causes a transparent rebuild
  with identical output.
- The large-corpus fixture records a latency floor showing repeated
  unchanged-corpus calls skip re-tokenization.
- The implementing ADR exists and is ratified before serving-path
  enablement.

## Success Metrics

- Per-call latency on a large corpus stays agent-tolerable at
  organisation scale, with the measured floor documented rather than
  asserted.

## Risks

- Stale results from wrong invalidation. Mitigation: REQ-004's pure
  content addressing plus REQ-002's byte-parity assertion — staleness
  cannot survive both.
- The cache drifts into a datastore. Mitigation: REQ-003 pins
  disposability; ADR-080's "files are truth, the index is disposable" is
  the recorded frame.

## Assumptions

- Corpus-level hashing per call is cheap relative to parse, tokenize, and
  index work — the seam's original premise.
- The cache ADR is authored and ratified before serving-path enablement;
  the roadmap schedules it.

## Related Decisions

- adr-002
- adr-032
- adr-066
- adr-080
- adr-099

## Related Roadmaps

- lore-at-team-scale
- corpus-sync

## Related Requirements

- rac-mcp-http-transport
