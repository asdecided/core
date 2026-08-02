---
schema_version: 1
id: RAC-01K8Q7MCP408
type: decision
---
# ADR-130: Transactional Rename Application

## Context

An artifact-id rename edits the target identity and every inbound reference.
Writing those files one at a time can leave the corpus half-renamed when a
later stale check, permission check, or filesystem replacement fails. A green
process exit must never hide a split identity/reference state.

## Decision

`decided rename --apply` uses a deterministic local transaction for all files in
the plan.

- Every affected file is read, checked for exact `old_line` staleness, and
  rendered in memory before any corpus path is replaced.
- Each rendered result is written and flushed to a hidden sibling staging file
  in the same directory. Staging uses exclusive creation; Unix opens the
  temporary final component with `O_NOFOLLOW`.
- During commit, each original moves to a hidden sibling backup and its staged
  replacement moves into the original path. Files are processed in the
  plan's first-seen path order.
- Any backup, replacement, or containment failure rolls committed files back
  in reverse order from their backups. A successful rollback says `corpus
  restored`; an incomplete rollback is reported explicitly with the paths that
  could not be recovered.
- Successful commits remove all staging and backup files. Cleanup failures are
  reported as a committed-but-cleanup-incomplete result; they never masquerade
  as a clean success.

The transaction remains bounded to the canonical root and the root-confined
mutation checks in ADR-129. Read-only walk behavior is unchanged.

## Status

Accepted

## Category

Technical

## Consequences

The identity and inbound references move together or the engine reports an
explicit failure. A later filesystem error can still make rollback impossible
if an external actor replaces a path during recovery, but the command reports
that condition rather than claiming success. Temporary siblings stay on the
same filesystem, so each rename operation is atomic at the individual-path
level and does not require a cross-volume coordination service.

The commit is intentionally not a database transaction: no filesystem-wide
multi-path atomic primitive exists across the supported platforms. Backups and
reverse-order restoration provide deterministic recovery within the corpus
boundary.

## Alternatives Considered

### Continue writing files sequentially in place

Rejected. A late stale or permission failure can leave references and identity
out of sync, which is precisely the integrity failure this decision closes.

### Stage files but do not retain backups

Rejected. Staging protects against a failure before commit, but cannot restore
files already replaced when a later rename fails.

### Use a database or filesystem snapshot

Rejected. The corpus is ordinary Markdown on filesystems with different
snapshot capabilities. Sibling backups preserve portability and keep the
mutation contract local and inspectable.

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "Transactional rename ordering and rollback are deterministic filesystem behavior."
rules:
  - id: rename-preflights-before-commit
    kind: require_pattern
    path_glob: "rust/rac-engine/src/rename.rs"
    pattern: "PreparedRenameFile"
    message: "Rename must render all affected files before replacing any corpus path."
  - id: rename-stages-sibling-files
    kind: require_pattern
    path_glob: "rust/rac-engine/src/rename.rs"
    pattern: "create_new"
    message: "Rename staging must use exclusive sibling temporary files."
  - id: rename-rolls-back-on-failure
    kind: require_pattern
    path_glob: "rust/rac-engine/src/rename.rs"
    pattern: "rollback_transaction"
    message: "Rename commit failures must attempt deterministic reverse-order recovery."
```

## Related Decisions

- adr-007
- adr-023
- adr-129

## Applies To

- rust/rac-engine/src/rename.rs
- rust/rac-engine/src/output.rs
- rust/rac-engine/tests/rename.rs
- rust/PORT-CONTRACT.d/16-closure-scaffold-writes.md
- docs/cli.md
