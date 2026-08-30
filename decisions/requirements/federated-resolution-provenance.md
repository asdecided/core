---
schema_version: 1
id: RAC-KWJ8S74MXFG0
type: requirement
---
# Requirement: Federated Resolution and Provenance

## Status

Proposed

Classification: `[internal]` — compose a verified corpus-source graph through
one deterministic read, routing, enforcement, MCP, cache, and export model while
keeping every artifact and explicit exception attributable.

## Problem

Source-aware keys make several corpora distinguishable, but a graph needs more
than a flat union. Aliases belong to the source that declares an edge, immutable
parent relationships must not change meaning when an unrelated sibling is
added, shared diamonds must count once, and overrides may form a recorded policy
chain. If each command or cache tier builds its own approximation, validation,
retrieval, enforcement, MCP, and exports can disagree about the effective
governance.

The graph must preserve historical records and complete override provenance
without letting history leak into live ranking or code enforcement.

## Requirements

- [REQ-001] Inherited artifacts MUST enter one source-aware verified closure before validation, resolution, search, graph, scope, export, cache, freshness, enforcement, or MCP models are derived. Consumers MUST NOT merge corpus walks independently or introduce a second resolver. The root-local walk MUST exclude every verified direct, transitive, diamond, and duplicate physical materialisation route.
- [REQ-002] A version-1 parent/child canonical-id collision MUST retain its explicit deterministic stable-coded finding and no implicit precedence. In version 2, equal canonical ids in distinct sources MUST remain separate catalog records; an unqualified use MUST surface a deterministic sourced ambiguity unless all visible candidates explicitly converge to one terminal `ArtifactKey`. Duplicate ids inside one source remain errors.
- [REQ-003] Overrides MUST remain explicit under the exact `## overrides` heading and its mapping version MUST equal the `## inherits` version. V1 MUST retain one qualified parent target, one local same-type replacement, one live local Decision rationale, and chain rejection. V2 MUST require one globally source-qualified inherited target, one same-type replacement local to the declaring corpus, and one live local Decision rationale, while permitting only the ancestry-level chains and explicit diamond convergence in ADR-147. A valid nested-v1 mapping in a v2 closure MUST normalize to one source-aware hop that a v2 ancestor may explicitly extend. Missing, ambiguous, mixed-version, cross-type, nonlocal, dead-rationale, duplicate-target, cyclic, order-dependent, and unresolved-fork mappings MUST be stable errors.
- [REQ-004] Provenance MUST be preserved end to end in resolution, search and grounding results, relationship and validation findings, code-enforcement reports, MCP responses and audit records, and every export projection (ADR-089, ADR-135). Fixed artifact origin MUST always contain stable source and root/inherited layer; inherited records MUST contain their owning-node pin and root-local records MUST omit `pin`. Full response/export override provenance MUST carry every hop's owner, target, replacement, rationale, and ADR-147 `overridden`/`replacement`/`lineage` state in its total order; ordinary artifact origin need not enumerate physical ancestry routes. Topology findings MUST carry the canonical route and route-count contract. Audit remains the narrower fixed identity in REQ-018.
- [REQ-005] Export composition MUST reuse `corpus-source-identity`, retain each record's own source and, for inherited records, pin, key global identity as `(source, id)`, and include the unique catalog by default (ADR-026, ADR-135, ADR-142). Inherited diamond records MAY deduplicate only when source, id, pin, path, and body agree; a mismatch MUST be an aggregation conflict. Root-local records MUST omit pin. Complete override-chain history MUST remain exported, and `--local-only` MUST emit only the root projection.
- [REQ-006] Determinism MUST hold: identical root and graph materialised bytes plus the same exact manifests MUST produce byte-identical validation, resolution, retrieval, enforcement, MCP, audit, and export output across runs, machines, and clones (ADR-002). Permuting only the root's direct-parent list MUST retain byte-identical public output although the exact-manifest generation changes. Permuting an inherited list MUST require bottom-up repinning and then retain the same effective non-provenance answer; its pin and generation provenance necessarily change. Filesystem iteration and checkout locations MUST NOT change answers or create precedence.
- [REQ-007] Single-corpus behavior MUST remain unchanged: without `.decided/corpus.md`, federation MUST add no output or behavior change relative to the contemporaneous engine. A v1 manifest MUST retain its accepted qualification, collision, override, finding, digest, pin-provenance, and output contract. V2 semantics MUST NOT reinterpret either mode.
- [REQ-008] Validation MUST never demand changes to inherited bytes, no finding's remediation may require editing a read-only node, and mutation commands MUST receive only the root-local projection (ADR-065, ADR-138). Every direct, transitive, and duplicate physical parent route MUST remain read-only to mutation and output commands.
- [REQ-009] Corpus items, validation rows, index entries, relationship endpoints, resolved artifacts, derived generations, and persistent-store records MUST distinguish `(source, relative_path)` and `(source, canonical_id)`. Physical locators and edge aliases MUST remain runtime/context data and MUST NOT become stable public identity.
- [REQ-010] Existing unqualified references MUST resolve when their visible effective candidates reduce to exactly one terminal key. When several historical candidates converge, the catalog endpoint MUST preserve the authored token and the complete candidate set sorted by `(source, canonical_id)`, plus the separate effective terminal; it MUST NOT invent a preferred original key. Version 2 MUST accept globally stable `corpus.source::canonical-id` for every visible source and direct `alias::canonical-id` only in the declaring corpus's lexical context; the suffix MUST be canonical-only. Authored references MUST resolve in the view rooted at their owning source, while public lookup uses the root context. No source, layer, depth, or manifest position may receive precedence.
- [REQ-011] A valid override MUST make its unique terminal effective for unqualified resolution, live retrieval, graph ranking, scope routing, and enforcement while global source-qualified lookup retains every original and intermediate record. The catalog MUST preserve every mapping owner and rationale. Historical relationship endpoints MUST retain the authored token and one-or-many sorted original candidate keys; effective endpoints MUST carry the unique redirected terminal or fail as ambiguous.
- [REQ-012] Effective inherited Decisions at every graph depth MUST participate in `decisions-for`, grounding, MCP path lookup, Gate, Sentry, and `gate --code`. Their `## Applies To` declarations and code-constraint patterns MUST match the invocation root code tree, never a materialisation tree. A terminal replacement governs; an override nonterminal does not.
- [REQ-013] Enforcement and MCP MUST NOT expose `--local-only`, cache-off, or another root-only bypass for a configured graph. Human review and advisory diagnostics MUST remain root-subject after central closure verification so inherited warnings are not duplicated. Viewer, documents, and graph exports MAY expose root-only output without changing their inherited-by-default contract.
- [REQ-014] Root-local and inherited terminal artifacts MUST share one deterministic lexical and relationship index. Ranking MUST add no source boost or local-first quota, MUST retain the v0.28 lexical graph-floor gate, and MUST use `(source, relative path)` as the stable final tie order in a federated corpus. It MUST also add no graph-depth boost, count diamond artifacts and edges once, and exclude override nonterminals from live popularity.
- [REQ-015] The six-tool MCP surface MUST remain unchanged. ID arguments in v2 MUST accept global source qualification and root direct aliases under the same contextual resolver. Every tool MUST use one request-current closure across cache-on, cache-off, resident, and store-hit paths. This requirement amends ADR-141's v2 response provenance to include the complete ADR-147 ordered chain atomically: optional content and list entries reduce first, and if the chain cannot fit the tool MUST return `response_budget_exceeded`. ADR-141/ADR-127 audit exclusions remain unchanged.
- [REQ-016] The logical generation and cache identity MUST be SHA-256 over ADR-148's exact `asdecided-federation-generation-v3\0` domain, tag/u64be-length framing, tag table, sorted node/edge/mapping/terminal rows, root and node snapshots, exact limit block, recursive/root-corpus inputs, and five literal subsystem fingerprints. Its canonical text MUST be `sha256-v3:` plus lowercase hex. Graph persistence MUST use `store/v3`; older segments MUST be cache misses and MUST NOT be interpreted as graph answers.
- [REQ-017] Freshness MUST observe the root and every inherited materialisation/corpus root, governing config, manifest path including absence, and captured artifact. Create, content/identity/type change, remove, rename, watcher overflow, and lost coverage on those inputs MUST trigger full candidate recapture in the first graph implementation. A failed edge, node, pin, bound, snapshot, or override MUST make the prior generation inaccessible; stale fallback is forbidden. Inherited recency MUST NOT be borrowed from root Git history.
- [REQ-018] MCP audit records MUST extend ADR-127's bounded returned-identity object only with path, source, layer, and optional pin: `pin` MUST be present for inherited records and omitted for root-local records. They MUST NOT copy artifact bodies, excerpts, topology, aliases, override mappings, or full response provenance. Any closure preparation or serving failure MUST emit exactly one audited error event with `returned: []`, without exposing a stale identity.
- [REQ-019] Every unique inherited node MUST be structurally and relationally validated in its owning source's visibility context. One node error MUST produce one deterministically selected sourced root blocker rather than one per physical route; parent warnings and review advisories MUST remain parent-owned. Cycle, divergent pin, cross-branch ambiguity, invalid override, and override divergence MUST be root composition findings.
- [REQ-020] The composed model MUST expose distinct catalog, root-effective, and root-local projections. Catalog MUST contain every unique historical and terminal record; effective MUST contain only root-local records and inherited terminals with redirected live endpoints; root-local MUST be the sole mutation and root-owned review projection. Qualified history MUST NOT leak catalog edges into live ranking or enforcement.
- [REQ-021] Portal identity and navigation MUST key every artifact and edge by `(source, id)` and MUST retain and independently address three or more records sharing one canonical id without collapse. Global qualified links MUST be copyable. OKF and generated agent-rule projections MUST remain root-local unless separately decided.
- [REQ-022] Override composition MUST be source-scoped and bottom-up. A node MUST union and deduplicate its direct parents' effective projections before applying its own mappings. A branch-local exception MUST NOT suppress the same shared ancestor still live through a sibling, and a join MUST reject divergent terminals unless its own explicit local mappings reconverge every live branch. Complete mapping rows MUST use parents-before-child, source-lexicographic Kahn owner rank and then bytewise `(owner source, target key, replacement key, rationale key)` order.

