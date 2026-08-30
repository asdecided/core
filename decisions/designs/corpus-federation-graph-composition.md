---
schema_version: 1
id: RAC-KZN54DB6S476
type: design
---
# Design: Corpus Federation Graph Composition

## Status

Proposed

This design extends the accepted one-parent foundation in
`corpus-federation-mechanism` without rewriting its history. ADR-144 through
ADR-148 are separately reviewable proposals. Until they are explicitly
accepted, ADR-133 through ADR-143 remain the implementation boundary and this
design does not authorize engine changes.

## Context

The first federation increment establishes the hard substrate: stable
`corpus.source` identity, source-aware artifact and path keys, verified offline
materialisation, one composed read model, explicit overrides, source-neutral
ranking, inherited root-code enforcement, bounded MCP provenance, composed
exports, and versioned serving state. Its accepted topology is deliberately one
direct leaf parent.

The broader use case needs independent corpora for concerns such as security,
engineering, regulation, and product policy. Those corpora may inherit shared
standards or each other. Flattening them into one synthetic repository hides
ownership and policy lineage. Concatenating several directory walks creates
order-dependent resolution, loses alias scope, double-counts diamonds, and
cannot authenticate transitive topology because digest v1 does not include the
parent manifest.

The graph extension must remain AsDecided: deterministic, offline, Git-native,
reviewable, source-attributable, and focused on supplying decisions to coding
agents. It is not a generic knowledge graph, hosted control plane, registry,
network fetcher, vector platform, or probabilistic retrieval system.

## User Need

A root repository needs to inherit several independently owned corpora, follow
their pinned dependencies, cite any visible artifact, apply every effective
Decision to root code, and retain the complete history of explicit exceptions.
The same checked-out bytes must produce the same answer on every machine.

An operator must be able to understand and update the graph using ordinary Git
diffs. A missing, stale, conflicting, cyclic, or oversized closure must fail
before any read or enforcement surface sees a partial result.

## Design

The graph is an explicit version-2 semantic mode built on the accepted
source-aware substrate. The following sections define its compatibility,
manifest, pin, topology, resolution, override, serving, and certification
contracts as one coherent design.

## Compatibility Modes

Federation behavior is selected by the operational manifest version.

| Mode | Topology | Qualification | Collisions and overrides | Serving compatibility |
| --- | --- | --- | --- | --- |
| No manifest | One local corpus | Existing forms | Existing rules | Byte-identical single-corpus path |
| Manifest v1 | One direct leaf parent | Child-local `alias::canonical-id` | Cross-layer canonical collision; no chains | Existing v1 findings, digest, output, and pin provenance |
| Manifest v2 | Bounded acyclic source graph | Global `source::canonical-id`, plus scoped direct aliases | Cross-source equal ids are legal but unqualified-ambiguous; explicit chains may converge | Closure-wide generation and store v3 |

V2 is an explicit semantic mode. It does not reinterpret a v1 root. A v2 edge
may target a corpus whose own manifest is absent, v1, or v2. A nested v1 node
keeps its one-parent/leaf-only relationship, collision, alias, and override
rules in its own context. Its validated effective projection and any valid v1
override hop are then normalized to source-aware keys for the enclosing v2
graph; a v2 ancestor may explicitly extend that hop.

## Graph Model

The runtime owns one immutable `VerifiedFederation`:

```text
FederationRoot  = (source, config, manifest, local snapshot, writable locator)
FederationNode  = (source, canonical v2 digest, config, manifest, local snapshot)
FederationEdge  = (owner source, alias, target source, root, corpus, declared pin)
ArtifactKey     = (source, canonical_id)
ArtifactPath    = (source, relative_path)
PhysicalRoute  = (declaring root, canonical materialisation root, corpus root)
```

The root is the only writable node. Every non-root record is `inherited`
relative to the invocation, even when it is local in its own repository. Stable
public identity never contains a checkout path, materialisation route, or edge
alias.

