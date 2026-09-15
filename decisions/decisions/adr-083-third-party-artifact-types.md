---
schema_version: 1
id: RAC-KVTSPK4CJHWK
type: decision
tags: [extensibility, plugins, schema, spec, rust, architecture]
---
# ADR-083: Third-Party Artifact Types via Pinned Spec Bundles (Schema Composition Over the Code-Defined Core)

## Status

Proposed

**Revised 2026-09-15.** This ADR was first recorded for the Python engine, with
discovery through a Python entry-point group (`rac.artifact_specs`, resolved by
`importlib.metadata`), templates shipped as package resources loaded through
`importlib.resources`, and `ArtifactSpec` published via `rac.__all__`. The
engine is now Rust (ADR-116, ADR-120); the Python engine, package, and PyPI
path were removed in v0.23.0 and survive only at the `python-engine-final`
tag. A statically linked native binary has no `importlib` equivalent: no
entry-point group, no package-resource loader, no installed-package discovery
of any kind. The original decisions 1, 3, and 5 are therefore obsolete and are
**superseded by this revision**; they are listed at the end of the Decision
section so the change is on the record rather than a silent edit. Decisions 2,
4, and 6 carry forward unchanged. The status stays Proposed until the mechanism
ships; nothing under `rust/` changes on the strength of this ADR alone.

## Category

Architecture

## Context

AsDecided recognises five artifact types. Since ADR-115 they are defined once,
as data, in the shared language-neutral registry `artifact-specs.json`
(canonical upstream in `asdecided/spec`, vendored at
`rust/rac-engine/assets/spec/artifact-specs.json`, embedded at build time via
`include_str!`) and consumed through one seam, `spec::specs()` /
`spec_for()` / `available_schemas()`, by classification, validation,
frontmatter `type:` admission, `schema`, `templates`, `new`, the per-type
counts in `inspect` and `stats`, relationship collection, and the OKF export.
A team needing a sixth, domain-specific type still has no supported path but
to fork the engine. The `rac-growth-extensibility` requirement records the
demand and its contract (REQ-001..REQ-005): discover additional specs
deterministically, isolate their failures, let them create artifacts, and let
them participate in every command through the same section-heading mechanisms
as built-ins, with no artifact-specific branches in core.

ADR-052 §5 deferred custom types post-PMF and named the intended path: "schema
composition over this code-defined core … recorded as its own ADR — never a
silent edit to an accepted decision." This ADR is that record. It fixes the
design so the decision is gate-validated corpus knowledge before any code is
written, and keeps the post-PMF / GATE-2 (CLA, ADR-071) boundary intact for any
public invitation. The companion design `third-party-artifact-extensibility`
holds the implementation *how*.

Three facts fix the shape now.

- **An artifact spec is already pure data in a published shape.** Nothing in a
  registry element — `name`, `display`, `required`, `recommended`, `optional`,
  `metadata`, `retired_status`, `descriptions`, `guidance`, `synonyms`,
  `id_field`, `starter_bodies` — is executable, and the shape is already the
  append-only contract (ADR-007) that ADR-115's sync gate protects. The
  "published `ArtifactSpec` contract" the original decision 5 had to create
  now exists without a code surface.
- **The corpus already carries pinned, governing data under `.decided/`.**
  `.decided/config.yaml` holds corpus identity (ADR-135), the ticketing
  provider (ADR-087), and per-rule validation severities (ADR-053);
  `.decided/corpus.md` pins parent corpora by content digest (ADR-134,
  ADR-145). The engine hashes those bytes into the validation-store
  fingerprint, the derived-cache generation key (ADR-148), and the federation
  digest. A pinned, committed file is the established way this engine takes
  team-wide, deterministic input.
- **Two recorded decisions still constrain the shape.** ADR-055 deferred
  custom *relationship* types, so a custom artifact type must not smuggle in
  new edge kinds. ADR-052 §5 requires composition over the code-defined core,
  not a replacement of it.

## Decision

