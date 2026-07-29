---
schema_version: 1
id: RAC-KXBD3T7Q9M2N
type: decision
---
# ADR-123: Deterministic Decision-to-Code Enforcement

## Status

Accepted

## Category

Architecture

## Context

AsDecided already blocks malformed or inconsistent decision corpora and can
show which decisions govern changed paths. That protects the record, but it
does not prove that changed source code obeys the subset of decisions that can
be checked mechanically.

The gap matters most for concrete boundaries: forbidden dependencies, import
directions, required integration points, and prohibited calls or statements.
An accepted decision can say that hard deletion is forbidden while a pull
request containing `DELETE FROM users` still passes a corpus-only gate.

Some decision intent cannot be reduced to a deterministic source check.
Pretending otherwise would replace review with false confidence. Adding an LLM
judge would also violate ADR-066 and make enforcement depend on variable,
networked interpretation.

## Decision

Accepted Decision artifacts MAY carry one versioned `## Code Constraints`
YAML block. Core validates that block during ordinary corpus validation and
Sentry evaluates its rules against repository bytes.

1. Version 1 supports `forbid_pattern`, `require_pattern`, and
   `forbid_import`, each with a stable rule ID, repository-relative path glob,
   deterministic regular expression, and optional message.
2. `decided sentry` is the dedicated enforcement command.
   `decided gate --code` composes the same engine into the existing blocking
   gate; neither surface reimplements policy.
3. Pull-request enforcement evaluates changed source lines relative to an
   explicit Git base. Full-tree certification is available through `--full`.
4. A diff-scoped `require_pattern` applies only when the pull request changes
   a matching file. Full-tree mode fails closed when its glob selects no file.
5. Malformed constraint documents, unsupported versions, invalid globs or
   regexes, duplicate rule IDs, unreadable selected source, and unsupported
   import languages are blocking findings.
6. Human, JSON, and SARIF reports are deterministic and include the governing
   Decision, rule, source path, line where available, and aggregate coverage.
7. Reporting separates corpus adoption from eligible enforcement coverage.
   Every live Decision is either explicitly `eligible`, explicitly
   `ineligible` with a reason, or unclassified. The report publishes both
   constrained/all-live adoption and constrained/eligible coverage, plus the
   unclassified count and active rule count.
8. The engine performs no model call, embedding lookup, network request, or
   semantic judgement. Nuanced decisions remain subject to human review.

## Code Constraints

```yaml
version: 1
eligibility: eligible
rules:
  - id: sentry-has-no-network-client
    kind: forbid_import
    path_glob: "rust/rac-engine/src/sentry.rs"
    pattern: "^(reqwest|hyper|ureq)(::|$)"
    message: "Sentry must remain local and network-free."
  - id: sentry-has-no-llm-judge
    kind: forbid_pattern
    path_glob: "rust/rac-engine/src/sentry.rs"
    pattern: "(?i)llm[_ -]?judge|openai|anthropic"
    message: "Sentry must not add an LLM judge or model-provider dependency."
  - id: gate-composes-sentry-engine
    kind: require_pattern
    path_glob: "rust/rac-engine/src/gate.rs"
    pattern: "crate::sentry::analyze"
    message: "The code gate must compose the single Sentry engine."
  - id: core-ci-runs-sentry
    kind: require_pattern
    path_glob: ".github/workflows/pr-checks.yml"
    pattern: "decided sentry decisions"
    message: "Core pull requests must run the blocking native Sentry check."
```

## Consequences

### Positive

- Concrete architectural decisions can now block violating code changes.
- Enforcement is reproducible locally and in CI, with no hosted service.
- Findings cite the governing record instead of presenting an unexplained
  lint rule.
- Published coverage separates what is machine-checkable from what still
  requires judgement.

### Negative

- Regex and lightweight import adapters cover only a bounded class of intent.
- Rule authors must maintain path globs as source layouts change.
- Full-tree checks can expose pre-existing violations when a constraint is
  first adopted.

## Alternatives Considered

### Use an LLM to judge every changed file against every Decision

Rejected. Results would be variable, non-local, difficult to reproduce, and
contrary to ADR-066.

### Treat Herald proximity as enforcement

Rejected. Governing-decision discovery is useful context, but an advisory
comment neither evaluates nor blocks violating source.

### Keep code policy in standalone linter configuration

Rejected as the primary contract. A linter may execute a check, but separating
the rule from its governing Decision loses rationale, status, and traceability.

## Related Decisions

- adr-005
- adr-007
- adr-049
- adr-063
- adr-065
- adr-066
- adr-067
- adr-075
- adr-120

## Related Requirements

- deterministic-decision-code-enforcement

## Related Designs

- sentry-code-constraint-evaluation
