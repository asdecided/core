---
schema_version: 1
id: RAC-KWGHXY1304J6
type: roadmap
tags: [connectors, enterprise, integrations]
---
# Atlassian Suite: Jira Verification and Confluence Publish

## Status

Planned

## Context

ADR-090 places the Atlassian suite at `rac-connectors/atlassian/` — one subdir
covering both directions on shared Cloud auth. ADR-087 introduced the
`related_tickets` external-reference edge and explicitly delegated ticket
existence and state checks to this satellite: the engine format-lints
references offline and never contacts a ticketing system. The engine half is
already live — `rac export --graph` marks ticket edges `external: true` with
the configured `provider` (ADR-074), so a Jira-aware consumer has everything
it needs on the published contract.

This roadmap schedules the connector half: the first two surfaces of the
ADR-090 suite, delivered in `rac-connectors` as thin export-contract
consumers (ADR-073).

## Outcomes

- An operator can verify every `related_tickets` Jira reference in a corpus
  against their own Jira Cloud instance —
  `rac export rac/ --graph | rac-connect atlassian verify` — and gate CI on
  the result.
- An operator can mirror corpus artifacts into a Confluence space as managed
  pages, idempotently, with the propose-only posture intact: the connector
  writes outward only, and corpus changes still arrive exclusively by human
  PR review (ADR-065).

## Initiatives

- **Jira reference verification** — a read-only `verify` verb over the
  `--graph` projection: select `external` edges with provider `jira`, check
  existence and status against the configured instance, report per reference
  with a CI-usable exit code.
- **Confluence page publish** — a `publish` verb over the `--documents`
  projection: render artifact Markdown deterministically to storage format
  and upsert managed pages keyed by artifact id, never by title.
- **Deferred surfaces, named** — inbound Confluence ingest (feeding
  `rac ingest` and the human-review loop), comment-mode posting, the Data
  Center transport profile, and OAuth are recorded ADR-090 intent but out of
  scope here; each returns as its own roadmap item.

## Success Measures

- `rac export rac/ --graph | rac-connect atlassian verify` reports
  exists / missing / forbidden per Jira reference, exits non-zero on
  findings, and needs no credentials in `--dry-run`.
- A second `rac-connect atlassian publish` run over an unchanged corpus
  performs zero writes.
- The connector test battery runs fully offline; CI needs no Atlassian
  account.

## Assumptions

- Export contract major 1 remains the consumed surface (additive growth
  only, ADR-007).
- One Atlassian Cloud site per corpus is sufficient for the first delivery;
  the reference-to-instance mapping is connector configuration.
- API-token Basic auth against the customer's own instance satisfies the
  air-gap posture (ADR-086) — the connector never calls a vendor endpoint.

## Risks

- **Confluence storage-format rendering fidelity** — complex Markdown may
  degrade. Mitigation: a deliberately small deterministic rendering subset,
  golden-file tests, and escape-first handling of untrusted corpus content
  (ADR-065).
- **Atlassian API churn** — Cloud endpoints and rate-limit regimes move.
  Mitigation: a thin internal client, pinned endpoint set, backoff on 429.
- **Scope creep toward a work tracker** — the connector must not mirror
  tickets into the corpus (ADR-017). Mitigation: outbound-and-verify only;
  inbound ingest stays a separately gated roadmap item.

## Related Decisions

- adr-090
- adr-087
- adr-096
- adr-086
- adr-073
- adr-065

## Related Roadmaps

- rac-connectors
- corpus-export-to-rag-backends

## Related Tickets

- itsthelore/asdecided-connectors#4