## Acceptance Criteria

- Root and inherited global qualification resolve through CLI and every MCP
  tool; owner-scoped aliases with the same spelling resolve independently and
  a qualified legacy/title alias is rejected.
- Three sources may carry the same canonical id and remain qualified-readable.
  Bare use is ambiguous until every candidate explicitly converges on one
  terminal.
- A parent-authored relationship is unchanged when an unrelated sibling is
  added. Catalog graph lookup retains the original endpoint, while root live
  graph lookup follows an explicit terminal override.
- `A -> B -> C` and an explicitly reconciled diamond retain every artifact,
  rationale, and hop. An incomplete fork, chain cycle, inherited replacement,
  wrong type, and dead rationale fail deterministically.
- A shared diamond artifact, relationship, warning owner, BM25 row, and inbound
  edge count appear once. An override nonterminal cannot gain ranking or govern
  root code.
- Inherited Decisions from every depth appear in path routing and grounding and
  fail violating root code through ordinary Sentry/Gate output with source and
  pin in human, JSON, and SARIF forms.
- All six MCP tools agree across cold, resident, store-hit, and cache-disabled
  paths. Tight budgets preserve the full chain or return the explicit budget
  error. A stale leaf produces one empty-return audit error and no old identity.
- Default exports retain the unique full catalog and complete chains;
  `--local-only` returns only root records. Portal keeps three same-id sources
  distinct and navigable.
