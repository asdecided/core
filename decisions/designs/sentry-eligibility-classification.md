---
schema_version: 1
id: RAC-KXBJ7Y3V5R9S
type: design
---
# Sentry Eligibility Classification

## Context

ADR-123 separates all-live corpus adoption from explicitly eligible coverage.
The classification process must now reduce the unclassified population without
turning nuanced engineering intent into decorative regexes.

## User Need

A maintainer needs a credible answer to three different questions: which
Decisions can be checked deterministically, which of those are actively
constrained, and which still require classification or human review.

## Design

### Tranche selection

Audit eight to twelve Decisions at a time. Prefer boundaries with commercial
or operational leverage:

1. security and trust;
2. deterministic/no-network behavior;
3. compatibility contracts;
4. required merge protection;
5. single-source registries and generated interfaces.

Defer Decisions whose lifecycle is ambiguous or whose only possible rule would
match wording rather than behavior.

### Eligibility test

A Decision is eligible when at least one obligation maps to a stable,
repository-local property that Sentry can check using a narrow path glob and a
deterministic pattern or import adapter.

A Decision is ineligible when its binding content is product positioning,
human judgement, delivery sequencing, an external setting, or another property
with no honest repository-local proxy. The reason records that boundary.

A partially enforceable Decision may be eligible, but its `reason` and rule
messages must identify the enforced subset. External remainder—such as GitHub
branch-protection configuration—stays explicitly outside the claim.

### Rule quality

- Prefer exact files over broad recursive globs.
- Prefer named functions, fixtures, workflow steps, and embedded contract files
  over prose tokens.
- Use `forbid_import` for dependency boundaries.
- Use `forbid_pattern` only for a concrete prohibited construct.
- Use `require_pattern` for stable integration or test anchors.
- Never add an LLM judge or semantic-scoring escape hatch.

### Tranche-one scope

The first tranche covers ADR-065, ADR-066, ADR-067, ADR-075, ADR-115, ADR-120,
ADR-121, and ADR-122. These decisions govern the trust boundary, deterministic
evaluation, agent integration, merge gating, shared specs, Rust CI authority,
MCP compatibility, and truthful OKF export.

### Tranche-two scope

The second tranche covers ADR-103, ADR-104, ADR-105, ADR-106, ADR-107, ADR-108,
ADR-109, ADR-112, ADR-114, ADR-118, and ADR-119. These decisions form the
native performance and freshness spine: one derived read model, mapped
persistence, bounded freshness detection, incremental validation, deterministic
parallel construction, indexed tags, cache defaults, constrained dependencies,
Linux event acceleration, and atomic base-plus-delta generations.

The tranche raises explicit corpus adoption to 20 of 121 live decisions
(16.53%). All 20 explicitly eligible decisions carry active rules, for 100%
eligible coverage and 46 deterministic rules. The remaining 101 decisions stay
visibly unclassified pending later review.

## Constraints

- Classification is explicit; absent sections remain unclassified.
- A green full-tree run is required before a rule is proposed.
- The audit does not alter Decision status or silently repair obsolete intent.
- Coverage is an observation, never a target that justifies weak rules.

## Rationale

Small tranches make each eligibility judgement reviewable and allow the rule
language to evolve from evidence. The first tranche exercises all three rule
kinds across security, CI, protocol, and export boundaries without claiming
semantic enforcement.

## Accessibility

Every rule carries a plain-language remediation message. Human and JSON reports
retain counts and governing paths without requiring SARIF.

## Style Guidance

Rule IDs use stable kebab-case and name the boundary, not the implementation
accident used to detect it.

## Open Questions

- Should a future report list unclassified Decision paths directly?
- When should an eligible Decision with no rules become a blocking policy gap?
- Which external settings merit a separate attestation mechanism?

## Related Decisions

- adr-123

## Related Requirements

- sentry-eligibility-audit