The logical graph is keyed by explicit `corpus.source`. Every physical edge is
verified before logical deduplication:

- same source and same canonical v2 node digest: one logical node;
- same source and different canonical digest: divergent-pin error;
- source repeated on the active ancestry stack: cycle error;
- same direct source twice in one manifest: duplicate-parent error.

All verified physical roots remain in the read-only set, including duplicate
routes to a deduplicated diamond node. Local corpus discovery excludes that
entire set, and every mutation/output guard rejects it.

## Manifest Version 2

Inheritance remains in `.decided/corpus.md`, outside artifact discovery,
search, relationships, and exports. Headings are exact lowercase Markdown.

````markdown
# Corpus

## inherits

```yaml
version: 2
parents:
  - alias: standards
    source: acme/standards
    root: vendor/standards
    corpus: decisions
    digest: sha256-v2:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
  - alias: security
    source: acme/security
    root: vendor/security
    corpus: decisions
    digest: sha256-v2:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210
```
````

The sequence is unordered and has one to 32 strict records. Unknown or duplicate
keys, YAML merge keys, duplicate aliases, duplicate direct sources, malformed
source identities, and out-of-bound values fail. The parser accepts no dormant
future fields.

The `## inherits` mapping selects the whole manifest's semantic version. The
optional `## overrides` mapping must have the same version; a mixed-version pair
or overrides without valid inheritance fails. In v2, operational mappings and
every governing config are restricted to mappings, sequences, and scalars with
at most 32 levels and 16,384 nodes. Anchors, aliases, custom tags, and merge
keys are rejected before construction. V1 and no-manifest parsing remain exact.

Edge paths are POSIX-relative UTF-8. Absolute, empty, `.`, `..`, backslash,
drive-prefix, UNC, symlink, and canonical-escape forms are rejected. A root is
relative to and strictly inside the repository that declares it. Therefore
every transitive route remains inside the invocation repository. Overlapping
sibling roots are rejected unless they are the same verified logical route;
legitimate nesting follows a declared ancestry edge.

Each `root` or `corpus` value is bounded to 4,096 UTF-8 bytes, 64 components,
and 255 UTF-8 bytes per component. Materialisation and corpus roots are real
directories; configs, present manifests, and Markdown artifacts are real
regular files. Symlinks, Windows reparse points, files with more than one hard
link, and mount/volume/junction crossings are rejected. If the platform cannot
provide the required file identity, link, volume/mount, and reparse metadata,
v2 fails closed as an unsupported filesystem.

No AsDecided command materialises or refreshes a source. Submodules and vendored
directories are equivalent local byte carriers. The operator updates them with
Git and explicitly updates pins.

## Digest Version 2

V2 authenticates a node's source, governing config, exact manifest state, and
owned local Markdown snapshot:

```text
domain = "asdecided-corpus-digest-v2\0"
frame(0x01, source UTF-8)
frame(0x02, exact .decided/config.yaml bytes)
frame(0x03, 0x00 for absent corpus.md or 0x01 for present corpus.md)
frame(0x04, exact .decided/corpus.md bytes)  # present only
for file in owned Markdown sorted by relative UTF-8 path bytes:
    frame(0x05, corpus-relative POSIX path)
    frame(0x06, exact file bytes)
```

Every frame is one tag byte, an unsigned 64-bit big-endian byte length, and the
exact payload. Owned Markdown excludes `.decided/` and every declared direct
materialisation subtree. Nothing is normalized. The digest excludes checkout
path, timestamps, traversal order, YAML meaning, and Markdown meaning.

The exact manifest bytes bind the outgoing edges and their pins. Those pins bind
descendant manifests in turn, producing a Merkle-like commitment without a
self-referential closure hash. Adding, removing, reordering, or commenting a
manifest requires a repin even though parent order does not change semantic
precedence.

The existing v1 domain, framing, `sha256:` prefix, known vector, and operator
command remain exact. V2 uses:

```text
decided corpus digest --version 2 --root <root> --corpus <corpus>
```

