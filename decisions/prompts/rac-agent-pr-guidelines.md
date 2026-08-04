---
schema_version: 1
id: RAC-KV2J3433M7NF
type: prompt
---
# RAC Pull Request Documentation Generator

## Objective

Generate a pull request description that acts as durable repository memory
after implementation conversations, AI sessions, and local context
disappear.

AsDecided development follows a roadmap-contract workflow: features are planned
through explicit Roadmap artifacts, architectural decisions are captured
through ADRs, and implementation happens through small scoped changes. A PR
should preserve what changed, why decisions were made, what was
intentionally excluded, what user-facing contract was introduced, and how
correctness was verified.

The PR is not a commit message. Commits describe individual changes; pull
requests document the accepted product and architecture contract. As a
maintainer reviewing RAC changes, the need is for pull requests to capture
implementation scope, decisions, verification evidence, and release
traceability, so future contributors understand why the system behaves the
way it does without the original planning conversation.

## Input

The implementation details, roadmap item, ADRs, commits, and code changes
for the RAC change being documented.

## Instructions

Generate a pull request description for an AsDecided change. Use the implementation
details, roadmap item, ADRs, commits, and code changes provided. Structure
the PR as a release-contract record, using the format in Output.

Title the pull request in the commit message format from
`rac-agent-commit-guidelines.md` — `<type>(<area>): <imperative summary>` with an
optional `[reference]`. The repository squash-merges pull requests, so the title
becomes the squashed commit's subject line on `main`; a prose title (for example
"Implement X" or "Add feature Y") breaks the `git log` readability the commit
standard exists to preserve. Use the dominant change's type and area — usually
the lead commit's.

Be aggressive about documenting exclusions: prefer stating what RAC does now
and what RAC deliberately does not do yet. Document accepted implementation
decisions (naming, schema, JSON contract, validation behavior, exit-code
behavior, architecture boundaries). Preserve verification evidence rather
than asserting "tests pass". Explain the implementation story through the
suggested review order.

## Output

A pull request body in this format:

```markdown
# Summary

Implements `<roadmap item / issue>`.

Adds:
- `<user-visible behavior>`
- `<CLI/API/schema behavior>`
- `<tests, fixtures, documentation included>`

# Roadmap / ADR Trace

Roadmap:
- `decisions/roadmaps/<codename>.md`

Relevant ADRs:
- `decisions/decisions/<adr-file>.md`

# Scope

## Included
- `<specific behavior shipped>`
- `<specific command/output/schema/test coverage>`

## Excluded
- `<explicitly deferred behavior>`
- `<nearby tempting capability intentionally avoided>`
- `<future roadmap boundary>`

# Product / Architecture Decisions

Document accepted implementation decisions: naming, schema, JSON contract,
validation behavior, exit-code behavior, architecture boundaries. Example:
- Chose `validation_issues` instead of `broken_relationships` because
  validation now covers multiple failure modes beyond missing links.

# User-Facing Contract

## CLI
Commands added or changed, with examples.

## Human Output
Visible terminal behavior.

## JSON Output
Fields or shape changes.

## Exit Codes
- `0`: <meaning>
- `1`: <meaning>
- `2`: <meaning>

# Verification

## Ran
Exact verification commands (for example: `cargo test --workspace --release`,
`decided <command>`, `decided <command> --json`).

## Covered
Tested scenarios: positive, negative, boundary. Avoid "tests pass";
preserve the evidence.

# Review Path

Suggested review order: core implementation, schema/artifact changes, CLI
interface changes, tests and fixtures, documentation.

# Notes For Reviewer

Files worth extra attention, known limitations, deferred follow-ups. Do not
repeat commit history.

# Implementation Process (Optional)

If relevant: implemented with AI assistance under the roadmap contract;
final scope, review, and acceptance decisions were made by the maintainer.
This section is the only sanctioned AI disclosure in a pull request.
```

## Constraints

- Title the PR in commit-message form (`<type>(<area>): <imperative summary>`,
  per `rac-agent-commit-guidelines.md`); it becomes the squash-merge commit
  subject on `main`. Never use a prose title like "Implement X" or "Add
  feature Y".
- Do not create generic release notes.
- Do not summarize only commits.
- Do not omit intentional exclusions.
- Do not replace verification evidence with "tests pass".
- Do not introduce scope that was not accepted in the roadmap.
- Do not invent ADRs or decisions.
- Keep implementation rationale in the PR, not individual commits.
- Keep commit messages concise and separate from PR documentation.
- Prioritize future maintainability over marketing language.
- Do not include tool attribution: no "Generated by ..." footers, no
  AI-tool signatures, no session links. Agent platforms commonly append
  these to the PR body on creation or update — after every create or edit,
  re-read the stored body and strip anything appended below your final
  section. The same applies to angle-bracket placeholders: GitHub's
  sanitizer silently deletes them, so verify the stored body matches what
  you submitted. The Implementation Process section is the only sanctioned
  AI disclosure (mirrors the commit identity and footer rules in
  `rac-agent-commit-guidelines.md`).

## Evaluation

Write like a maintainer documenting a system contract.

Prefer: "Relationship failures are reported as validation issues because
validation covers duplicate identifiers, ambiguous targets, and missing
references."

Avoid: "Added relationship validation. Tests pass."

A good PR lets a future contributor understand the behavioral contract and
the decisions behind it without the original planning conversation.

## Related Decisions

- ADR-047
