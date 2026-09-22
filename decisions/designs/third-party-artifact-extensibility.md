---
schema_version: 1
id: RAC-KVTSPM9V9VDQ
type: design
tags: [extensibility, plugins, spec, registry, validation, rust, architecture]
---
# Design: Third-Party Artifact Type Extensibility

## Context

**Revised 2026-09-15** to match the revised ADR-083. The first version of this
design was the Python *how*: a `core/artifact_registry.py` seam, entry-point
discovery, a `warnings` channel, `importlib.resources` templates, and an
`ArtifactSpec` export. The engine is now Rust (ADR-116, ADR-120) and ADR-083's
mechanism is a pinned spec bundle; this revision replaces the Python mechanics
throughout. The registry-seam idea survives: the one genuinely invasive move is
still turning constant access into registry access, isolated as the inert
first step.

ADR-083 decides *that* the native engine discovers additional artifact types
from one JSON bundle in the shape of `artifact-specs.json`, pinned by path and
content digest in `.decided/config.yaml`; that built-ins win and bad or
colliding specs warn and skip; that custom types get generic structural
validation only, are not relationship targets, and add no edge kinds
(ADR-055); that a type's starter bodies are its template; that each type
declares (or defaults) its OKF export type; and that the determinism caveat
surfaces through `decided inspect`. This design is the *how*: the registry
seam, bundle loading and pin verification, the admission guard and its
warnings, the one predicate-keyed validation arm, scaffolding from the spec,
the OKF mapping, the fingerprint and serving-generation hooks, and the test
shape that proves the change is inert when no bundle is pinned.

## User Need

Two audiences, one mechanism:

- A **corpus owner** whose repository needs a sixth, domain-specific type (a
  `runbook`, a `policy`) wants `decided new`, `validate`, `inspect`, `schema`,
  `find`, `export`, and the MCP tools to handle it the way they handle a
  requirement — by committing one file and one pinned config stanza, reviewed
  in the same pull request as the artifacts, not by forking or rebuilding the
  engine.
- A **maintainer** wants a corpus that pins nothing to be provably untouched:
  same bytes out of every command, same cache keys, the `asdecided/spec`
  conformance tier and the live-corpus invariants green throughout (ADR-120).

Neither needs new analysis behaviour. The need is that the existing
deterministic, section-heading machinery becomes reachable by types the
binary was not built with.

## Design

### 1. The registry seam (load-bearing)

Every consumer today calls `crate::spec::specs()`, `spec_for(name)`, or
`available_schemas()`, each reading a process-global `OnceLock<SpecData>`
built from the embedded `artifact-specs.json`. Eighteen modules —
`classify`, `validate`, `frontmatter`, `scaffold`, `inspect`, `stats`,
`output`, `relationships`, `resolve`, `composition`, `federated_corpus`,
`graph_federated_corpus`, `parallel_build`, `delta_generation`,
`agent_rules`, `improve`, `sentry`, `commands` — bind that static. The design
rests on converting **static access into registry access**.

`spec.rs` gains:

- `pub struct Registry { specs: Vec<ArtifactSpec>, builtin_len: usize,
  warnings: Vec<Issue> }` — built-ins first in embedded order, then admitted
  bundle elements in bundle order; `warnings` carries every skip.
- `Registry::builtin()` — the embedded five and nothing else.
- `load_bundle(repository_root, &BundlePin) -> Result<Registry, SpecBundleError>`
  — built-ins plus the pinned bundle's admitted elements (sections 2 and 3);
  `sync_registry(start_dir)` reads the governing config, installs the
  built-ins when no stanza is present, and reloads only when the pin differs
  from the active one.
- Methods `specs()`, `spec_for(name)`, `available_schemas()`,
  `is_builtin(name)`, `warnings()`. The embedded parse helpers (`build_spec`,
  `str_list`, `list_map`, `str_map`) are reused verbatim for bundle elements,
  so one parser produces both.
- `ArtifactSpec` gains one appended optional field, `okf_type: Option<String>`
  (section 6); built-ins leave it `None`.

