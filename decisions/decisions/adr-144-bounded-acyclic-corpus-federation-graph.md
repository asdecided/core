---
schema_version: 1
id: RAC-KZN54DB1VNPB
type: decision
---
# ADR-144: Compose Federation as a Bounded Acyclic Source Graph

## Context

ADR-133 deliberately bounded the first corpus-federation implementation to one
direct parent and rejected transitivity. That boundary let the source-aware
identity, read-model, retrieval, enforcement, cache, MCP, and export substrate
be built without pretending that directory concatenation was a graph model.

The intended v0.29 capability is broader. A repository may need independent
security, engineering, regulatory, and product corpora, and those corpora may
share common ancestors. Requiring teams to manufacture one synthetic parent
would hide provenance and move composition policy outside AsDecided. Merely
changing the singular parent field into a list would be worse: it would leave
cycles, diamonds, divergent pins, traversal order, resource exhaustion, and
the write boundary undefined.

## Decision

Manifest version 2 composes a rooted, bounded, directed acyclic graph of corpus
sources.

- The invocation repository is the graph root and the only writable node.
  Every other node is inherited and read-only relative to that root, including
  a node that is local when inspected in its own repository.
- Every node may declare between one and 32 direct parents in its own version-2
  manifest. The engine recursively verifies those already-materialised parents
  before constructing any effective corpus.
- Logical node identity is the explicit `corpus.source`. Every physical edge is
  verified independently. A source reached through several branches is one
  logical node only when every route verifies the same canonical version-2 node
  digest. The engine retains every verified physical root as read-only.
- Reaching the same source with a different canonical node digest is a
  deterministic divergent-pin error. Reaching a source already on the active
  ancestry stack is a cycle error, even when its pin matches. Cycle diagnosis
  takes precedence over divergent-pin diagnosis for an active-stack revisit.
- Two declarations of the same source in one manifest are an error even when
  their pins match. Diamond deduplication applies across independently verified
  ancestry branches, not to redundant sibling declarations.
- Manifest order has no semantic meaning and grants no precedence. Discovery,
  validation, findings, catalog construction, and output use fixed bytewise
  source-aware ordering. Permuting the root manifest's direct-parent list
  therefore produces byte-identical public output. Permuting an inherited
  node's list requires repinning because that node's exact manifest bytes are
  authenticated; after bottom-up repinning it produces the same effective
  identities, relationships, ranking, and findings, while pin and generation
  provenance change by design.
- Nothing is partially overlaid. A cycle, divergent pin, invalid edge, invalid
  node, or exceeded bound blocks the complete root composition.

Version 2 has fixed, versioned safety ceilings. Exactly-at-limit succeeds and
limit-plus-one fails with `corpus-federation-limit-exceeded`, naming the
dimension, limit, and observed value. Limits are not machine-adaptive and the
engine never truncates a closure to fit:

- 1 MiB for each manifest and governing config;
- 64 bytes for an edge alias and 255 bytes for a source identity;
- 4,096 bytes, 64 components, and 255 bytes per component for each `root` or
  `corpus` path;
- 32 YAML levels and 16,384 YAML nodes per manifest or governing config;
- 32 direct-parent declarations per manifest;
- 16 inheritance edges of maximum depth;
- 256 unique inherited source nodes, excluding the root;
- 1,024 declared inheritance edges and 4,096 override declarations;
- 50,000 unique inherited Markdown files, with 16 MiB per file;
- 256 MiB of unique inherited captured bytes;
- 512 MiB of physical verification work across distinct canonical roots; and
- 200,000 visited filesystem entries.

Counts for logical nodes, files, and logical bytes occur after verified diamond
deduplication. Every declared edge counts, and every distinct physical route is
verified and charged to the physical-work budget. Implementations store unique
nodes and edges rather than enumerating every possible diamond route.

The 256 MiB logical-byte count is the sum of each unique inherited node's exact
config bytes, present manifest bytes, and owned Markdown bytes; root bytes are
excluded and override declarations are already counted inside manifest bytes.
The 512 MiB physical-work count charges those same bytes once for each distinct
canonical `(materialisation root, corpus root)` capture, including a route that
later deduplicates or fails its pin. An identical canonical route is captured
and charged once. Edge and override declarations are counted before logical
node deduplication across distinct physical manifests; the root manifest is
counted once.

A visited entry is every directory entry examined below an inherited
materialisation or corpus root during config/manifest discovery, exclusion
discovery, and corpus walking, including ignored, excluded, rejected, and
eventually deduplicated entries. The root-local walk is not charged. Every
counter stops as soon as it reaches `limit + 1` and reports that saturated
value as `observed`; it does not finish an unbounded scan merely to report a
larger total.

This decision supersedes ADR-133 in full. ADR-133 correctly bounded the first
implementation; it no longer governs a version-2 graph composition. ADR-138's
one writable child plus read-only parent model becomes one writable root plus a
read-only unique-source closure. ADR-139's source-neutral ranking and ADR-140's
root-code enforcement apply unchanged across that closure.

This decision also clarifies ADR-135's distinct-source rule. Each logical graph
node has one explicit source distinct from every other logical node. Several
verified physical routes with the same source and canonical node digest are one
node, not several nodes allowed to share an identity; the same source with a
different digest remains an error.

## Consequences

Teams can compose independent policy corpora without manufacturing a synthetic
repository or accepting load-order semantics. Shared ancestors are represented
once in the logical catalog while every physical pin is still checked. Cycles,
split pins, and pathological inputs fail in a bounded and reproducible way.

The engine must replace singular parent and read-only-root concepts with a
verified closure, source-contextual visibility, and a set of materialisation
roots. Verification becomes more expensive because every physical route is
checked before deduplication. That cost is deliberate: deduplication is not a
permission to trust an unchecked copy.

## Status

Accepted

## Category

Architecture

## Supersedes

- adr-133

## Alternatives Considered

### Support several direct parents but continue rejecting transitivity

Rejected. It still needs deterministic collision, ordering, and pin rules while
forcing every shared hierarchy to be flattened outside AsDecided.

### Treat manifest order as parent precedence

Rejected. Reordering reviewed YAML would silently change governance, make
diamonds path-dependent, and contradict the existing ban on implicit source
preference.

### Deduplicate on source without comparing pins

Rejected. Two branches could silently disagree about the bytes represented by
one global identity.

### Leave limits to available machine memory

Rejected. The same corpus could validate on one machine and exhaust another,
and a server could allocate unbounded state before producing a finding.

## Related Decisions

- adr-002
- adr-018
- adr-065
- adr-080
- adr-089
- adr-103
- adr-135
- adr-138
- adr-139
- adr-140

## Related Designs

- corpus-federation-mechanism
- corpus-federation-graph-composition

## Related Requirements

- parent-corpus-inheritance
- federated-resolution-provenance
- corpus-source-identity

## Related Roadmaps

- corpus-federation
