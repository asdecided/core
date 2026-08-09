---
schema_version: 1
id: RAC-KWJ8S74MXFG0
type: requirement
---
# Requirement: Federated Resolution and Provenance

## Status

Proposed

Classification: `[internal]` — combine one verified parent with a child through
the existing deterministic read, routing, and enforcement paths while keeping
every artifact attributable.

## Problem

A verified parent is useful only if it participates in the same effective
corpus as local artifacts. The released Rust engine keys parsed items,
relationships, search entries, graph views, caches, and MCP results primarily
by an unqualified path or id. A directory-level concatenation would make
collisions and provenance ambiguous, and integrating only search would leave
`decisions-for` and `gate --code` unaware of inherited governance.

## Requirements

- [REQ-001] Inherited artifacts MUST enter one source-aware effective corpus as a read-only layer through the engine's existing seams before validation, resolution, search, graph, scope, export, cache, freshness, or MCP models are derived. Consumers MUST NOT merge corpus walks independently or introduce a second resolver, and the local walk MUST exclude the declared parent materialisation subtree so the same Markdown file cannot enter both layers.
- [REQ-002] A parent/child canonical-id collision MUST surface as an explicit, deterministic, stable-coded cross-corpus finding with no implicit precedence in either direction (ADR-089, ADR-136). A colliding legacy or title alias MAY coexist, but an unqualified reference to it MUST be ambiguous and require qualification.
- [REQ-003] Overrides MUST be explicit under `## overrides` in `.decided/corpus.md`: one qualified parent canonical id MUST map to one local same-type replacement and one live local Decision as rationale. An undeclared duplicate and an absent, ambiguous, cross-type, retired-rationale, chained, or parent-to-parent mapping MUST remain validation errors.
- [REQ-004] Provenance MUST be preserved end to end in resolution, search and grounding results, relationship and validation findings, code-enforcement reports, MCP responses and audit records, and all export projections (ADR-089, ADR-135). It MUST distinguish local from inherited, name the stable source, and identify the verified pin for inherited output.
- [REQ-005] Export composition MUST reuse `rac-export-source-identity`, retain each record's own source, key global identity as `(source, id)`, and include inherited records by default (ADR-026, ADR-135, ADR-142). Repeated records MAY deduplicate only when their bodies and verified pins agree; a mismatch MUST be an explicit aggregation conflict, and `--local-only` MUST emit only the child projection without changing the default contract.
- [REQ-006] Determinism MUST hold: identical child and parent materialised bytes plus the same manifest MUST produce byte-identical validation, resolution, retrieval, enforcement, MCP, and export output across runs, machines, and clones (ADR-002).
- [REQ-007] Single-corpus behaviour MUST remain unchanged: without `.decided/corpus.md`, federation MUST add no output or behaviour change relative to the contemporaneous single-corpus engine after the export-schema and source-identity prerequisites, asserted by golden regression.
- [REQ-008] Child validation MUST never demand changes to parent bytes, no finding's remediation may require editing the read-only layer, and mutation commands MUST receive only the local writable layer (ADR-065, ADR-138).
- [REQ-009] Corpus items, validation rows, index entries, relationship endpoints, resolved artifacts, derived generations, and persistent-store records MUST carry enough source and layer identity to distinguish `(source, relative path)` and `(source, canonical id)`.
- [REQ-010] Existing unqualified references MUST resolve when exactly one artifact across both layers matches. The syntax `alias::canonical-id` MUST resolve only within the named source; aliases after `::` MUST be rejected, and no layer may receive implicit resolution precedence.
- [REQ-011] A valid override MUST make the local replacement effective for unqualified resolution, live retrieval, scope routing, and enforcement while leaving the original parent addressable by qualified id. Exports and responses MUST preserve the parent, its overridden state, and complete override provenance.
- [REQ-012] Inherited live Decisions MUST participate in `decisions-for`, `retrieve_grounding`, MCP `find_decisions` path lookup, `gate`, `sentry`, and `gate --code` through the existing scope and enforcement evaluators. Their `## Applies To` declarations and code-constraint globs MUST match against the child repository code tree, never the parent materialisation.
- [REQ-013] Enforcement and MCP MUST NOT expose a local-only bypass for a configured federated corpus. Human diagnostic reads and exports MAY accept `--local-only`.
- [REQ-014] Local and inherited artifacts MUST share one deterministic lexical and relationship index. Ranking MUST add no source boost or local-first quota, MUST retain the v0.28 lexical graph-floor gate, and MUST use `(source, relative path)` as the stable final tie order in a federated corpus.
- [REQ-015] The six-tool MCP surface MUST remain unchanged. Existing id arguments MUST accept qualified ids, and responses MUST retain source, layer, and pin provenance under the existing hard character budget; provenance MUST NOT be truncated before artifact content.
- [REQ-016] The logical generation and cache identity MUST include the existing child corpus and governing-config inputs, manifest bytes, parent source, verified parent digest, and overrides. The source-aware store MUST use a new internal layout version; an older segment MUST degrade to a cache miss and MUST NOT be interpreted as a federated answer.
- [REQ-017] The freshness tracker MUST observe both materialised corpus roots, both governing config files, and the child manifest, and MUST refuse to serve a generation after the parent no longer matches its pin. Inherited recency MUST NOT be derived from child Git history.
- [REQ-018] MCP audit records MUST extend ADR-127's bounded returned-identity object only with fixed source, layer, and pin fields. They MUST NOT copy artifact bodies, excerpts, override mappings, or full response provenance; ADR-141 is the explicit amendment to ADR-127's path-only shape.