Installation: a process-wide slot `static ACTIVE: RwLock<Option<&'static
Registry>>`, `None` meaning the embedded registry. A loaded registry is leaked
and swapped in; the existing free functions become thin readers of it, so
their `&'static` return signatures and every consumer compile unchanged. Each
CLI command that takes a corpus, file, or output path calls `sync_registry`
at entry (`schema` and `templates` from the working directory);
`decided-mcp` calls it on every tool call so a re-pin lands on the next
request (section 7). The leak is bounded to one small registry per config change and
accepted in v1; threading an explicit `&Registry` through the read-model
constructors is the recorded follow-up. Commands that locate no corpus run on
the built-in registry.

### 2. Bundle loading and pin verification

The config section (ADR-083 decision 1):

```yaml
artifact_types:
  version: 1
  bundle:
    path: .decided/artifact-specs.json
    digest: sha256:<64 lowercase hex>
```

`version` must be `1`; `bundle` must be a mapping with exactly `path` and
`digest`. `path` is validated with the federation path rules already in
`federation.rs` (POSIX-relative, no absolute forms, backslashes, empty
segments, `.` or `..`; resolves to a contained regular file; symlinks and
reparse points rejected). The file is read as bytes, bounded at 1 MiB and
16,384 JSON nodes (the ADR-145 posture), hashed with the engine's `Sha256`,
and compared to `digest` byte-for-byte after the `sha256:` prefix. Only then
is it parsed with the same `serde_json` (`preserve_order`) path as the
embedded bytes; the top-level `artifact_specs` array is taken and `_meta` and
`relationship_descriptions` are ignored.

Any failure before admission — stanza malformed, path invalid, file missing,
symlinked, or over bound, digest invalid or mismatched, JSON invalid or not in
the registry shape — is a hard error (`SpecBundleError`, stable codes
`artifact-spec-bundle-config-invalid`, `-path-invalid`, `-missing`,
`-symlink-traversal`, `-limit-exceeded`, `-unreadable`, `-digest-invalid`,
`-digest-mismatch`, `-parse-failed`). `decided validate <dir>` renders it as
one error row for the bundle path and exits 1; every other command prints
`decided: <code>: <detail>` and exits 1; an MCP tool call returns an error
result. The registry is never partially loaded.

### 3. Admission and warnings

Each element is admitted in bundle order through one guard:

1. **Name** — matches `^[a-z][a-z0-9_-]{1,31}$`; is not `unknown`; is not a
   built-in name (built-ins always win); has not already been admitted (first
   wins).
2. **Shape** — a JSON object with no keys outside the element shape (the
   twelve registry keys plus `okf_type`); absent optional keys default to
   empty.
3. **Contract** — `display` non-empty; `required` non-empty; every section
   name in `required`, `recommended`, `optional`, the synonym targets, and
   the map keys already normalised (trimmed, casefolded, single-spaced);
   every metadata value list non-empty strings; `retired_status` a subset of
   `metadata["status"]` when both are present; `okf_type`, when present, a
   non-empty trimmed string.

A rejected element is skipped and recorded as one `Issue` with code
`artifact-spec-skipped`, severity `warning`, naming the bundle path, the
element's `name` (or its index when the name is unusable), and the reason —
including the built-in or earlier name collided with. `decided validate <dir>`
and `decided doctor` merge `registry.warnings()` into their reports ahead of
per-artifact issues; warnings are ordered by bundle position and are never
free text on stderr. No other command's exit code depends on them. A skipped
bundle or element is not silent in practice: every artifact whose frontmatter
`type:` names the unregistered type fails `invalid-metadata-field` through
the existing path, so the corpus gate goes red where it should.

### 4. Validation routing

`validate::validate` keeps its five named arms and replaces the bare fallback
with one predicate-keyed arm:

```text
other => match registry.spec_for(other) {
    Some(spec) if !registry.is_builtin(other) => validate_generic(artifact, spec),
    _ => validate_requirement(artifact),   // unknown/legacy fallback, unchanged
}
```

