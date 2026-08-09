---
schema_version: 1
id: RAC-KWJ8RWSW356V
type: design
---
# Design: Corpus Federation Mechanism

## Status

Accepted

The post-v0.28 design pass resolves the mechanism questions left by ADR-089 and
acts as a synthesis for ADR-133 through ADR-143. Those decisions were separately
human-ratified on PR #451; together they govern and authorize the first useful
federation increment. This design does not authorize extensions outside their
combined boundary.

## Context

ADR-089 accepts corpus federation in principle under five non-negotiable
constraints: the capability is available to everyone; resolution is
deterministic and offline over materialised bytes; the child repository remains
its own canonical state with a read-only parent and explicit overrides; the
declaration is Git-native and human-readable; and provenance is preserved end
to end.

The original proposal named Python seams that no longer own the product. The
released engine is Rust. Its single-corpus boundary starts at
`relationships::corpus_items`, whose `CorpusItem` carries only a path, parsed
artifact, and specification. `ValidationRow`, `resolve::IndexEntry`,
`Relationship`, `ResolvedArtifact`, `DerivedIndex`, the persistent index store,
the freshness tracker, and the MCP `GraphView` all inherit that path-only
identity. Commands and MCP tools call the same corpus reader, but several call
it directly. Combining directory walks without changing that identity model
would create path collisions, lose source attribution in caches and responses,
and let serving and enforcement disagree about the effective corpus.

Federation therefore begins by making source and layer explicit in the one
read model every consumer uses. It does not add a second resolver, a network
fetcher, or a general knowledge-graph platform.

## User Need

An organisation keeps firm-wide standards in one repository and needs a child
repository to cite and obey them as ordinary AsDecided artifacts. A coding
agent working in the child must receive the applicable local and inherited
decisions in one grounding response. Validation must resolve cross-corpus
references, and inherited mechanical constraints must gate the child code.
Every answer and finding must show where the governing artifact came from.

The same capability must work for a solo developer sharing decisions across
two repositories. It is not an enterprise-only mode.

## Design

### First increment: one direct parent

A child may inherit exactly one direct parent. Multiple parents and a parent
that itself declares inheritance are rejected with stable findings before any
overlay is built. The first increment therefore has two layers:

1. the local, writable child corpus; and
2. one verified, inherited, read-only parent corpus.

This is the smallest shape that provides real organisation-wide governance
without prematurely deciding graph ordering across an arbitrary parent DAG.

### Stable source identity

Every corpus participating in federation declares a stable source identity in
the nearest `.decided/config.yaml`:

```yaml
repository_key: APP
corpus:
  source: acme/payments-service
```

`corpus.source` is the identity used in provenance and composite keys. It is
independent of checkout location, directory spelling, and the short
`repository_key`; the parent and child source identities must differ. The
source-identity derivation defined by `corpus-source-identity` uses this
value first and retains repository-key and basename fallbacks only for
non-federated, backward-compatible exports. Federation requires the explicit
value on both layers.

The child gives its parent a local, readable alias such as `standards`. The
alias is used in authored qualified references; it is not the parent's global
identity.

### Fixed Markdown manifest

Inheritance is declared at the fixed repository path
`.decided/corpus.md`. It is an operational Markdown manifest, not an artifact:
it is not part of the corpus walk, search index, relationship graph, or export.
Released engines already exclude `.decided/` from ordinary corpus discovery,
so the declaration is inert before federation support exists.

The first increment accepts exactly one mapping in a fenced YAML block under
`## inherits`:

````markdown
# Corpus

## inherits

```yaml
version: 1
alias: standards
source: acme/standards
root: vendor/standards
corpus: decisions
digest: sha256:<64 lowercase hexadecimal characters>
```
````

`root` is a repository-relative materialisation root. `corpus` is the corpus
directory relative to that root. Absolute paths, `..` components, symlinks in
the path to the parent config or any discovered artifact, and canonical paths
outside the child repository are rejected. A Git submodule and a vendored
directory are both valid ways to materialise the root; the resolver treats
both as bytes already present on disk.