1. **Discovery is a pinned spec bundle, declared in the corpus config.** A
   corpus declares additional artifact types by committing one JSON file — a
   *spec bundle* — in the same shape as
   `rust/rac-engine/assets/spec/artifact-specs.json` (a top-level
   `artifact_specs` array of registry elements), and pinning it in
   `.decided/config.yaml` by path and content digest:

   ```yaml
   artifact_types:
     version: 1
     bundle:
       path: .decided/artifact-specs.json
       digest: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
   ```

   `path` is POSIX-relative to the repository root, must resolve to a regular
   file inside it, and follows the federation path rules (ADR-145: no absolute
   forms, backslashes, empty segments, `.` or `..`, no symlinks or reparse
   points). `digest` is `sha256:` over the file's raw bytes, the same pin
   style the federation manifest uses for a materialised parent. The engine
   reads the pin once per invocation (on every tool call in `decided-mcp`, so
   a re-pin lands on the next request) from the nearest governing config —
   never from the environment, the home directory, or an installed package. With no
   `artifact_types` section, every command's behaviour, output, exit code, and
   cache key is unchanged (REQ-001). The bundle's own `_meta` and
   `relationship_descriptions` keys are ignored: the first is provenance, the
   second would be a new relationship vocabulary (decision 4).

   This **reverses the original rejection of a repo-local types file**, and
   the reason is worth stating. That rejection rested on "installed,
   versioned capability" being preferable to per-checkout configuration. In
   a native binary there is no installation channel for capability: no
   package index, no entry point, nothing an engine can enumerate at
   start-up. What the repository offers instead is stronger than what
   `pip install` offered — a committed, reviewed (ADR-065), Git-versioned
   (ADR-080) file whose bytes are pinned by digest in the same config the
   federation manifest already pins parents with. Every clone of the corpus,
   every CI runner, and every serving generation sees the identical type set,
   which is exactly the team-wide determinism the federation pin exists to
   give. ADR-024 is not offended: the bundle is externally owned source in the
   user's repository, read, never hosted.

2. **Built-ins always win; bad or colliding specs warn and skip.** *(Carried
   forward.)* Each element of `artifact_specs` is admitted in bundle order
   only if it is a structurally valid registry element (a non-empty `name`
   matching `^[a-z][a-z0-9_-]{1,31}$` and not `unknown`, a non-empty
   `display`, a non-empty `required`, string-valued metadata enums, every
   section name already normalised), its name is not a built-in's, and its
   name has not already been admitted (first wins). A rejected element is
   skipped and reported as one warning-severity finding, code
   `artifact-spec-skipped`, naming the bundle, the element, and the reason
   (including the name it collided with). Warnings surface through
   `decided validate` and `decided doctor` as ordinary findings — the engine's
   own deterministic catalog, replacing the original's Python `warnings`
   channel — and never as free text on stderr; no command crashes and no read
   command changes its exit code because of a skipped element (REQ-002).

   The bundle as a whole is held to the federation pin's standard: a
   malformed `artifact_types` stanza, a path that is not a contained regular
   file, a bundle over the size bound, unreadable or unparseable bytes, or
   bytes that do not hash to the pinned digest is a **hard error**, never a
   warning. `decided validate` renders it as one error row for the bundle
   (`artifact-spec-bundle-digest-mismatch` and its siblings, the shape a
   `corpus-manifest` failure already takes) and exits 1; every other command,
   and every MCP tool call, refuses with `decided: <code>: <detail>` and exit
   1. A pin the engine cannot honour must stop the run — loading an unpinned
   bundle and warning would make the pin advisory.

3. **Custom types get generic structural validation only.** *(Carried
   forward.)* A loaded type participates in classification, validation,
   frontmatter `type:` admission, `schema`, `templates`, `new`, `inspect`,
   `stats`, `find`, `retrieve`, the derived index, the exports, and every MCP
   tool through the existing section-heading mechanisms, with no
   artifact-specific branch in core (REQ-004). Validation routes a registered,
   non-built-in type to the composition of the shared structural validators —
   title, required sections, status metadata — and nothing else: no bespoke
   rule, no standards checks, no bundle-supplied validator of any kind.

4. **Custom types are first-class for knowledge, not for the graph.**
   *(Carried forward.)* A custom artifact may list the built-in relationship
   sections in its `optional` set and reference built-in types. Custom types
   are **not** valid relationship targets and introduce no edge kinds
   (ADR-055): a built-in `## Related Decisions` reference resolving to a
   custom-type artifact still reports `relationship-target-type-mismatch`, and
   that sharp edge is accepted and pinned by a test as deliberate v1 behaviour.
   Auto-derived `related_<type>` edges stay deferred under ADR-055's lineage,
   liftable only by a future ADR.