`validate_generic` is pure composition of the existing shared helpers —
`validate_title`, `validate_required_sections(spec)`,
`validate_status_metadata(spec)` — and adds no logic; it is the shape
`validate_prompt` and `validate_design` already have (ADR-060). The named
validators are deliberately not collapsed into it. The branch keys on
`is_builtin`, never a type name, so REQ-004 holds. `validate_metadata`'s
identity check and `validate_ticketing_references` already take the spec from
the registry; `frontmatter.rs`'s `type:` admission already calls `spec_for`,
so a bundled type is accepted in frontmatter the moment the seam is in.

### 5. Scaffolding from the spec

`scaffold.rs` keeps `TEMPLATE_BYTES` for the five built-ins. `load_template`
becomes: built-in index → embedded bytes (unchanged); bundled →
`output::render_schema_template(spec)`, the generator `decided schema <type>
--template` already uses, rendered once and returned; unregistered →
`TemplateNotFound` (unchanged). `create_artifact` and `quickstart` follow it;
the `<type>s` family directory rule is unchanged, so `decided new runbook
decisions/runbooks/deploy.md` is the natural call. `cmd_schema` and
`cmd_templates` read `available_schemas()` from the registry and list bundled
types after the built-ins with no change; `render_schema_json` gains the
`okf_type` key only when set (ADR-007).

### 6. OKF export mapping

`okf::okf_type(rac_type)` becomes a registry lookup: a built-in returns its
fixed row from the profile table; a bundled type returns its element's
`okf_type`, or its `display` when unset; an unregistered type returns `None`
and the artifact takes the existing unknown-type exclusion, retiring the
panic. The OKF `index.md` gains one section per bundled type after the five
fixed sections, in registry order, headed by the OKF type name; the five fixed
sections are byte-identical for a corpus that pins nothing. `docs/okf-profile.md`
gains the "declared type → its `okf_type`" row and the qualifier ADR-083
decision 6 records. `okf_type` ships upstream in `asdecided/spec` as an
optional appended element field before the engine reads it.

### 7. Fingerprints, freshness, and serving generations

The "governing config bytes" notion widens to a **governing config set**: the
config file plus, when pinned, the bundle file. Concretely:

- The validation store is keyed by the config bytes already, and a re-pin is
  a config change, so it misses on a re-pin with no further change.
- `corpus_hash_from_complete_manifest` appends the pinned digest to the
  preimage when a bundle is active, so the derived store and the serving
  generation are keyed to the registry; with no bundle the preimage is
  byte-identical to before.
- `decided-mcp` re-reads the pin on every tool call and reloads the registry
  when it changed; the bundle digest is folded into the corpus hash that keys
  the derived store and the freshness tracker's generation, so a re-pin
  rebuilds the read model on the next request. The built-in registry is
  installed for a corpus with no pin.
- The federation digest preimage (ADR-134, ADR-145) is unchanged. A
  parent's raw `.decided/config.yaml` bytes are already framed in it, and
  that config pins the parent's bundle digest, so a parent cannot change its
  types without changing the digest the child pinned. In a federated closure
  the corpus hash folds every effective bundle digest in composition order
  (section 10), not only the local one; the closure generation is already
  keyed to every node's config bytes (ADR-148) and needs nothing new.

### 8. Sequencing and the smallest reversible step

1. **Registry seam, built-ins only** — `Registry`, the slot, the thin free
   functions, `okf_type: None` on `ArtifactSpec`. Provably inert: the
   `asdecided/spec` conformance tier, the live-corpus invariants, cache-on ==
   cache-off, and the freshness regression all pass unchanged (ADR-120).
2. **Bundle loading and pin verification** — section 2.
3. **Admission and warnings** — section 3; `validate` and `doctor` surface
   them.
4. **Validation routing** — section 4.
5. **Scaffolding** — section 5.
6. **OKF mapping** — section 6, after `okf_type` lands upstream.
7. **Fingerprints, freshness, serving** — section 7.
8. **Fixtures and tests** — section 9.
9. **Docs** — `docs/validation.md` (the stanza and the two warning codes),
   `docs/cli.md` (`new`, `schema`, `templates` with a bundled type),
   `docs/okf-profile.md` (the mapping row), and a worked `runbook` bundle.

Each step is revertible on its own; step 1 is the acceptance gate for the
rest.

### 9. Test shape

