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
- `corpus`: `name`, `rac_version`, and `artifact_count`
- `artifacts[]`: `id`, `aliases`, `type`, `status`, `title`, `path`, and
  `body_html`
- `relationships[]`: `from`, `to`, and the flattened `relates-to` `type`

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

## Compatibility rule

All schema objects allow unknown additional properties. This is intentional:
an additive producer release must remain readable by an existing consumer.
Consumers should ignore fields they do not understand.

Every field emitted today is nevertheless declared and required. Removing a
required field, changing its type incompatibly, or changing its meaning is a
breaking contract change and requires a `schema_version` bump plus a new
versioned schema file. Adding a field requires updating the current schema and
its producer drift test in the same change.
