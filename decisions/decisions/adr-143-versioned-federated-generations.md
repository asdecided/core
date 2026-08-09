---
schema_version: 1
id: RAC-KZKMJAF599TB
type: decision
---
# ADR-143: Version Federated Generations, Cache State, and Freshness

## Context

The native engine reuses derived and persistent state across reads. A cached
single-corpus index has no source or layer identity, and a generation keyed only
to child files cannot detect a changed manifest, parent config, parent pin, or
override. Reinterpreting old segments as federated state could serve an answer
whose parent no longer matches the child's declaration.

Federation needs an explicit compatibility and invalidation contract rather
than relying on incidental cache misses.

## Decision

Federation changes the logical generation identity and persistent-store layout.

- A composed generation includes the existing child corpus and governing-config
  inputs, the exact `.decided/corpus.md` bytes, parent source identity, verified
  parent digest, parent governing-config input, and override mapping.
- Freshness observes both materialised corpus roots, both governing config
  files, and the child manifest.
- A generation is never served after the materialised parent stops matching its
  declared digest. Reverification precedes reuse after a relevant change.
- Source and layer fields require a new explicit persistent-store layout
  version. Segments from the old layout are cache misses and are never decoded
  as federated entries.
- Parent recency is not borrowed from child Git history. An inherited result
  carries its verified pin; recency is absent unless it can be derived from the
  parent materialisation itself.
- A repository without `.decided/corpus.md` follows the contemporaneous
  single-corpus generation path and retains byte-identical released behavior
  after the source-identity and export-schema prerequisites.

## Consequences

Changing any input that can alter the composed truth invalidates the generation,
and old path-only indexes cannot leak unattributed results into federation.
Freshness remains deterministic over local materialised bytes.

The first run after the store-layout change rebuilds the index, and federated
repositories watch more paths and hash more inputs. Those costs are accepted to
prevent stale inherited governance from being served as current.

## Status

Accepted

## Category

Technical

## Alternatives Considered

### Reuse the existing store layout and infer missing source as local

Rejected. An inherited entry could be misidentified as local, and path
collisions would make old segments ambiguous.

### Key only on the recorded parent digest

Rejected. Manifest aliases, overrides, child config, and source declarations can
change the effective corpus without changing parent artifact bytes.

### Verify the parent only at process startup

Rejected. Long-lived MCP and watch processes could continue serving a stale
generation after the materialisation changes.

### Borrow child Git recency for inherited artifacts

Rejected. A submodule or vendored update date is not the artifact history of the
parent corpus and would misstate provenance.

## Related Decisions

- adr-089
- adr-103
- adr-104
- adr-105
- adr-112
- adr-118
- adr-119
- adr-135
- adr-138

## Related Designs

- corpus-federation-mechanism

## Related Requirements

- rac-federated-resolution-provenance
- rac-parent-corpus-inheritance
