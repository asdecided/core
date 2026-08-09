---
schema_version: 1
id: RAC-KWJ4VK0879KE
type: requirement
---
# Requirement: Chunk-Ready Section Anchors

## Status

Proposed

Classification: `[internal]` — deterministic chunk boundaries without
chunking. Feature D of the `corpus-sync` programme: additive section
anchors on documents records, plus the `--live-only` ingest filter and
outgoing edges in documents metadata.

## Problem

The artifact is the atomic knowledge unit and RAC never chunks it — that is
settled (ADR-004, ADR-010). But downstream consumers do chunk before
embedding, and today they do it blind, at arbitrary boundaries the corpus
cannot see, so two consumers of the same artifact produce different,
irreproducible chunkings. Recording the artifact's section structure as
export metadata gives every consumer the same deterministic boundaries
while RAC stays chunk-agnostic. The same change settles two open questions
recorded in the export shape design: an opt-in live-only ingest filter, and
whether a documents record should know its outgoing typed edges.

## Requirements

- [REQ-001] Documents records MUST gain an additive `metadata.sections` array of ordered `{heading, level, ordinal, slug, start, end}` objects, where `start` and `end` are Unicode code-point offsets into the record's `text` value such that the slice from `start` to `end` is exactly the section's content including its heading line; the artifact remains one record — RAC records anchors and MUST NOT emit chunk records (ADR-004, ADR-010, ADR-007).
- [REQ-002] Anchors MUST be derived from the same CommonMark parse the export already uses, covering ATX and setext headings, and MUST be deterministic: identical bytes in, identical anchors out; content before the first heading is addressable from offset zero, and section ranges never overlap.
- [REQ-003] `ordinal` MUST be the zero-based document-order index and `slug` a deterministic lowercase-hyphen slug of the heading text with numeric-suffix deduplication, so both `(id, ordinal)` and `(id, slug)` are stable chunk keys across exports of unchanged content.
- [REQ-004] An additive `--live-only` option on `rac export --documents` MUST exclude artifacts that are not live, using one shared liveness predicate across artifact types rather than a duplicated status list; the default remains include-all-with-status, and the graph projection is unaffected so topology stays complete.
- [REQ-005] Documents records MUST gain an additive `metadata.related` array carrying the artifact's outgoing typed edges as sorted `{type, target, resolved}` objects, derived from the same relationship resolution the graph projection uses, so a single record knows its neighbours without a second projection (ADR-055, ADR-074).
- [REQ-006] All additions MUST stay within the projection's current `schema_version` — existing fields byte-unchanged, the published documents schema updated in the same change — and the viewer and graph projections' existing fields MUST NOT change (ADR-007, ADR-063).

## Acceptance Criteria

- For a fixture artifact with nested second- and third-level headings, a
  test asserts slice equality — the text between each anchor's offsets
  equals that section's exact Markdown — and that ranges are ordered and
  non-overlapping.
- A heading repeated twice yields distinct deduplicated slugs; an artifact
  with no headings yields an empty `sections` array with the whole body
  addressable from offset zero.
- In a corpus containing one Superseded decision, default `--documents`
  includes it with its status stamped; `--live-only` omits it; two runs of
  each are byte-identical.
- `metadata.related` on an artifact declaring a supersession matches the
  corresponding graph-projection edges originating at that artifact,
  including an unresolved literal target flagged unresolved.
- The updated documents schema validates the new output, and a consumer
  reading only the pre-existing fields sees unchanged values.

## Success Metrics

- Two independent consumers chunking the same artifact at the recorded
  anchors produce identical chunk sets, reproducibly across exports.

## Risks

- Anchors invite chunk-emission scope creep. Mitigation: REQ-001 states
  RAC never emits chunk records; the roadmap carries the matching Non-Goal.
- Offset arithmetic drifts from the emitted text (encoding, line endings).
  Mitigation: the slice-equality acceptance test pins offsets to the exact
  `text` value, not to file bytes.

## Assumptions

- The existing CommonMark token stream exposes reliable heading line maps
  for both ATX and setext headings.
- Whole-artifact records remain the right default; anchors are metadata for
  consumers that chunk, not a change to the atomic unit (ADR-004, ADR-010).

## Related Decisions

- adr-002
- adr-004
- adr-007
- adr-010
- adr-055
- adr-066
- adr-074

## Related Designs

- corpus-export-shape-contract

## Related Roadmaps

- corpus-sync

## Related Requirements

- export-contract-schemas
- rac-corpus-documents-export