and emits `sha256-v2:<hex>`.

## Secure Discovery and Verification

The loader follows this fail-closed sequence:

1. Capture and bound the root config and manifest.
2. Strictly parse direct declarations only far enough to establish lexical
   local-walk exclusions.
3. Sort outgoing edges by `(source, digest, alias, root, corpus)`.
4. For each target, resolve containment without following an unverified nested
   declaration; capture its bounded config, manifest state, and owned local
   Markdown bytes.
5. Calculate and compare the incoming edge pin.
6. After the target pin succeeds, detect an active-stack source repeat as a
   cycle; otherwise compare a completed source's canonical node digest for
   diamond deduplication or divergent-pin failure.
7. For every previously unseen canonical physical capture, traverse and verify
   the child paths named by that physical copy's manifest even when its logical
   source and digest will deduplicate. An identical canonical physical capture
   may reuse its already verified route result.
8. After all distinct physical routes pass, deduplicate logical nodes and
   validate them bottom-up with source-lexicographic topological
   tie-breaking.
9. Compile visibility, relationships, overrides, catalog, and effective views.
10. Publish the generation only after the entire closure passes.

Unverified manifest bytes never direct recursive filesystem traversal. Digest,
parse, validation, composition, persistence, and serving consume the captured
buffers. Secure no-follow opens are preferred; portable pre/open/post identity
checks reject swaps or shape changes during capture.

The implementation stores unique nodes and edges, not every diamond route.
Every graph topology or inherited-node blocker carries one lexicographically
minimal source route plus `route_count`, the exact number of verified physical
routes represented by that finding. `route_count` is at least one; other
finding families omit it.

## Qualification and Visibility

Global qualification is stable:

```text
acme/standards::STD-KWJ4VMKVSS65
```

The left side is an exact visible `corpus.source`; the right side must be a
canonical id. It selects the historical record in that source and never follows
an override.

A direct alias such as `standards::STD-...` is a convenience scoped to the
manifest that declares it. Alias lookup is keyed by `(referrer source, alias)`.
Intermediate aliases do not leak to root callers; root public lookup uses the
root alias table. Persisted keys, export links, findings, and provenance use the
global source, never the alias.

References resolve in the view rooted at their authoring source. Root-authored
references see the complete root closure. Source A's immutable references see A
and A's declared closure, not an unrelated sibling introduced by an ancestor.
The catalog endpoint stores the authored token plus a sorted non-empty set of
historical candidate `ArtifactKey`s. Qualified or unique tokens have one
candidate. A bare token with several candidates may resolve only when all have
one terminal; the catalog keeps every original candidate and the effective
projection stores that separate terminal. No arbitrary historical winner is
invented. A later ancestor override can change only the effective terminal.

Equal canonical ids in different sources are legal. An unqualified reference
resolves only if its visible effective candidates reduce to one key. Otherwise
the reference is ambiguous and must be source-qualified or explicitly
converged. Duplicate ids within one source remain errors. No source, layer,
depth, or manifest position wins implicitly.

## Override Graph

V2 overrides are explicit and globally targeted:

````markdown
## overrides

```yaml
version: 2
items:
  - target: acme/standards::STD-KWJ4VMKVSS65
    with: APP-KWJ9ABCD1234
    rationale: APP-KWJ9D3C1S10N
```
````

`target` must be inherited and visible from the declaring corpus. `with` must
be a same-type artifact local to that corpus. `rationale` must be a live local
Decision. All operands are canonical, a target appears once per manifest, and
mapping order has no meaning. The section version must equal the version under
`## inherits`.

Each node first unions its direct parents' effective projections, then applies
its own mappings. A branch-local exception affects that branch only. All mapping
edges are retained with their owner and rationale. Chains across ancestry levels
are allowed; same-manifest indirect chains, inherited replacements, type
changes, and cycles fail.

A shared ancestor may have divergent terminals in separate branches. The join
fails with `corpus-federation-override-divergence` unless the joining corpus
explicitly maps every live branch terminal to one local same-type replacement.
Only a unique terminal enters the root effective view.