### Materialisation and pin verification

The engine never clones, pulls, updates, or otherwise contacts the declared
source. Refreshing a submodule or vendored directory is an explicit user Git
operation outside AsDecided.

Before loading the parent, the engine verifies:

- the materialisation and corpus directory exist;
- the parent config declares the source named by the manifest;
- the parent does not declare its own inherited parent; and
- the canonical versioned corpus digest equals the full digest in the
  manifest.

The digest is a versioned SHA-256 fold over a domain separator, the parent
source identity, the governing parent `.decided/config.yaml` bytes, and the
sorted corpus-relative paths and bytes of every discovered parent Markdown
file. It does not include checkout location or timestamps. The implementation
exposes the same pure digest calculation for operators; validation remains the
authoritative verification path. Missing bytes, a source mismatch, an
unsupported transitive declaration, and a stale digest are distinct stable
error findings. No parent artifact enters the effective corpus until all four
checks pass.

### One source-aware read model

The engine introduces source-aware equivalents of its path-only concepts:

```text
CorpusLayer  = (source, alias, root, corpus_root, digest, access)
ArtifactKey  = (source, canonical_id)
ArtifactPath = (source, relative_path)
```

`access` is local-writable or inherited-read-only. The verified layer set is
loaded once and feeds the existing validation, resolution, search, graph,
scope, export, cache, freshness, and MCP derivations. Consumers do not merge
their own walks. Write commands receive only the local layer and cannot name an
inherited path as a mutation target. The local walk excludes the declared
parent materialisation subtree even when the requested child corpus would
otherwise contain it, so no Markdown file can enter both layers.

For a repository without `.decided/corpus.md`, the loader produces the current
single local layer. Measured against the engine after the export-schema and
source-identity prerequisites but before federation, all human and machine
outputs remain byte-identical.

### Resolution and qualified references

Canonical identity is composite: `(source, artifact id)`. Existing unqualified
references continue to resolve when exactly one artifact across both layers
matches. A reference can name a source alias explicitly with
`alias::artifact-id`, for example:

```text
standards::STD-KWJ4VMKVSS65
```

The qualified form resolves within that source and requires a canonical
artifact id; aliases are not accepted after `::`. A canonical-id collision
between parent and child is a deterministic cross-corpus finding. Ordinary
legacy or title aliases may exist in both layers, but an unqualified reference
to a colliding alias is ambiguous and must be changed to the qualified form.
No source receives implicit resolution precedence.

Relationships retain source-aware endpoints. Cross-corpus cycles and type
checks use the same registry and algorithms as local relationships over
`ArtifactKey`, rather than a federation-specific resolver.

### Explicit overrides

An inherited artifact is overridden only through an entry under
`## overrides` in the same manifest:

````markdown
## overrides

```yaml
version: 1
items:
  - parent: standards::STD-KWJ4VMKVSS65
    with: APP-KWJ9ABCD1234
    rationale: APP-KWJ9DECISION01
```
````

`parent` must resolve to the parent canonical id, `with` must resolve to one
local artifact of the same type, and `rationale` must resolve to a live local
decision. The mapping records an intentional replacement, clears any related
collision finding, and makes the local artifact effective for live retrieval
and enforcement. In the effective unqualified view, the overridden parent's
canonical id resolves to the local replacement; callers use the qualified
parent id to retrieve the original. The parent remains addressable through its
qualified id and is exported with an overridden state plus the mapping
provenance; it is never silently deleted or rewritten.

An absent, ambiguous, cross-type, retired-rationale, or parent-to-parent
mapping is a validation error. There is no implicit child-wins or parent-wins
rule.

### Validation semantics

The parent is validated as a source corpus before overlay. A structural or
relationship error in the parent yields one `parent-corpus-invalid` error at
the child manifest and prevents the overlay; the finding names the source and
the command for validating the parent directly. Parent warnings and review
advisories remain owned by the parent and are not repeated in every child.

