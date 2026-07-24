# OKF Profile

RAC stores product knowledge the same way [Google Cloud's Open Knowledge Format
(OKF)](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
does — a Git tree of Markdown files with YAML front matter. This page is RAC's
**informative profile** of OKF **v0.1 Draft**: it defines how a RAC repository
presents itself as a conformant OKF bundle. It is a derived, interoperability
view; RAC Core and `decided validate` remain the source of truth, and adopting this
profile loosens none of RAC's guarantees. The decision behind it is
[ADR-048](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-048-okf-carrier-profile.md),
scoped by the `rac-okf-carrier-profile` requirement.

> The key words MUST, SHOULD, and MAY in this document are to be interpreted as
> described in BCP 14 (RFC 2119, RFC 8174) when, and only when, they appear in
> all capitals.

## Producing a bundle

`decided export <dir> --okf` writes the bundle described here:

```bash
decided export decisions/ --okf --out okf-bundle/
```

It emits one Markdown file per typed artifact (front matter projecting the OKF
`type`), plus a generated `index.md` and `log.md`, mirroring the corpus layout.
The bundle is a derived build artifact, parallel to `decided export --json` and
`--html`; it never feeds back into RAC.

Each bundle artifact also carries OKF's descriptive fields where RAC has them
(ADR-050): `tags` projected from the source frontmatter (a RAC artifact MAY
declare `tags: [...]`), and `created`/`updated` derived from git history — first
and last commit. Timestamps are never stored in the source frontmatter; recency
is git-derived (ADR-045), so the source stays date-free while the bundle is fully
timestamped. RAC does not project a frontmatter `title` (it derives from the H1)
or a `description`.

## Type mapping (normative)

Every RAC artifact carries a `type` in its front matter. In the OKF bundle view
each artifact MUST present a non-empty OKF `type`, mapped from its RAC `type`:

| RAC `type`    | OKF `type`    |
| ------------- | ------------- |
| `requirement` | `Requirement` |
| `decision`    | `ADR`         |
| `roadmap`     | `Roadmap`     |
| `prompt`      | `Prompt`      |
| `design`      | `Design`      |

The RAC `type` is authoritative; the OKF `type` is derived from it. A RAC
artifact MUST NOT present an empty or unmapped OKF `type`.

### Worked example

A RAC decision artifact:

```yaml
---
schema_version: 1
id: RAC-KV2KWK55FC49
type: decision
---
# ADR-048: OKF as an Informative Carrier Profile
```

presents, in the OKF bundle view, as `type: ADR` — the same file, the same body,
a derived front-matter projection.

## Checking conformance (write-time gate)

Conformance is enforced, not just produced on export. Running `decided validate` over
a corpus checks OKF v0.1 conformance as part of the run (ADR-048, ADR-049):

```bash
decided validate decisions/
# … PASS  decisions/ — N artifact(s) checked: N valid, 0 invalid. OKF v0.1: conformant.
```

The check is deterministic and per-artifact. It reports, with a file-named,
fixable diagnostic, and fails the `validate` exit code:

| Code | Meaning |
| --- | --- |
| `okf-unmapped-type` | a typed artifact whose `type` has no OKF mapping (it would be silently dropped from the bundle) |
| `okf-reserved-filename-collision` | a typed artifact named `index.md` or `log.md`, which OKF reserves for generated entry points |

Untyped documents are excluded (ADR-010): they are legitimately skipped and the
bundle already omits them, so an `index.md`/`log.md` that is an untyped entry
point is recognized, never flagged. Referential integrity ("links resolve") stays
with `decided relationships --validate`. The directory `validate` JSON carries an
additive `okf` section (ADR-007). Per ADR-052, `rac-core` — the OKF-superset
frontmatter envelope (`schema_version`, `id`, `type`, `relationships`, `tags`) —
is defined in code and enforced this way; RAC ships no JSON Schema `rac-core` file
and takes no schema-validation dependency.

## Conventions (recommended)

Where RAC has gaps, a RAC repository SHOULD adopt the following OKF conventions.
Each is a derived output generated from the corpus, never a hand-maintained
source file.

### `index.md` — progressive disclosure

An OKF bundle SHOULD ship an `index.md` entry point that discloses the corpus in
layers: an overview, then the artifact types, then individual artifacts. It is
the front door for a human or agent that has never seen the bundle.

```markdown
# Knowledge Index

## Decisions
- [ADR-048: OKF as an Informative Carrier Profile](decisions/decisions/adr-048-okf-carrier-profile.md)
  — RAC adopts OKF as an informative carrier profile and derived export target.

## Requirements
- [REQ-OKF-Carrier-Profile](decisions/requirements/rac-okf-carrier-profile.md)
  — conformance, derived bundle export, and the no-loosening invariant.
```

### `log.md` — date-grouped history

An OKF bundle SHOULD ship a `log.md` recording how the corpus evolved, grouped
by date, newest-first. RAC derives this from Git history of the `decisions/` tree
(consistent with ADR-045: recency is derived from Git, not stored in front
matter), so it never drifts from reality.

```markdown
# Log

## 2026-06-14
- Added ADR-048 (OKF as an informative carrier profile).
- Added REQ-OKF-Carrier-Profile (conformance, profile, and bundle export).
```

### `# Citations` — body references

Where a RAC artifact wants a human-facing list of what it draws on, it SHOULD use
a `# Citations` body section. This complements — and never replaces — RAC's typed
`## Related <Type>` structural references, which remain the machine-resolved,
validated edges (see [relationships](relationships.md) and ADR-016). In the
derived OKF view, each resolved structural relationship also appears as a body
link, so the relationship survives for permissive OKF consumers.

```markdown
# Citations

- [ADR-016: Relationships as Explicit Structural References](decisions/decisions/adr-016-relationships-as-structural-references.md)
- [ADR-007: JSON Contract Stability](decisions/decisions/adr-007-json-contract-stability.md)
```

## Status of this profile

The dependency on OKF is **informative and pinned to OKF v0.1**. RAC takes no
code, package, or network dependency on OKF or Google tooling. OKF is a pre-1.0,
single-vendor draft; if it diverges materially at 1.0, RAC can re-pin or drop the
profile without touching Core. Promoting the OKF bundle to a frozen RAC contract
(alongside the JSON/Portal export) would extend RAC's stability obligations and
is therefore a future, separately recorded ADR — not a change to this page.

## See also

- [relationships.md](relationships.md) — how RAC's typed structural references work.
- [artifacts.md](artifacts.md) — artifact types and identity.
