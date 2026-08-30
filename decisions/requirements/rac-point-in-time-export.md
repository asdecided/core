---
schema_version: 1
id: RAC-KWJ4VFR4JZAA
type: requirement
---
# Requirement: Point-in-Time Export

## Status

Proposed

Classification: `[internal]` — reproduce any historical corpus state from a
commit SHA. Feature B of the `corpus-sync` programme: `decided export --at
<rev>` over the existing ADR-043 revision-materialisation seam.

## Problem

Watchkeeper already materialises any git revision read-only through the
ADR-043 seam, but export cannot use it: there is no way to reproduce the
corpus's consumption projections as of a named commit. Provenance-grade
questions — "what did the corpus assert at release X?", "rebuild the index
our auditors reviewed" — require checkout gymnastics outside the tool. A
point-in-time export makes every projection a pure function of the repository
content at a revision for a fixed producing CLI version, which is the
foundation the incremental change feed builds on.

## Requirements

- [REQ-001] An additive `decided export --at <rev>` option MUST apply to the three JSON payload modes — the default viewer JSON, `--documents`, and `--graph` — and emit the projection computed from the corpus path's content at git revision `<rev>`, materialised through the existing revision seam: read-only, offline, never mutating `.git` (ADR-043).
- [REQ-002] Combining `--at` with `--html`, `--okf`, or `--agent-rules` MUST be rejected as a usage error; those modes write into the working tree and are out of point-in-time scope.
- [REQ-003] Output MUST be a pure function of the repository content at `<rev>`, the corpus path, the mode, and the producing CLI version: byte-identical across runs, working directories, and clones of the same commit when those inputs match, the required object closure is locally available, and the bounded snapshot admits it (ADR-002).
- [REQ-004] Path fields and the corpus identity in `--at` output MUST be derived from the requested directory argument, never from the materialisation location, so an `--at HEAD` export is byte-identical to the plain export when worktree bytes match committed blobs, the required submodule object closure is locally available, and the corpus is within the documented snapshot safety limits.
- [REQ-005] An unknown revision MUST exit non-zero with an actionable message naming the revision; a non-git directory MUST report that it is not a repository; a revision where the corpus path is absent MUST export an empty corpus as a valid result, matching the seam's fresh-adoption semantics.
- [REQ-006] The viewer payload's tool-version field MUST remain the producing CLI's version — `--at` time-travels content, not the toolchain — and no field carrying wall-clock or environment data may be introduced (ADR-002, ADR-007).
- [REQ-007] A configured or federated historical export MUST materialise only the requested corpus, governing config and manifest files, and recursively declared parent corpus paths. A submodule-backed path MUST be read offline from the already-local object database at the gitlink commit recorded by the selected revision; unavailable objects MUST fail actionably without fetching or observing the submodule worktree's current commit (ADR-043, ADR-134).
- [REQ-008] The point-in-time seam MUST fail closed on archive, extraction, or object errors. Git archive attributes (`export-subst` or `export-ignore`) MUST NOT change or omit selected committed bytes; the seam MUST read exact blob objects or reject an affected path rather than violate byte parity.
- [REQ-009] A governing `.decided/config.yaml` outside the containing Git repository MUST be rejected for `--at`, because that uncommitted ancestor cannot be reproduced from the selected revision.
- [REQ-010] Revision reads MUST ignore local replace refs, disable lazy object fetching, and bound config/manifest discovery at the temporary snapshot root. The requested repository-relative corpus path MUST be derived from the argument rather than a current worktree symlink or checkout state.

## Acceptance Criteria

- In a fixture repository with commits A and B where an artifact's body
  changes between them, `decided export --documents --at <A>` carries the A
  content and `--at <B>` the B content, each byte-stable across two runs.
- With a working tree at B whose bytes match committed blobs and whose fixture
  is within the snapshot safety limits, `decided export --graph --at <B>` is
  byte-identical to `decided export --graph`.
- `decided export --at <unknown-sha>` exits non-zero naming the revision;
  `decided export --okf --at <A>` exits with the usage error code.
- `git status` output is identical before and after `--at` runs: no `.git`
  mutation, no worktree registration, no leftover temp state.
- A vendored parent and a submodule-backed parent whose recorded objects remain
  local (including a deinitialised, custom-named checkout) each reproduce their
  checked-out `--at HEAD` federation export byte-for-byte; changing the
  submodule worktree after the selected superproject commit does not change the
  historical output.

## Success Metrics

- A consumer reproduces the exact export their pipeline ingested at a past
  release from its commit SHA, byte-for-byte, on any clone with the required
  object closure available locally and within the snapshot safety limits.

## Risks

- Materialisation paths or a tempdir-derived corpus name leak into the
  payload and break parity with the plain export. Mitigation: REQ-004 pins
  parity as an acceptance criterion, not an implementation detail.
- Large-repository materialisation is slow at export scale. Mitigation: the
  read-only snapshot is short-lived and restricted to the corpus, governing
  metadata, and declared parent closure; the programme's evidence initiative
  documents a performance floor.

## Assumptions

- The existing revision seam's semantics (read-only snapshot, empty corpus for
  an absent subpath, typed errors for unknown revisions) are sufficient without
  worktree or network machinery (ADR-043). Point-in-time export extends the
  seam's selected path set to historical governing metadata and declared
  parent paths, preserving the configured and federated closure without
  materialising unrelated repository content.
- Consumers address revisions by commit SHA or ref name; RAC resolves but
  does not invent revision identifiers.
- The local Git object database is inside the trusted host boundary. RAC
  rejects missing, malformed, or type/size-inconsistent objects, while Git
  remains responsible for detecting hostile on-disk object-store tampering.

## Related Decisions

- adr-002
- adr-007
- adr-011
- adr-043
- adr-080
- adr-134

## Related Designs

- corpus-export-shape-contract

## Related Roadmaps

- corpus-sync

## Related Requirements

- export-contract-schemas
- rac-export-change-feed
