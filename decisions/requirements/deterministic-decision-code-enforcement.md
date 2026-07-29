---
schema_version: 1
id: RAC-KXBE4V8R2N6P
type: requirement
---
# Requirement: Deterministic Decision-to-Code Enforcement

## Status

Accepted

## Problem

Corpus validation proves that engineering decisions are well-formed and linked,
but not that changed source code obeys them. AsDecided needs a blocking,
explainable check for the subset of decision intent that can be evaluated
deterministically without overstating coverage.

## Requirements

- [REQ-001] Accepted Decision artifacts MUST be able to declare a versioned, machine-checkable code-constraint block without adding a second policy file.
- [REQ-002] Version 1 MUST support deterministic forbidden-pattern, required-pattern, and forbidden-import rules scoped by repository-relative path globs.
- [REQ-003] Constraint syntax MUST be validated by ordinary corpus validation, including version, unique stable rule ID, glob, regular expression, rule kind, and non-empty rule set.
- [REQ-004] Pull-request enforcement MUST evaluate violations against an explicit Git diff base and MUST report only forbidden matches on changed lines.
- [REQ-005] A diff-scoped required-pattern rule MUST apply to matching changed files and MUST NOT fail an unrelated change merely because its glob selects no changed file.
- [REQ-006] Full-tree enforcement MUST evaluate every matching repository file and MUST fail when a required-pattern glob selects no file.
- [REQ-007] `decided sentry` and `decided gate --code` MUST use the same enforcement engine and MUST return a blocking non-zero exit for violations or invalid constraints.
- [REQ-008] Reports MUST be deterministic in human, JSON, and SARIF forms and MUST identify the governing Decision, rule ID, source path, line where available, and message.
- [REQ-009] Reports MUST publish constrained-live-Decision coverage against all live Decisions and MUST NOT describe unconstrained decisions as enforced.
- [REQ-010] Enforcement MUST remain local and MUST NOT use embeddings, model calls, an LLM judge, or a network service.
- [REQ-011] Core pull requests MUST dogfood Sentry as a blocking check against the pull request base branch.

## Success Metrics

- A prohibited changed line fails locally and in pull-request CI with the same
  governing Decision and rule.
- An unrelated documentation-only diff does not fail a required-pattern rule.
- Full-tree certification catches missing required files and existing
  violations.
- Repeated runs over identical corpus, repository, and base inputs are
  byte-identical.
- The report always exposes machine-enforceable coverage.

## Risks

- Regex rules can be written too broadly. Stable IDs, narrow globs, full-tree
  certification, and code review keep each rule inspectable.
- Lightweight import parsing may not cover every language construct.
  Unsupported selected languages fail explicitly instead of being ignored.
- Coverage may initially be low. Publishing the number is a feature: it keeps
  the boundary between deterministic enforcement and human review visible.

## Assumptions

- The repository is available locally and a pull-request base revision has
  been fetched.
- Decision authors can distinguish checkable code boundaries from nuanced
  intent.
- Human review remains authoritative for decisions without a deterministic
  constraint.

## Related Decisions

- adr-066
- adr-120
- adr-123

## Related Designs

- sentry-code-constraint-evaluation

## Verified By

- rust/rac-engine/src/sentry.rs
- rust/rac-engine/src/gate.rs
- .github/workflows/pr-checks.yml
