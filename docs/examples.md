# Examples

Short, realistic artifacts you can adapt. These show the *shape* of each type — for a
blank starter with section guidance, use `decided schema <type> --template`
([cli.md](cli.md#schema)).

For a complete repository-shaped example, see
[`examples/federation/`](https://github.com/asdecided/core/tree/main/examples/federation):
two direct parents, a shared diamond ancestor, exact pins, and a reviewed
override that can be inspected with `decided corpus status` and
`decided corpus explain`.

## A requirement

```markdown
# Usage Export

## Problem

Analysts need to take dashboard data into their own spreadsheets, but today the only
way out is copy-paste, which is slow and error-prone.

## Requirements

- [REQ-001] Users can export the current chart as a CSV file.
- [REQ-002] The export reflects the active filters and date range.

## Success Metrics

- 25% of active accounts export at least once per month.

## Risks

- Large exports could time out for accounts with long histories.
```

Save it as `usage-export.md` and it resolves to the id `usage-export`.

## A decision that references it

Artifacts connect through `## Related …` sections. This decision points back at the
requirement above:

```markdown
# ADR-010 CSV as the Export Format

## Context

Usage Export (REQ-001) needs a file format. The choices are CSV, XLSX, and JSON.

## Decision

Export as CSV. It opens everywhere, stays diff-friendly, and needs no new dependency.

## Consequences

Rich types (colors, formulas) are lost, which is acceptable for tabular analytics.

## Status

Accepted

## Category

Technical

## Related Requirements

- usage-export
```

Validate the link with `decided relationships . --validate` — see
[relationships.md](relationships.md).

## A roadmap

```markdown
# Export Capabilities

## Outcomes

Analysts can move dashboard data into their own tools without manual rekeying.

## Initiatives

- Ship CSV export for charts.
- Add scheduled email summaries.

## Success Measures

- A quarter after launch, a quarter of accounts use an export feature.
```

## A prompt

```markdown
# Requirement Review

## Objective

Review a requirement artifact for structural completeness.

## Input

A single requirement Markdown file.

## Instructions

Check for a clear problem statement and testable [REQ-NNN] requirement lines.

## Output

A bulleted list of findings, one per gap.
```

## A design

```markdown
# Export Button Placement

## Context

The dashboard toolbar is getting crowded as we add an export action.

## User Need

Analysts want export within reach without hunting through a menu.

## Design

A single "Export" button in the toolbar opens a small menu (CSV today, more later).

## Constraints

Must stay keyboard-accessible and meet WCAG AA contrast.
```

## Diffing two versions

This repository ships a before/after pair under `examples/`. Compare them to see how
RAC reports requirement and metric changes:

```bash
decided diff examples/example_dashboard_v1.md examples/example_dashboard_v2.md
```

See [cli.md](cli.md#diff) for the full output.
