---
schema_version: 1
id: RAC-KXBF5W9S3P7Q
type: design
---
# Sentry Code-Constraint Evaluation

## Context

Core already parses the authoritative corpus once and owns the blocking gate.
Decision-to-code checks should reuse those boundaries and produce stable
findings without introducing a generic policy runtime or language server.

## User Need

A maintainer should be able to encode a concrete architectural boundary beside
its rationale, run it locally, and receive the same blocking result in CI with
an explicit statement of how much of the live Decision corpus is enforceable.

## Design

### Artifact contract

An accepted Decision may contain exactly one second-level `Code Constraints`
section with one fenced YAML document:

```yaml
version: 1
eligibility: eligible
rules:
  - id: no-hard-delete
    kind: forbid_pattern
    path_glob: "src/**/*.sql"
    pattern: "DELETE\\s+FROM\\s+users"
    message: "ADR-014 requires recoverable account closure."
```

`eligibility` is `eligible` or `ineligible`. Existing v1 documents that predate
the field remain `eligible` for compatibility. An eligible Decision may carry
an empty rule list while its deterministic boundary is still being authored.
An ineligible Decision carries no rules and must state a non-empty `reason`.
No classification is inferred for Decisions without the section.

Unknown document fields, unknown rule kinds, unsafe paths, invalid regexes, and
duplicate IDs are validation errors. Rule IDs use stable lowercase kebab-case.

### Evaluation modes

Diff mode receives an explicit Git base and parses zero-context unified hunks.
Forbidden patterns and imports report only matches intersecting added or
modified lines. Required patterns evaluate each matching changed file; if the
diff changes no matching file, the rule is out of scope for that run.

Full mode walks the repository deterministically, excluding `.git`, and
evaluates every file selected by each rule. A required-pattern rule whose glob
matches no file is a blocking empty-match finding.

### Import adapters

Version 1 uses bounded deterministic adapters for Python, Rust, JavaScript, and
TypeScript families. The adapter extracts literal import targets and applies
the rule regex to those targets. A selected extension without an adapter
produces an unsupported-language finding; it is never silently skipped.

### Gate and reporting

`decided sentry` calls the engine directly. `decided gate --code` calls the
same function and appends its findings to the gate report. Human, JSON, and
SARIF renderers consume the same sorted findings.

The report includes:

- live Decision count;
- classified and unclassified Decision counts;
- explicitly eligible Decision count;
- constrained live Decision count;
- active rule count;
- constrained/all-live corpus adoption;
- constrained/eligible enforcement coverage;
- diff base or full-tree mode;
- deterministic findings ordered by path, line, Decision, and rule.

### Core dogfood

Core pull requests run `decided sentry decisions --base origin/<base>` after a
full-history checkout. SARIF is uploaded under the distinct
`decided-sentry` category and the native exit code remains blocking.

## Constraints

- No network request, model integration, embedding search, or LLM judge.
- Constraint documents remain part of accepted Decision artifacts.
- The Git diff and checked-out repository bytes are the entire code input.
- Coverage describes declared deterministic rules, not semantic compliance.
- Findings fail closed when inputs selected for evaluation cannot be checked.

## Rationale

The design deliberately resembles a small policy linter rather than a semantic
reviewer. That narrower claim is reproducible, inspectable, and strong enough
to turn concrete decisions into merge protection without weakening
AsDecided's deterministic substrate.

## Accessibility

Every violation has a textual message and stable code. SARIF annotations are an
additional navigation surface, not the only way to understand a failure.

## Style Guidance

Rule messages should state the governing boundary and remediation, not merely
repeat that a regex matched. Path globs should be as narrow as the decision
allows.

## Open Questions

- Which additional language import adapters have enough demand to justify a
  versioned extension?
- Should future releases support deterministic dependency-manifest rules as a
  separate kind rather than expressing them as patterns?
- What coverage threshold, if any, is useful once teams have observed their
  baseline? Version 1 reports the number but does not gate on it.

## Related Decisions

- adr-123

## Related Requirements

- deterministic-decision-code-enforcement
