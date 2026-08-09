# Export contracts

AsDecided publishes Draft 2020-12 JSON Schemas for every JSON export
projection. The schemas describe the minimum v1 contract a consumer can rely
on, and the CLI prints the packaged files without reading a corpus or using the
network:

```sh
decided export --schema viewer
decided export --schema documents
decided export --schema graph
```

The emitted bytes are exactly the packaged resources:

- [viewer export v1](https://github.com/asdecided/core/blob/main/rust/rac-engine/assets/schemas/export-viewer-v1.schema.json)
- [documents record v1](https://github.com/asdecided/core/blob/main/rust/rac-engine/assets/schemas/export-documents-v1.schema.json)
- [graph export v1](https://github.com/asdecided/core/blob/main/rust/rac-engine/assets/schemas/export-graph-v1.schema.json)

Those files are the machine-readable source of truth. CI validates exports of
both a fixed fixture corpus and AsDecided's own decision corpus against them. A
separate field-set guard also requires the producer and schema to name exactly
the same current fields.

## Viewer object

The default `decided export` projection is one JSON object containing:

- `schema_version`
- `corpus`: `name`, `source`, `rac_version`, and `artifact_count`
- `artifacts[]`: `id`, `aliases`, `type`, `status`, `title`, `path`, and
  `body_html`
- `relationships[]`: `from`, `to`, and the flattened `relates-to` `type`

`rac_version` is a retained v1 machine key. It carries the version of the
AsDecided CLI that produced the payload; it is not a current product or command
name.

The viewer schema is reconciled with the existing Portal input contract. The
committed demonstration payload's additive `corpus.sample` field remains valid,
although Core does not emit that field.

## Documents records

`decided export --documents` is JSON Lines, not one enclosing JSON array. Each
non-empty line is independently validated against the documents schema and
contains:

- `schema_version`, `id`, `type`, `status`, `title`, and Markdown `text`
- `metadata`: `path`, `aliases`, `tags`, and `source`

The schema describes one line. A consumer should split the UTF-8 stream on line
boundaries and validate each record separately.

## Graph object

`decided export --graph` is one JSON object containing `schema_version`,
`source`, `nodes`, and `edges`. Nodes carry `id`, `type`, `status`, and `title`.
Edges carry `source`, `target`, `type`, `directed`, `resolved`, `external`, and
nullable `provider` provenance.

The graph edge `type` is the engine's real relationship kind. It is not the
viewer projection's flattened `relates-to` value.

## Corpus source identity

Every JSON projection uses one shared source derivation. Configure the stable
identity in the nearest governing `.decided/config.yaml`:

```yaml
repository_key: APP
corpus:
  source: acme/payments-service
```

`corpus.source` is returned byte-for-byte after validation. It must be a
lower-case, slash-namespaced value whose segments use letters, digits, `.`,
`_`, or `-`, for example `acme/payments-service`. Treat it as durable
provenance, not a display name. Moving a checkout does not change it; changing
the configured value is an identity migration.

For a non-federated export, AsDecided derives the source in this order:

1. explicit `corpus.source`;
2. the lower-case `repository_key`;
3. the existing corpus-directory basename when neither value is configured.

The viewer exposes the value as `corpus.source`. Documents records expose it
as `metadata.source`; the graph exposes it as its top-level `source`. A graph
edge's own `source` field remains the source *node ID* and is not corpus
provenance. `corpus.name` remains the existing display value and is not an
identity.

The repository key continues to namespace newly generated artifact IDs. It is
not globally unique, and different corpora may legitimately use the same key.
Federation therefore requires explicit, distinct `corpus.source` values and
never relies on either fallback.

## Aggregating corpora

Consumers aggregate documents streams by concatenating their records and
keying each artifact on `(metadata.source, id)`. They aggregate graph exports
by lifting the graph's top-level source onto every node: a node key is
`(graph.source, node.id)`, and each edge endpoint is resolved in that same
namespace before the node and edge sets are unioned. The viewer's
`corpus.source` provides the equivalent namespace for its artifacts.

Configure distinct explicit sources whenever repository-key or basename
fallbacks could collide. Source identity alone does not make cross-corpus
references resolvable and does not add inheritance, cross-corpus validation,
or precedence rules.

Once federated exports carry verified-parent pin provenance, the same inherited
record arriving through several children may be deduplicated only when source,
canonical ID, record body, and verified pin all agree. A different body or pin
for the same `(source, id)` is an aggregation conflict, never a
last-writer-wins update.

### Migration from basename sources

Before this contract, documents and graph exports stamped the corpus-directory
basename. An initialised repository without explicit `corpus.source` now uses
its lower-case repository key instead. Consumers indexed by the old basename
must migrate that namespace or configure the intended durable source before
ingesting the new export.

A repository with neither `corpus.source` nor `repository_key` retains the
released basename source value. Documents and graph output therefore keep
their previous source-bearing bytes in that fallback case; the viewer gains
only the additive `corpus.source` field required by the current schema.

## Compatibility rule

All schema objects allow unknown additional properties. This is intentional:
an additive producer release must remain readable by an existing consumer.
Consumers should ignore fields they do not understand.

Every field emitted today is nevertheless declared and required. Removing a
required field, changing its type incompatibly, or changing its meaning is a
breaking contract change and requires a `schema_version` bump plus a new
versioned schema file. Adding a field requires updating the current schema and
its producer drift test in the same change.