A valid nested-v1 override remains governed by v1 syntax and validation, then
becomes one source-aware hop in the v2 graph. Complete mapping provenance uses
the closure's parents-before-child, source-lexicographic Kahn owner rank and
then `(owner source, target key, replacement key, rationale key)` byte order.

## Catalog and Effective Projections

The composed model has explicit projections rather than one overloaded vector:

- **Catalog:** every unique artifact and historical relationship endpoint,
  including authored tokens, complete original candidate sets, overridden
  history, and every rationale.
- **Effective:** root-local artifacts plus inherited terminal artifacts, with
  relationship endpoints redirected through valid override chains.
- **Root local:** writable artifacts only, used by every mutation command and
  root-owned review.

Search, BM25 statistics, inbound graph counts, scope routing, grounding, Gate,
and Sentry use the effective projection. Qualified historical lookup and graph
inspection can select catalog history without leaking its edges into live
ranking. Viewer, documents, and graph exports include the unique catalog by
default; `--local-only` selects the root-local projection.

Catalog order is `(source, relative_path, canonical_id)`. Ranking remains one
source-neutral deterministic BM25 and relationship index with no local quota or
source boost, the lexical graph-floor invariant, and `(source, relative_path)`
as the final tie order. Diamond rows and edges count once. Override
nonterminals contribute neither search rows nor live inbound popularity.

## Validation and Finding Ownership

Each unique node's local structural state is validated under its own config.
Its authored relationships resolve in that node's visibility context. Parent
`Applies To` syntax is validated there, while target existence and matching for
effective inherited Decisions are evaluated against the final root code tree.

A node error blocks root composition through one deterministically selected,
sourced root-manifest finding; a diamond does not duplicate the same node error
per route. Parent warnings and review advisories remain owned by that source and
are not replayed in the root. Errors created only by combining branches—cycles,
divergent pins, ambiguities used by root artifacts, or override divergence—are
root composition findings.

A failed descendant edge is attributed to the manifest source that declares it,
and the root blocker includes the source route. Remediation may tell the user
where to validate or repin but never instructs an AsDecided command to edit an
inherited path.

## Routing, Enforcement, and Mutation

Every effective inherited Decision participates in `decisions-for`, grounding,
MCP path lookup, Gate, Sentry, and `gate --code`. `Applies To` patterns and code
constraints match the invocation root's code, at any inheritance depth. A
terminal replacement governs; an override nonterminal does not.

Sentry and full/diff code enumeration exclude every physical materialisation
and corpus root. Mutation commands receive only root-local items and reject all
direct, transitive, and duplicate-route roots, including nonexistent output
suffixes, symlinked ancestors, and lexical `..` forms. No enforcement or MCP
surface gains a local-only bypass.

Review and advisory diagnostics remain root-subject views after central closure
verification, so inherited warnings are not multiplied. Explicitly physical
historical diagnostics may remain local when they cannot materialise a graph at
another revision; they are never enforcement paths.

## MCP, Audit, and Exports

The six MCP tools remain unchanged. Every request uses one request-current
verified generation for resolution, search, relationships, scope, summary, and
content. Cache-on, cache-off, resident, and store-hit behavior is identical.

Artifact responses carry source and inherited/root layer; inherited records
also carry their owning-node pin, while root-local records omit `pin`. Complete
override-chain provenance is indivisible under hard budgets: optional content
and result-list entries reduce first, then the tool returns
`response_budget_exceeded` rather than an incomplete chain. Audit stays bounded
to returned path, source, layer, and optional inherited pin; it does not record
bodies, excerpts, topology, aliases, or mappings. Any closure preparation error
produces one audited error event with an empty returned list.

