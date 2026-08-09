---
schema_version: 1
id: RAC-KWJ8RTRK4JWM
type: roadmap
---
# Corpus Federation Programme

## Status

Planned

ADR-089 accepts federation in principle and fixes five non-negotiable
constraints. The refreshed `corpus-federation-mechanism` design now resolves the
first-increment questions against the released Rust engine: one direct parent,
one fixed Markdown manifest, verified materialised bytes, explicit source
identity and overrides, one source-aware read model, and inherited code
enforcement. ADR-133 through ADR-143 separate those choices for individual
human ratification. They are now Accepted and govern the engine work.
Execution order and task state live in the GitHub epic named in
`## Related Tickets` under ADR-093.

## Context

The released engine is single-corpus. `relationships::corpus_items` and its
downstream validation, resolution, index, graph, cache, freshness, enforcement,
and MCP models identify artifacts by an unqualified path or id. Org-wide reach
exists today through ADR-117's two-endpoint topology, but the child cannot
validate a reference to the org corpus or apply an org Decision's code
constraints to child code.

The original federation proposal named retired Python seams and left seven
mechanism choices open. The design refresh closes those questions without
turning AsDecided into a live cross-repository service: the parent is one
read-only layer already materialised inside the child repository, and every
answer remains a deterministic function of reviewed bytes.

Two additive `corpus-sync` capabilities precede published federation output:
machine-checkable export schemas (`export-contract-schemas`) and one stable
source identity (`corpus-source-identity`). The remaining point-in-time,
delta, and section-anchor work does not block federation.

## Outcomes

- A child repository declares and verifies one pinned parent in
  `.decided/corpus.md`, fully offline.
- Child artifacts cite parent artifacts with ordinary or qualified
  relationships, and collisions never acquire implicit precedence.
- Applicable inherited Decisions appear in grounding and path lookup and apply
  their deterministic code constraints to child code.
- Local and inherited artifacts share one deterministic read model while every
  result, finding, audit record, and export remains attributable to its source.
- A valid local override records the replacement and its live Decision
  rationale without deleting or rewriting parent history.
- The same pinned parent deduplicates across N child exports on `(source, id)`;
  differing pins surface as a conflict.
- Adding federation leaves repositories without a federation manifest
  byte-identical to the contemporaneous single-corpus path.

## Initiatives

### Mechanism design (`corpus-federation-mechanism`)

The design synthesis resolves the manifest home, one-parent cardinality,
transitivity rejection, parent finding ownership, export opt-out, repository-key
interaction, and MCP budget behaviour. It also records the current Rust seams,
source-aware identity, code-scope semantics, enforcement, cache invalidation,
and write boundary that the original proposal omitted.

### Federation decision set

ADR-133 through ADR-143 were independently human-ratified on PR #451. Together
they decide the topology, parent declaration and verification, source identity,
qualified resolution, explicit overrides, unified read model, ranking,
child-code enforcement, bounded MCP provenance, export composition, and
versioned generation/cache contract. Engine implementation may now begin
within their combined boundary; ADR-141 explicitly amends ADR-127 for the MCP
provenance change.

### Source and export prerequisites

Land the export schemas before adding source-aware fields, then land
`corpus.source` as the one identity used by exports and federation. Pull these
two initiatives from `corpus-sync`; do not pull point-in-time export, change
feeds, or section anchors into the federation critical path.

### Resolver, validation, routing, and enforcement

Load one verified parent through a central source-aware layer set before
deriving validation, resolution, search, graph, scope, cache, freshness, and MCP
models. Support `alias::canonical-id`, explicit manifest overrides, sourced
collision findings, inherited `decisions-for` and grounding, and inherited
Sentry/`gate --code` evaluation against the child code tree. Mutation commands
receive only the local layer.

### Profile unhollowing (ADR-088)

Once the mechanism ships, expose the reserved parent declaration guidance only
when explicitly requested. Unconfigured profile and init output remain
byte-identical.

### Composition with corpus-sync

