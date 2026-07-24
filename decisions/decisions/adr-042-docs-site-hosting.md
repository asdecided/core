---
schema_version: 1
id: RAC-KTYXDTB4299E
type: decision
---
# ADR-042: Documentation Site Hosting

## Status

Accepted

## Category

Product

## Context

> **Amended by ADR-101.** This ADR's project-page hosting model — `rac-core`
> publishing its own `docs/` to `itsthelore.github.io/asdecided-core/` via its own
> `docs.yml` — is retired in favor of an org-wide umbrella site
> (`itsthelore.github.io`) that vendors `docs/` from each product repo at
> build time. ADR-101 governs where the two differ (the publishing repo and
> mechanism); this ADR's authoritative-source and build-artifact discipline
> (repository Markdown is canonical, the site is never a second source of
> truth, the `rac/` corpus stays unpublished) carries forward unchanged.

ADR-022 established three documentation layers — README as doorway,
`docs/` as user documentation, `rac/` as the product-knowledge corpus —
and committed to repository Markdown as the canonical documentation
source. It also named its own review trigger: revisit the decision "when
introducing external documentation hosting."

That trigger now fires. The approved scoping design (`docs-site-scoping`)
introduces a documentation website: a landing page and the nine `docs/`
guides, browsable with navigation and search, hosted on GitHub Pages and
built by GitHub Actions from this repository. The question ADR-022 left
open is whether hosted documentation changes where canonical documentation
lives, and what keeps the README and the site from diverging.

## Decision

The `docs/` layer gains a generated, hosted view; nothing else about
ADR-022's model changes.

- The site is built with MkDocs (Material theme) from `docs/` and deployed
  to GitHub Pages by a GitHub Actions workflow on every push to `main`.
- **Repository Markdown remains authoritative.** The site is a build
  artifact: it is never edited directly, never committed to the
  repository, and never holds content that does not exist in `docs/`.
- ADR-022's three layers stand unchanged. The site is a rendering of
  layer 2 (`docs/`, plus a new `docs/index.md` landing page); the README
  remains the layer-1 doorway; the `rac/` corpus is not published on the
  site.
- Drift-prevention policy, normative for both surfaces:
  - The site is generated only from `docs/`.
  - The README contains nothing that also appears on the site except the
    install command, the tagline, and the minimal usage example and
    common-commands block that ADR-022 requires the doorway to carry.
  - The README's documentation links point only at the site, so site nav
    changes cannot strand README links.
  - Content relocated from the README lives in exactly one place
    (`docs/index.md`) and does not regrow in the README.

## Consequences

### Positive

- Lore gets a real front door: rendered landing page, sidebar navigation,
  and search over all user documentation.
- `mkdocs build --strict` in CI turns broken links and nav into build
  failures, a check the raw Markdown never had.
- The README shrinks toward ADR-022's one-minute doorway, with the depth
  content owned by exactly one surface.
- ADR-022's "avoid GitHub Wiki / external systems as source of truth"
  principle is preserved: hosting is added without moving authorship.

### Negative

- The header art is duplicated (`rac/assets/images/` for the README,
  `docs/images/` for the site); both copies must change together, accepted
  because the art changes rarely and in the same PR.
- One more workflow to maintain, and the first deploy depends on a manual
  repository setting (Pages source: GitHub Actions) only the maintainer
  can flip.
- Site URLs become public API of a sort: renaming a `docs/` page later
  breaks inbound links, a cost that did not exist for blob URLs.
- The landing page adopts the lore-web visual identity, which is the
  pre-launch Lore brand held behind GATE-1 (nothing public until
  external-communications review). Publishing — enabling Pages — is
  therefore gated on that same review; merging the work publishes nothing.
  This couples the docs-site go-live to the lore-web launch sequence.

## Alternatives Considered

### GitHub Wiki

Rejected — ADR-022 already rejected the wiki as a documentation home:
changes bypass code review and drift from the repository.

### External documentation platform (Read the Docs, hosted SaaS)

Rejected — moves builds and configuration outside the repository workflow
and adds an account/service dependency for no capability the constraints
allow us to use.

### Committing built HTML / `gh-pages` branch

Rejected — generated artifacts in history blur what is authored versus
built; the official Pages actions deploy from a build artifact and need no
long-lived branch.

### Publishing the `rac/` corpus on the site

Rejected — ADR-022 explicitly keeps internal artifacts out of the user
documentation path; the site dogfoods by being governed by artifacts, not
by publishing them.

## Related Requirements

- rac-documentation-structure
- rac-docs-site-platform
- rac-docs-site-landing-page
- rac-docs-site-publish-pipeline
- rac-docs-site-readme-doorway

## Related Decisions

- ADR-022
- ADR-018
- ADR-036
- ADR-001
- ADR-101

## Related Designs

- docs-site-scoping

## Related Roadmaps

- v0.11.1-docs-site