## Acceptance Criteria

- A child relationship naming a parent Decision resolves in CLI and MCP, and a
  qualified lookup returns source, inherited layer, and pin provenance.
- A same-canonical-id fixture fails with a sourced collision. A valid override
  clears it, routes and enforces the local replacement, and leaves the parent
  available through its qualified id.
- An inherited Decision scoped to a child path appears in `decisions-for` and
  `retrieve_grounding`; a violating child code change fails `sentry` and
  `gate --code` with the inherited Decision and source named in human, JSON,
  and SARIF output.
- MCP returns the same six tools, stays within its hard character budgets, and
  never drops provenance before optional artifact content.
- Audit extraction attributes a repeated artifact id to the exact source,
  layer, and verified pin without recording artifact content or override detail.
- A parent-byte or manifest change invalidates the serving generation and
  persistent cache. An old store layout degrades to rebuild without changing
  the answer.
- A combined-corpus ranking fixture proves a strongly matched local or parent
  artifact cannot be displaced by a weak but well-connected artifact below
  the v0.28 graph floor.
- Two clones of the same pinned state produce byte-identical outputs, while a
  no-manifest fixture remains byte-identical to the contemporaneous
  single-corpus goldens.

## Success Metrics

- A child repository cites, routes, and enforces a firm-wide Decision exactly
  as it does a local Decision, while every consumer can identify its source.
- Federation adds no MCP tool and no second implementation of resolution,
  scope matching, or code enforcement.

## Risks

- Different consumers build different effective corpora. Mitigation: REQ-001
  requires one source-aware loader before every derived model.
- A large parent changes relevance or consumes the response budget.
  Mitigation: one ranking contract, the lexical graph-floor invariant, hard
  budgets, and a federation track in the DecisionGrounding evaluation.
- An override silently deletes the governing history. Mitigation: the parent
  remains qualified-addressable and carries explicit override provenance.
- The watcher serves a stale parent after a pin or materialisation change.
  Mitigation: the parent digest and manifest are generation inputs and are
  verified before serve.

## Assumptions

- `rac-parent-corpus-inheritance` supplies one verified, materialised parent.
- `rac-export-source-identity` and the export schemas land before source-aware
  records are published.
- The current shared scope and Sentry evaluators can consume source-aware items
  without introducing parallel matching semantics.

## Related Decisions

- adr-002
- adr-007
- adr-016
- adr-026
- adr-033
- adr-055
- adr-065
- adr-066
- adr-080
- adr-089
- adr-103
- adr-104
- adr-105
- adr-112
- adr-117
- adr-119
- adr-121
- adr-123
- adr-127
- adr-128
- adr-133
- adr-134
- adr-135
- adr-136
- adr-137
- adr-138
- adr-139
- adr-140
- adr-141
- adr-142
- adr-143

## Related Designs

- corpus-federation-mechanism
- code-scope-consumption
- sentry-code-constraint-evaluation

## Related Roadmaps

- corpus-federation
- corpus-sync

## Related Requirements

- rac-parent-corpus-inheritance
- rac-export-contract-schemas
- rac-export-source-identity
- rac-path-decisions-lookup
- deterministic-decision-code-enforcement
