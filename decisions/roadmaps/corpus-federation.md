---
schema_version: 1
id: RAC-KWJ8RTRK4JWM
type: roadmap
---
# Corpus Federation Programme

## Status

Planned

The intended v0.29.0 release target is graph-complete corpus federation, not
only the one-parent foundation. The accepted ADR-133 through ADR-143 set and its
implementation establish the source-aware substrate and exact v1 compatibility.
The proposed ADR-144 through ADR-148 set adds a bounded multi-parent DAG,
topology-binding pins, global source qualification, explicit override chains,
and closure-wide serving state. Engine work outside the accepted one-parent
boundary waits for explicit human ratification of those five decisions.

Execution order and task state remain in GitHub under ADR-093. This roadmap
records durable intent and does not claim that proposed graph behavior has
shipped.

## Context

The released engine began from one corpus root. The federation foundation makes
identity, relationships, retrieval, scope, cache, MCP, enforcement, and exports
source-aware through one read model. It verifies one materialised parent and
preserves no-manifest output exactly.

Organisations rarely have only one shared concern. A service may need a firm
standard, a security policy, a regulatory corpus, and product decisions; those
sources may share ancestors or make recorded exceptions at intermediate levels.
A synthetic combined parent would hide ownership and move conflict handling
outside AsDecided. A naive parent list would introduce order precedence,
double-count diamonds, mis-scope aliases, and leave transitive topology outside
the direct pin.

Graph federation extends the same deterministic substrate. Every source remains
pre-materialised inside the root repository, every edge and node is pinned,
the root remains the only writable truth, and every consumer uses one verified
closure. AsDecided remains the decision layer for coding agents rather than a
generic graph platform or hosted synchronisation service.

Machine-checkable export schemas and `corpus.source` are still the only
`corpus-sync` prerequisites. Point-in-time export, change feeds, and section
anchors do not block federation.

## Outcomes

- A root repository declares several independently pinned direct parents in a
  strict `.decided/corpus.md` v2 manifest and recursively verifies their
  materialised ancestry without networking.
- Cycles, divergent pins, duplicate direct sources, escaping paths, invalid
  nodes, unresolved policy forks, and fixed-limit excesses fail before any
  partial overlay reaches a consumer.
- Same-source/same-pin diamonds verify every physical route and deduplicate to
  one logical catalog record, relationship, warning owner, search row, and
  export record.
- Any visible artifact is durably addressable as
  `corpus.source::canonical-id`; direct aliases remain context-local
  conveniences. Equal ids in distinct sources remain distinct and bare use is
  ambiguous rather than order-resolved.
- Decision-backed overrides can form explicit ancestry chains and reconcile
  diamond policy branches without deleting any original, intermediate,
  rationale, or mapping history.
- One root-effective projection feeds retrieval, relationship ranking, path
  routing, Gate, Sentry, all six MCP tools, and summaries. Inherited Decisions
  at every depth govern root code.
- One closure generation feeds cold, resident, persistent, cache-disabled, and
  event-rebuilt reads and fails closed after any leaf or topology change.
- Viewer, documents, and graph exports retain the unique full catalog and
  complete chain provenance; `--local-only` remains root-only and cannot bypass
  MCP or enforcement.
- No-manifest and v1 repositories retain their accepted observable behavior.

## Initiatives

### Graph authority and contract

Review ADR-144 through ADR-148 separately. On explicit acceptance, mark
ADR-133, ADR-136, and ADR-143 Superseded atomically and record the narrower
amendments to ADR-134, ADR-137, ADR-138, ADR-141, and ADR-142. Keep ADR-135,
ADR-139, and ADR-140 substantively active while recording ADR-144's
same-source/same-digest physical-route clarification to ADR-135. Move the graph
design to Accepted in that transition. Amend accepted
`corpus-source-identity` REQ-005/007/008 to reference the new authority set,
unique logical nodes, same-digest diamond routes, and version-2 equal-id
ambiguity. Keep the implementation requirements Proposed until evidence is
complete and the roadmap Planned. The accepted one-parent design remains
historical foundation.

