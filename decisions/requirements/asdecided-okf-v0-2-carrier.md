---
schema_version: 1
id: RAC-KX9J5N8R3W6D
type: requirement
---
# Requirement: OKF v0.2 Carrier Alignment

## Status

Accepted

## Problem

AsDecided exports an informative OKF v0.1 bundle. OKF v0.2 supersedes the
export's timestamp and citations conventions and adds versioned lifecycle,
provenance, trust, freshness, and deterministic attestation families. Merely
changing the displayed conformance version would produce a misleading carrier;
copying every optional field would weaken the semantic boundaries of the
authoritative AsDecided corpus.

## Requirements

- [REQ-000] This requirement MUST supersede the v0.1 version, timestamp, and citation clauses of `rac-okf-carrier-profile` while retaining its informative-dependency and no-loosening constraints.
- [REQ-001] `decided export --okf` MUST emit a bundle-root `index.md` declaring `okf_version: "0.2"`.
- [REQ-002] Every exported concept MUST retain its mapped OKF `type`, stable AsDecided `id`, safely encoded `title`, and tags when present.
- [REQ-003] When Git supplies a valid last-content timestamp, the export MUST emit `generated.by` as the versioned AsDecided exporter and `generated.at` as that timestamp; it MUST NOT describe the exporter as the original author.
- [REQ-004] Exported lifecycle MUST map proposed states to `draft`, retired or superseded states to `deprecated`, and other valid live states to `stable`.
- [REQ-005] Resolved structural relationships MUST remain deterministic Markdown links under `# Related concepts` and MUST NOT be emitted as OKF `sources` unless an authoritative derivation contract exists.
- [REQ-006] The exporter MUST NOT infer `verified`, trust tier, `stale_after`, source credibility, or attestation from acceptance status, Git history, proximity, or AsDecided `Verified By` edges.
- [REQ-007] The authoritative AsDecided validator and relationship gate MUST retain their existing strict behavior; OKF's permissive consumer rules MUST NOT expand the registered AsDecided artifact types.
- [REQ-008] New OKF exports MUST target v0.2 only, while tests and documentation MUST explain the bounded v0.1 compatibility fallback for existing consumers.
- [REQ-009] Verification MUST be deterministic, network-free, and fixture-driven, including lifecycle, generated metadata, relationship links, safe YAML strings, and absence of invented trust fields.

## Success Metrics

- A generated bundle satisfies the mandatory OKF v0.2 structure.
- Repeated exports of the same corpus and Git history are byte-identical.
- Existing non-OKF workspace tests and live-corpus invariants remain green.
- No production dependency on Google code or tooling is added.

## Risks

- Downstream consumers may rely on the old `# Citations` heading. The release
  notes and profile document the migration to standard related-concept links.
- Lifecycle projection may erase type-specific nuance. The original AsDecided
  artifact remains authoritative and its stable ID is preserved.
- Optional v0.2 fields may appear incomplete. Their absence is preferable to
  fabricating trust or provenance, and is explicitly conformant with OKF v0.2.

## Assumptions

- OKF v0.2 remains an informative, pre-1.0 carrier dependency.
- Git timestamps are available only as derived recency evidence.
- AsDecided's structural relationships are not automatically provenance edges.

## Related Decisions

- adr-048
- adr-122

## Related Designs

- okf-v0-2-export-projection

## Related Requirements

- rac-okf-carrier-profile

## Related Tickets

- asdecided/core#392

## Verified By

- rust/rac-engine/tests/okf_v02.rs