5. **No separate template resource; starter bodies are the template.**
   *(Supersedes original decision 3.)* The registry element already carries
   `starter_bodies`, `descriptions`, and `guidance`; `decided schema <type>
   --template` already renders a complete starter artifact from them. `decided
   new <type>` for a custom type renders through that same generator, so a
   bundle ships no Markdown file, declares no resource, and there is no
   template-loading path to secure (REQ-003, ADR-002). Built-ins keep their
   embedded templates (ADR-021), unchanged.

6. **OKF export mapping is declared per type, defaulting to `display`.**
   *(New.)* The OKF type table in `docs/okf-profile.md` is fixed at five
   built-in mappings, and the export currently cannot emit a type outside
   them. A registry element gains one optional, appended field, `okf_type`, a
   non-empty string naming the OKF `type` the export writes for that artifact
   type; when absent it defaults to the element's `display`. A custom-type
   artifact is therefore exported, never silently dropped, under a type OKF
   consumers are already required to tolerate (the profile's permissive read
   rule). The five built-in mappings stay fixed and a bundle cannot override
   them: an `okf_type` on a name that collides with a built-in is rejected
   with the element (decision 2). `okf_type` is an append-only extension of
   the shared element shape and must land upstream in `asdecided/spec`
   (ADR-115) before the engine reads it; the profile's type table gains a row
   for "declared type → its `okf_type`", and its note that the permissive read
   rule "does not expand AsDecided's authoritative artifact registry" is
   qualified to the built-in registry, since a pinned bundle does expand a
   corpus's registry under this ADR (ADR-122: the carrier stays truthful — it
   reports the declared type, not a guess).

7. **The published contract is the registry element shape, not a code
   surface.** *(Supersedes original decision 5.)* Bundle authors depend on the
   `artifact_specs[]` element of `artifact-specs.json`, canonical upstream in
   `asdecided/spec` (ADR-115) and append-only under ADR-007; `okf_type` joins
   it by that discipline. The original decision extended ADR-062's
   `rac.__all__` surface with `ArtifactSpec`; that extension is withdrawn.
   ADR-062 itself is untouched and unaffected — it governs the Python SDK,
   which now lives outside this repository (`asdecided/sdk`), and nothing in
   this revision asks anything of it.

8. **Determinism is preserved, not absolute.** *(Carried forward.)* The
   classifier is unchanged; bundle types score after the built-ins in bundle
   order, so a built-in wins any exact tie, and a bundle whose section
   vocabulary overlaps a built-in can shift the best fit of a genuinely
   ambiguous document. This is inherent, bounded by the built-in-wins rules in
   decision 2, and stays explainable through `decided inspect`'s scored
   breakdown (ADR-002).

9. **The bundle is governing config: hashed, watched, per-corpus.** The bundle
   bytes join the config bytes in every fingerprint that already covers them —
   the validation-store fingerprint, the derived-cache generation key
   (ADR-148), and the serving freshness watch list — so a re-pinned bundle
   invalidates cached classification exactly as a changed severity override
   does. In v1 the effective registry is the **invoking corpus's own bundle**,
   applied to every source in its federated closure; a parent's bundle is not
   inherited, an inherited artifact whose type only the parent declares
   classifies as `unknown` in the child, and the federation digest preimage
   (ADR-134, ADR-145) is unchanged. Propagating bundles across the federation
   graph, and adjudicating a same-name / different-spec collision between
   parents, is deferred to a follow-up decision on the ADR-137 lineage.

### Superseded entry-point decisions (original text, retired by this revision)

- *Original 1 — "Discovery is entry-point based and absent-safe."* Specs were
  to be discovered through the `rac.artifact_specs` entry-point group via
  `importlib.metadata`, loaded once and cached per process, importing nothing
  beyond the registered entry point. Replaced by decision 1: there is no
  entry-point machinery in the native binary.
- *Original 3 — "Templates load from the contributing package,
  deterministically."* A `(package, resource)` template reference resolved
  through `importlib.resources`, with plugin loader callables rejected.
  Replaced by decision 5: the spec element already carries the starter bodies.
- *Original 5 — "`ArtifactSpec` becomes a published, append-only contract"
  via `rac.__all__`.* Replaced by decision 7: the contract is the shared JSON
  element shape, and ADR-062's surface is not extended.

Original decisions 2 (built-ins win, warn and skip), 4 (knowledge, not the
graph), and 6 (determinism caveat) are carried forward as decisions 2–4 and 8.

## Consequences

### Positive