Findings caused by the composition itself — missing parent, stale digest,
source mismatch, collision, bad override, or a child relationship to a parent
— are child findings and carry both source identities where applicable. No
remediation instructs a command to edit inherited bytes.

Filesystem-scoped `## Applies To` entries on inherited decisions are evaluated
against the child repository's code tree. Their syntax is validated in the
parent; target existence and matching are evaluated in the child composition.
They are never interpreted relative to the parent materialisation.

### Retrieval, MCP, and response budgets

Local and inherited artifacts share one deterministic lexical and relationship
index. BM25 statistics are calculated over the effective combined corpus. No
source boost or local-first quota is introduced. Exact score ties use
`(source, relative path)` as the stable final order. The v0.28 lexical floor
continues to prevent graph popularity in a large standards corpus from
displacing a materially stronger lexical match.

The six-tool MCP surface remains unchanged. Existing id arguments accept the
qualified form. Results gain additive source, layer, and pin provenance; that
provenance is never removed to satisfy a character budget. Existing hard
response budgets and truncation rules still apply to artifact content. No MCP
tool is added. Any additive description or response-schema wording must remain
within the existing measured standing-surface budget.

MCP audit records extend ADR-127's bounded returned-identity object only with
the fixed source, layer, and pin fields needed to identify an inherited result.
They do not copy artifact bodies, excerpts, override mappings, or the full
response provenance. Because ADR-127 pins the current path-only shape, ADR-141
must amend that decision explicitly before these fields ship.

Default reads use the effective combined corpus. Human-facing diagnostic and
export commands may request `--local-only` to inspect the child layer. MCP and
enforcement do not expose a local-only bypass: an agent connected to a
federated repository and `decided gate --code` both receive inherited
governance.

### Code scope and enforcement

Inherited live decisions participate in `decisions-for`,
`retrieve_grounding`, the MCP `find_decisions` path lookup, `gate`, and
`gate --code` through the existing shared scope and Sentry evaluators. Their
declared paths match the child code tree. Code constraints never inspect or
modify files under the parent materialisation.

An explicit override changes the effective decision set before scope lookup
and enforcement. A command-line flag cannot suppress inherited constraints.
This keeps the full Record -> Route -> Enforce loop intact across corpora;
federation is not only a search feature.

### Cache and freshness

The logical generation identity includes the existing child corpus and
governing-config inputs, manifest bytes, parent source identity, verified
parent digest, and override mapping. Any change to those inputs invalidates
the derived read model and persistent store. Source and layer fields require a
new internal store-layout version; old cache segments degrade to a miss and
are never reinterpreted as federated answers.

The freshness tracker observes both materialised corpus roots, both governing
config files, and the child manifest. It never serves a generation whose parent
no longer matches the declared digest. Git-derived recency is not borrowed from
the child for inherited artifacts: inherited results carry their pin
provenance, while recency remains absent unless it can be derived from the
parent materialisation itself.

### Export behaviour

Viewer, documents, and graph exports include the inherited layer by default
and stamp every record with its own `corpus.source`. Global consumer identity
is `(source, id)`. A shared parent therefore deduplicates across exports from N
children at the same verified pin without changing its source identity. The
same `(source, id)` arriving with a different pin or record body is an explicit
aggregation conflict, never a last-writer-wins duplicate. `--local-only` emits
the child projection only.

The export schemas land before source-aware fields, under
`export-contract-schemas`; source derivation lands under
`corpus-source-identity`. Federation reuses both contracts and introduces
no second export identity mechanism.

## Constraints

- The five ADR-089 non-negotiables apply in full.
- One direct parent only in the first increment; multiple and transitive
  inheritance fail loudly rather than being partially interpreted.
- Parent bytes are materialised inside the child repository and verified
  before use; there is no network access in validate, resolve, retrieve,
  enforce, export, or serve paths.
- One source-aware read model feeds every consumer. There is no second
  resolver, search index, scope matcher, or enforcement path.
- The parent layer is read-only. Rename, scaffold, migration, and generated
  agent-rule writes are confined to the child.
- Provenance survives resolution, findings, retrieval, MCP, enforcement,
  audit, and export.
