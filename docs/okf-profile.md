# OKF Profile

AsDecided can project an authoritative decision corpus into [Google Cloud's
Open Knowledge Format (OKF)
v0.2](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md):
a portable Git tree of Markdown files with YAML frontmatter.

This is an **informative carrier profile**. AsDecided remains the source of
truth and keeps its stricter write-time validation, typed relationships, and
lifecycle rules. The export never feeds back into the corpus and requires no
Google code, package, service, or network call. The governing decisions are
[ADR-048](https://github.com/asdecided/core/blob/main/decisions/decisions/adr-048-okf-carrier-profile.md)
and
[ADR-122](https://github.com/asdecided/core/blob/main/decisions/decisions/adr-122-okf-v0-2-carrier-profile.md).

## Producing a bundle

```bash
decided export decisions/ --okf --out okf-bundle/
```

The command emits one concept file per typed artifact, preserving the corpus
layout, plus generated root `index.md` and `log.md` files. New exports target
OKF v0.2 only.

The root index declares the profile explicitly:

```yaml
---
okf_version: "0.2"
---
```

## Concept projection

Each concept preserves the stable AsDecided ID and body while projecting the
carrier fields:

```yaml
---
type: ADR
id: "RAC-KV2KWK55FC49"
title: "ADR-048: OKF as an Informative Carrier Profile"
status: stable
generated:
  by: asdecided/0.25.0
  at: 2026-07-29T11:00:00+01:00
tags: ["interoperability"]
---
```

- `title` and `tags` are safely encoded YAML strings.
- `generated.at` is the last meaningful Git commit time, when available.
- `generated.by` identifies the versioned deterministic exporter. It does not
  claim that AsDecided authored the underlying knowledge.
- `generated` is omitted when Git cannot supply a valid timestamp.
- `description` remains absent because AsDecided has no equivalent canonical
  field.

### Type mapping

| AsDecided `type` | OKF `type` |
| --- | --- |
| `requirement` | `Requirement` |
| `decision` | `ADR` |
| `roadmap` | `Roadmap` |
| `prompt` | `Prompt` |
| `design` | `Design` |

Unknown documents remain outside the derived export. OKF consumers must tolerate
unknown types, but that permissive read rule does not expand AsDecided's
authoritative artifact registry.

### Lifecycle mapping

OKF v0.2 has a deliberately small lifecycle vocabulary. The projection is:

| AsDecided status | OKF `status` |
| --- | --- |
| `Proposed`, `Draft` | `draft` |
| `Retired`, `Superseded`, `Deprecated`, `Obsolete` | `deprecated` |
| other valid non-empty live states | `stable` |
| absent or unknown | omitted |

The source artifact retains the full type-specific lifecycle meaning.

## Relationships are not provenance

Resolved AsDecided relationships are emitted as deterministic Markdown links
under `# Related concepts`. They are not emitted as OKF `sources`: a governance
or navigation edge does not necessarily mean the target was a source for the
concept.

For the same reason, this profile does not infer:

- `verified` or a trust tier from an accepted status;
- verification actors from AsDecided `Verified By` test and trace paths;
- `stale_after` from Git recency;
- source credibility from relationship proximity; or
- executor or attester fields without an authoritative Attested Computation.

Those optional v0.2 fields can be added only when the AsDecided corpus carries
equivalent semantics. Their absence is valid OKF and is more truthful than
invented provenance.

## Generated navigation

- `index.md` declares `okf_version: "0.2"` and provides progressive disclosure
  by artifact type.
- `log.md` groups concepts by their last Git commit date, newest first.
- Both are derived output and must not be hand-maintained as authoritative
  corpus state.

## Checking conformance

`decided validate` keeps a stricter AsDecided profile gate:

```bash
decided validate decisions/
# … PASS decisions/ — N artifact(s) checked: N valid, 0 invalid. OKF v0.2: conformant.
```

It fails on mapped artifacts that cannot be represented or collide with the
reserved generated `index.md` and `log.md` paths. Relationship integrity remains
the responsibility of `decided relationships --validate`. General OKF v0.2
consumers are more permissive: optional fields, unknown types, broken links, and
a missing index are not rejection conditions.

## Migrating from the v0.1 profile

New exports make two intentional changes:

- `created` and `updated` are replaced by v0.2 `generated`; and
- generated `# Citations` are replaced by `# Related concepts`.

An existing consumer can temporarily fall back from `generated.at` to legacy
`updated`, and can continue parsing legacy `# Citations`, while it adopts v0.2.
AsDecided does not maintain a dual-output flag; the small v0.1 retirement
fixture exists only to keep that migration explicit and bounded.

## See also

- [Relationships](relationships.md)
- [Artifacts](artifacts.md)
- [Validation](validation.md)
