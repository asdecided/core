---
schema_version: 1
id: RAC-KVNM01QPBPXB
type: decision
---
# ADR-075: The Pre-Merge Check Tier Is a Required Merge Gate on `main`

## Context

ADR-027 fixed RAC's CI test topology: the full battery × version grid is
merge-gated on `main`, and pull requests instead run a deliberately small
pre-merge tier in `.github/workflows/pr-checks.yml` — the static quality gates
(`ruff check`, `ruff format --check`, `mypy src/`) plus a smoke battery and the
dogfood gates (`validate`, `pr-gate`, `doctor`, `watchkeeper`, `agent-rules`,
`grounding-eval`). ADR-027 describes *what runs* on a pull request, and why.

What it never states is whether those checks **block the merge**. That is a
separate, GitHub-side control: a branch-protection rule on `main` listing the
required status checks. The two are independent — a workflow can run and report
red on a pull request while the merge button stays green, because the check is
advisory unless branch protection marks it *required*.

That gap is not hypothetical. PR #178 (the v0.26.3 split layout) merged to
`main` with the `lint (ruff + mypy)` job failing: `mypy` had flagged a
`str | None` value passed where `str` was expected. The pre-merge tier did its
job and reported the failure, but nothing stopped the merge, so the regression
landed on `main` and turned the post-merge run red. The follow-up PR had to
carry the fix.

`main` is already a protected branch, so some protection exists — but the
pre-merge tier's checks are evidently not in its required set, which is exactly
how a red `lint` job reached `main`. Absent a recorded decision, *which* checks
gate a merge is an invisible repository setting that no one reviews and that
drifts silently. ADR-027 made the trigger policy deliberate; this ADR does the
same for the enforcement policy it implies.

## Decision

The `pr-checks.yml` pre-merge tier is a **required merge gate** on `main`.

1. **Branch protection on `main` requires the pre-merge checks to pass before
   merging.** Every job `pr-checks.yml` runs on `pull_request` is a required
   status check: at minimum `lint (ruff + mypy)`, and the smoke and dogfood
   jobs that constitute the tier (`smoke (core + golden + dogfood, py3.11)`,
   `validate`, `pr-gate`, `doctor`, `watchkeeper`, `agent-rules`,
   `grounding-eval`). A pull request cannot merge while any of them is red or
   pending.

2. **The gate is not bypassable by routine merges.** Branch protection applies
   to everyone using the merge button, maintainers included; a red pre-merge
   tier is fixed, not merged through. (An explicit administrator override
   remains available for genuine emergencies, used knowingly, not as the
   default path.)

3. **Pull requests must be up to date with `main` before merging.** "Require
   branches to be up to date" is enabled, so a check that went green against a
   stale base cannot merge over newer changes it never ran against.

This is the GitHub-side complement to ADR-027: ADR-027 says the tier *runs* on
pull requests; this ADR says a failing tier *blocks* the merge.

## Principles

### Principle 1 — Running a check and enforcing it are distinct decisions

A workflow that reports red is worth nothing to `main`'s health unless the
merge is blocked on it. Enforcement is its own choice and is recorded as one.

### Principle 2 — The merge gate is recorded knowledge, not an invisible setting

Which checks gate a merge lives here, where it is reviewable, rather than only
in a repository settings page that drifts unseen. A change to the required set
is a deliberate edit to this decision.

### Principle 3 — `main` stays green by construction

The cheapest place to catch lint, type, contract, and corpus breakage is before
it lands. Requiring the pre-merge tier keeps `main` releasable, which protects
the release gate ADR-027 rule 2 depends on.

## Consequences

### Positive

- A red pre-merge tier blocks the merge, so regressions like #178's `mypy`
  failure cannot reach `main` through the normal flow.
- The post-merge full grid (ADR-027 rule 1) starts from a `main` that already
  passed lint, types, and the smoke/dogfood gates, so it fails far less often.
- The required set is auditable in this ADR; widening or narrowing it is a
  reviewed change.

### Negative

- Merges wait for the pre-merge tier to finish, adding the tier's runtime
  (~2 minutes) to the merge path. This is the intended cost.
- The required-check **names** in branch protection must track the job names in
  `pr-checks.yml`; renaming a job without updating the rule silently drops it
  from the gate. Mitigation: treat a `pr-checks.yml` job rename as also editing
  the branch-protection required set, and keep both in step with this ADR.
- Branch protection is a GitHub repository setting, not a file in the repo, so
  this decision states the policy but cannot itself apply it; the setting is
  maintained by hand to match.

## Alternatives Considered

### Leave the tier advisory (the prior state)

Run `pr-checks.yml` on pull requests but require none of its checks to merge.

#### Pros

- Maximum flexibility; a maintainer can merge over a red check at will.

#### Cons

- Exactly the state that let #178 merge red. The signal exists but does not
  protect `main`.

Rejected — the signal is only worth the runtime if it gates.

### Require the full battery grid on pull requests

Make the merge gate the whole per-service × version grid rather than the smoke
tier.

#### Pros

- Strongest possible pre-merge guarantee.

#### Cons

- Reverses ADR-027 rule 1, which deliberately keeps the full grid merge-gated
  on `main` and pull requests light, to bound Actions usage on in-flight
  branches.

Rejected — the smoke tier plus the post-merge grid is the balance ADR-027
already chose; this ADR enforces that tier, it does not enlarge it.

## Status

Accepted

## Category

Process

## Relationship to Other ADRs

### ADR-027 CI Test Topology — Merge-Gated, Per-Service Batteries

ADR-027 defines the pre-merge tier and *when* it runs. This ADR adds the
enforcement ADR-027 left implicit: the tier is a required, non-bypassable merge
gate on `main`. The two are read together.

### ADR-005 CLI First and ADR-007 JSON Contract Stability

The smoke and dogfood gates in the tier exercise the CLI surface and the JSON
output. Requiring them before merge keeps RAC's public contracts green on
`main`, upstream of the release gate that ultimately protects them.

## Success Measures

- No change reaches `main` through the merge button while any `pr-checks.yml`
  job is red — a regression like #178 is blocked at the PR, not fixed afterward.
- The post-merge full grid on `main` is green on the merge commit far more often
  than before, because `main` only receives changes that already passed the
  tier.
- The required-check set changes only by a deliberate edit to this decision and
  the matching branch-protection setting.

## Review Date

Review before v1.0.0, or sooner if `pr-checks.yml`'s job set changes materially
or RAC accepts outside contributors who need a different gate.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The checked-in pull-request workflow is enforceable; GitHub branch-protection settings remain externally verified."
rules:
  - id: premerge-workflow-runs-on-prs
    kind: require_pattern
    path_glob: ".github/workflows/pr-checks.yml"
    pattern: '(?m)^\s*pull_request:\s*$'
    message: "The pre-merge tier must continue to run on pull requests."
  - id: premerge-workflow-keeps-code-gate
    kind: require_pattern
    path_glob: ".github/workflows/pr-checks.yml"
    pattern: '(?m)^\s*sentry:\s*$'
    message: "The pre-merge tier must retain the blocking Sentry job."
```

## Related Decisions

- adr-027
- adr-005
- adr-007
