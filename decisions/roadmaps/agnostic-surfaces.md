---
schema_version: 1
id: RAC-KWGMT8B29552
type: roadmap
---
# Language-Agnostic Surfaces Programme

## Status

Planned

## Context

The SDK expansion council (`sdk-expansion-strategy`) ranked "deepen the
language-agnostic surfaces — no new language SDK this cycle" first, as a
funded workstream. The structural finding: under ADR-063's thin-client
model every language SDK still requires the pip-installed Python `rac`
CLI, so no wrapper removes the real conversion blocker for non-Python
users — while the surfaces where those users already meet Lore (MCP,
editors, CI, agent-rules) are language-agnostic. This programme funds
those surfaces and pays down the standing preconditions any future SDK
inherits.

## Outcomes

Non-Python developers adopt Lore through contract surfaces, without a new
language SDK and without the `pip install` barrier deciding the outcome:

- A team can run the engine anywhere a container runs — GitLab CI,
  Bitbucket Pipes, Jenkins docker agents — from one official image, with
  no Python toolchain of their own.
- CI platforms beyond GitHub consume findings natively: GitLab
  code-quality widgets and JUnit-consuming dashboards render `rac`
  results without translation glue.
- The ADR-063 native-port preconditions stop being hypothetical: the
  artifact specs live in a language-neutral data file and a conformance
  fixture suite proves output parity, with the existing TypeScript SDK
  as its first consumer.
- The TypeScript SDK reaches a stable 1.0, so any future SDK copies a
  proven, conformance-tested surface instead of a moving one.

## Initiatives

Two parallel tracks with no ordering between them; each is sequenced
internally. Each member item is recorded in `rac/roadmaps/future/` and
graduates to execution with a GitHub issue per ADR-093.

- Track 1 — Distribution and CI reach:
  - Official OCI image (`oci-image`): one published image bundling the
    CLI; the single artifact that unlocks every docker-native CI
    platform with zero wrapper code. Shipped (`ghcr.io/itsthelore/asdecided-core`,
    quickstart-documented); its artifact is now Achieved and lives
    top-level, no longer under `future/`.
  - CI report formats (`ci-report-formats`): GitLab code-quality JSON
    and JUnit XML renderers beside the shipped SARIF output (ADR-054),
    additive per ADR-007.
  - Platform wrappers: delegated to the existing `rac-ci` topology item
    (ADR-092), which consumes the image — a GitLab CI component and a
    Bitbucket pipe mirroring the shipped GitHub Actions. This programme
    adds no wrapper scope of its own.
- Track 2 — Contract hardening:
  - Artifact-spec extraction (`artifact-specs-extraction`): move
    `ARTIFACT_SPECS` to a language-neutral data file the engine reads —
    ADR-063 native-port precondition one.
  - Conformance fixtures (`conformance-fixtures`): a cross-language
    fixture suite proving output parity, seeded from the existing golden
    outputs — ADR-063 native-port precondition two; the TypeScript SDK
    is its first consumer. May start immediately; consumes the spec
    extraction where useful.
  - TypeScript SDK stable release (`ts-sdk-stable-release`): close the
    documentation and CI gaps and publish 1.0 once the conformance suite
    is green.
- Delegated, not claimed: MCP tool depth and harness integration recipes
  (`lean-context-delivery`, `integration-recipe-factory`) are sequenced
  by the `deterministic-substrate` programme, Tranche A. This programme
  relates to them and must not double-claim their scope.

## Success Measures

- Each member item graduates from `rac/roadmaps/future/` with a GitHub
  issue per ADR-093 before implementation begins.
- A non-GitHub CI platform runs a `rac` gate using only the official
  image and documented snippets — no bespoke install steps.
- The conformance suite runs in the TypeScript SDK's CI against a real
  engine, replacing the currently skipped integration path.
- `rac validate rac/`, `rac relationships rac/ --validate`, and
  `rac review rac/` stay clean across the programme's output.
- The reordering triggers recorded in `sdk-expansion-strategy` remain
  the only mechanism that promotes new language SDK work; this
  programme creates none.

## Assumptions

- ADR-063 holds: thin clients over the contract, native port gated
  behind the two preconditions this programme funds.
- ADR-092 topology holds: wrapper delivery consolidates under `rac-ci`;
  language SDKs stay subdirs of `rac-sdk`.
- The `deterministic-substrate` programme proceeds independently; its
  Tranche A items are not blocked by, and do not block, either track
  here.
- Solo-maintainer capacity: tracks are parallel in dependency terms, not
  necessarily in execution.

## Risks

- The image becomes a second, drifting install surface; mitigated by
  building it in the engine repo, pinning it to the same CalVer releases
  (ADR-076), and treating it as packaging of `rac-core`, not a fork.
- New report renderers grow non-deterministic output (timestamps,
  unordered findings) that breaks byte-pinned goldens; mitigated by
  following the SARIF renderer's deterministic-ordering pattern
  (ADR-002, ADR-066).
- Spec extraction quietly changes validation semantics; mitigated by
  extending the schema-agreement gate to assert data-file/engine parity
  and keeping the change behaviour-neutral (ADR-023 clean break applies
  to internals only).
- A 1.0 label on the TypeScript SDK before the conformance suite exists
  would freeze an unproven surface; mitigated by sequencing 1.0 after
  the suite is green within Track 2.

## Related Decisions

- ADR-007
- ADR-027
- ADR-054
- ADR-063
- ADR-111
- ADR-086
- ADR-092
- ADR-093
- ADR-094

## Related Designs

- sdk-expansion-strategy

## Related Roadmaps

- oci-image
- ci-report-formats
- artifact-specs-extraction
- conformance-fixtures
- ts-sdk-stable-release
- rac-ci
- deterministic-substrate

## Related Tickets

- itsthelore/asdecided-core#280
