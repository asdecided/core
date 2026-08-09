---
schema_version: 1
id: RAC-KWJ4VE5CFJWM
type: requirement
---
# Requirement: Export Contract Schemas

## Status

Accepted

Classification: `[internal]` — make the export contracts machine-checkable.
Feature A of the `corpus-sync` programme: published JSON Schema files for
the viewer, documents, and graph projections, validated against real
exports in CI.

## Problem

ADR-007 promises that AsDecided's JSON outputs are stable, additive contracts —
but nothing checks that promise. No schema artifact exists for any export
projection; the `--documents` and `--graph` modes are absent even from the
prose viewer contract. An enterprise platform that wants to build on the
export surface must reverse-engineer the emitting code and re-verify it on
every upgrade. A published, versioned, machine-readable schema per
projection lets a consumer validate exports in its own CI, and lets AsDecided's
CI prove that emitted shapes and published contracts never drift apart.

## Requirements

- [REQ-001] AsDecided MUST publish JSON Schema (draft 2020-12) files for the three export projections — the viewer JSON payload, a single documents JSONL record, and the graph JSON object — as packaged resources, one schema file per projection, each identifying the projection's `schema_version` (ADR-007).
- [REQ-002] An additive `decided export --schema <viewer|documents|graph>` mode MUST print the packaged schema to stdout, offline and byte-identical to the packaged resource, without altering any existing export mode's behaviour (ADR-002, ADR-007, ADR-011).
- [REQ-003] Each schema MUST require and type every field the projection currently emits while leaving objects open to additive extension, so the schema pins the minimum contract a consumer may rely on and an unknown extra field never fails validation (ADR-007).
- [REQ-004] CI MUST validate real exports against the schemas — golden fixtures and a dogfood export of the repository corpus for all three modes — using a CI-only schema-validation dependency; the Rust engine's runtime dependency set MUST be unchanged (ADR-086).
- [REQ-005] A drift guard MUST fail CI when the emitted shape and the packaged schema diverge in either direction: a field added to the code without a schema update, or a schema field the code no longer emits.
- [REQ-006] The documents and graph projections MUST be documented on a contracts page with the schema files as the source of truth, and the viewer schema MUST be reconciled with the existing viewer contract rather than redefined against it.
- [REQ-007] Removing or retyping a schema-required field MUST be defined as a breaking change requiring a `schema_version` bump, and that rule MUST be recorded on the contracts page (ADR-007, ADR-063).

## Acceptance Criteria

- Every line of `decided export --documents` over a fixture corpus validates
  against the documents record schema; the graph and default JSON outputs
  validate against theirs.
- `decided export --schema documents` output parses as a valid draft 2020-12
  schema and is byte-identical across two runs and to the packaged file; an
  unknown mode argument exits with the usage error code.
- A test that deletes a required key from a synthetic payload fails schema
  validation, proving the schema is load-bearing rather than vacuous.
- A payload carrying an extra unknown field still validates, pinning the
  additive tolerance.
- The runtime dependency set is unchanged; the schema-validation library is
  pinned only in the CI contract-test requirements.

## Success Metrics

- A downstream platform validates AsDecided exports in its own CI using only
  the published schema files, with no reference to AsDecided's source code.
- Shape drift between code and contract is caught by AsDecided's CI before
  release, not by a consumer after one.

## Risks

- The schemas drift from the emitted shapes. Mitigation: REQ-004 and
  REQ-005 make round-trip validation and the bidirectional drift guard part
  of CI, so drift fails the build.
- Schema validation pulls a heavy dependency into the engine. Mitigation:
  REQ-004 confines the validator to the development extra; the runtime
  engine never imports it.

## Assumptions

- The current `to_dict` shapes of the three projections are the intended
  contract baseline; no shape change is smuggled in with the schemas.
- JSON Schema draft 2020-12 is expressive enough for the three payloads,
  including the JSONL one-record-per-line framing documented alongside.

## Related Decisions

- adr-002
- adr-007
- adr-011
- adr-063
- adr-074
- adr-086

## Related Designs

- corpus-export-shape-contract

## Related Roadmaps

- corpus-sync

## Related Requirements

- rac-corpus-documents-export
- rac-corpus-graph-export
