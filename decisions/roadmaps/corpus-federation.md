---
schema_version: 1
id: RAC-KWJ8RTRK4JWM
type: roadmap
---
# Corpus Federation Programme

## Status

Planned

ADR-089 accepted federation in principle, deferred it in mechanism, and
sequenced it last among the enterprise decisions. The sequencing condition
is now satisfied — the additive enterprise ADRs it was scheduled behind
(ADR-084, ADR-086 through ADR-088, ADR-090, ADR-091) have landed — and the
design-partner scenario ADR-089 anticipated is real: an adopting
organisation with a genuine firm-wide standards corpus is expanding Lore
toward multi-thousand-seat scale. Execution is tracked in GitHub (ADR-093): the
epic in `## Related Tickets` carries ordering and task state, with a
sub-issue per initiative.

## Context

The corpus is single-tree today: one canonical root per repository
(ADR-018) with git `main` as the only source of truth (ADR-080);
cross-repository references do not resolve; `## inherits` is deliberately
unrecognised; and the enterprise profile scaffold stays hollow on its
parent-corpus line (ADR-088). ADR-089 records the bar any federation
design must clear — five non-negotiable constraints — and anticipates "a
future design and its own implementing ADR, built with the design partner
against a real shared-standards corpus."

This programme is that mechanism work. The `corpus-federation-mechanism`
design artifact is the concrete proposal; the implementing ADR is authored
with the design partner and human-ratified when the build starts —
deliberately not pre-drafted here.

## Outcomes

- A child corpus resolves a pinned, materialised parent corpus fully
  offline: firm-wide standards live in one place and every repository's
  references to them resolve deterministically.
- Duplicate identities between parent and child are explicit findings;
  masking a parent artifact is a declared act with recorded provenance,
  never an implicit precedence rule.
- Every inherited artifact remains attributable to its source corpus in
  resolution, MCP responses, findings, and exports.
- Federated exports compose with the corpus-sync `(source, id)`
  aggregation model, so a shared parent deduplicates cleanly across N
  child corpora downstream.
- Nothing is enterprise-gated: the solo developer gets the identical
  capability (ADR-085, ADR-089).

## Initiatives

### Mechanism design with the design partner

Iterate the `corpus-federation-mechanism` design (Proposed) against the
partner's real shared-standards corpus until its Open Questions close. The
design carries the declaration shape, materialisation flow, overlay
semantics, and provenance model inside ADR-089's five constraints.

### Implementing ADR

The resolver ships under its own implementing ADR, authored with the
design partner and human-ratified — explicitly not pre-drafted by this
programme. It must clear the five ADR-089 constraints and record the
decision for everyone.

### Resolver, validation, and export integration (`rac-parent-corpus-inheritance`, `rac-federated-resolution-provenance`)

Parent artifacts enter resolution as a read-only overlay through the
engine's existing seams — the corpus walk feeding the resolution and
identifier indexes, the repository index feeding search, and the MCP
identity map — with no second resolver, collision findings at the existing
duplicate-identity detection point, and provenance carried end to end.

### Profile unhollowing (ADR-088)

Once the mechanism ships, the enterprise profile scaffold gains the
reserved parent-corpus declaration line it has been hollow on, emitted
only when a parent is configured; unconfigured profile output stays
byte-identical.

### Composition with corpus-sync

Federated exports stamp parent-origin records with the parent corpus's own
source identity via the `rac-export-source-identity` derivation — one
identity mechanism across the engine, not a second one for federation.

## Constraints

The five ADR-089 non-negotiables govern everything here:

- Never gated behind "enterprise": if federation is sound it is sound for
  the solo developer; it changes resolution for everyone or not at all
  (ADR-085).
- Deterministic and offline (ADR-002): parent resolution reads
  materialised bytes — a pinned submodule, a vendored bundle, or a path —
  never a live network fetch inside the validate, resolve, or serve paths.
- Single canonical state preserved per repo (ADR-018, ADR-080): a parent
  is an inherited, read-only layer; the child's `main` remains its own
  truth; overrides are explicit, not implicit precedence.
- Git-native and human-readable (ADR-016, ADR-055): inheritance is
  declared in Markdown — a `## inherits` section plus a pinned source
  reference; no database and no hidden index becomes the source of truth.
- Provenance preserved: an inherited artifact is always attributable to
  its source corpus, never silently absorbed.

Additionally: artifact identity and per-repository key prefixes stay as
ADR-026 defines them; all layers remain read-only with writes landing only
by PR (ADR-065).

## Non-Goals

- Any live network fetch in validate, resolve, or serve paths (ADR-002,
  ADR-089).
- Enterprise gating of any federation behaviour (ADR-085, ADR-089).
- Implicit precedence in either direction between parent and child.
- Cross-corpus writes: the engine never writes under a parent
  materialisation, and write-back of any kind stays propose-only via PR
  (ADR-065).
- A database or hidden index as a source of truth (ADR-080).
- Embeddings or semantic resolution (ADR-066).

## Success Measures

- A fixture child corpus with a pinned, vendored parent validates fully
  offline, byte-identically across clones of the same pinned state.
- A parent/child identifier collision fixture yields a deterministic
  finding naming both sources, and a declared override clears it with
  override provenance recorded.
- An export fixture over a federated corpus shows zero `(source, id)`
  collisions, with parent records stamped with the parent's source.
- Corpora with no `## inherits` produce output byte-identical to the
  pre-federation engine.
- `rac validate rac/`, `rac relationships rac/ --validate`, and
  `rac review rac/` stay clean across the programme's output.

## Assumptions

- The design partner's shared-standards corpus remains available to
  iterate the mechanism against, as ADR-089 anticipated.
- The corpus-sync source-identity derivation (`rac-export-source-identity`)
  is the export identity mechanism federation composes with; no second
  derivation is introduced.
- `## inherits` remains inert for released engines until the implementing
  ADR ships, per ADR-089's compatibility clause.

## Risks

- Pressure to ship federation enterprise-gated. Mitigation: ADR-089
  forbids it in advance; the constraint is restated here and in both
  requirements.
- Scope creep toward a live cross-repo index. Mitigation: the
  materialised-bytes constraint is recorded, and the design's rejected
  alternatives include exactly that shape.
- Parent staleness confusion — a child resolving against bytes that no
  longer match the declared pin. Mitigation: pin-verification findings are
  a named requirement; stale state fails loudly rather than resolving
  silently.
- Silent precedence sneaking in through resolution-order accidents.
  Mitigation: the collision-finding requirement pins the semantics at the
  engine's existing duplicate-identity detection point.

## Related Decisions

- adr-002
- adr-016
- adr-018
- adr-026
- adr-055
- adr-065
- adr-080
- adr-084
- adr-085
- adr-088
- adr-089
- adr-093
- adr-094

## Related Designs

- corpus-federation-mechanism

## Related Roadmaps

- lore-at-team-scale
- deterministic-substrate
- corpus-sync

## Related Requirements

- rac-parent-corpus-inheritance
- rac-federated-resolution-provenance
- rac-export-source-identity

## Related Tickets

- itsthelore/asdecided-core#267