- **Inert-when-absent**: every existing battery unchanged after step 1 and
  after step 9 for a corpus with no `artifact_types` stanza (the live
  `decisions/` corpus is exactly that).
- **Fixture corpus** `rust/fixtures/spec-bundle/`: a pinned bundle declaring
  `runbook` (with `okf_type: Runbook`) and `policy` (no `okf_type`); one valid
  artifact of each; one of each missing a required section.
- **Negative boundary tests per new type**: a `runbook` with prompt-like
  headings does not classify as `prompt` and a prompt does not classify as
  `runbook`, with `inspect`'s fit breakdown explaining the winner; a `policy`
  document carrying only built-in relationship sections classifies `unknown`.
- **Skip negatives**: digest mismatch, missing file, oversized file,
  symlinked path, invalid JSON, built-in collision, duplicate name, bad name,
  unknown key, unnormalised section, empty `okf_type` — each yields exactly
  one warning with the expected code, and the remaining elements (or, for a
  bundle-level failure, the built-ins) stay admitted; `find` and `resolve`
  exit 0.
- **Contract pins**: `relationship-target-type-mismatch` for a built-in
  reference to a `runbook`; OKF export emits the `runbook` under `Runbook` and
  the `policy` under `Policy`, with the five fixed index sections
  byte-identical; `decided new runbook` output passes `decided validate`.
- **Freshness**: re-pinning the bundle flips the validation-store fingerprint
  and the derived-cache generation; a running `decided-mcp` rebuilds its
  generation and `get_summary` reflects the new registry.
- **Federated composition (section 10)**: a child with a parent that declares
  `runbook` classifies, validates, exports, and serves the parent's runbooks
  as runbooks; an identical element declared by two sources is silent; a
  same-name, different-content element is
  `corpus-federation-artifact-type-conflict` on every command and MCP tool;
  a Decision-backed override resolves it and a missing, inherited, retired,
  or non-Decision rationale, a duplicate name, a `prefer` outside the
  inherited view, and an override for a name that does not collide each fail
  with `corpus-federation-invalid-override`; a parent bundle whose bytes no
  longer match the digest in the parent's captured config fails composition
  with the parent's source in the message; a closure in which no source pins
  a bundle is byte-identical to before.

### 10. Federated registry composition (ADR-150)

ADR-150 makes the effective registry closure-dependent. The mechanism is one
function over the verified federation, called from the two places the engine
already composes a closure, so the CLI and `decided-mcp` cannot disagree.

**Inputs.** For every source in the verified closure (the root and each
`VerifiedFederationNode`, or the child and its one `VerifiedParent` on the
version-1 path): its global source, layer, repository root, captured config
bytes, and its direct parents in the canonical order the manifest loader
fixes for `parents`. That canonical order is the order every other consumer
of the manifest already uses; the raw byte order is authenticated but not
semantic, so the ADR's "declaration order" is realised as the loader's
canonical order rather than a second ordering rule. Order never changes which
types exist or their content (a different-content duplicate is an error), only
the registry order of inherited types and the listing order of provenance.

**Stanza.** `artifact_types` gains an optional `overrides` list beside
`bundle`; `bundle` becomes optional when `overrides` is present, so a child
that pins nothing of its own can still adjudicate between its parents. Unknown
keys under `artifact_types` are `artifact-spec-bundle-config-invalid`, as they
already are under `bundle`. Each override is `{name, prefer, rationale}`:
`name` a type name, `prefer` the literal `local` or a global source, and
`rationale` a canonical Decision identifier.

**Composition.** `effective(source)` is computed bottom-up with a per-source
memo:

1. Parse the source's stanza from its captured config bytes. A pin is read and
   verified (path containment, size, digest, node budget) and admitted
   exactly as section 2 and 3 do for the local corpus; the admitted elements
   are tagged with the source that declared them. A parent-side failure keeps
   its `artifact-spec-bundle-*` code and reports the parent's source.
2. Candidates are the source's own admitted elements, then, for each direct
   parent in canonical order, that parent's effective elements (recursively;
   an element inherited through a parent keeps its original declaring
   source).
