---
schema_version: 1
id: RAC-KZN54DB55X9R
type: decision
---
# ADR-148: Key Serving State to the Entire Federated Closure

## Context

ADR-143 correctly requires source-aware store state, re-verification before
reuse, fail-closed freshness, and no borrowed child Git recency. Its generation
formula and watcher topology enumerate one child, one parent, two configs, two
roots, and one manifest. Repeating that singular structure for several parents
would miss transitive manifests, duplicate-route verification, graph edges,
override chains, and topology changes.

A server must never keep exposing the last valid subset after one leaf changes
or becomes unverifiable. Cold, resident, persistent-store, cache-disabled, and
event-driven paths need one closure capture and one composition contract.

## Decision

Version-2 federation introduces an immutable `VerifiedFederation` closure that
owns the exact bytes and verified topology used by every derived model. It
contains:

- the root source, governing config, exact manifest, and root-local snapshot;
- every unique node sorted by `(source, canonical version-2 node digest)`;
- every declared edge sorted by owner source, target source, digest, alias,
  root, and corpus;
- every canonical physical materialisation and corpus root, including all
  verified routes to a deduplicated diamond node;
- exact config, manifest-presence, manifest, and owned-artifact byte buffers;
- source visibility and edge-local alias tables;
- parsed override declarations and the compiled terminal/chain graph; and
- the fixed graph-contract version and ADR-144 limit values.

Digest verification, parsing, node validation, relationship resolution,
composition, indexing, persistence, and serving consume those captured bytes.
They do not reopen inherited public paths. No-follow opens are used where
available; portable pre/open/post file-identity checks fail with a stable
snapshot-changed error when bytes or filesystem shape change during capture.

The canonical logical generation is SHA-256 over the raw domain literal
`asdecided-federation-generation-v3\0` followed by frames. A frame is one tag
byte, an unsigned 64-bit big-endian payload length, and the exact payload. Its
canonical text form is `sha256-v3:` plus 64 lowercase hexadecimal characters.
Strings are exact UTF-8; stable paths are POSIX-relative UTF-8. Tables and
snapshots use the following closed byte contract:

```text
frame(0x01, "corpus-federation-graph/v2")
frame(0x02, exact newline-delimited limit block defined below)
frame(0x03, one byte: 0x00 non-recursive or 0x01 recursive)
frame(0x04, root source)
frame(0x05, stable root-relative corpus path)
frame(0x06, exact root config bytes)
frame(0x07, one byte root-manifest presence)
frame(0x08, exact root manifest bytes)                 # only when present
for each root-owned file in relative-path byte order:
    frame(0x09, relative path); frame(0x0a, exact bytes)
for each inherited node in (source, canonical v2 digest) order:
    frame(0x10, source); frame(0x11, canonical sha256-v2 text)
    frame(0x12, exact config); frame(0x13, one-byte manifest presence)
    frame(0x14, exact manifest)                        # only when present
    for each owned file in relative-path byte order:
        frame(0x15, relative path); frame(0x16, exact bytes)
    frame(0x17, empty)
for each edge in (owner source, target source, declared pin, alias, root, corpus):
    frame(0x20, owner); frame(0x21, target); frame(0x22, declared pin text)
    frame(0x23, alias); frame(0x24, root); frame(0x25, corpus)
    frame(0x26, empty)
for each mapping edge in ADR-147's total order:
    frame(0x30, owner source)
    frame(0x31, target source); frame(0x32, target canonical id)
    frame(0x33, replacement source); frame(0x34, replacement canonical id)
    frame(0x35, rationale source); frame(0x36, rationale canonical id)
    frame(0x37, empty)
for each terminal redirect in (target source, target canonical id) order:
    frame(0x38, target source); frame(0x39, target canonical id)
    frame(0x3a, terminal source); frame(0x3b, terminal canonical id)
    frame(0x3c, empty)
frame(0x40, "artifact-spec-registry/v1")
frame(0x41, "relationship-description-registry/v1")
frame(0x42, "tokenizer-ranking-graph-floor/v1")
frame(0x43, "federation-derived/v3")
frame(0x44, "store/v3")
```

The exact 0x02 payload is ASCII, in the shown order, with a final newline:

```text
manifest-bytes=1048576
config-bytes=1048576
alias-bytes=64
source-bytes=255
path-bytes=4096
path-components=64
path-component-bytes=255
yaml-depth=32
yaml-nodes=16384
direct-parents=32
depth=16
unique-inherited-sources=256
edges=1024
overrides=4096
inherited-files=50000
file-bytes=16777216
logical-bytes=268435456
physical-bytes=536870912
visited-entries=200000
```