Viewer, documents, and graph exports emit each diamond record once when source,
id, pin, path, and body agree. Any mismatch is an aggregation conflict. Graph
endpoints are source-aware. Every historical and terminal artifact and every
override hop remains attributable. Portal keys and routes use `(source, id)` so
three or more records may share a canonical id without collapsing. OKF and
generated agent rules remain root-local unless separately decided.

## Generation, Store, and Freshness

The closure generation is the exact SHA-256 `sha256-v3:` framed-byte stream in
ADR-148: domain, root snapshot, sorted node snapshots, sorted edges, the totally
ordered mapping table, compiled terminal redirects, fixed limits, recursive
mode, stable root-relative corpus path, and the five closed subsystem
fingerprints. It excludes physical checkout paths.

Graph persistence uses `store/v3`; older stores are misses. Before any resident
or persistent reuse, the engine captures and verifies the full closure and
confirms the model's sources, pins, identity rows, terminals, and relationship
projections. Cache flags control persistence, not verification.

Freshness observes every root, config, manifest path including absence, and
captured artifact. Create, content/identity/type change, remove, rename, watcher
overflow, or lost coverage performs a full candidate recomposition in the
initial graph implementation. Failure makes the previous model inaccessible;
stale fallback is prohibited. Inherited Git recency is absent unless derived
from that source's own materialisation history.

## Deterministic Bounds

| Dimension | Version-2 limit |
| --- | ---: |
| Manifest or config | 1 MiB each |
| Alias / source identity | 64 / 255 bytes |
| Root/corpus path | 4,096 bytes, 64 components, 255 bytes/component |
| YAML depth / nodes | 32 / 16,384 |
| Direct parents per manifest | 32 |
| Inheritance depth | 16 edges |
| Unique inherited sources | 256 |
| Declared edges | 1,024 |
| Override declarations | 4,096 |
| Unique inherited Markdown files | 50,000 |
| Individual inherited Markdown file | 16 MiB |
| Unique inherited captured bytes | 256 MiB |
| Physical verification bytes | 512 MiB |
| Visited filesystem entries | 200,000 |

The exact limit succeeds; plus one fails without a partial overlay. Logical
bytes are unique inherited config, present-manifest, and owned-Markdown bytes
after verified diamond deduplication. Physical bytes charge those inputs once
per distinct canonical `(materialisation root, corpus root)` capture. Override
and edge declarations count across distinct physical manifests before logical
deduplication. Visited entries include ignored, excluded, rejected, and later
deduplicated entries examined below inherited roots. Counters stop and report
the saturated observed value `limit + 1`; every edge counts and every distinct
physical route is charged.

## Acceptance Strategy

The implementation is complete only when the following families are covered:

- no-manifest byte goldens and the complete v1 suite remain unchanged;
- v2 one-parent parity, two and three direct parents, byte-identical root-list
  permutations, and semantically identical nested permutations after repinning;
- deep transitivity, same-pin diamonds, divergent pins, self/two-node/long
  cycles, and root-source recurrence;
- scoped aliases with repeated spellings, global transitive qualification,
  canonical-only suffixes, equal ids across three sources, and unqualified
  ambiguity;
- `A -> B -> C` overrides, same-id chains, divergent forks, explicit diamond
  reconciliation, wrong types, nonlocal replacements, dead rationales, and
  full provenance;
- original versus effective relationship endpoints, diamond graph counts,
  source-neutral ranking, and large-parent hard negatives;
- inherited scope and enforcement from every depth against root code;
- all mutation commands and output targets against every materialisation route;
- v2 digest known vectors for manifest absence/presence, CRLF, comments,
  ordering, config, path, and artifact byte changes;
- cold, warm, persistent, cache-disabled, and event-rebuilt byte parity, plus
  old-store misses and stale-leaf refusal;
- six MCP tools, tight budgets, one error audit event, full-catalog exports,
  root-only local exports, and source-aware Portal navigation; and
- Linux, macOS, and Windows containment and stable-order fixtures.

Security fixtures also cover YAML depth/node/alias bombs, path component and
length boundaries, hard-link aliases, bind/mount or volume crossings, Windows
reparse points, and unsupported file-identity capabilities.

