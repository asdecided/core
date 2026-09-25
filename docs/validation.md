# Validation: overrides & SARIF

`decided validate` is AsDecided's write-time gate: it fails when any artifact carries an
error-severity finding, and (over a directory) when the corpus is not a
conformant OKF v0.2 bundle. Two features make that gate adoptable in CI on a
real, pre-existing repository.

## Per-type standards checks

Beyond structure, `decided validate` lints each type against the standards it cites
(ADR-056) — all deterministic, no AI:

| Code | Severity | Standard |
| --- | --- | --- |
| `requirement-normative-keyword` | error | BCP-14: only uppercase MUST/SHALL/SHOULD/MAY are normative; lowercase is ambiguous. |
| `requirement-not-singular` | warning | ISO 29148: one normative statement per requirement line. |
| `requirement-non-ears` | warning | EARS: a requirement must state a normative response (SHALL/SHOULD/MAY). |
| `requirement-ears-clause` | warning | EARS: a sentence-initial `If …` needs a `then` response clause. |
| `invalid-roadmap-horizon` | error | A `## Horizon` value must be `now`/`next`/`later` or a quarter (e.g. `Q3 2026`); the section is optional. |
| `roadmap-no-advancement-link` | warning | A roadmap should link a `## Related Requirements` or `## Related Decisions` it advances. |

The BCP-14 error is the only gate-breaker; the rest are warnings, and all are
overridable below. (AsDecided's own corpus predates these checks and disables them in
its `.decided/config.yaml` — the warnings-first path in action.)

## Severity overrides (warnings-first onboarding)

Pointing `decided validate` at a legacy corpus for the first time can surface many
pre-existing findings at once. Rather than fail the build on all of them, a
repository can downgrade or silence specific findings in its committed
`.decided/config.yaml`, then tighten the gate over time. The decision behind this is
[ADR-053](https://github.com/asdecided/core/blob/main/decisions/decisions/adr-053-validation-severity-overrides.md).
Overrides are **repository-wide**: a downgrade applies to `decided review`,
`decided watchkeeper`, and `decided portfolio` as well as `decided validate`.

Add an optional `validation` section:

```yaml
repository_key: RAC

validation:
  rules:                 # rule code -> error | warning | off
    ambiguous-verb: off
    too-many-requirements: warning
  types:                 # artifact type -> error | warning  (a ceiling)
    roadmap: warning
```

- **`rules`** sets a finding's severity by its stable code (the `[code]` shown in
  `decided validate` output, e.g. `invalid-decision-status`). `off` suppresses the
  finding entirely.
- **`types`** caps a whole artifact type at `error` or `warning`. A `warning`
  ceiling downgrades that type's errors so they no longer fail the run.
- **Precedence:** a per-rule entry is more specific and **wins** over the
  per-type ceiling — so a downgraded type can still force one rule back to
  `error`.

The config is committed and versioned, so CI and every teammate share the same
policy (a per-developer file would not keep CI green). Determinism holds: the
same corpus *and config* yield the same findings and exit code. An absent
`validation` section is a pure no-op — the default gate is strict.

Overrides are repository-wide (ADR-053): a downgrade applies to `decided review`,
`decided watchkeeper`, and `decided portfolio` as well as `decided validate`, so a
warnings-first policy is consistent across every surface.

A typical onboarding path: start by capping noisy types to `warning`, get CI
green, then remove entries (or restore `error`) rule-by-rule as the corpus is
cleaned up.

## Custom artifact types (spec bundles)

The five built-in types come from the shared registry that ships inside the
binary. A repository can add its own types — a `runbook`, a `policy` — by
committing one **spec bundle**: a JSON file in the same shape as
`artifact-specs.json` (a top-level `artifact_specs` array of type elements),
pinned by path and content digest in `.decided/config.yaml` (ADR-083):

```yaml
artifact_types:
  version: 1
  bundle:
    path: .decided/artifact-specs.json
    digest: sha256:<sha256 of the bundle's raw bytes, 64 lowercase hex>
```