### Version-2 manifest and verified closure

Add a strict unordered `parents` carrier, digest v2 including exact nested
manifest state, recursive secure capture, per-edge verification, canonical
cycle diagnostics, same-pin diamond deduplication, divergent-pin rejection,
fixed resource bounds, restricted v2 YAML, portable path limits, hard-link and
mount/reparse rejection, and a union of every read-only physical root. The
inherits version governs the optional matching-version overrides section. V1
parsing, digest vectors, findings, and output remain exact.

### Contextual resolution and override graph

Replace the singular parent identity with a verified source graph. Resolve
authored references in their owning source's visibility context, expose global
source qualification, retain aliases only on declaring edges, allow legal
cross-source equal ids, and compile explicit local-replacement override chains.
Represent a converged bare relationship with its complete sorted historical
candidate set and separate effective terminal. Normalize valid nested-v1 hops
without changing their local rules. Maintain separate catalog/historical and
root-effective/redirected relationship projections.

### Root routing, validation, and enforcement

Route every read command through the central closure. Validate each node in its
own context, collapse inherited errors without importing warnings, evaluate all
effective inherited scope and code constraints against root code, exclude every
materialisation from full/diff enumeration, and keep all writes root-local.

### Closure generations and persistence

Key the logical generation through ADR-148's exact SHA-256 v3 domain, frame-tag
table, ordering, limit block, root/closure bytes, node digests, topology,
override/terminal tables, and five subsystem fingerprints. Introduce store v3,
verify before every reuse, watch every config, manifest, corpus, and
materialisation root, and use full recomposition for graph changes until a
later byte-parity-proven acceleration is accepted.

### MCP and bounded audit

Keep the six-tool surface. Make global qualification, contextual relationships,
complete chain provenance, effective/history graph behavior, and stale-closure
refusal identical across cache modes. Preserve whole chains or return the hard
budget error. Keep audit to fixed returned path/source/layer/pin identity and
record every closure error exactly once with an empty returned set.

### Export and Portal composition

Deduplicate verified diamond records, preserve all override history and
source-aware endpoints, fail conflicting copies, and keep root-local export as
an explicit projection. Portal keys and routes remain `(source, id)` even when
three or more sources share an id. OKF and generated agent rules remain
root-local.

### Evaluation and certification

Extend the existing DecisionGrounding family with several direct parents,
transitive standards, diamonds, sibling alias collisions, equal-id hard
negatives, override chains, and high-inbound weak matches. Certify v1/no-manifest
compatibility, v2 digest vectors, every graph bound, path security, mutation
isolation, YAML structural bombs, overlong paths, hard links, mount/reparse
boundaries, unsupported filesystem identity, cold/warm/no-cache parity, MCP
budgets/audit, exports/Portal, and Linux/macOS/Windows containment.

### Profile guidance

Update explicit `decided init --parent-corpus` guidance to show the v2 manifest
and `decided corpus digest --version 2` without creating a manifest, fetching a
source, or changing ordinary init/profile bytes.

## Constraints

- Materialised, reviewed local bytes only. No clone, fetch, refresh, registry,
  or other network path enters federation.
- One writable invocation root; every inherited physical route is read-only.
- One exact captured closure and one source-contextual resolver feed every
  consumer.
- No manifest order, graph depth, local layer, or source receives precedence or
  ranking boost.
- Every graph node has explicit stable `corpus.source`; physical path and alias
  are not identity.
- Complete provenance survives responses, findings, enforcement, audit
  identity, and exports within their deliberately different bounds.
- Inherited scope and constraints evaluate against root code and cannot be
  bypassed by a local-only mode.
- Fixed graph resource limits fail before overlay and never truncate.
- No-manifest and v1 compatibility are load-bearing release gates.
- The graph ADRs must be accepted before their engine implementation begins.