Every deterministic bound has an at-limit and limit-plus-one fixture. A
many-diamond fixture proves work scales with nodes, edges, and verified physical
bytes rather than the number of possible routes.

## Constraints

- Materialised local bytes only; no network operation in any engine path.
- One writable root; inherited bytes are never mutated.
- One verified closure and one contextual source-aware resolver for every
  consumer.
- No source, local-layer, graph-depth, or manifest-order precedence.
- Provenance survives every response, finding, audit identity, and export.
- Root enforcement cannot be bypassed with a local-only projection.
- No-manifest and v1 observable compatibility are load-bearing.

## Non-Goals

- Adjacent-checkout, absolute-path, URL, registry, or live-fetched parents.
- Automatic materialisation refresh or pin writing.
- Writes into any inherited source.
- Hosted federation, per-artifact ACLs, or a control plane.
- Embedding-based resolution, vector search, probabilistic source selection, or
  source-biased ranking.
- A new MCP tool or an unbounded audit record.
- Cross-revision graph materialisation for Watchkeeper in this increment.
- Federation of OKF or generated agent-rule write projections without a
  separate decision.

## Rationale

A verified source DAG is the smallest model that preserves independent corpus
ownership, shared ancestry, and explicit exception history without importing
load order. Exact manifest-bound pins make the topology reviewable in Git, and
source-contextual resolution prevents an unrelated sibling from changing an
immutable parent's authored meaning. Separate catalog and effective projections
preserve audit history without letting it govern live agent work.

## Alternatives

- **Publish one synthetic combined parent.** Rejected because it hides source
  ownership, duplicates upstream policy, and moves conflict handling outside
  the decision corpus.
- **Allow several direct parents but reject transitivity.** Rejected because it
  still needs collision and order semantics while forcing shared hierarchies to
  be flattened.
- **Use manifest order as precedence.** Rejected because a YAML reorder would
  silently change governance.
- **Resolve through a hosted graph service.** Rejected because federation must
  remain offline, materialised, Git-native, and available to every user.

## Accessibility

Human output must name source identities, pins, and source routes in text rather
than relying on colour, indentation, or graph visualization alone. JSON and
SARIF retain equivalent structured identity. Cycle and divergent-pin findings
show one bounded canonical route and a route count so they remain usable in
terminals, screen readers, and CI annotations without dumping every diamond
path.

## Style Guidance

Examples use exact lowercase manifest headings, lower-case slash-namespaced
sources, POSIX-relative paths, and complete pins. Documentation calls aliases
local conveniences and `corpus.source` durable identity. It describes catalog
history separately from the effective governing view and never presents
materialisation refresh as an AsDecided network operation.

## Open Questions

None within this proposal. Changes to topology limits, precedence, network
materialisation, inherited writes, qualification, or override ownership require
a new or superseding decision rather than an implementation default.

## Ratification Gate

The graph has no remaining product-default question in this proposal. Engine
work begins only after a human explicitly accepts ADR-144 through ADR-148. At
that point ADR-133, ADR-136, and ADR-143 are marked Superseded atomically; the
accepted amendments to ADR-134, ADR-137, ADR-138, ADR-141, and ADR-142 are
recorded without erasing their retained rules; ADR-135 receives the
same-source/same-digest physical-route clarification from ADR-144. The graph
design moves to Accepted. The accepted `corpus-source-identity` requirement is
amended in the same transition to reference the new authority set, require one
explicit source per unique logical node, and permit only same-digest physical
diamond reuse. The two implementation requirements remain Proposed until their
acceptance evidence is complete, and the roadmap remains Planned.

## Related Decisions

- adr-002
- adr-018
- adr-026
- adr-065
- adr-080
- adr-089
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

## Related Requirements

- parent-corpus-inheritance
- federated-resolution-provenance
- corpus-source-identity
- deterministic-decision-code-enforcement

## Related Roadmaps

- corpus-federation
- corpus-sync
