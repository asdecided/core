---
schema_version: 1
id: RAC-01K8Q7MCP407
type: decision
---
# ADR-129: Confine Rename Writes to the Corpus Root

## Context

The corpus walker deliberately includes symlinked Markdown files on read-only
surfaces for compatibility. A rename is different: `decided rename --apply`
turns those paths into write targets. Following a symlink there can modify a
file outside the corpus the operator supplied, and a path can also be swapped
between planning and replacement.

## Decision

The native rename engine treats the requested corpus root as a mutation
boundary.

- The root is canonicalized before a plan is built and again before a plan is
  applied.
- Every target and relationship-bearing path in a plan must be a regular,
  non-symlink path whose canonical destination remains below that root.
- A symlinked mutation path, an unresolvable path, or a path that resolves
  outside the root produces a refused dry-run with a stable reason code and
  the offending path; no file is written.
- Application repeats the containment and symlink checks immediately before
  reading each file and immediately before replacing it.
- Unix staging opens each temporary final component with `O_NOFOLLOW` as a
  final-component race guard; same-directory replacement uses the staged file
  after the immediate root checks. Read-only discovery remains unchanged and
  may still report symlinked Markdown files.

## Status

Accepted

## Category

Technical

## Consequences

Rename cannot silently write through a corpus symlink or escape the requested
root. A corpus that intentionally exposes a symlinked Markdown target must
materialize that file before renaming it; this is an explicit safety refusal,
not a partial edit. The extra metadata and canonicalization checks are bounded
by the number of files in the deterministic edit set.

The protection is deliberately narrow. It does not change read-only walk
parity, rename ordering, identity semantics, or the exact-line stale-plan
check. `O_NOFOLLOW` closes the final-component race while staging on Unix; the
immediate rechecks provide the same root-boundary policy on other platforms.

## Alternatives Considered

### Follow symlinks as the walker does

Rejected. Read compatibility is not authorization to mutate an arbitrary
target selected by a link.

### Silently skip symlinked files

Rejected. A skipped inbound reference would make a successful rename silently
incomplete. The dry-run must identify the path and refuse the whole plan.

### Rename through directory handles only

Rejected for this release. No-follow directory-handle APIs vary across the
supported platforms; canonical containment, immediate rechecks, and the Unix
final-component no-follow flag provide a deterministic cross-platform contract
without changing the CLI surface.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "Rename safety is a deterministic source-level boundary with no model judgement."
rules:
  - id: rename-confines-mutation-paths
    kind: require_pattern
    path_glob: "rust/rac-engine/src/rename.rs"
    pattern: "check_mutation_path"
    message: "Rename must recheck every mutation path against the canonical corpus root."
  - id: rename-no-follow-final-write
    kind: require_pattern
    path_glob: "rust/rac-engine/src/rename.rs"
    pattern: "O_NOFOLLOW"
    message: "Unix rename staging must refuse a final-component symlink race."
```

## Related Decisions

- adr-007
- adr-023
- adr-063
- adr-080
- adr-123

## Applies To

- rust/rac-engine/src/rename.rs
- rust/rac-engine/tests/rename.rs
- rust/PORT-CONTRACT.d/16-closure-scaffold-writes.md
- docs/cli.md
