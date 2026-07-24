---
schema_version: 1
id: RAC-KVSQ2CV0AM13
type: roadmap
---
# Note-Tool Ingest Sources

## Status

Achieved

Delivered across all four initiatives (itsthelore/asdecided-core#227). `rac ingest`
now imports note-tool exports by normalisation through the ADR-072 registry
(ADR-079): Obsidian vaults (#292), Logseq graphs (#297), Notion "Markdown & CSV"
exports (#298), and Roam JSON graphs (#303). Each note becomes a reviewable
RAC-shaped draft; the source's wikilinks / page links become **candidate**
`## Related` references a human promotes, never asserted edges (ADR-074, ADR-065);
conversion is deterministic and offline, lossless by default, and never
overwrites an existing artifact (ADR-002). The four tool converters share one
resolver and draft-assembly path so a format's drift cannot break another, and a
committed golden fixture pins byte-identical drafts. The engine core and the
markitdown binary-document path are unchanged. The design's Open Questions
closed as recorded on `note-tool-ingest-sources` (forward-only candidates; Notion
CSVs skipped as a redundant index; detection auto plus `--from`).

## Context

`rac ingest` converts rich documents through markitdown (ADR-072) — DOCX, PDF,
PPTX, XLSX, HTML, Markdown — but stops at office documents. A large share of the
product knowledge teams already hold lives in note / PKM tools (Obsidian, Logseq,
Notion, Roam), whose exports are already Markdown, organised as a *graph* of
notes with wikilink syntax. Today those teams cannot bring that knowledge into
Lore without rewriting it, and the link graph they maintain — the relationship
signal RAC values most — is lost on the way in.

This release opens that door. ADR-079 adds note-tool exports as a normalising
converter family in the ADR-072 registry: each note becomes a RAC-shaped draft,
wikilinks become candidate `## Related` references (promoted by a human, never
auto-asserted — ADR-074, ADR-065), and conversion stays deterministic, offline,
and lossless (ADR-002). The *how* is in the `note-tool-ingest-sources` design.

## Outcomes

- A team can run `rac ingest` over an Obsidian, Logseq, Notion, or Roam export and
  get a set of reviewable RAC drafts — adoption becomes an import, not a rewrite.
- The source's wikilink graph is carried in as candidate relationships, so
  connectivity is offered for promotion rather than flattened to plain text.
- Conversion is deterministic and lossless: identical export, identical drafts,
  nothing silently dropped, no existing artifact overwritten.

## Initiatives

### Initiative 1 — Note-tool converter family (registry)

Add a converter family behind optional extras (`ingest-obsidian`,
`ingest-logseq`, `ingest-notion`, `ingest-roam`) registered in the ADR-072
converter registry, selected by detected export shape or an explicit `--from`
flag. The engine core and the markitdown path for binary documents are unchanged.

### Initiative 2 — Directory ingest (a vault is a set)

Teach `rac ingest` to accept a directory export and process each note as a
candidate artifact draft, never overwriting an existing artifact — the unit of
import is the graph, not a single file.

### Initiative 3 — Wikilink and frontmatter normalisation

Parse and resolve wikilink syntax (`[[Note]]`, `[[Note|alias]]`, heading/block
refs) against the export, emit resolved targets as candidate `## Related`
references and report ambiguous ones, map known frontmatter to RAC's shape, and
preserve everything unmapped verbatim (lossless by default).

### Initiative 4 — Determinism, losslessness, and docs

Golden tests pin byte-identical drafts for a fixed export and prove no content is
dropped; `docs/cli.md` documents the supported note tools, the `--from` flag, and
the candidate-link review step.

## Constraints

- Deterministic and offline (ADR-002): identical export yields byte-identical
  drafts; no model or network; never overwrites an existing artifact.
- Lossless by default: unmapped content is preserved verbatim.
- Registry, not core (ADR-072): note-tool sources are registered extras; the core
  and the markitdown binary-document path are untouched.
- Candidates, not asserted edges (ADR-074, ADR-065): wikilinks become suggested
  references a human promotes.

## Non-Goals

- Running PKM exports through markitdown (there is no binary to parse).
- Auto-creating, writing, or asserting any `## Related` edge from a wikilink.
- A single universal importer across all tools; each tool is its own converter.
- Live, API-based sync with a hosted tool — this is export ingestion, offline.

## Success Measures

- An Obsidian vault and a Notion export each convert to RAC drafts that pass
  `rac validate` after human review, with no content silently dropped.
- Wikilinks in the source appear as candidate `## Related` references in the
  drafts.
- Re-running ingest on the same export produces byte-identical drafts.

## Assumptions

- PKM exports are predominantly Markdown with predictable link and frontmatter
  conventions, so deterministic normalisation is feasible without a model.
- Teams hold real product knowledge in these tools and adopt Lore faster when
  importing it is a command rather than a rewrite.

## Risks

- **Export format drift** across tool versions. Mitigation: per-tool converters
  with their own tests, isolated behind the registry.
- **Wikilink ambiguity** where `[[Name]]` has no unique target. Mitigation:
  deterministic resolution that reports ambiguity for human resolution, never a
  guessed edge.

## Related Requirements

- rac-note-tool-ingest-sources

## Related Decisions

- adr-079
- adr-072
- adr-074
- adr-065
- adr-002

## Related Designs

- note-tool-ingest-sources

## Related Tickets

- itsthelore/asdecided-core#227