- Federation adds no output or behaviour change to repositories without a
  manifest; parity is measured against the same engine with its source-identity
  prerequisite applied.
- The capability is available to every user and introduces no enterprise
  operating mode.

## Rationale

One direct, digest-pinned parent is enough to deliver the real need: a firm
standard can be cited, routed, and enforced in a child repository. Requiring
materialisation inside the child makes a clone self-describing and keeps path
containment, cache invalidation, and offline verification tractable. A stable
source identity separates provenance from repository-key conventions, while
qualified references solve ambiguity without hidden precedence.

Centralising the layer union before all derived models is the architectural
requirement. Merging independently in validation, MCP, and Sentry would create
three subtly different effective corpora. Explicit source-aware keys make the
composition visible and testable at the same seams the Rust engine already
uses.

## Alternatives

- **Complete arbitrary multi-parent federation first.** Rejected: ordering,
  transitive pins, cycle handling, and override precedence multiply the risk
  before one parent has been proven useful.
- **Fetch a parent URL at command or server startup.** Rejected: it makes
  answers depend on network state and violates ADR-002 and ADR-089.
- **Permit adjacent or absolute parent paths.** Rejected for the first
  increment: they are not clone-stable and expand the containment and trust
  boundary. A future decision may add a verified external materialisation.
- **Use `repository_key` as global source identity.** Rejected: it is a short
  artifact-id namespace and can legitimately repeat across organisations.
- **Implicit child-wins precedence.** Rejected: it hides a governance change
  in resolver order and loses the reason for the exception.
- **Serve the parent as a second MCP endpoint permanently.** Retained as the
  ADR-117 pre-federation topology, but it cannot validate cross-corpus edges or
  enforce parent decisions against child code.
- **Add a federation-specific resolver or vector index.** Rejected: one
  source-aware deterministic read model is sufficient and preserves the
  product boundary.

## Accessibility

Source and layer are rendered as text fields, not colour or position alone.
Human findings name the readable alias and stable source identity. Qualified
references remain copyable plain text. A user can inspect the manifest,
materialised parent, pin, and override rationale with a text editor and Git;
no visual explorer is required to understand why an inherited artifact
applies.

## Style Guidance

- Manifest headings are exactly `## inherits` and `## overrides`.
- Authored source aliases and qualified references are lowercase-alias plus
  `::` plus canonical id.
- Digests are full lowercase SHA-256 values; never abbreviate them.
- Human output says `local` or `inherited` and names the source. Do not use
  colour alone or the vague term `remote`.
- Error codes use the `parent-corpus-*` or `cross-corpus-*` families and stay
  stable once released.

## Open Questions

No first-increment design question remains open. The seven original questions
are resolved as follows:

- **Manifest home** -> `.decided/corpus.md`, an operational Markdown manifest
  outside the artifact walk.
- **Multiple parents** -> disallowed in the first increment.
- **Transitive inheritance** -> rejected with a stable finding in the first
  increment.
- **Parent findings** -> parent errors block overlay through one sourced child
  finding; parent warnings and advisories remain parent-owned.
- **Export opt-out** -> inherited by default; `--local-only` is available for
  diagnostic reads and exports, never as an enforcement or MCP bypass.
- **Repository-key interaction** -> `corpus.source` is the composite identity;
  repository keys may repeat, while collisions and unqualified ambiguities are
  handled explicitly.
- **MCP budget behaviour** -> one combined deterministic ranking, no source
  boost or new tool, additive provenance retained under the existing hard
  budget.

ADR-133 through ADR-143 record the independent choices behind this synthesis.
Each is Accepted following separate human ratification on PR #451. Support for
multiple or external parents requires a later decision and evidence from the
one-parent implementation.

## Related Requirements

- parent-corpus-inheritance
- federated-resolution-provenance
- export-contract-schemas
- corpus-source-identity
- rac-path-decisions-lookup
- deterministic-decision-code-enforcement

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

## Related Roadmaps

- corpus-federation
- corpus-sync
- org-grounding-plane
