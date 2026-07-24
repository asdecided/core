# Traceability gaps — consolidated report

Growth-programme traceability gap audit. **Refreshed 2026-06-28 against the live
corpus** (the original pass was 2026-06-12 at corpus v0.10.3). Each gap below
carries its current status — **OPEN**, **PARTIAL** (a mechanism shipped but the
gap is not fully closed), or **CLOSED** — with the shipped mechanism named, and
≥3 concrete current instances for the ones still open. Every instance cites a
real file in this repository; none is speculative.

The linking mechanisms that exist **today**: per-type `## Related <Type>`
sections for all five families including **self-type** `related decisions` /
`related roadmaps` (shipped v0.13.5), `## Supersedes` (decisions only), and
external `## Related Tickets` (ADR-087). Relationship validation now flags
broken, ambiguous, self, cyclic, **type-mismatched** (`relationship-target-type-
mismatch`), **unsupported** (`relationship-edge-unsupported`), and **retired**
targets, and `rac doctor` surfaces **mentioned-but-unlinked** body references
(`unlinked-reference`, ADR-082, shipped #225).

## Headline gap (self-type relationships) — CLOSED

roadmap→roadmap and decision→decision references produced zero edges at v0.10.3.
**Closed:** `related roadmaps` and `related decisions` are now in the relationship
vocabulary (v0.13.5); the audit entry `rac-traceability-self-relationships`
(Accepted) records it. The 11 + 8 evidence instances now resolve.

---

## 1. Links from artifacts to non-artifact repo files — OPEN

A requirement satisfied by a README section, a skill file, a CI workflow, or a
doc page cannot reference it; renames break the link silently. ADR-019 added
**asset** references (images), which narrows but does not close this — arbitrary
repo-file traceability (`## Satisfied By` / `## Related Files`) is still missing.

- Current instances: `rac-growth-positioning` (satisfied by a README section it
  cannot reference), `rac-growth-agent-skill` (satisfied by
  `.claude/skills/rac-artifacts/SKILL.md`), `rac-documentation-structure` (places
  testable requirements on doc files by bare path), `adr-027` (governs CI
  workflow files the same way).
- Minimal addition: an optional `## Satisfied By` / `## Related Files` section
  accepting repo-relative paths, existence-checked by `rac relationships
  --validate`.

## 2. External source / URL references as structured data — PARTIAL

**Partially closed:** ADR-087 shipped the external-reference mechanism as
`## Related Tickets` (format-linted, not fetched — determinism holds), now used
in 7 artifacts. **Still open:** generic source/URL citations (a `## Sources`
section) — competitor-comparison sources and the essay-series URLs remain
unstructured prose/comments invisible to validation and MCP.

- Current instances: `rac-growth-positioning` comparison sources, the
  `growth-essay-mapping` external article side, `rac-grounding-eval-benchmark`
  external references.
- Minimal addition: an optional `## Sources` section, one URL per line with an
  optional status token, exposed via `get_artifact`, never fetched.

## 3. Typed edge semantics (implements / depends-on / satisfies) — OPEN (narrowing)

ADR-074 made the **graph export** surface typed edges, but the **authoring**
vocabulary is still invented and unvalidated: edge-label sub-headings inside
non-schema sections. Down to 2 live instances (others archived).

- Current instances: `adr-028-explorer-surface` (`### Implements`),
  `rac-product-knowledge-navigator-explorer` (`### Depends On`).
- Minimal addition: validated optional edge-label sub-headings
  (`### Implements`, `### Depends On`, `### Satisfies`) inside `Related <Type>`
  sections, defaulting to untyped when absent.

## 4. Machine-readable lifecycle / gate status — PARTIAL

**Partially closed:** ADR-061 gave roadmaps a validated lifecycle vocabulary
(Planned / Achieved / Superseded / Abandoned), and per-type **Status is already a
validated enum** for requirements/decisions/designs/prompts (a value off the
allowed set is an error). **Still open (narrowed):** the enum has no `Deferred`
(parked) state distinct from `Proposed`, no structured `Blocked` state, and
GATE-1/GATE-2 blocks are free-text lines nothing can enumerate.

- Current instances: `rac-growth-contribution-policy` (GATE-2 in prose),
  `rac-growth-positioning` (GATE-1 in prose), parked work reading `Proposed`
  with no `Deferred` state.
- Minimal addition: append `Deferred` / `Blocked` to the status enums plus a
  structured gate-reason, so blocked artifacts are enumerable by gate.

## 5. Supersession outside decisions — PARTIAL

**Partially closed:** ADR-061 gave roadmaps a `Superseded` **status**. **Still
open:** there is no roadmap→roadmap supersedes **edge** (the target it was
replaced by); `## Supersedes` stays decision-only, so the link is a status flag,
not a resolvable edge.

- Current instances: the archived single-item series flattened during the
  v0.28–v0.30 → codename renames; `repo-extraction-programme` superseded by the
  `repo-topology` series in prose only.
- Minimal addition: permit `## Supersedes` on roadmaps, existence-checked
  (including archived targets).

## 6. Relationship directionality / back-references — OPEN

`related_*` edges are undirected, so symmetry is implicit for declared links —
but a target artifact still cannot advertise who cites it, and there is no
advisory when an expected back-reference is absent. The new `unlinked-reference`
finding (#225) is adjacent (it surfaces prose mentions), not this.

- Current instances: ADR-036 ← `v0.10.2` (no reverse edge); early ADRs cited by
  `rac-agent-context-guide` that carry no Related sections themselves.
- Minimal addition: an advisory asymmetric-edge finding in
  `rac relationships --validate` (no schema change).

## 7. Scoping / applies-to — OPEN

Decisions scope themselves to a path or component in **prose**, not data, so
"which decisions apply to the file I'm editing" is unanswerable.

- Current instances: `adr-035` (applies to RAC Core + OSS extensions),
  `adr-018` (governs `rac/`), `adr-027` (CI workflows), `adr-033` (Guide tool
  surface).
- Minimal addition: an optional `## Applies To` section on decisions accepting
  paths or component names; enables scoped grounding in the Lore MCP server.

## 8. Validation blind spots in relationship extraction — CLOSED

**Closed:** targets are now type-checked (`relationship-target-type-mismatch`),
unrecognised/unsupported `Related *` sections are flagged
(`relationship-edge-unsupported`), retired targets warn
(`relationship-target-superseded`), and mentioned-but-unlinked body references
surface in `rac doctor` (`unlinked-reference`, #225). The v0.10.3 instances
(`## Related Artifacts` resolving to nothing; targets to Unknown types) now
produce findings instead of silent passes.

## Authoring-friction findings (not schema gaps)

- **`[REQ-NNN]` statements cannot hard-wrap** — still OPEN: a continuation line
  raises `req-missing-id` (`validation.py:466`), forcing one-line statements
  against the 72-column prose convention.
- **`rac new` does not create parent directories** — still OPEN by design
  (`create.py`: deliberate no-auto-create); mitigated for the cold-start path by
  `rac quickstart`, which scaffolds the directory and the artifact in one step.

---

## Summary

| # | Gap | Status |
| --- | --- | --- |
| — | Self-type relationships (roadmap/decision) | CLOSED (v0.13.5) |
| 1 | Artifact → repo-file links | OPEN |
| 2 | External source / URL references | PARTIAL (tickets via ADR-087; URLs open) |
| 3 | Typed edge semantics | OPEN (narrowing) |
| 4 | Lifecycle / gate status as data | PARTIAL (roadmaps via ADR-061; requirements open) |
| 5 | Supersession outside decisions | PARTIAL (status via ADR-061; edge open) |
| 6 | Directionality / back-references | OPEN |
| 7 | Scoping / applies-to | OPEN |
| 8 | Relationship-extraction blind spots | CLOSED |

Four schema gaps remain squarely open (1, 3, 6, 7) and three are partial (2, 4,
5). Each is specific enough to design a schema change from, which is the audit's
purpose (growth-programme success measure).

## Recorded as corpus intent

The open/partial gaps are now promoted to the trusted corpus (the pattern set by
`rac-traceability-self-relationships`): the `relationship-vocabulary` roadmap with
a requirement per gap, tracked under epic **itsthelore/asdecided-core#236**.

| Gap(s) | Requirement |
| --- | --- |
| 1 + 2 | `rac-external-and-file-references` |
| 5 | `rac-roadmap-supersession` |
| 3 | `rac-typed-relationship-edges` |
| 6 | `rac-relationship-back-references` |
| 7 | `rac-decision-applies-to-scope` |
| 4 | `rac-artifact-status-vocabulary` |
| (closed) self-type | `rac-traceability-self-relationships` |
