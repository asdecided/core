---
schema_version: 1
id: RAC-KX9K7P2T5Y9F
type: design
---
# OKF v0.2 Export Projection

## Context

The native exporter already builds a deterministic derived tree from one
validated AsDecided corpus walk plus Git recency. OKF v0.2 changes the carrier
envelope but does not justify a second parser or a network dependency.

## User Need

An operator should be able to export current OKF bundles for other tools without
changing the authoritative AsDecided artifacts or overstating their provenance
and trust.

## Design

### Root index

The generated root `index.md` starts with:

```yaml
---
okf_version: "0.2"
---
```

The existing progressive-disclosure body and deterministic type ordering remain.

### Concept envelope

Each generated concept carries:

```yaml
---
type: ADR
id: RAC-EXAMPLE
title: "ADR-001: Example"
status: stable
generated:
  by: asdecided/0.25.0
  at: 2026-07-29T06:00:00+00:00
tags: [example]
---
```

`generated` is omitted when Git cannot supply a valid last commit. YAML string
values that can contain punctuation are encoded deterministically as JSON-style
quoted scalars, which YAML accepts.

### Lifecycle

Comparison is ASCII case-insensitive after trimming:

| AsDecided status | OKF status |
| --- | --- |
| `Proposed`, `Draft` | `draft` |
| `Retired`, `Superseded`, `Deprecated`, `Obsolete` | `deprecated` |
| any other non-empty valid status | `stable` |
| absent/unknown | omit |

The mapping is a carrier simplification only. It never feeds back into Core.

### Relationships and provenance

Resolved outgoing relationships render under `# Related concepts` using
bundle-relative links. The exporter does not populate `sources`: relationship
edges express governance and navigation, not necessarily derivation.

No `verified`, `stale_after`, source credibility, executor, or attester field is
generated in this release. Future projections require an authoritative semantic
source and a new governed change.

### Compatibility

The v0.1 exporter shape remains captured as a small retirement fixture and
documented for consumers. There is no dual-output flag: new `--okf` exports are
v0.2, avoiding an indefinitely maintained second exporter.

## Related Decisions

- adr-048
- adr-122

## Related Requirements

- asdecided-okf-v0-2-carrier
