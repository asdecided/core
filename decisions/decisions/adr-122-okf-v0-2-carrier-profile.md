---
schema_version: 1
id: RAC-KX9H2M7Q4V8C
type: decision
---
# ADR-122: OKF v0.2 Is a Truthful Derived Carrier

## Status

Accepted

## Category

Architecture

## Context

ADR-048 adopted Google's Open Knowledge Format as an informative carrier
profile and pinned the derived export to OKF v0.1. Google has now published OKF
v0.2. The new revision keeps Markdown plus YAML frontmatter as its carrier but
makes provenance, trust, freshness, lifecycle, and deterministic attestation
first-class optional conventions.

OKF v0.2 also supersedes two v0.1 shapes that AsDecided's exporter currently
uses: a timestamp-style history projection is replaced by `generated`, and body
`# Citations` are replaced by frontmatter `sources` for genuine derivation.

AsDecided has related concepts, but they are not interchangeable:

- Git recency is evidence of when an artifact changed, not proof that its claims
  were verified.
- `Verified By` relationships point to external tests or traces, not actors who
  confirmed a concept against a source.
- Structural relationships preserve navigation and integrity, but they do not
  necessarily assert that one artifact derives from another.
- AsDecided lifecycle values are type-specific and stricter than OKF's
  `draft | stable | deprecated` carrier vocabulary.

A mechanical field rename would therefore claim provenance or trust the
authoritative corpus does not record.

## Decision

AsDecided updates its informative export profile to OKF v0.2 as a truthful
derived carrier. This decision supersedes ADR-048 only where that decision pins
the profile to v0.1 or adopts the v0.1 timestamp and citation conventions; its
informative-dependency and no-loosening boundaries remain in force.

1. The root generated `index.md` declares `okf_version: "0.2"`.
2. Every exported concept retains its mapped OKF type and stable AsDecided ID,
   and adds a safely encoded display title.
3. Git's last meaningful content commit may populate `generated.at`.
   `generated.by` identifies the deterministic AsDecided exporter, not the
   original author.
4. AsDecided lifecycle maps conservatively: proposed work becomes `draft`,
   retired or superseded work becomes `deprecated`, and other valid live states
   become `stable`.
5. Resolved structural relationships remain ordinary Markdown links under
   `# Related concepts`. They MUST NOT be mislabeled as `sources`.
6. `sources`, `verified`, `stale_after`, and Attested Computation fields are
   emitted only when an authoritative AsDecided contract carries equivalent
   semantics. This release does not invent them from proximity, Git history, or
   external verification edges.
7. Core remains stricter than OKF when reading its authoritative corpus.
   Unknown OKF concept types and optional-field absence remain acceptable to
   general OKF consumers; they do not expand AsDecided's own artifact registry.
8. The migration is network-free and fixture-driven. OKF v0.1 remains a
   documented compatibility input, but new exports target v0.2 only.

## Consequences

### Positive

- New exports declare the current OKF revision and stop emitting a superseded
  citations convention.
- Trust and provenance retain their meaning instead of becoming marketing
  labels inferred from unrelated evidence.
- AsDecided's strict write-time guarantees remain unchanged.
- The exporter remains deterministic, local, and independent of Google tooling.

### Negative

- OKF consumers expecting the old generated `# Citations` section must follow
  standard related-concept links instead.
- The derived bundle does not populate every optional v0.2 family until the
  authoritative corpus has equivalent data.
- The carrier profile must keep tracking a pre-1.0 upstream specification.

## Alternatives Considered

### Copy every new OKF field into AsDecided frontmatter

Rejected. It would make an informative export format part of the authoritative
artifact schema and duplicate existing lifecycle and relationship contracts.

### Infer trust from accepted status and `Verified By`

Rejected. Review status, executable verification, and source confirmation are
different claims. Conflating them would overstate trust.

### Stay pinned to v0.1

Rejected. New exports would knowingly use superseded conventions and miss the
versioned interoperability surface.

## Related Decisions

- adr-007
- adr-016
- adr-045
- adr-048
- adr-049
- adr-065
- adr-066
- adr-096
- adr-120

## Related Requirements

- asdecided-okf-v0-2-carrier

## Related Designs

- okf-v0-2-export-projection
