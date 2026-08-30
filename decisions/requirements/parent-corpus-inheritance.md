---
schema_version: 1
id: RAC-KWJ8S53D06CH
type: requirement
---
# Requirement: Parent Corpus Inheritance

## Status

Proposed

Classification: `[internal]` — declare and verify one direct, read-only parent
corpus from materialised bytes. This is the declaration and materialisation
half of the `corpus-federation` programme.

## Problem

The released engine has one corpus root. It cannot validate a child reference
to a firm-wide decision, route that decision into child work, or apply the
decision's mechanical constraints to child code. ADR-089 accepts a pinned,
offline parent under strict provenance and write-boundary constraints, but the
original proposal left the manifest home, parent cardinality, transitivity,
pin shape, and parent-validation behaviour unresolved.

## Requirements

- [REQ-001] A child corpus MUST declare inheritance through a pinned source reference under the exact lowercase heading `## inherits` in the fixed operational Markdown manifest `.decided/corpus.md` (ADR-089). The section MUST contain exactly one fenced YAML mapping with a full SHA-256 digest, and the manifest MUST remain outside the artifact walk, search index, relationship graph, and exports.
- [REQ-002] Parent resolution MUST read only materialised bytes already on disk: it MUST NOT clone, pull, fetch, refresh, or perform network I/O in validate, resolve, retrieve, enforce, export, or serve paths (ADR-002, ADR-089). Git submodules and vendored directories are supported only because both present ordinary local bytes; refreshing and repinning them remain explicit user Git operations outside AsDecided.
- [REQ-003] The parent MUST be an inherited, read-only layer: no AsDecided command may write, rename, migrate, scaffold, install generated rules, or recover a transaction under the parent root, and the child remains the only writable canonical state (ADR-018, ADR-065, ADR-080).
- [REQ-004] Validation MUST emit deterministic, distinct, stable-coded errors for missing materialisation, missing parent corpus, malformed declaration, source mismatch, unsupported transitivity, and digest mismatch. No parent artifact may enter the effective corpus after any verification failure.
- [REQ-005] The declaration MUST remain backward-compatible under ADR-089: engines predating federation continue to ignore `.decided/corpus.md` through the existing hidden-directory walk. An implementing engine MUST add no output or behaviour change without that manifest, measured against the contemporaneous single-corpus engine after its prerequisites.
- [REQ-006] Optional configuration defaults MAY assist materialisation discovery only through the established section-loader pattern; the Markdown manifest MUST remain the inheritance and pin source of truth and configuration MUST NOT silently create or update it (ADR-089).
- [REQ-007] The capability MUST be available to every user and MUST NOT be enterprise-gated (ADR-085, ADR-089). It MUST ship only within the human-accepted boundary of ADR-133 through ADR-143; this requirement does not substitute for those decisions.
- [REQ-008] Under ADR-088, when the mechanism ships, `decided init --parent-corpus` MUST emit deterministic parent-declaration guidance only when explicitly requested, with or without a profile and for fresh or already-initialised repositories. The guidance MUST name `.decided/corpus.md`, the exact lowercase `## inherits` and `## overrides` headings, parent materialisation before pinning, and `decided corpus digest`; it MUST NOT create a manifest, fetch anything, or write parent bytes. Init and profile files plus human and JSON output without the flag MUST remain byte-identical, and JSON MUST add the guidance field only when requested.
- [REQ-009] The declaration MUST contain `version`, a child-local parent `alias`, the parent's configured `source`, a repository-relative materialisation `root`, a parent-relative `corpus` directory, and a full lowercase `sha256` digest. The child and parent MUST each declare an explicit, non-empty, distinct `corpus.source` under `corpus-source-identity`.
- [REQ-010] The first federation increment MUST accept exactly one direct parent. A second parent and a materialised parent that itself declares inheritance MUST produce distinct stable error findings before overlay; neither case may be flattened or partially interpreted.
- [REQ-011] The parent materialisation MUST remain inside the canonical child repository root. Absolute paths, `..` components, unresolved paths, symlinks in the path to the parent config or any discovered artifact, and canonical paths outside the child repository MUST be rejected before those bytes enter the digest or corpus. Once verified, the materialisation subtree MUST be excluded from the local artifact walk.
- [REQ-012] Before overlay, the engine MUST verify the parent source identity and a canonical versioned SHA-256 corpus digest over a domain separator, parent source, governing parent `.decided/config.yaml` bytes, and sorted corpus-relative Markdown paths and bytes. The digest MUST exclude checkout location, timestamps, and filesystem iteration order, and the same pure calculation MUST be available to operators.
- [REQ-013] The parent MUST pass structural and relationship validation before overlay. A parent error MUST surface in the child as one sourced `parent-corpus-invalid` error and block overlay; parent warnings and review advisories MUST remain parent-owned and MUST NOT be repeated in every child.

## Acceptance Criteria

- A child with one vendored or submodule-backed parent validates fully with
  networking disabled and resolves the same bytes across two clones.
- Removing the parent, changing its governing config or one parent Markdown
  byte without updating the digest, or adding a parent-of-parent declaration
  produces the corresponding stable error and no effective overlay.
- Absolute, escaping, and symlink-traversing parent paths are rejected, and
  tests prove that every command leaves the complete parent tree
  byte-identical.
- A parent with an error yields one sourced child error and no inherited
  results. Parent warnings do not duplicate into the child report.
- A repository without the manifest preserves all contemporaneous
  single-corpus CLI, MCP, export, and cache goldens byte-for-byte.

## Success Metrics

- A repository can pin one firm-wide standards corpus in an ordinary reviewed
  diff and validate it completely offline.
- Updating the parent is an explicit materialisation and pin change; no command
  silently changes the governing bytes.

## Risks

- Pin verification is skipped for speed. Mitigation: verification precedes
  overlay and the composite generation/cache key includes the verified digest.
- A repository-relative path still escapes through a symlink. Mitigation:
  canonical containment and symlink-root rejection occur before walking.
- Repeated parent warnings create fleet-wide noise. Mitigation: only a parent
  error blocks composition; the parent owns its warnings and review queue.
- One-parent scope is mistaken for the permanent model. Mitigation: multiple
  and transitive parents are explicit future decisions, not silently accepted
  syntax.

## Assumptions

- A submodule or vendored directory inside the child repository covers the
  first real adoption path and keeps clones reproducible.
- The source identity requirement lands before parent records reach exports or
  MCP.
- ADR-133, ADR-134, ADR-135, ADR-138, and ADR-143 are separately ratified before
  parent loading, identity, composition, or cache behavior ships.

## Related Decisions

- adr-002
- adr-016
- adr-018
- adr-055
- adr-065
- adr-080
- adr-085
- adr-088
- adr-089
- adr-117
- adr-133
- adr-134
- adr-135
- adr-138
- adr-143

## Related Designs

- corpus-federation-mechanism

## Related Roadmaps

- corpus-federation

## Related Requirements

- corpus-source-identity
- federated-resolution-provenance
