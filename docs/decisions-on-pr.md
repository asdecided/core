# Decisions on Pull Requests

AsDecided's core promise — the agent or reviewer stops re-doing what the team
already ruled out — is only felt if someone thinks to ask. The **Herald**
action asks at the one moment it matters most: a pull request editing
governed code gets **one advisory comment** naming the recorded decisions
whose declared [`## Applies To`](cli.md#decisions-for) scope covers the
changed paths — id, title, the scope that matched, and a link — updated in
place on every re-run.

Facts, never verdicts: the comment reports what governs and recommends
review. It never gates the merge, never fails the check on findings, and the
human PR review stays the trust boundary.

Herald ships in the CI delivery repo,
[`itsthelore/asdecided-ci`](https://github.com/itsthelore/asdecided-ci), beside the
Watchkeeper, Gatekeeper, and Registrar wrappers.

## 1. Wire it up

```yaml
name: AsDecided decisions
on:
  pull_request:

permissions:
  contents: read
  pull-requests: write

jobs:
  decisions:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
        with:
          fetch-depth: 0   # the diff needs the merge base

      - uses: itsthelore/asdecided-ci/herald/github@main
        with:
          path: rac
```

That is the whole integration. Prefer a tag over `@main` once one that
includes Herald is published. Inputs:

- **`path`** — the corpus directory (default `rac`).
- **`max-inline`** — decisions listed in full before the rest collapse into a
  details expander (default `5`).
On forks, where the token is read-only, the comment degrades to the step
summary instead of failing the check.

## 2. What the comment says

One bullet per governing decision, deduplicated across paths and sorted by
id:

> - **[RAC-KTW0M81HX5C6 — ADR-033: Guide Response Budget](…)** (Accepted) —
>   applies to `src/decisions/mcp/` — changed: `src/decisions/mcp/server.py`
>
> …review recommended.

The matched scope is the engine's answer, not a heuristic: the action shells
to [`decided decisions-for --json`](cli.md#decisions-for) per changed path and
re-derives nothing. Only **live** decisions appear — a superseded or
deprecated decision no longer binds, by the same liveness rule the MCP
`find_decisions` tool uses. When a PR touches nothing governed, no comment is
created; an existing comment is updated even to the empty state, so it never
outlives its diff.

The body is deterministic — a pure function of the corpus and the changed
paths, no timestamps — so re-runs on an unchanged PR rewrite the same bytes.

## 3. Getting governed

The comment only fires where decisions declare scope. Add an `## Applies To`
section to a decision (paths are validated by
[`decided relationships --validate`](relationships.md)):

```markdown
## Applies To

- src/payments/
```

Start with the handful of decisions that agents and reviewers actually
violate — the comment's value is precision, and every entry you declare is
also what grounds [`decided decisions-for`](cli.md#decisions-for) and the MCP
path lookup.

## 4. Boundaries

- **Advisory only.** No merge status changes here; a gate, if ever wanted,
  is a separate later decision after the advisory has proven signal quality —
  the same advisory-before-gate sequencing the drift work records.
- **Post-edit, not interception.** The comment appears at review time; it
  does not intercept the agent mid-edit.
- **Comment identity is your CI's.** The action posts with the workflow's
  own token; AsDecided holds no credentials and no write path into the corpus.