3. Group candidates by name in order of first appearance. One distinct
   content per name is admitted as is; elements that compare equal as
   admitted data are silent duplicates (map-like fields compare as maps, lists
   in order, because list order is rendered order), so key order and
   whitespace in a bundle file cannot manufacture a conflict. Two or more distinct contents
   are a collision: with no override at this source the composition stops
   with `corpus-federation-artifact-type-conflict`, naming the type and every
   declaring source; with an override, the candidate declared by `prefer`
   (`local` meaning this source) wins and the rest are dropped.
4. An override at this source that names a non-colliding name, repeats a
   name, prefers a source outside this source's transitive parents, or prefers
   a source that declares no candidate for the name is
   `corpus-federation-invalid-override`, the finding the artifact-override
   family already uses for a malformed ADR-137 declaration.
5. The effective registry is the built-ins in registry order followed by the
   surviving elements in first-appearance order. The winner of a collision is
   what descendants inherit through this source; a descendant that declares
   yet another content for the name, or that reaches a losing declaration
   again through another parent (a diamond), collides afresh and needs its
   own override, mirroring the override chains and explicit diamond
   convergence of ADR-147. A middle node's resolution governs its own view
   only; it is not carried past a sibling path.

**Rationale check.** `rationale` must resolve to exactly one live local
Decision of the declaring source: one item whose origin is that source and
whose canonical identifier matches ignoring case (as ADR-137 rationales do),
classified `decision`, and live by the same predicate the artifact overrides
use. An inherited source's corpus is captured whole; the root's captured
files may be only the command's directory or `--top-level` scope, so a
root-owned rationale not found there is resolved over a walk of the root's
working tree with every materialised parent excluded. An override is
config-level policy, so its validity cannot depend on the directory a
command names. Items only exist after the
closure's files are parsed, so this check runs immediately after parsing in
both composition functions; a failure is `corpus-federation-invalid-override`
and fails the composition like any other override defect. The registry is
installed before parsing so inherited types classify; a failed rationale
check stops the run before anything is served, so the ordering is not
observable.

**Process slot.** Composed registries are memoised by a key over the root
source and, per source in source order, its identity, layer, config bytes,
and canonical parent list: the configs fix the pins and the overrides, and
the parent lists fix the topology, which the manifests carry rather than the
configs, so a changed graph under byte-identical configs recomposes. The
local-only registry of section 1 keys on its one `(source, config bytes)`
frame. Bundle bytes are re-read and re-hashed against their pin on every
sync before the memo is consulted, so a bundle edited without a re-pin fails
closed on the next command or request rather than serving the previous
registry. `sync_registry` keeps an installed federated registry while the
local config bytes are unchanged and the manifest is still present, and
composition replaces it whenever the closure's key changes.

**Where it runs.** `graph_federated_corpus::compose_verified_federation`
(version 2) and `federated_corpus::compose_verified_generation_from_snapshot`
(version 1) install the effective registry before parsing any file and run
the rationale check after; `decided-mcp` re-verifies and recomposes on every
request, so a parent re-pin lands on the next call. `decided new` and
single-file `validate` inside a federated repository compose the closure
first so an inherited type scaffolds and validates; `schema` and `templates`
list the effective registry of the corpus given with `--corpus <dir>` and the
local registry without it, which the CLI reference records.

**Provenance and keys.** `validate --json` gains `artifact_spec_bundles`: one
entry per source that pinned a bundle, in composition order, each with
`source`, `layer`, `path`, `digest`, `admitted`, and `warnings`; the ADR-083
`artifact_spec_bundle` object stays for the local bundle so existing readers
are unchanged (ADR-007). The human renderer adds one `WARN` block per
inherited bundle with skipped elements, labelled with its source; `doctor`
carries the source in each inherited `artifact-spec-skipped` finding. The
corpus hash folds every effective bundle digest in composition order, one
frame each, so a corpus with exactly one local bundle keeps today's preimage.

## Constraints

- **Inert when absent (REQ-001).** No stanza → byte-identical output, exit
  codes, and cache keys; the conformance tier and live-corpus invariants are
  the referee (ADR-120).
- **One parser, one shape.** Bundle elements go through the embedded
  registry's parse helpers and match the element shape `asdecided/spec`
  publishes (ADR-115); no Rust-only field.
