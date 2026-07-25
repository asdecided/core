---
schema_version: 1
id: RAC-KWV63BR8AW71
type: decision
tags: [org, docs, branding, design]
---
# ADR-102: Org-Site Brand Direction — Light, Restrained, Site Rhymes With Product UI

## Status

Accepted

## Category

Product

## Context

ADR-101 made `asdecided.github.io` the org-wide documentation site, and
ADR-092 places the brand at the org rather than in repository slugs. The
site's first iterations translated rac-localview's design system — amber on
warm near-black, JetBrains Mono for all text, dashed chrome, pixel-art
mascot in the hero — directly onto the public surface, first dark, then as
a lightened variant.

The maintainer's verdict on both: not clean, not professional. A design
council (visual design, UX/IA, GTM messaging, brand strategy) reviewed the
site against contemporary developer-tool marketing surfaces (Supermemory,
Linear, Vercel, Stripe) and converged on the same diagnosis from every
lens: a terminal aesthetic that works *inside* a product UI reads as a
hobby project on a marketing/documentation surface. Mono body text hurts
reading speed and polish; amber headings on cream read dated; dashed
borders read as placeholders; a large pixel mascot undercuts the
enterprise-adjacent trust claims the copy makes (deterministic, read-only,
air-gapped).

The alternative — restyling the product UI to match a new site look — was
out of scope and undesirable: rac-localview's dark terminal identity is
fit for purpose where it lives.

## Decision

The org documentation site (`asdecided.github.io`, ADR-101) adopts its own
**light, restrained visual identity** rather than mirroring the rac-localview
product-UI theme:

- **Ground and text:** near-white surfaces (`#fcfcfb` / `#f6f6f4` /
  `#efefed`), near-black text (`#111113`), low-alpha solid hairline borders.
  The dark warm-near-black ground does not carry to the marketing/docs
  surface.
- **One accent:** deep amber `#B45309` (hover `#92400E`), used for links,
  active navigation, and focus only — never as fills or structure. The
  bright lantern amber `#f5a623` survives only in non-text touches.
- **Typography:** Inter (self-hosted variable woff2) for prose and headings;
  JetBrains Mono remains the voice of code only. The product UI's
  mono-everywhere rule does not apply to the site.
- **Mascot:** the lamplighter is demoted to favicon scale on the site — no
  in-page illustration. Pixel art reads as a mark at 32px and as a toy at
  hero size.
- **Chrome:** solid hairlines and modest radii (6/8/12px) replace the
  product UI's dashed-border / sharp-corner language on the site.

**The site and the product rhyme, they do not match.** Exactly three brand
constants are shared with rac-localview: the amber hue (deep on light
ground, bright in the dark product UI), JetBrains Mono as the voice of code,
and the lamplighter at icon size. Ground, body type, border style, and
corner language are decided per surface.

rac-localview's dark tokens and DESIGN.md five rules are **unchanged** —
they remain the product-UI theme. This decision governs the org's public
web surface only.

## Consequences

### Positive

- The public surface reads as a credible, contemporary developer-tool
  site, aligned with what its audience (engineering leads running coding
  agents) expects from tools they adopt.
- A recorded, bounded relationship between the two identities — three
  shared constants — stops future drift arguments in both directions:
  the site does not creep dark, the product UI does not creep light.
- Accessibility improves: all text-on-ground pairs are WCAG AA or better
  by construction (deep amber replaces bright amber for text).

### Negative

- Two visual identities to maintain instead of one; a reader moving from
  the site into the product UI experiences an intentional theme shift.
- The lamplighter loses its hero placement — some brand warmth is traded
  for credibility on the public surface.
- The site's stylesheet no longer derives mechanically from
  rac-localview's tokens; shared-constant changes (the amber hue) must be
  carried across by hand.

## Alternatives Considered

### Keep translating the rac-localview theme to the site

The approach the first two site iterations took (dark, then lightened).

#### Advantages

- One design system everywhere; site styles derive from product tokens.

#### Disadvantages

- Rejected — both iterations failed the maintainer's professionalism bar,
  and the council's four lenses independently attributed that to the
  terminal aesthetic itself (mono body, amber headings, dashed chrome,
  hero mascot), not to its execution.

### Full supermemory-style pivot (blue accent, no brand carryover)

Adopt the reference site's palette and drop the amber entirely.

#### Advantages

- Maximum distance from the "hobby project" reading.

#### Disadvantages

- Rejected — severs the only visual threads connecting the site to the
  product family. Deep amber at AA contrast keeps the lantern hue without
  the toy-like brightness.

## Related Decisions

- adr-092
- adr-101
- adr-036

## Review Date

Revisit if rac-localview's design system is itself revised (the three
shared constants must be re-confirmed), or when a second product surface
(e.g. a hosted app) needs to decide which identity it follows.
