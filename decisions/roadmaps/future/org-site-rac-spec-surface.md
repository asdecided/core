---
schema_version: 1
id: RAC-KWV9B0WMBYRG
type: roadmap
tags: [org, docs, spec, essays]
---
# Org Site: rac-spec Surface and Essays

## Status

Planned

Unscheduled — captured as future intent, not yet on a release. Records the
maintainer's stated direction: **rac-spec** (the RAC specification) and its
associated **essays** are the next content added to the org documentation
site (`asdecided.github.io`, ADR-101), and they are to sit **front and
centre on the home page** — headline placement, not another nav section.

## Context

The org site ships with one vendored section (`/rac-core/`, ADR-101) and a
landing page whose hero sells the product (install → quickstart, ADR-102's
brand direction). The vendor contract was written to admit new member repos
without amendment — a `vendor_repo` line, nav entries, landing placement.

rac-spec changes more than the section count. A specification plus essays is
a *standard-first* story, and the maintainer wants it leading the home page,
which rebalances the landing's current product-first GTM order. Essays are
also a different content shape from reference docs — long-form, dated,
authored voice.

**Decided:** essays are delivered with MkDocs Material's first-class blog
plugin (index page, dates, post metadata) rather than as plain pages.

**Open (decide at implementation):**

- Where essays are authored: in the rac-spec repo and vendored (preserving
  ADR-101's single-source rule, with the vendor script landing posts into
  the blog plugin's `posts/` layout), or authored directly in the site repo
  as its first owned content beyond the landing page.
- What "front and centre" displaces: hero stays product with a prominent
  spec/essays band above "How it works", or the hero itself becomes
  spec-led. This is a positioning call (product-first vs. standard-first)
  that revises ADR-102's landing composition and should be settled
  deliberately, not defaulted.

## Outcomes

- rac-spec's specification content is published on the org site under its
  own section, vendored per the ADR-101 contract.
- Essays are live via the Material blog plugin, with an index the home page
  links prominently.
- The home page leads with the spec/essays surface per the maintainer's
  placement intent, with the product path (install → quickstart) still
  one action away.

## Initiatives

### Initiative 1 — Vendor and publish the rac-spec section

Add rac-spec to `scripts/vendor-docs.sh` and `mkdocs.yml` nav once the repo
exists with publishable content; extend ADR-092's topology table if rac-spec
lands as its own repo.

### Initiative 2 — Essays via the Material blog plugin

Enable the blog plugin, settle the authoring/vendoring model for posts, and
ship the essays index.

### Initiative 3 — Landing rebalance

Redesign the home page composition to give rac-spec and the essays headline
placement, resolving the product-first vs. standard-first question against
ADR-102's recorded brand direction (which governs look, not page order).

## Success Measures

- `mkdocs build --strict` stays green with the new section and plugin.
- The spec and at least one essay are reachable within one click of the
  home page.
- ADR-101's single-source rule holds: no vendored content is committed to
  the site repo.

## Assumptions

- rac-spec will exist as content the site can vendor (repo and `docs/`
  layout to be settled when it is created).
- The Material blog plugin is compatible with the site's pinned
  mkdocs-material version at implementation time.

## Risks

- Headline placement for a spec that is still maturing could set
  expectations the spec itself is not ready to meet; sequencing the landing
  rebalance after the spec content is publishable mitigates this.
- Vendoring dated blog posts is a less-trodden path than vendoring plain
  pages; if the plugin's layout fights the vendor script, the authoring
  location decision may be forced rather than chosen.

## Related Decisions

- adr-101
- adr-102
- adr-092