## Non-Goals

- Absolute, adjacent-checkout, URL, registry, or live-fetched parents.
- Automatic materialisation refresh, pin updates, or cross-corpus writes.
- Manifest-order precedence, child-wins, parent-wins, depth-wins, or source
  ranking preference.
- Enterprise-only behavior, per-artifact ACLs, or a hosted control plane.
- Embeddings, semantic identity resolution, vector indexing, or probabilistic
  source selection.
- A new MCP tool or unbounded audit/provenance records.
- Point-in-time export, change feeds, and section anchors from `corpus-sync`.
- Cross-revision graph materialisation for Watchkeeper in this programme.
- Federated OKF or generated agent-rule output without a separate decision.

## Success Measures

- A root with at least three direct parents and a transitive same-pin diamond
  validates, resolves, retrieves, serves, exports, and enforces fully offline
  with byte-identical output across two clones and root-list permutations.
  Nested-list permutations preserve effective semantics after bottom-up
  repinning while pin/generation provenance changes as authenticated.
- Missing, escaping, cyclic, duplicate, divergent-pin, stale, oversized, and
  invalid-override fixtures fail before overlay with deterministic sourced
  findings.
- An inherited Decision from every tested depth appears in path routing and
  grounding and fails a violating root code change through ordinary Sentry and
  Gate output.
- Equal ids in three sources remain independently qualified and Portal-visible;
  bare use is ambiguous until an explicit complete convergence exists.
- An override chain and reconciled diamond preserve every original,
  intermediate, rationale, mapping owner, and terminal while only the terminal
  affects live ranking and enforcement.
- Diamond artifacts and relationships count once in retrieval and export, while
  every physical copy is verified and protected from writes.
- A changed leaf, nested manifest, edge, pin, or override invalidates resident
  and persistent state; cold, warm, cache-disabled, and rebuilt answers agree.
- The six MCP tools remain within their hard budgets, whole chain provenance is
  never partially emitted, and closure errors produce one empty-return audit
  event.
- Complete no-manifest and v1 CLI, MCP, export, cache, and enforcement goldens
  remain unchanged.

## Assumptions

- The one-parent implementation is retained as v1 compatibility substrate, not
  discarded or silently broadened.
- Submodules and vendored directories inside each declaring repository cover
  the graph materialisation model.
- Source identity and export schemas remain available before graph output is
  published.
- ADR-144 through ADR-148 are explicitly ratified and status transitions are
  recorded before engine work leaves the accepted v1 boundary.

## Risks

- Singular parent assumptions survive in one consumer. Mitigation: activate all
  reads through `VerifiedFederation` and certify cache-on/off plus cold/warm
  parity across the complete surface.
- Contextual aliases are flattened globally. Mitigation: resolution is keyed by
  authoring source and tests add siblings with the same alias spelling.
- A diamond copy or branch exception is silently preferred. Mitigation: verify
  all physical routes, deduplicate only same source+pin, and require explicit
  policy convergence.
- Graph verification creates unbounded work. Mitigation: fixed topology, file,
  logical-byte, physical-byte, and filesystem-entry limits with boundary tests.
- History leaks into live ranking or enforcement. Mitigation: separate catalog
  and effective artifacts and relationship endpoints.
- Pressure to ship causes proposed ADRs to be treated as accepted. Mitigation:
  the ratification gate is explicit and engine PRs stack only after it.

## Related Decisions

- adr-002
- adr-007
- adr-018
- adr-026
- adr-065
- adr-080
- adr-085
- adr-088
- adr-089
- adr-093
- adr-094
- adr-103
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

- corpus-sync
- org-grounding-plane
- lore-at-team-scale
- deterministic-substrate

## Related Requirements

- parent-corpus-inheritance
- federated-resolution-provenance
- export-contract-schemas
- corpus-source-identity
- rac-path-decisions-lookup
- deterministic-decision-code-enforcement

## Related Tickets

- asdecided/core#267