The mapping table is the normalized override graph; the terminal table commits
its compiled result. Aliases are already committed inside sorted edge rows.
Checkout paths, canonical materialisation spellings, event order, and
timestamps are excluded. The five literal subsystem fingerprints are the
closed answer-affecting constant set for the first graph implementation. A
change to artifact specifications, relationship descriptions,
tokenization/ranking including graph-floor or tie rules, derived schemas, or
store layout must bump its corresponding literal before state can be reused.

Because exact manifests are inputs, reordering even the root parent list
changes the internal generation. It does not change public corpus output: all
semantic tables are independently sorted. Reordering an inherited manifest
also changes that node's pin and requires bottom-up repinning, so pin/generation
provenance changes even though the effective non-provenance answer remains the
same.

Persistent federation state uses a new explicit `store/v3` layout and a new
derived-schema generation. V1 and V2 store segments are cache misses and are
never decoded as graph answers. Persisted keys and endpoints use stable
`ArtifactKey` and `ArtifactPath`; absolute locators remain runtime-only.

Before every resident or store reuse, the engine verifies and captures the
complete closure, computes its generation, and validates that the stored model
contains exactly the expected sources, layers, inherited pins, root pin
absence, identities, redirect terminals, and relationship projections. A cache
option controls persistence, not federation verification or semantics.

Freshness observes the root corpus/config/manifest and every node corpus root,
config, manifest path (including currently absent manifests), captured
artifact, and physical materialisation root. Create, content/identity/type
change, remove, and rename events on those inputs, plus watcher overflow or lost
coverage, trigger a full candidate recapture and recomposition in the first
graph implementation; source-blind delta mutation is not used across the
closure. If any edge, node, pin, bound, or override fails, the prior model
becomes inaccessible and the request fails closed. It is never served as stale
fallback.

All consumers receive the same request-current closure generation:
validation, relationships, resolution, retrieval, scope routing, Gate, Sentry,
the six MCP tools, audit extraction, exports, and diagnostics do not assemble
independent parent lists. A configured graph has no cache-off, MCP, enforcement,
or export code path that silently degrades to root-local reads.

Inherited records carry exactly stable source, inherited layer, and the verified
canonical pin for their owning node as fixed origin identity. Override
provenance carries the owner, target, replacement, rationale, and state fields
defined by ADR-147; ordinary artifact results need not enumerate ancestry
routes. Topology findings carry the canonical route contract from the graph
design. Recency is absent unless it can be derived from that source's own
materialisation history; root Git history is never borrowed. Override-chain
provenance remains atomic under response budgets, while audit remains bounded
to path, source, layer, and pin.

No-manifest repositories retain the contemporaneous single-corpus path and
byte-identical output. A version-1 manifest retains its accepted observable
behavior and pin provenance; an internal store-layout miss is not an output
change.

This decision supersedes ADR-143. It preserves versioned layouts, fail-closed
freshness, verified reuse, no-manifest parity, and the ban on borrowed root
recency, but replaces the two-root generation formula with a canonical
commitment to the complete verified closure.

## Consequences

A leaf change, topology edit, divergent pin, or newly invalid override cannot
leave one consumer or one cache tier serving a different corpus from another.
Two clones of the same graph share stable generation and output identities even
though their physical paths differ.

Every request-current verification can be more expensive, and graph changes
initially force full recomposition rather than incremental deltas. Version-2
federation also causes one deliberate store rebuild. These costs buy a clear
atomicity boundary; later acceleration must preserve this generation contract.

## Status

Proposed

## Category

Technical

## Supersedes

- adr-143

## Alternatives Considered

### Hash only the root manifest and direct pins

Rejected. It omits captured descendant state and cannot validate that a stored
model represents the same closure.

### Keep one cache generation per parent and merge on read

Rejected. Consumers could combine generations captured at different times and
recreate independent overlay semantics.

### Keep serving the last valid graph after a leaf fails verification

Rejected. The answer would claim current governance from bytes that no longer
match the reviewed pins.

### Incrementally patch any changed graph node from the first release

Rejected. Full closure recomposition is the safe initial boundary. A later
acceleration may be accepted only with cold/warm/delta byte parity.

## Related Decisions

- adr-080
- adr-103
- adr-104
- adr-105
- adr-112
- adr-118
- adr-119
- adr-128
- adr-135
- adr-138
- adr-141
- adr-143
- adr-144
- adr-145
- adr-147

## Related Designs

- corpus-federation-mechanism
- corpus-federation-graph-composition

## Related Requirements

- parent-corpus-inheritance
- federated-resolution-provenance

## Related Roadmaps

- corpus-federation
