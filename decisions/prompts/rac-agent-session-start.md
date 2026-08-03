---
schema_version: 1
id: RAC-KV2J31Z1EV4T
type: prompt
---
# AsDecided Agent Session Start

## Objective

Establish the working frame for an agent session on AsDecided, so changes stay
correctly scoped, respect recorded decisions, and pass the corpus gates
before they are pushed.

AsDecided is a native Rust CLI and MCP server. It models requirements,
decisions, roadmaps, prompts, and designs as deterministic, typed Markdown
artifacts served from the repository that owns them.

## Input

- The AsDecided repository and its corpus under `decisions/` — requirements, decisions
  (ADRs), roadmaps, prompts, and designs.
- The roadmap item relevant to the task, and the ADRs it touches.
- The AsDecided MCP tools when available in the session; without them, the
  same knowledge via the `decided` CLI (`find`, `resolve`, `relationships`).

## Instructions

### Core principles

- Markdown-first.
- Deterministic classification.
- Structural validation, not semantic scoring unless explicitly planned.
- CLI contracts matter: human output, JSON output, exit codes, and templates must be specified.
- Roadmaps are identified by codename, not a version; release versions are SemVer `vX.Y.Z` (ADR-094, ADR-111, which reverted the CalVer of ADR-076).
- Prefer schema/artifact-spec-driven behavior over artifact-specific branches.
- Keep classification separate from validation.
- Invalid but recognizable artifacts may still classify as their artifact type, then fail validation.
- Durable thinking lives in the corpus, not in ephemeral tool scratch space.
  Record plans, designs, and decisions as AsDecided artifacts under `decisions/` — a
  Design for the *how*, a Roadmap for the *what/why* (a non-versioned
  `decisions/roadmaps/future/` item when unscheduled), an ADR for a decision —
  where the gates validate them. A tool's plan or scratch file is working
  memory only; it has no authority and does not persist.

### Before coding

1. Refresh from `origin/main` unless told otherwise.
2. Confirm branch state; work on a feature branch, never on main.
3. Read the relevant roadmap item; do not expand release scope beyond it.
4. Check against ADRs.
5. Produce an implementation contract.
6. Wait for approval.

### Grounding (when the AsDecided MCP tools are available in your session)

- Call `get_summary` once at session start to learn what recorded
  knowledge exists.
- Call `search_artifacts` before designing or implementing; recorded
  decisions take precedence over conventions inferred from the code.
- When an artifact ID is mentioned, call `get_artifact`; call
  `get_related` before changing anything an artifact covers.
- Cite decisions by ID. If a task conflicts with a recorded decision,
  say so and stop — do not silently override it.

Without the tools, the same knowledge lives under `decisions/`; use the `decided` CLI
(`find`, `resolve`, `relationships`) instead.

### Testing

- Add negative boundary tests for each new artifact type.
- Test that adjacent artifact types do not misclassify as each other.
- Run the relevant Rust tests before commit, normally `cargo test --workspace --release`.

## Output

A correctly scoped, approved change that passes the gates in Evaluation.
After a GitHub merge, refresh local main; prune merged branches when asked.

## Constraints

- Do not implement until the plan is approved.
- Never work on main; always use a feature branch.
- Do not expand release scope beyond the roadmap item.
- If a task conflicts with a recorded decision, say so and stop — do not
  silently override it.

## Evaluation

Before pushing:

- `decided validate decisions/` and `decided relationships decisions/ --validate` exit 0.
- `decided review decisions/` reports no priority 1-2 findings.
- Commits follow `decisions/prompts/rac-agent-commit-guidelines.md`: format,
  maintainer identity on author and committer, no tool attribution.

## Related Decisions

- ADR-047
