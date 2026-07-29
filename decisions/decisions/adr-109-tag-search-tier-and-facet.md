---
schema_version: 1
id: RAC-KWY7886GSEE5
type: decision
---
# ADR-109: Tag Search Tier and Tag Facet

## Context

Frontmatter `tags` are a validated field (a list of non-empty strings) that
authors already write, but they are invisible to retrieval: the search field
set is `{id, title, path, heading, body}` only, tags never enter the token
vectors or the persisted index, and there is no way to filter by tag. The
first benchmark run put candidate discovery under scrutiny, and this is a
concrete discovery-quality gap — a curated topical label the engine ignores.

ADR-038 established the precedent and the boundary: a lexical body tier was
added by decision, and "no embeddings, semantic scoring, stemming, or synonym
expansion in Core — ever." A tag tier sits squarely inside that
deterministic-lexical envelope. The tokens the store persists are a
byte-parity contract (ADR-104), so making tags searchable is also a store
format change, not only a tokenizer change.

## Decision

`tags` become a searchable lexical field and a filter facet.

- **Tier.** `tags` is a metadata match tier at rank 2, between title and path:
  a curated tag outranks an incidental path token but not the artifact's own
  title. Like id/title/path it carries no snippet (ADR-038). Tag strings are
  tokenised by the same ADR-037 rule as every field — a multi-word tag
  `data-model` yields `data`/`model` — so a query term matches a tag uniformly,
  with no special matcher.
- **Ranking.** The tags field gets a BM25F boost of 2.5 (above path's 2.0,
  below title's 3.0), the graded contribution that is the real ranking lever
  (ADR-078). The boost value, not the field's position in the field list, sets
  the rank; `tags` is appended last in the field list so the existing five
  fields keep their exact float-summation order and an untagged artifact scores
  byte-identically.
- **Facet.** A `--tag` flag on `rac find` (repeatable) and a `tags` argument on
  the `search_artifacts` tool constrain the matched set to artifacts carrying
  every requested tag. The facet matches the **raw whole tag, casefolded,
  exactly** — `--tag data-model` matches only that tag, never the token
  `model` — a deliberately different mechanism from the tokenised tier. AND
  semantics across repeated tags, like the query's term AND. The facet is a
  pre-scoring constraint applied alongside the type filter, so corpus-wide BM25
  statistics (IDF, mean field length) stay corpus-global. `rac find` and
  `search_artifacts` serve it identically (ADR-031).
- **Persistence and format.** The tags field vector is persisted like every
  other field; the raw tag strings are persisted in the `entries.seg` identity
  block so the facet can match whole tags reconstructed from the store. This
  bumps the segment format version (3→4) and the cache bundle version ("2"→"3");
  an older store fails the version gates closed and is rebuilt. The store's
  scoring fingerprint already changes with the new boost, a third independent
  guard. Index-served output stays byte-identical to a fresh walk-and-parse
  build, worker-count invariant, re-proven across the bump.
- **JSON contract.** Tags are surfaced additively on a *search result*, emitted
  only when non-empty, so an untagged hit is byte-identical to the pre-tags
  shape (ADR-007). The identity-only `rac index` manifest is unchanged.
- **No semantics.** No embeddings, semantic scoring, stemming, or synonym
  expansion — the tier is a deterministic lexical extension (ADR-038, ADR-066).

## Consequences

### Positive

- The curated topical signal authors already record becomes findable and
  filterable, improving candidate discovery without widening it semantically.
- One mechanism per need: tokenised matching for the tier, whole-tag exact
  matching for the facet — each explainable and deterministic.

### Negative

- A persisted-store format bump: the byte-parity gate must be re-proven across
  the new field, and older stores rebuild once. Bounded — a rebuild is a
  latency cost, never an answer change (ADR-104).
- The tier renumber shifts the `evidence.tier` integer for path/heading/body
  matches by one in `--explain`/tool output; the fused result *order* is
  unchanged (BM25F-driven). The evidence-bearing fixtures are regenerated
  deliberately.

## Status

Accepted

## Category

Technical

## Alternatives Considered

### A tag facet only, no tier

A filter without a tier would let a user narrow by tag but never *rank* on one,
so a query whose only signal is a tag would miss the artifact entirely.
Rejected: tags are a genuine relevance signal, not only a filter axis.

### Match the facet on tokenised tags

Filtering on tokenised tags would make `--tag model` match `data-model`,
conflating the facet with the tier and surprising a user who asked for a
specific label. Rejected: the facet matches whole tags exactly; the tier does
the tokenised matching.

### Embedding or semantic tag expansion

Rejected on ADR-038 and ADR-066: no embeddings or semantic scoring in Core. The
tier is lexical only.

## Relationship to Other Decisions

- ADR-037 (RAC-KTXTAF6ZKDK8): tags tokenise by the same token-boundary rule.
- ADR-038 (RAC-KTXTAG63E89H): extends the lexical tier ladder; re-affirms no
  embeddings/semantic scoring.
- ADR-078 (RAC-KVSQ24G2H2D6): the tags BM25F boost is the graded ranking lever.
- ADR-104 (RAC-KWS7QCT10Q5A): the persisted store the tags field and format bump
  extend; the byte-parity gate holds.
- ADR-007 (RAC-KTQ63DPYKJF4): tags are additive in search results; the manifest
  contract is unchanged.
- ADR-031 (RAC-KTW0M81B0GBB): CLI and MCP serve the facet identically.
- ADR-066 (RAC-KV6KFCC8MHTM): the deterministic, embedding-free retrieval line.

## Related Roadmaps

- candidate-discovery

## Code Constraints

```yaml
version: 1
eligibility: eligible
reason: "The indexed tag field, scoring tier, and store-format gate are explicit native constants."
rules:
  - id: index-store-keeps-tags-field
    kind: require_pattern
    path_glob: "rust/rac-engine/src/index_store.rs"
    pattern: 'pub const FIELDS: \[&str; 6\] = \["id", "title", "path", "heading", "body", "tags"\];'
    message: "Tags must remain a persisted and searchable index field."
  - id: tag-tier-keeps-format-version-gate
    kind: require_pattern
    path_glob: "rust/rac-engine/src/index_format.rs"
    pattern: 'pub const SEGMENT_FORMAT_VERSION: u16 = 4;'
    message: "The tag-aware store format must remain explicitly version-gated unless the decision is amended."
```