Viewer, documents, and graph exports stamp parent and child records with their
own `corpus.source`, using the published schemas and global `(source, id)` key.
Inherited records are the default projection; `--local-only` is a diagnostic
and export view, never an MCP or enforcement bypass.

## Constraints

- The five ADR-089 non-negotiables govern the programme: available to all;
  deterministic and offline; one writable child truth plus read-only inherited
  parent; Git-native human-readable declaration; provenance end to end.
- Exactly one direct parent in the first increment. Multiple and transitive
  inheritance fail loudly.
- The parent is materialised inside the child repository, with canonical path
  containment and a verified full digest before overlay.
- One source-aware read model feeds every consumer. No command, MCP tool, or
  enforcement surface merges directories independently.
- No source boost or implicit precedence enters resolution or ranking; the
  v0.28 lexical graph-floor invariant remains in force.
- Inherited code scope is evaluated against the child code tree. Enforcement
  cannot be disabled with `--local-only`.
- Parent writes and live network fetches are prohibited.
- Output changes are additive, except an internal persistent-store layout bump;
  old caches degrade to misses.

## Non-Goals

- Multiple parents, parent DAGs, or transitive inheritance.
- Absolute, adjacent-checkout, URL, registry, or live-fetched parents.
- Cross-corpus writes or automatic parent refresh and pin updates.
- Enterprise-only behaviour, per-artifact ACLs, or a hosted control plane.
- Embeddings, semantic resolution, a vector index, or source-biased ranking.
- Point-in-time export, incremental feeds, and section anchors; those remain in
  `corpus-sync`.
- A new MCP tool; any additive wording must remain within the measured standing
  MCP surface budget.

## Success Measures

- A child with one vendored or submodule-backed parent validates, resolves,
  retrieves, serves, and enforces fully offline with byte-identical output
  across two clones.
- Missing, escaping, source-mismatched, transitive, and stale parent fixtures
  fail before overlay with distinct stable findings.
- A child relationship to a parent Decision resolves; source and pin survive
  CLI, MCP, audit, finding, and export serialization without copying artifact
  content into the audit record.
- An inherited Decision scoped to a child path appears in `decisions-for` and
  fails a violating child code change through the ordinary Sentry engine.
- Collision and override fixtures prove there is no resolution-order
  precedence and that parent history remains qualified-addressable.
- A large-parent DecisionGrounding fixture includes hard negatives and proves
  the v0.28 lexical floor still protects the stronger match under the existing
  response budget.
- The parent tree is byte-unchanged after every command, old index segments
  rebuild as cache misses, and federation leaves contemporaneous no-manifest
  goldens byte-identical.

## Assumptions

- One materialised parent covers the initial organisation-standards use case.
- ADR-117's shared-endpoint topology remains the unmerged reach path and can
  coexist during migration.
- Export schemas and source identity land before federation publishes
  source-aware records.
- Implementation remains within the accepted federation ADR set; any change to
  those boundaries requires a new or superseding decision.

## Risks

- Source-aware identity is added to one surface but omitted from another.
  Mitigation: the central layer set feeds all derivations, and acceptance spans
  CLI, MCP, enforcement, cache, audit, and export.
- A large parent changes ranking or exhausts the response budget. Mitigation:
  one combined deterministic ranking, no source boost, the graph-floor gate,
  hard budgets, and a federation evaluation fixture.
- Pin verification or watcher invalidation is weakened for performance.
  Mitigation: verification precedes overlay and the pin is part of generation
  and cache identity.
- Overrides become undocumented local exceptions. Mitigation: every override
  requires a local same-type replacement and a live local Decision rationale,
  both validated and exported as provenance.
- The first increment expands into arbitrary federation. Mitigation: multiple,
  transitive, external, and live parents are explicit Non-Goals requiring new
  decisions.

## Related Decisions

- adr-002
- adr-005
- adr-007
- adr-016
- adr-018
- adr-026
- adr-033
- adr-055
- adr-065
- adr-066
- adr-080
- adr-085
- adr-088
- adr-089
- adr-093
- adr-094
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