- The path ADR-052 §5 promised is implementable on the engine that ships: a
  team commits one JSON file and one pinned config stanza, and `decided new`,
  `validate`, `inspect`, `schema`, `find`, `export`, and the MCP tools handle
  the new type with no engine change.
- No third-party code runs at start-up. The original's accepted negative — a
  slow or misbehaving plugin taxing every invocation — disappears; a bundle is
  bounded, parsed JSON that is never executed.
- One format for built-in and bundled types: a bundle element that admits is a
  valid registry element, so promoting a proven type upstream is a file move,
  and the `asdecided/spec` conformance tier (ADR-120) and sync gate (ADR-115)
  remain the single authority.
- The pin makes the type set part of what review, CI, federation, and serving
  generations already agree on; an edited bundle without a re-pin is caught by
  the digest, not discovered by drift.
- Custom-type artifacts reach OKF consumers under a declared type instead of
  vanishing from the export.

### Negative

- The type set is per-corpus, not per-installation. Two corpora agree on a
  `runbook` only by sharing the same bundle bytes until the deferred
  federation decision lands; a custom-type artifact copied into a corpus that
  lacks the bundle is `unknown` there.
- `.decided/config.yaml`'s `artifact_types` stanza, the bundle path, and the
  element shape (now with `okf_type`) become compatibility surfaces,
  constraining future registry refactors under the append-only discipline.
- Re-pinning is a manual step on every bundle edit; a helper that prints the
  digest is a CLI nicety the design may add, not a decision here.

### Neutral

- Per-type JSON that enumerates counts stays additive: a bundled type's key
  appears only in a corpus that pins it (ADR-007).
- Custom relationship types remain deferred (ADR-055); the OKF built-in
  vocabulary remains the five fixed rows (ADR-048, ADR-052).
- The `rac-growth-extensibility` requirement's REQ-001, REQ-003, and REQ-005
  are restated in bundle terms; the capability is unchanged (ADR-020).

## Alternatives Considered

- **Keep the entry-point mechanism (the original decision).** Impossible on
  the shipping engine: nothing in a Rust binary enumerates installed
  packages, and resurrecting an interpreter to regain `importlib` inverts
  ADR-116 and ADR-120.
- **Dynamic-library plugins (`dlopen`).** Rejected: executes untrusted native
  code at start-up with no failure isolation, is platform-specific in a
  per-platform-binary product (ADR-124), and puts a layer beyond the reach of
  `decided doctor`'s review aids (ADR-065).
- **WebAssembly plugins.** Rejected for v1: a runtime dependency the
  workspace's dependency discipline (ADR-114) has no need for, solving a
  problem — running plugin *logic* — that a data-only spec does not have.
  Reconsider only if a future decision admits custom validators.
- **An unpinned path (or an environment variable) to a spec file.** Rejected:
  without a digest the type set can drift between a review and a merge, and
  an environment variable is per-machine, invisible to the pull request, and
  absent from every cache key — the class of objection that retired the
  `RAC_*` layer in v0.23.0.
- **One JSON file per type, inferred from a directory.** Rejected: registry
  order is the classification tie-break and must be authored; a single
  ordered bundle carries the order explicitly and pins as one digest.
- **Inline YAML specs in `config.yaml`.** Rejected: the config reader is a
  bounded YAML subset, whereas a bundle in the registry's own shape goes
  through the same `serde_json` path and the same parser as the embedded
  bytes — one parser, one shape, one upstream schema.
- **Ship a Markdown template with the bundle.** Rejected (decision 5): the
  element already carries the starter bodies, and a second source of the
  starter text could disagree with the spec it accompanies.
- **Skip custom types in the OKF export.** Rejected (decision 6): a truthful
  carrier (ADR-122) does not silently drop typed artifacts, and OKF consumers
  are already required to tolerate unfamiliar types.
- **Auto-deriving a `related_<type>` edge for every bundled type.** Rejected
  for v1: it would introduce new edge kinds at pin time, make graph-integrity
  checks depend on which bundle is pinned, and contradict ADR-055's
  code-defined relationship registry.

## Related Decisions

- adr-052
- adr-055
- adr-062
- adr-021
- adr-007
- adr-002
- adr-024
- adr-048
- adr-115
- adr-116
- adr-120
- adr-122
- adr-145

## Related Requirements

- rac-growth-extensibility

## Related Designs

- third-party-artifact-extensibility

## Applies To

- rust/rac-engine/src/spec.rs
- rust/rac-engine/src/validate.rs
- rust/rac-engine/src/okf.rs