Each element carries the registry keys — `name`, `display`, `required`,
`recommended`, `optional`, `metadata`, `retired_status`, `descriptions`,
`guidance`, `synonyms`, `id_field`, `starter_bodies` — plus an optional
`okf_type`, the OKF `type` the export writes for that artifact type (default:
the element's `display`). Section names must already be normalised (trimmed,
lower-case, single-spaced). `display` and `okf_type` are single-line labels
with no surrounding whitespace, and `okf_type` may not be one of the built-in
OKF types (`Requirement`, `ADR`, `Design`, `Roadmap`, `Prompt`).
`descriptions` and `starter_bodies` map sections to strings and `guidance`
maps sections to lists of strings. A `name` may not be `unknown` or a key the
engine already emits beside the per-type families in `decided stats --json`
(for example `decisions`, `invalid`, `metrics`). `synonyms` map an
alternative heading to a canonical section for classification only: as for
the built-ins (SPEC §6.6), `decided validate` still requires each `required`
section under its canonical heading, so a synonym helps a document be
recognised as the type but does not satisfy a required section. The digest
is `sha256sum` of the file; edit the bundle, then re-pin.

What the engine does with it:

- **Built-ins always win.** The merged registry is the five built-ins in their
  fixed order, then admitted bundle elements in file order, so classification
  tie-breaks are stable. A bundle type classifies a document only when no
  built-in qualifies for it, so a bundle can never take a built-in artifact's
  type away. An element whose `name` collides with a built-in,
  duplicates an earlier element, or fails the structural contract is skipped
  with a warning-severity `artifact-spec-skipped` finding in `decided validate`
  and `decided doctor`; nothing else changes.
- **A pin that cannot be honoured is a hard error.** A config that declares
  `artifact_types` but cannot be parsed, a malformed stanza, a
  missing or symlinked bundle, an oversized or unparseable file, or bytes that
  do not match the pinned digest fail `decided validate` with one error row for
  the bundle (`artifact-spec-bundle-digest-mismatch` and its siblings, exit 1);
  every other command refuses with `decided: <code>: <detail>` and exit 1, and
  MCP tool calls return an error. This is the federation pin's standard. The
  decisions-only commands `decided decisions-for` and `decided herald` read
  decision artifacts alone and do not load the bundle. `decided watchkeeper`
  compares both sides under the working tree's registry, because a base
  revision is materialised from the corpus path without its config.
