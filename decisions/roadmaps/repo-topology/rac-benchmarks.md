---
schema_version: 1
id: RAC-KW6F4BCY46EQ
type: roadmap
tags: [structure, org, benchmark, distribution]
---
# Restructure decisiongrounding into rac-benchmarks

## Status

Planned

## Context

ADR-092 places benchmarks in a single `rac-benchmarks` repository, subdir per
benchmark, with the existing grounding benchmark as `decisiongrounding/`. The
`decisiongrounding` repository was extracted in v0.22.4 (history-preserving) and
treats `rac` as an external CLI on `PATH`, importing no engine code; ADR-066
fixes its scoring as deterministic. This item reshapes that standalone repo into
the family form so future benchmarks join as sibling subdirs rather than new
repos.

## Outcomes

- `itsthelore/asdecided-benchmarks` exists with the grounding benchmark under
  `decisiongrounding/`; new benchmarks land as sibling subdirs.
- The benchmark still consumes `rac` only as an external CLI on `PATH`, with no
  engine coupling and no change to its deterministic scoring (ADR-066).
- `rac-core` references to the benchmark's home resolve to `rac-benchmarks`.

## Initiatives

- **Create `rac-benchmarks`** and move the `decisiongrounding` repo's contents
  under `decisiongrounding/` (history preserved); archive the standalone
  `decisiongrounding` repo with a redirect note.
- **Establish the subdir convention** so a second benchmark is a new subdir, not
  a new repo (the family pattern, ADR-092).
- **Update references** in `rac-core` docs/corpus from the standalone
  `decisiongrounding` repo to `rac-benchmarks/decisiongrounding/`.

## Success Measures

- `itsthelore/asdecided-benchmarks` exists with `decisiongrounding/`; the standalone
  repo is archived with a redirect.
- The benchmark runs unchanged against the published `rac` CLI; its determinism
  guarantees (ADR-066) are untouched.
- No `rac-core` reference points at the standalone `decisiongrounding` repo.

## Assumptions

- The benchmark continues to consume `rac` only as an external CLI on `PATH`
  (DG-ADR-0001 holds), so the restructure needs no code change.
- ADR-066's deterministic scoring contract is independent of where the benchmark
  lives.
- The maintainer can create `rac-benchmarks` and archive the standalone repo.

## Risks

- **Stranded links** to the old `decisiongrounding` repo. Mitigation: GitHub
  rename/redirect and a `rac-core` reference sweep.
- **Single-benchmark repo looks premature.** Mitigation: the family form is the
  point — it makes the next benchmark a subdir, not a repo decision to
  re-litigate.

## Related Decisions

- adr-092
- adr-066

## Related Roadmaps

- repo-topology-convergence
- v0.22.4-extract-decisiongrounding
- tool-benchmarks
