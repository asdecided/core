---
schema_version: 1
id: RAC-KWJ4VHB2GRGA
type: requirement
---
# Requirement: Incremental Export Change Feed

## Status

Proposed

Classification: `[internal]` — re-embed only what changed. Feature C of
the `corpus-sync` programme: a deterministic change feed between two corpus
states, keyed on canonical id, with consumer-owned cursors.

## Problem

Every downstream sync today is a full re-export and a full re-embed,
however small the change between two corpus states. Watchkeeper computes
base-to-head deltas internally but exposes them as review findings, not as
consumption-shaped records. A RAG backend keeping an index current needs an
incremental feed: which artifacts were added, modified, or removed between
a revision it has already ingested and the current state, in the exact
record shape it already consumes, with a cursor it can resume from.

## Requirements

- [REQ-001] An additive `rac export --documents --since <rev>` mode MUST emit a JSONL change feed between the corpus at revision `<rev>`, materialised through the existing revision seam (ADR-043), and the working tree — or a second revision when combined with `--at <rev2>`, making the feed fully CI-reproducible commit-to-commit.
- [REQ-002] Each documents feed record MUST carry the projection's `schema_version` and a `change` field taking `added`, `modified`, or `removed`; `added` and `modified` records MUST embed the full document record in the exact `--documents` shape so the published record schema applies unchanged, and `removed` records MUST carry the canonical `id` and last-known path.
- [REQ-003] `rac export --graph --since <rev>` MUST emit a single JSON object with `nodes_added`, `nodes_modified`, `nodes_removed`, `edges_added`, and `edges_removed` arrays, node entries in the graph node shape and edge entries in the graph edge shape, all deterministically sorted (ADR-002, ADR-007).
- [REQ-004] The feed MUST carry cursor metadata: `base` as the fully resolved commit SHA of `<rev>`, and `head` as the resolved SHA when `--at` is given or an explicit working-tree marker otherwise; RAC MUST NOT persist any cursor or sync state — the consumer owns resumption (ADR-080).
- [REQ-005] Change detection MUST key on the canonical artifact id (ADR-026): present only in head is `added`, only in base is `removed`, in both with any differing serialised record content is `modified` — a file move that preserves the id is `modified`, never `removed` plus `added`.
- [REQ-006] The replay law MUST hold and be asserted in CI: applying the feed to the base export — dropping `removed` and `modified` ids, appending `added` and `modified` records, re-sorting — reproduces the head export byte-identically.
- [REQ-007] An empty delta MUST produce an empty feed with exit code 0 as a valid outcome, and the capability MUST remain offline, timestamp-free, and CLI-only; no MCP tool exposes the feed without a decision revisiting stateless reads (ADR-002, ADR-032).

## Acceptance Criteria

- In a fixture repository where commit B adds one artifact, edits one, and
  deletes one relative to commit A, `rac export --documents --since <A>
  --at <B>` emits exactly three records, one per change kind, byte-identical
  across runs and clones.
- The replay-law test passes: the base documents export plus the feed
  equals the head documents export, byte-for-byte.
- `--since HEAD` on a clean tree emits zero records and exits 0; `base` and
  `head` in the output are full commit SHAs when both sides are commits.
- A `git mv` of an artifact between commits yields a single `modified`
  record whose path metadata is the new path.
- Feed records validate against the published documents record schema,
  composing with the export contract schemas.

## Success Metrics

- A backend syncing a large corpus after a one-artifact change re-embeds
  one document instead of the whole corpus, verified against the feed.

## Risks

- Rename detection diverges between the feed and consumer expectations.
  Mitigation: REQ-005 defines change identity on the canonical id, which is
  rename-stable by construction (ADR-026).
- Feed shape drifts from the snapshot shape and consumers need two parsers.
  Mitigation: REQ-002 embeds the unmodified record shape, so one schema
  covers both.

## Assumptions

- Point-in-time export (`rac-point-in-time-export`) lands first; the feed
  composes two materialised states rather than inventing new git access.
- Byte-level record comparison is an acceptable modification signal; no
  semantic diffing is wanted in the feed (ADR-002).

## Related Decisions

- adr-002
- adr-007
- adr-011
- adr-026
- adr-032
- adr-043
- adr-080

## Related Designs

- corpus-export-shape-contract

## Related Roadmaps

- corpus-sync

## Related Requirements

- rac-point-in-time-export
- export-contract-schemas