- **Determinism (ADR-002).** No semantic scoring; bundle order is authored;
  warnings are ordered; no environment or home-directory input; the pin is
  verified before parsing.
- **Additive contracts (ADR-007).** Count maps, `schema --json`, and the OKF
  index gain keys or sections only when a corpus pins a bundle; no
  `schema_version` bump.
- **No new dependency (ADR-114, REQ-005).** `serde_json`, the bounded YAML
  reader, the engine's `Sha256`, and the federation containment helpers
  suffice; no bundle code is ever executed.
- **Federation rules, not a second precedence model (ADR-150).** Inherited
  types compose bottom-up with no precedence; a collision is an error resolved
  only by a Decision-backed override declared by the corpus that owns the
  resolution; the pin the child already holds covers the parent's bundle.
- **Deferred boundaries.** No new edge kinds (ADR-055); no custom validators;
  a descendant cannot un-inherit a type short of overriding it; any public
  invitation stays behind GATE-2 (ADR-071).

## Rationale

The seam-first order isolates the only invasive move — eighteen modules
moving from a static to a slot — behind an inert, fully certified step, after
which every bundle semantic is additive and locally testable. Reusing the
embedded parser makes "one format" true rather than aspirational: an element
that admits is, byte for byte, a valid registry element, so promotion upstream
is a file move. Verifying the pin before parsing means a stale or edited
bundle can never partially load. Composing the ADR-060 validators for
`validate_generic` gives a bundled type correct structural checks with no new
code and no type-named branch. Rendering the template from the spec removes a
second source of starter text that could disagree with the spec. Widening the
fingerprint set reuses the machinery that already keys on severity overrides
instead of adding a freshness rung. Leaving the federation digest untouched
keeps every existing pin valid and makes bundle propagation its own decision.

## Alternatives

- **Thread `&Registry` explicitly through every consumer now.** Deferred: the
  cleaner end state, but it changes `classify`, `validate`, and read-model
  signatures in the same change as the seam; the slot keeps step 1 mechanical.
- **Collapse the four spec-driven validators into `validate_generic`.**
  Deferred: correct eventually, but it churns finding order for built-ins.
- **Load the bundle without verifying the digest, warning on mismatch.**
  Rejected: a mismatched bundle is by definition not the reviewed one; loading
  it and warning would make the pin advisory.
- **Bump the federation digest to cover a parent's bundle now.** Rejected for
  v1: it invalidates every existing pin for a capability the child does not
  yet consume from the parent.

## Accessibility

Not applicable — a code-level extension mechanism with no user-facing UI
beyond existing CLI output, whose form is unchanged. Bundle-authoring
documentation follows the repository's readable-prose conventions.

## Style Guidance

- The registry type is `spec::Registry`; the slot accessor is `spec::install`;
  the free functions keep their names.
- Warning codes are `artifact-spec-bundle-skipped` (whole bundle) and
  `artifact-spec-skipped` (one element); the config stanza is
  `artifact_types` with `version` and `bundle { path, digest }`.
- The validation branch stays predicate-keyed — `is_builtin`, never a named
  custom type; the generic validator is `validate_generic`.
- The element field is `okf_type`; it reads as data, not behaviour.

## Open Questions

- Whether `decided` should offer a pin helper (print the `sha256:` digest of
  a bundle file) to make re-pinning a one-liner; a CLI nicety, not a decision.
- When to replace the leaked-registry slot with an explicitly threaded
  `&Registry` owned by the serving generation. The leak is now bounded to one
  registry per distinct pin a process has served (registries are memoised by
  pin), so the remaining cost is the `&'static` shape, not growth.
- The trigger that schedules implementation, and the separate GATE-2 trigger
  for any public ecosystem invitation.

## Related Decisions

- adr-083
- adr-150
- adr-137
- adr-144
- adr-146
- adr-147
- adr-148
- adr-052
- adr-055
- adr-060
- adr-115
- adr-120
- adr-021
- adr-007
- adr-002
- adr-112
- adr-114
- adr-122

## Related Requirements

- rac-growth-extensibility
