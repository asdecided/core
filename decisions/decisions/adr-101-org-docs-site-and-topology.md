---
schema_version: 1
id: RAC-KWV0HZ2SSGE2
type: decision
tags: [structure, org, docs, hosting]
---
# ADR-101: Org-Wide Documentation Site Amends Docs Hosting and Repository Topology

## Status

Accepted

## Category

Product

## Context

ADR-042 hosts `rac-core`'s user documentation at
`https://itsthelore.github.io/asdecided-core/` — a GitHub Pages *project* page built
by `rac-core`'s own `docs.yml` workflow from that repository's `docs/` with
MkDocs (Material theme). That scope was correct when `rac-core` was the only
public surface, but ADR-092 now names a small constellation of `rac-*`
repositories (`rac-core`, `rac-ci`, `rac-connectors`, `rac-sdk`,
`rac-benchmarks`, `rac-editors`) plus sibling products (`wayfinder-router`,
`proofkeeper`). None of them has a hosted documentation surface, and
`rac-core`'s project page has no way to represent the org as a whole.

GitHub serves a user/org root Pages site — with no `/repo-name/` path prefix —
only from a repository named exactly `<org>.github.io`. `itsthelore/
itsthelore.github.io` has been created for this purpose: it is public (a
private repo would need a paid org plan for Pages, and the rendered output is
public by default anyway), and deploys via GitHub Actions
(`actions/upload-pages-artifact` → `actions/deploy-pages@v4`), matching the
deploy method ADR-042 already established for `rac-core`.

Two questions ADR-042 and ADR-092 leave open: whether the org site stands
alone and links out to each product's own docs, or aggregates their content;
and, per ADR-064's caution that vendoring "is deferred until a publish/vendor
contract exists," what that contract is if aggregation is chosen. The
maintainer has decided in favor of aggregation, and for `rac-core`'s existing
project page to be retired in the umbrella site's favor, not kept as a
permanent parallel surface. `rac-spec` (a further product repo, not yet
created) is anticipated as the next surface this site will carry; this
decision's vendoring contract is written to admit that member without
amendment.

## Decision

`itsthelore.github.io` becomes the hosted documentation surface for the whole
org. It **aggregates `docs/` from each product repo at build time** rather
than linking out to per-repo project pages.

- The site is built with Astro and deployed to GitHub Pages by a GitHub
  Actions workflow in `itsthelore.github.io`, using the same
  `upload-pages-artifact` / `deploy-pages@v4` steps ADR-042 established.
- **Vendor contract:** the build sparse-checks out each source repo's `docs/`
  directory at a pinned ref (a tag or `main`, per repo) into the Astro content
  tree at build time. Nothing vendored is committed to either repository's
  history — the umbrella site remains a pure build artifact, exactly as
  ADR-042 requires of the `rac-core` project page today. Each product repo's
  own `docs/` stays the authoritative source; the umbrella site never edits
  vendored content, only renders it.
- Site structure: `/` is the org landing page (brand, product overview, links
  into each section); `/rac-core/` renders `rac-core`'s vendored `docs/`;
  further sections (`/rac-ci/`, `/rac-connectors/`, `/rac-spec/`, …) are added
  the same way as each source repo gains a `docs/` directory worth
  publishing. No section is scaffolded ahead of its source repo existing.
  Per ADR-042's existing constraint, the `rac/` corpus is never published on
  the site, in `rac-core` or any other member repo.
- `rac-core`'s project-page deployment is **retired**: `docs.yml`'s Pages
  publish step is removed once the umbrella site's `/rac-core/` section is
  live, so `itsthelore.github.io/asdecided-core/` is served by the umbrella
  deployment instead of `rac-core`'s own. `rac-core` keeps its `docs/`
  directory and MkDocs config for local `mkdocs serve` authoring; it stops
  being a second published surface at that URL. Disabling the old Pages
  deployment is a manual repository-settings action only the maintainer can
  take (the same class of manual step ADR-042 already required to enable
  Pages in the first place).
- Naming: `itsthelore.github.io` is added to ADR-092's topology table as its
  own repository — a distinct concern (org presence and docs aggregation),
  not a family-pattern member and not folded into any consolidated repo,
  because GitHub's root-Pages rule forces this exact name regardless of the
  `rac-*` convention.

## Consequences

### Positive

- The org gets one coherent documentation home spanning every product,
  instead of a single project page that implied `rac-core` was the whole
  story.
- The vendor contract is decided once, generically (pinned sparse-checkout of
  `docs/`), so `rac-spec` and future repos add a section without a further
  ADR or a new build mechanism.
- ADR-042's authoritative-source and build-artifact discipline is preserved
  end to end: vendoring reads `docs/`, never writes it, and the site is
  never a second source of truth.

### Negative

- The umbrella build now depends on the state of every vendored repo at
  build time; a broken or unreachable source repo can fail the umbrella
  build even though `itsthelore.github.io` itself has not changed.
- Retiring `rac-core`'s project-page deployment is a breaking change for
  anyone who bookmarked or linked `itsthelore.github.io/asdecided-core/` expecting
  `rac-core`'s own MkDocs build; the URL keeps working but the rendering
  engine and look change underneath it.
- One more repository (`itsthelore.github.io`) with its own build pipeline
  and brand/design surface to maintain, on top of ADR-092's topology.

### Risks

- **Cross-repo build coupling.** The umbrella build breaks if a vendored
  repo renames or removes its `docs/` directory without updating the
  umbrella's checkout path. Mitigation: pin each vendor step to a specific
  ref and treat a moved `docs/` path as a coordinated two-repo change, the
  same discipline ADR-064 already applies to cross-repo contracts.
- **Stranded links.** External links into the old `rac-core` project-page
  layout could break if the umbrella's `/rac-core/` structure diverges from
  MkDocs's nav. Mitigation: keep the vendored section's URL structure close
  to the source `docs/` tree so deep links keep resolving.

## Alternatives Considered

### Stand-alone landing site, link out to each product's own docs

Keep `itsthelore.github.io` as a brand/landing page only, with outbound links
to `rac-core`'s existing project page and to each future product's own Pages
site.

#### Advantages

- No cross-repo build coupling; no vendor contract to define or maintain.
- Each product repo keeps full control of its own docs build and cadence.

#### Disadvantages

- Rejected — leaves the org without one coherent documentation surface, and
  defers exactly the aggregation problem ADR-064 flagged and left open. The
  maintainer's stated goal is one shared surface across products, not a set
  of independently branded project pages.

### Keep rac-core's project page permanently alongside the umbrella site

Aggregate other repos into the umbrella site but leave `rac-core`'s own
`docs.yml` Pages deployment running in parallel.

#### Advantages

- No breaking change to the existing `itsthelore.github.io/asdecided-core/` URL's
  rendering engine.

#### Disadvantages

- Rejected — leaves two published surfaces claiming the same content and the
  same URL space, one MkDocs-rendered and one Astro-rendered, with no rule
  for which is canonical. The maintainer chose retirement precisely to avoid
  that duplication.

## Related Decisions

- adr-042
- adr-064
- adr-092

## Review Date

Revisit when a vendored repo's `docs/` structure changes enough to break the
sparse-checkout contract, or when `rac-spec` (or another anticipated product
repo) is created and its `docs/` section is added to the umbrella site.
