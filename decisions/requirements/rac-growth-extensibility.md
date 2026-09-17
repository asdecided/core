---
schema_version: 1
id: RAC-KTYBK6T1NQMY
type: requirement
---
# RAC Growth — Third-Party Artifact Schema and Template Extensibility

## Status

Proposed

## Problem

RAC recognises five artifact types, defined in a single in-package
registry (`ARTIFACT_SPECS` in `src/rac/core/artifacts.py`) and created
from templates that ship as package resources (ADR-021). A team whose
corpus needs a sixth type — a domain-specific artifact with its own
sections and template — has no supported path other than forking the
engine. ADR-012 names "community contributions to schemas and tooling"
and "third-party tooling built around RAC artifacts" as success
measures of the open-core strategy, but the engine offers no mechanism
for either: the registry is closed and the spec set is fixed at build
time.

The engine already has a pattern for optional capability that stays
out of the core install: the `explorer` extra ships the TUI without the
core ever importing Textual. Schema extensibility should follow the
same shape — discovered when present, invisible when absent.

## Requirements

- [REQ-001] RAC shall discover additional artifact specs through a spec bundle — one JSON file in the shape of the shared artifact registry, pinned by path and content digest in `.decided/config.yaml` (ADR-083, revised) — and when no bundle is pinned, every command's behaviour and output shall be unchanged from the built-in-only registry.
- [REQ-002] A discovered spec shall satisfy the same structural contract as a built-in `ArtifactSpec` (name, display label, required and recommended section tuples); a spec that fails this contract, or whose name collides with a built-in or previously loaded type, shall be reported as a warning and skipped — it shall never alter built-in behaviour or crash a command.
- [REQ-003] An additional type's starter template shall be rendered from the spec element itself (its starter bodies, descriptions, and guidance; ADR-083 decision 5), never loaded from a separate resource or loader: `decided new <type>` shall create an artifact from it, and the newly generated artifact shall pass baseline validation for that type.
- [REQ-004] A loaded third-party type shall participate in classification, validation, `rac schema`, and template listing through the same deterministic, section-heading-based mechanisms as built-in types, with no artifact-specific branches in core code.
- [REQ-005] The discovery mechanism shall add no mandatory dependency to the engine and shall execute no third-party code at any point; a bundle is bounded, digest-verified, parsed data in the published registry element shape (ADR-115), and nothing from it runs.

## Success Metrics

- A reference third-party package containing one spec and one template
  can be installed alongside RAC, after which `rac new`, `rac
  validate`, `rac inspect`, and `rac schema` handle the new type with
  no engine changes.
- With no third-party package installed, the full test suite and the
  golden CLI output tests pass unchanged.
- A deliberately malformed spec package degrades to a warning, with
  every built-in command still exiting successfully.

## Risks

- Entry-point loading executes third-party code at CLI start-up; a
  misbehaving package can slow or break every invocation, so failure
  isolation (REQ-002) is load-bearing, not cosmetic.
- Third-party section vocabularies may overlap built-in ones and shift
  classification confidence for existing corpora; the deterministic
  classifier needs collision rules before this ships.
- A published extension point becomes a compatibility contract; the
  `ArtifactSpec` dataclass is currently internal and would need a
  declared stability boundary first.

## Assumptions

- Python entry points remain the distribution-native discovery
  mechanism for installed-package plugins; no registry service or
  network discovery is in scope (consistent with ADR-024 — RAC reads
  what is installed, it does not host anything).
- Demand is plausible but unproven; this requirement is a prerequisite
  for inviting derivatives, not a response to a filed request.

## Draft: Third-Party Bundle Convention

Blocked: GATE-2 (CLA not yet in place)

Python-era packaging draft, retained as history. Under the revised ADR-083 a
shared type ships as a spec bundle — a JSON file in the shared registry
shape — that an adopting corpus commits and pins by digest in its
`.decided/config.yaml`; there is no package to install. A type that proves
general is promoted into the shared registry in `asdecided/spec`.

This convention is a draft recorded for when the contribution policy
goes live. It is not published in `docs/` and is not yet an invitation.

<!-- Packaging model verified against the OpenSpec README,
     https://github.com/Fission-AI/OpenSpec , which describes community
     schemas as "third-party schema bundles distributed via standalone
     repositories". Fetched 2026-06-12. -->

Modelled on OpenSpec's approach, a third-party RAC bundle would be a
standalone repository, not a directory in this one:

- One repository per bundle, owned by its author, containing a small
  Python package (suggested naming: `rac-<bundle-name>`).
- The package holds the artifact spec(s) and the matching template(s)
  as package resources, and declares them under the entry-point group
  from REQ-001 in its own `pyproject.toml`.
- Installation is plain `pip install` alongside RAC; nothing is copied
  into the host engine and the engine repository takes no dependency
  on any bundle.
- A bundle that exists and works may be listed in `docs/ecosystem.md`
  under that list's entry criteria.

## Related Decisions

- adr-012
- adr-015
- adr-021
- adr-024
- adr-083

## Related Designs

- third-party-artifact-extensibility

## Related Requirements

- rac-growth-ecosystem-list
