# Watchkeeper — product knowledge review on pull requests

Watchkeeper reviews *changes* to product knowledge. Where `decided review`
answers "what needs attention in this repository right now?", Watchkeeper
answers "what changed between these two states, and does it need a human?"
— changed artifacts, validation and relationship deltas, deterministic
intent findings (specificity regressions, weakened constraints, removed
acceptance criteria, …), and a review verdict with reasons.

It is one CLI command, plus a thin GitHub Action and a reusable workflow
that run it on pull requests. Everything is deterministic and offline; the
only network use in CI is installing the package.

## The command

```bash
decided watchkeeper rac --base main
```

See the [CLI reference](cli.md#watchkeeper) for every flag, the finding
codes, the JSON contract, and the exit-code policy. The short version:

- `--base` / `--head` each take a git revision **or** a plain directory;
  the working tree is the default head.
- `--fail-on error|warning|none` turns the review verdict into CI policy.
- `--format github` writes a Markdown report to stdout (for the step
  summary) and inline-annotation workflow commands to stderr.

Revisions are materialized read-only with `git archive` (ADR-043): no
worktrees, no `.git` mutation, safe under concurrent CI runs.

## The GitHub Action

Add a workflow to your repository:

```yaml
name: Watchkeeper
on: pull_request
permissions:
  contents: read
jobs:
  watchkeeper:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: itsthelore/asdecided-core@<release-tag>
```

On every pull request that touches your corpus you get:

- **a failed check** when the change needs review (per `fail-on`),
- **inline annotations** on the artifacts that triggered attention
  (errors for recommendation triggers, warnings/notices for the rest),
- **a step-summary report**: change table, deltas, findings, verdict.

### Inputs

| Input | Default | Meaning |
| --- | --- | --- |
| `path` | `rac` | Corpus directory to compare |
| `base` | `''` | Base revision; empty resolves to `origin/<PR base branch>` |
| `fail-on` | `error` | `error` · `warning` · `none` (report, never fail) |
| `annotate` | `true` | Emit inline annotations |

The action builds the native CLI from its pinned action source, resolves the base ref, runs one
`decided watchkeeper --format github` invocation, and propagates its exit code
unchanged. All analysis and policy live in the engine.

### Reusable workflow

Prefer calling a workflow instead of composing steps:

```yaml
jobs:
  watchkeeper:
    uses: itsthelore/asdecided-core/.github/workflows/watchkeeper.yml@<release-tag>
    with:
      path: rac
```

It wires up the full-history checkout and passes every input through to
the action.

## Version pinning

Pin **exact release tags** (`@vX.Y.Z`). Dependabot and Renovate both understand
exact action tags. This repository's own PR
checks run the action from the checked-out native source, which doubles as the
end-to-end test of `action.yml` on every pull request.

## A worked example

A pull request weakens a requirement:

```diff
- [REQ-001] Payment confirmation must complete within 2 seconds
+ [REQ-001] Payment confirmation should complete quickly
```

The watchkeeper check fails, the file gets an inline error annotation —
`specificity_regression: Measurable requirement REQ-001 became vague.` —
and the job summary ends with:

```markdown
## Verdict

**Review recommended.**

Reasons:

- A measurable requirement became vague. (`specificity_regression`)
- A mandatory requirement was weakened. (`constraint_weakened`)
```

Watchkeeper never judges whether the change is *right* — it makes sure a
human looks before it merges.