- **Custom types are structural only.** They classify, validate (title,
  required sections, status metadata — no bespoke rules), appear in `schema
  --list`, `templates`, `inspect`, `stats`, `find`, exports, and every MCP tool,
  and `decided new <type>` scaffolds them from their starter bodies. They are
  not relationship targets and add no edge kinds (ADR-055): a built-in
  `## Related Decisions` reference to a custom-type artifact still reports
  `relationship-target-type-mismatch`. `decided doctor` therefore still
  counts an unreferenced custom-type artifact as `orphaned-artifact` (the
  portfolio's orphan count is unchanged) but advises that no action is
  needed. Every per-type listing — `stats --json` families and the MCP
  `get_summary` `by_type` counts — puts custom types after the built-ins in
  registry order.
- **`validate --json` reports the bundle.** `artifact_spec_bundle` carries the
  local bundle's `path`, `digest`, `admitted` names, and `warnings`, and
  `artifact_spec_bundles` lists one entry per source that pins a bundle, with
  its `source` and `layer`: in an unfederated corpus, the local bundle alone.
  Both keys are absent without a stanza.
- **Without the stanza nothing changes.** Every command's bytes, exit codes,
  and cache keys are identical to an engine with no bundle support.

The repository's own `rust/fixtures/spec-bundle/` is a worked example with a
`runbook`, a `policy`, and a deliberately colliding element.

### Inherited types across a federation (ADR-150)

In a federated repository a child inherits its parents' admitted bundle
types. The effective registry is the five built-ins, then the child's own
bundle elements in file order, then, for each parent in the manifest's
canonical `parents` order, that parent's effective registry beyond the
built-ins, recursively. A parent that declares `runbook` and publishes
runbooks therefore has them classified, validated, searched, exported, and
served as runbooks in every child, under the pin the child already holds: the
parent's `.decided/config.yaml` bytes are framed in the federation digest and
carry the bundle's own digest, so no new pin is needed and a parent cannot
change its types without changing the digest the child verifies.

- **Identical declarations are silent; different ones are an error.** Two
  sources declaring the same element content are one type; content is
  compared as data, so JSON key order does not matter while list order (the
  rendered order of sections and values) does. Two sources
  declaring the same name with different content stop every command and MCP
  tool with `corpus-federation-artifact-type-conflict`, naming the type and
  the sources, in the same failure class as a duplicate parent or a cycle.
  Nearest-wins, first-wins, and field merging are all refused.
- **The only resolution is a Decision-backed override.** The composing corpus
  records which declaration wins in its own stanza:

  ```yaml
  artifact_types:
    version: 1
    bundle:                      # optional when only overrides are declared
      path: .decided/artifact-specs.json
      digest: sha256:...
    overrides:
      - name: runbook
        prefer: acme/standards   # `local`, or a parent in the inherited view
        rationale: APP-KWJ9D3C1S10N   # a live local Decision
  ```

  `rationale` must resolve, ignoring case, to exactly one Accepted, unretired
  Decision of the declaring corpus. An override is config-level policy, so the
  Decision is looked up across that whole corpus (a materialised parent's tree
  excluded) whatever directory or `--top-level` scope a command names. A name
  may be overridden at most once; an override for a
  name that does not collide, a `prefer` outside the corpus's transitive
  parents, or a preferred source that declares no candidate is
  `corpus-federation-invalid-override`, the finding artifact overrides already
  use. The winner is what the corpus's descendants inherit through it; a
  descendant that declares yet another content, or that reaches the losing
  declaration again through another parent (a diamond), collides afresh and
  needs its own override, which may prefer any source in its inherited view.
- **A parent's bundle is verified on every command.** Its bytes are checked
  against the digest in the parent's captured config before any element is
  admitted; a mismatch is the parent-side `artifact-spec-bundle-digest-mismatch`,
  reported with the parent's source, and fails composition.
- **Provenance is per source.** `decided validate --json` gains
  `artifact_spec_bundles`: one entry per source that pinned a bundle, in
  composition order, with `source` (`null` for a local bundle whose config
  declares no `corpus.source`), `layer`, `path`, `digest`, `admitted`, and
  `warnings`. The single `artifact_spec_bundle` object stays for the local
  bundle. The human output adds one `WARN` block per inherited bundle with
  skipped elements, and `decided doctor` names the source in each inherited
  `artifact-spec-skipped` finding.
- **Cache keys follow the effective registry.** The corpus hash folds every
  effective bundle digest in composition order, so a re-pin anywhere in the
  closure rebuilds cached classification. A running `decided-mcp` re-verifies
  every pinned bundle in the closure on each request and rebuilds its served
  model when the registry changes, so a re-pin lands on the next call. While
  a type override is applied it also re-checks the override's rationale on
  each request, wherever in the corpus that Decision lives, so retiring it
  fails the next call.
- **Unchanged boundaries.** Inherited types are structural only, are not
  relationship targets, and add no edge kinds; a parent's bundle cannot
  override a built-in. `decided schema --list` and `decided templates` list
  the local registry, or the effective registry of the corpus given with
  `--corpus <dir>` (see the CLI reference); `decided new` and
  single-file `decided validate` compose the closure of the top-level
  directory that holds their target first, so an inherited type scaffolds and
  validates. A closure in which no source pins a bundle is byte-for-byte
  unchanged.

`rust/fixtures/spec-federation/` is the worked example: a child that declares
`policy` and inherits `runbook` from a pinned `standards` parent.

## SARIF output for GitHub Code Scanning

`decided validate <dir> --sarif` emits a [SARIF 2.1.0](https://json-schema.org/)
document covering core validation findings and OKF conformance findings, so a CI
job can upload it and have GitHub Code Scanning annotate findings inline on a
pull request. The decision behind this is
[ADR-054](https://github.com/asdecided/core/blob/main/decisions/decisions/adr-054-sarif-validation-output.md).

```bash
decided validate decisions/ --sarif > rac.sarif
```

- `--sarif` is mutually exclusive with `--json`, and applies to directory
  validation only (single-file `--sarif` is a usage error).
- Severity maps to the SARIF `level` (`error`/`warning`); suppressed (`off`)
  findings never appear. A finding's line becomes a `region` when known.
- Output is deterministic and offline: results are sorted, no timestamps are
  emitted, and the same corpus state produces a byte-identical document.

The exit code is unchanged by the output format: `decided validate` still exits `1`
when an error-severity finding remains after overrides, and `0` otherwise.

A worked example of the output is checked in at
[`docs/examples/rac-validate.sarif.json`](examples/rac-validate.sarif.json): one
`error` (an out-of-enum decision status), two recommended-section `warning`s, and
a line-anchored `ambiguous-verb` finding (note the `region.startLine`).

### Relationship and review findings (`--sarif`)

The same SARIF envelope is emitted by the two other repository-level checks, so a
CI gate can surface cross-artifact integrity and review findings inline alongside
validation (v0.21.13):

```bash
decided relationships decisions/ --validate --sarif > relationships.sarif
decided review decisions/ --sarif > review.sarif
```

- `decided relationships --validate --sarif` annotates each broken, ambiguous,
  self-referencing, retired-target (superseded), wrong-type, cyclic, or
  duplicate-identifier finding on the referencing artifact. Referential-integrity
  and graph-shape breakages map to `error`; advisory findings (self-reference,
  unsupported edge, retired-target reference) map to `warning`. `--sarif` requires
  `--validate`, and the exit code is unchanged: `1` when any finding is present.
- `decided review --sarif` annotates each prioritized finding with its suggested
  action in the message; the advisory `info` severity maps to the SARIF `note`
  level. The exit code is unchanged: `1` when a priority 1–2 finding remains.

## Running in CI (GitHub Action)

A composite GitHub Action wraps `decided validate --sarif` and uploads the result to
GitHub Code Scanning, so findings annotate the pull request inline. The decision
behind it is
[ADR-058](https://github.com/asdecided/core/blob/main/decisions/decisions/adr-058-validation-github-action.md);
it is a thin wrapper — the `decided` CLI stays the source of truth.

```yaml
# .github/workflows/asdecided.yml
name: AsDecided
on: [pull_request]
permissions:
  contents: read
  security-events: write          # required to upload SARIF to Code Scanning
jobs:
  validate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: asdecided/core/validate-action@v0
        with:
          path: decisions/
```

Inputs: `path` (default `decisions`), `upload-sarif` (default `true`), and
`sarif-file`. Errors
fail the check; warnings — including findings downgraded in `.decided/config.yaml` —
annotate without failing, so a legacy repo can adopt the gate green on day one and
tighten over time.

> **Extensibility boundary.** AsDecided's built-in artifact types and relationship
> edges are the supported surface, defined in code. Custom artifact types and
> custom relationship edges are deferred (ADR-052, ADR-055); a repo-local schema
> registry is a future, separately recorded decision.

(The Watchkeeper action at the repository root is the complementary PR-review
surface — see [Watchkeeper](watchkeeper.md).)

### The full PR gate (`decided gate`)

To carry the whole contract into one required check, `decided gate` composes
validation, relationship integrity, and review into a single enforced verdict
under the corpus **enforcement policy**, and emits one combined SARIF document.
The `pr-gate-action` runs it and uploads that single SARIF to Code Scanning
under one category (`decided-gate`), failing when any finding is *blocking*.
It is the same thin wrapper — the engine decides what is blocking, the action
computes nothing ([ADR-063](https://github.com/asdecided/core/blob/main/decisions/decisions/adr-063-non-python-clients-are-thin.md)):

```yaml
# .github/workflows/asdecided.yml
name: AsDecided
on: [pull_request]
permissions:
  contents: read
  security-events: write          # required to upload SARIF to Code Scanning
jobs:
  gate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: asdecided/core/pr-gate-action@v0
        with:
          path: decisions/
```

Inputs mirror `validate-action`: `path` (default `decisions`),
`upload-sarif` (default `true`), and `sarif-dir` (default `decided-sarif`, with
one `gate.sarif` output).

`decided gate <dir>` is also runnable locally — `--json` and `--sarif` produce the
machine contracts, the exit code is `0` when nothing is blocking and `1`
otherwise. **Which findings are blocking versus advisory is governed centrally**
by an `enforcement:` section in the committed `.decided/config.yaml`. See
[Governance](governance.md) for the policy shape, the default classifications,
and how to standardise one policy across a fleet of repositories.

## See also

- [Governance](governance.md) — the `enforcement:` policy and `decided gate`.
- [Security posture](security.md) — the no-egress guarantee, SBOM, and how to verify it.
- [CLI Reference](cli.md) — all `decided validate` flags and exit codes.
- [OKF Profile](okf-profile.md) — the conformance findings SARIF also reports.
- [Repository Workflow](repo-workflow.md) — `decided init` and `.decided/config.yaml`.
