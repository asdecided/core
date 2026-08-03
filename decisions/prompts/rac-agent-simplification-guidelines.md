---
schema_version: 1
id: RAC-KV2J371GG7K7
type: prompt
---
# AsDecided Module Simplification Refactor Contract

## Objective

Reduce implementation complexity in a target AsDecided module without changing
user-facing behavior.

AsDecided has grown quickly through roadmap-led implementation, and some modules
now risk accumulating additive paths faster than they are simplified. This
prompt is for behavior-preserving module simplification.

## Input

- The target module or command area.
- The relevant roadmap item, ADRs, tests, and fixtures for that area.

## Instructions

We are simplifying the native Rust modules in `rust/`.

Before editing:

- Refresh from `origin/main`.
- Create a new branch named `<branch-name>`.
- Read the relevant roadmap item, ADRs, tests, and fixtures for this area.
- Do not implement until I approve the plan.

Analysis required:

- Identify the current responsibilities of the target module.
- List public functions/classes and who calls them.
- List CLI behavior, JSON output, validation rules, exit codes, and fixtures that must not change.
- Identify duplicated logic, mixed responsibilities, dead branches, or logic that belongs in `ArtifactSpec`.
- Identify one simplification seam that can be changed safely.

## Output

Return an implementation contract only, in this shape:

```markdown
## Current behavior to preserve

- ...

## Complexity found

- File/function:
- Problem:
- Why it matters:

## Proposed simplification

- Move/extract/delete:
- Files touched:
- Behavior preserved:

## Safety checks

- Tests to run:
- Fixtures to add or reuse:
- CLI commands to verify:

## Commit plan

Use step-based commits, not per-file commits:

- `refactor(<area>): <behavior-preserving simplification> [roadmap/refactor:<name>]`
- `test(<area>): cover preserved behavior around <case> [roadmap/refactor:<name>]`
```

## Constraints

- No user-facing behavior changes.
- No output shape changes unless explicitly approved.
- No new feature work.
- No broad rewrites.
- No formatting-only churn.
- No README changes unless needed to preserve existing docs accuracy.
- No model attribution in commits.
- Prefer deleting branches and moving existing rules into existing data structures over inventing new abstractions.
- Stop after the plan and wait for approval.

## Related Decisions

- ADR-047