- Every direct and transitive materialisation is absent from full/diff Sentry
  code enumeration and remains byte-identical after all mutation and output
  commands.
- A topology, manifest, config, artifact, pin, edge, or override change
  invalidates resident and persistent state. Old stores miss; cold, warm,
  cache-disabled, and rebuilt outputs agree byte-for-byte.
- Complete no-manifest and v1 golden suites remain unchanged.

## Success Metrics

- A root repository can cite, retrieve, route, and enforce decisions from a
  valid bounded parent graph while every consumer identifies exact source and
  pin.
- Federation adds no MCP tool and no second implementation of resolution,
  ranking, scope, enforcement, freshness, or export composition.
- Explicit policy history survives graph composition without letting
  historical rows change the root's live answer.

## Risks

- A flat alias map reinterprets immutable parent relationships. Mitigation:
  one contextual resolver keyed by authoring source and edge-local alias.
- Branch policy silently suppresses a sibling. Mitigation: bottom-up scoped
  effective views and explicit join reconciliation.
- Catalog history leaks into live ranking or enforcement. Mitigation: separate
  catalog/historical and effective/redirected projections with parity tests.
- One cache or MCP fallback reconstructs a different closure. Mitigation: all
  consumers receive one request-current `VerifiedFederation` generation.
- Long chains exceed response budgets. Mitigation: chain provenance is atomic
  and fails explicitly after optional reductions.

## Assumptions

- `parent-corpus-inheritance` supplies the fully verified exact-byte closure and
  complete read-only-root set.
- `corpus-source-identity` remains the stable outer namespace.
- Existing source-neutral ranking and root-code evaluators can consume the
  effective source-aware projection without parallel semantics.
- ADR-146 through ADR-148 are accepted authority for resolution, chain, and
  serving implementation.

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
- adr-119
- adr-123
- adr-127
- adr-128
- adr-135
- adr-137
- adr-138
- adr-139
- adr-140
- adr-141
- adr-142
- adr-144
- adr-145
- adr-146
- adr-147
- adr-148

## Related Designs

- corpus-federation-mechanism
- corpus-federation-graph-composition
- code-scope-consumption
- sentry-code-constraint-evaluation

## Related Roadmaps

- corpus-federation
- corpus-sync

## Related Requirements

- parent-corpus-inheritance
- corpus-source-identity
- export-contract-schemas
- rac-path-decisions-lookup
- deterministic-decision-code-enforcement
