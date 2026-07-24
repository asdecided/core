# Validation: overrides & SARIF

`decided validate` is RAC's write-time gate: it fails when any artifact carries an
error-severity finding, and (over a directory) when the corpus is not a
conformant OKF v0.1 bundle. Two features make that gate adoptable in CI on a
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
overridable below. (RAC's own corpus predates these checks and disables them in
its `.decided/config.yaml` — the warnings-first path in action.)

## Severity overrides (warnings-first onboarding)

Pointing `decided validate` at a legacy corpus for the first time can surface many
pre-existing findings at once. Rather than fail the build on all of them, a
repository can downgrade or silence specific findings in its committed
`.decided/config.yaml`, then tighten the gate over time. The decision behind this is
[ADR-053](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-053-validation-severity-overrides.md).
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

## SARIF output for GitHub Code Scanning

`decided validate <dir> --sarif` emits a [SARIF 2.1.0](https://json-schema.org/)
document covering core validation findings and OKF conformance findings, so a CI
job can upload it and have GitHub Code Scanning annotate findings inline on a
pull request. The decision behind this is
[ADR-054](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-054-sarif-validation-output.md).

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
[ADR-058](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-058-validation-github-action.md);
it is a thin wrapper — the `rac` CLI stays the source of truth.

```yaml
# .github/workflows/rac.yml
name: RAC
on: [pull_request]
permissions:
  contents: read
  security-events: write          # required to upload SARIF to Code Scanning
jobs:
  validate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: itsthelore/asdecided-core/validate-action@v0
        with:
          path: decisions/
```

Inputs: `path` (default `rac`), `upload-sarif` (default `true`), `sarif-file`,
`rac-version` (pin a release), and `install-from` (`pypi` or `source`). Errors
fail the check; warnings — including findings downgraded in `.decided/config.yaml` —
annotate without failing, so a legacy repo can adopt the gate green on day one and
tighten over time.

> **Extensibility boundary.** RAC's built-in artifact types and relationship
> edges are the supported surface, defined in code. Custom artifact types and
> custom relationship edges are deferred (ADR-052, ADR-055); a repo-local schema
> registry is a future, separately recorded decision.

(The Watchkeeper action at the repository root is the complementary PR-review
surface — see [Watchkeeper](watchkeeper.md).)

### The full PR gate (`decided gate`)

To carry the whole contract into one required check, `decided gate` composes
validation, relationship integrity, and review into a single enforced verdict
under the corpus **enforcement policy**, and emits one combined SARIF document
(v0.21.14). The `pr-gate-action` runs it and uploads that single SARIF to Code
Scanning under one category (`rac-gate`), failing when any finding is *blocking*.
It is the same thin wrapper — the engine decides what is blocking, the action
computes nothing ([ADR-063](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-063-non-python-clients-are-thin.md)):

```yaml
# .github/workflows/rac.yml
name: RAC
on: [pull_request]
permissions:
  contents: read
  security-events: write          # required to upload SARIF to Code Scanning
jobs:
  gate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: itsthelore/asdecided-core/pr-gate-action@v0
        with:
          path: decisions/
```

Inputs mirror `validate-action`: `path` (default `rac`), `upload-sarif` (default
`true`), `sarif-dir` (default `rac-sarif`, now one `gate.sarif`), `rac-version`,
and `install-from` (`pypi` or `source`).

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
