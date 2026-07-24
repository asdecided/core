---
schema_version: 1
id: RAC-KXFK11FQDN1Y
type: decision
tags: [architecture, spec, engine, parity, clients]
---
# ADR-115: The Shared Artifact-Spec Registry Both Engines Read (ADR-063 Guard 1)

## Context

ADR-063 permits a native (non-Python) reimplementation of RAC's analysis engine
only as a guarded exception, under two guards: (1) the artifact specs
(`ARTIFACT_SPECS`) are first extracted to a shared, language-neutral data file
**both engines read**, and (2) a cross-language conformance fixture suite proves
output parity. The native derived-index port (roadmap:native-derived-index)
satisfied the maintainer's recorded sequencing precondition for reconsidering
ADR-063, but Guard 1 itself was only half-met.

What existed was one-directional, not shared. `rust/spec/extract_artifact_specs.py`
serialised the Python engine's in-code `ARTIFACT_SPECS` to
`rust/spec/artifact-specs.json`, which only the Rust engine read (via
`include_str!`). The Python engine still hardcoded the same specs in
`rac.core.artifacts`. There were, in substance, two sources of truth kept in
lockstep by a regeneration script — precisely the drift surface Guard 1 exists to
remove. The file was also derived *from* Python, so Python, not the file, was the
source of truth; "both engines read one shared file" was not true.

The neutral cross-repo home for RAC's contracts is `itsthelore/asdecided-spec`
(ADR-064, ADR-092), which today carries the prose specification, a structural
JSON Schema, and vocabulary — but not the complete machine-readable registry
(section sets, metadata enums, descriptions, guidance, synonyms, starter bodies)
the engines actually read to produce byte-identical output.

## Decision

Invert the direction: a single shared, language-neutral registry file is the
source of truth that **both** engines read.

- The canonical machine-readable registry is `artifact-specs.json`: the ordered
  `ARTIFACT_SPECS` (requirement, decision, roadmap, prompt, design) plus the
  relationship-section descriptions.
- The Python engine loads it. `rac.core.artifacts` no longer hardcodes the
  specs; it reconstructs the `ArtifactSpec` dataclasses (and their declared
  field/map ordering) from the file at import via `importlib.resources`. The file
  ships with the distribution as `rac.spec` package data (the packaging pattern
  of ADR-021 templates, `rac.hooks`, `rac.skills`).
- The Rust engine embeds the **same bytes** of the same file via `include_str!`.
- The two engines therefore read one artifact and cannot drift by construction: a
  section, enum, synonym, or ordering added once is reflected in both.
- The upstream source of truth is `itsthelore/asdecided-spec`; the in-repo
  `src/rac/spec/artifact-specs.json` is the vendored copy both in-tree engines
  embed, kept byte-identical to the upstream by a sync gate. (In-tree engines can
  only read an in-tree file at build/runtime, so a vendored copy plus an equality
  gate is the mechanism by which "the rac-spec file is what both engines read.")

This closes ADR-063 Guard 1. Guard 2 (the cross-language conformance fixture
suite) is tracked separately and is not decided here.

The inversion is behavior-neutral, and this is proven, not asserted: the
reconstructed `ARTIFACT_SPECS` matches the frozen oracle golden vector
field-for-field and in order; the full Python test suite and the full Rust
byte-parity battery (CLI, closure, retrieve, index, MCP cache-on and cache-off)
stay green. No `rac … --json` payload, exit code, template, or MCP frame changes.

## Consequences

There is now one source of truth for classification and validation structure, so
Python and Rust cannot diverge on it — the strongest possible form of the
no-drift promise ADR-063 protects. New artifact types or section changes are
authored once, in rac-spec, and vendored; neither engine can be edited into
disagreement with the other because neither carries its own copy.

The Python tree is no longer "never modified" in the literal sense the
native-derived-index port maintained as an oracle-freeze discipline. That
discipline was a porting methodology, not a product constraint; it is retired for
this change because the change is provably output-neutral (the oracle's bytes are
unchanged) and because keeping Python frozen would have permanently blocked
Guard 1's literal requirement that both engines read the shared file.

Trade-offs accepted: the Python engine now performs one cached file load at
import instead of holding a pure in-code constant (negligible; memoised). The
build depends on the vendored file being present (it ships as package data for
Python and is compiled in for Rust). The obligation this creates: rac-spec must
host the canonical `artifact-specs.json` and a cross-repo sync gate must enforce
that the vendored copy is byte-identical to it; until rac-spec carries the file,
the in-repo drift gate (`rust/spec/extract_artifact_specs.py`) holds the line by
proving the shared file reconstructs the certified registry.

## Status

Accepted

## Category

Architecture

## Alternatives Considered

- **Keep the one-directional extraction (Python is source; Rust reads a derived
  copy).** Rejected: this is the status quo Guard 1 was written to end. Two
  copies kept in sync by a script is a drift surface, and the file is not a
  source of truth, so the guard is unmet in fact however green the regeneration.
- **Keep the Python tree frozen; add only a byte-equivalence CI gate.** A gate
  that re-derives from Python and diffs the shared file makes drift impossible
  and satisfies the guard's *intent*. Rejected as the primary mechanism: it does
  not satisfy the guard's literal text ("both engines read" the shared file) —
  Python would still read its own in-code copy — and it entrenches Python as the
  privileged source rather than making the neutral file authoritative.
- **Host the canonical file only in rac-core, never in rac-spec.** Rejected:
  rac-spec is the neutral, language-agnostic contract home (ADR-064, ADR-092);
  the machine-readable registry belongs beside the prose spec and JSON Schema so
  future non-Python clients derive from one place. rac-core vendors it.

## Related Decisions

- adr-063
- adr-062
- adr-064
- adr-092
- adr-114
- adr-021
- adr-007

## Related Roadmaps

- native-derived-index

## Applies To

- rust/rac-engine/assets/spec/artifact-specs.json
- rust/rac-engine/src/spec.rs
