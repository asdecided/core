---
schema_version: 1
id: RAC-KWJ4VFR4JZAA
type: requirement
---
# Requirement: Point-in-Time Export

## Status

Proposed

Classification: `[internal]` — reproduce any historical corpus state from a
commit SHA. Feature B of the `corpus-sync` programme: `rac export --at
<rev>` over the existing ADR-043 revision-materialisation seam.

## Problem

Watchkeeper already materialises any git revision read-only through the
ADR-043 seam, but export cannot use it: there is no way to reproduce the
corpus's consumption projections as of a named commit. Provenance-grade
questions — "what did the corpus assert at release X?", "rebuild the index
our auditors reviewed" — require checkout gymnastics outside the tool. A
point-in-time export makes every projection a pure function of the
repository content at a revision, which is the foundation the incremental
change feed builds on.

## Requirements

- [REQ-001] An additive `rac export --at <rev>` option MUST apply to the three JSON payload modes — the default viewer JSON, `--documents`, and `--graph` — and emit the projection computed from the corpus path's content at git revision `<rev>`, materialised through the existing revision seam: read-only, offline, never mutating `.git` (ADR-043).
- [REQ-002] Combining `--at` with `--html`, `--okf`, or `--agent-rules` MUST be rejected as a usage error; those modes write into the working tree and are out of point-in-time scope.
- [REQ-003] Output MUST be a pure function of the repository content at `<rev>`, the corpus path, and the mode: byte-identical across runs, working directories, and clones of the same commit (ADR-002).
- [REQ-004] Path fields and the corpus identity in `--at` output MUST be derived from the requested directory argument, never from the materialisation location, so an `--at HEAD` export over a clean working tree is byte-identical to the plain export.
- [REQ-005] An unknown revision MUST exit non-zero with an actionable message naming the revision; a non-git directory MUST report that it is not a repository; a revision where the corpus path is absent MUST export an empty corpus as a valid result, matching the seam's fresh-adoption semantics.
- [REQ-006] The viewer payload's tool-version field MUST remain the producing CLI's version — `--at` time-travels content, not the toolchain — and no field carrying wall-clock or environment data may be introduced (ADR-002, ADR-007).

## Acceptance Criteria

- In a fixture repository with commits A and B where an artifact's body
  changes between them, `rac export --documents --at <A>` carries the A
  content and `--at <B>` the B content, each byte-stable across two runs.
- With a clean working tree at B, `rac export --graph --at <B>` is
  byte-identical to `rac export --graph`.
- `rac export --at <unknown-sha>` exits non-zero naming the revision;
  `rac export --okf --at <A>` exits with the usage error code.
- `git status` output is identical before and after `--at` runs: no `.git`
  mutation, no worktree registration, no leftover temp state.

## Success Metrics

- A consumer reproduces the exact export their pipeline ingested at a past
  release from its commit SHA, byte-for-byte, on any clone.

## Risks

- Materialisation paths or a tempdir-derived corpus name leak into the
  payload and break parity with the plain export. Mitigation: REQ-004 pins
  parity as an acceptance criterion, not an implementation detail.
- Large-repository materialisation is slow at export scale. Mitigation: the
  seam archives only the corpus subpath, and the programme's evidence
  initiative documents a performance floor.

## Assumptions

- The existing revision seam's semantics (read-only archive, empty corpus
  for an absent subpath, typed errors for unknown revisions) are sufficient
  without new git machinery (ADR-043).
- Consumers address revisions by commit SHA or ref name; RAC resolves but
  does not invent revision identifiers.

## Related Decisions

- adr-002
- adr-007
- adr-011
- adr-043
- adr-080

## Related Designs

- corpus-export-shape-contract

## Related Roadmaps

- corpus-sync

## Related Requirements

- export-contract-schemas
- rac-export-change-feed
