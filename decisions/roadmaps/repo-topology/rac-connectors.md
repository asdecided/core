---
schema_version: 1
id: RAC-KW6F47QRN0ND
type: roadmap
tags: [structure, org, distribution]
---
# Rename lore-connectors to rac-connectors

## Status

Planned

## Context

ADR-073 established that backend connectors are export-contract consumers
consolidated in one repository, not one repo per provider. ADR-092 renames that
repository `lore-connectors` → `rac-connectors` and generalises its scope: one
integrations repo covering **inbound** (`rac ingest`) and **outbound**
(`rac export`) alike, plus provider suites — Atlassian is a subdir
(`atlassian/`), not its own repo. The consolidation principle is unchanged; this
item applies the `rac-*` name and the wider remit.

## Outcomes

- `itsthelore/lore-connectors` is renamed `rac-connectors`, a subdir per
  integration (`atlassian/`, `supermemory/`, …), spanning inbound and outbound
  adapters.
- A provider grows its own repo only via the ADR-073 escape hatch (an installable
  product with independent cadence/ownership) — not by default.
- Documentation that points at the connectors companion resolves to
  `rac-connectors`.

## Initiatives

- **Rename the repo** `lore-connectors` → `rac-connectors` (GitHub redirect
  preserves existing links) and adopt the subdir-per-integration layout.
- **Fold in the inbound axis**: the would-be separate inbound work (e.g.
  Atlassian/Confluence ingestion, the long tail of source fetchers) lands as
  subdirs here rather than a separate `lore-sources` repo.
- **Update `rac-core` references** to the connectors companion (for example the
  README "Export the corpus" note) from `lore-connectors` to `rac-connectors`.

## Success Measures

- `itsthelore/asdecided-connectors` exists with the subdir-per-integration layout; the
  old name redirects.
- No `rac-core` doc or corpus reference points to `lore-connectors`.
- Connectors still consume only the published export/ingest contracts and the
  public CLI — no engine internals (ADR-002, ADR-063).

## Assumptions

- Connectors remain thin contract-consumers without independent
  cadence/ownership, so consolidation holds (ADR-073).
- The export and ingest contracts (`rac export --documents` / `--graph`,
  `rac ingest`) are stable enough to consume across providers.
- The maintainer can rename the repository under `itsthelore`.

## Risks

- **The repo becomes a grab-bag** as integrations accumulate. Mitigation: the
  line is *concern* (integrations), and a provider that grows into a product
  graduates out via the ADR-073 escape hatch.
- **Stranded links** to `lore-connectors`. Mitigation: GitHub rename redirect
  plus a `rac-core` reference sweep.

## Related Decisions

- adr-092
- adr-073
- adr-063
- adr-002
- adr-095

## Related Roadmaps

- repo-topology-convergence
- corpus-export-to-rag-backends
