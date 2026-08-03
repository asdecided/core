---
schema_version: 1
id: RAC-KV2J352KS2YM
type: prompt
---
# RAC Minor Release Gate

## Objective

Catch scope drift, duplication, and missing verification before completing
any minor (0.x.n) release.

## Input

The changes in the pending release and the current repository state.

## Instructions

Before completing any 0.x.n release, answer each question below.

### Duplication

- Did this release add artifact-specific classification logic?
- Did it duplicate validation, schema, template, stats, or improve behavior?
- Can new behavior be expressed through ArtifactSpec instead?
- Are there two sources of truth for sections, metadata, or guidance?

### Simplification

- What code became obsolete?
- What branch or special case can be removed?
- Did any file grow past the agreed limit?
- Should a helper be extracted?
- Did this release increase the number of artifact-specific conditionals?

### Verification

- Are there negative classification tests?
- Are adjacent artifact types tested against each other?
- Are incomplete-but-recognizable artifacts tested?
- Are CLI human and JSON outputs tested?
- Was the relevant Rust test battery run before commit (normally
  `cargo test --workspace --release`)?

## Output

A direct answer to each gate question, naming any issue that must be
resolved before the release proceeds.

## Related Decisions

- ADR-047
