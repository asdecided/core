# Lore export viewer — input contract

Status: **Reconciled v1** — matches `rac export` as of roadmap v0.11.0.
This document defines the JSON payload the static export viewer
(`rac export --html`) consumes, as implemented in `src/viewer/`. The
viewer consumes exactly what Core emits; any future shape change is a
joint change to Core's export, this contract, and the viewer, with the
vendored Portal shell rebuilt (`npm run vendor:shell`).

## 1. Payload: the export document

A single JSON document, as emitted by `rac export --json`.

```json
{
  "schema_version": "1",
  "corpus": {
    "name": "rac",
    "source": "asdecided/core",
    "rac_version": "0.11.0",
    "artifact_count": 106
  },
  "artifacts": [
    {
      "id": "RAC-KTQ63DSC8SZW",
      "aliases": ["adr-027", "adr-027-ci-test-topology"],
      "type": "decision",
      "status": "Accepted",
      "title": "ADR-027: CI test topology",
      "path": "rac/decisions/adr-027-ci-test-topology.md",
      "body_html": "<p>…</p>"
    }
  ],
  "relationships": [
    { "from": "RAC-KTQ63DSC8SZW", "to": "RAC-ABCDEF123456", "type": "relates-to" }
  ]
}
```

### `schema_version`

String, currently `"1"` (matching the index contract). The viewer
renders any payload it can parse and does not hard-fail on a different
version, but a bump signals a breaking shape change.

### `corpus` — minimal metadata

| field            | type    | meaning                                          |
| ---------------- | ------- | ------------------------------------------------ |
| `name`           | string  | Human-readable corpus name — the exported directory name. |
| `source`         | string  | Stable corpus provenance identity shared with documents and graph exports. |
| `rac_version`    | string  | Version of the RAC CLI that produced the export. The header shows it when present and tolerates its absence. |
| `artifact_count` | integer | Number of artifacts in the export.               |
| `sample`         | boolean | Optional, default false. True marks demonstration data; the viewer then shows SAMPLE DATA labels in the header and footer. Never emitted by Core; used by the committed sample corpus. |

The export is deterministic: there is no `generated_at` timestamp. A
configured `corpus.source` is independent of checkout location; see the Core
[export contract](../docs/export-contracts.md#corpus-source-identity) for its
fallback behaviour. The viewer tolerates older payloads without `source`.

### `artifacts[]`

Ordered by `path`.

| field       | type     | meaning                                                |
| ----------- | -------- | ------------------------------------------------------ |
| `id`        | string   | Opaque stable artifact ID, unique within the corpus (`RAC-KTQ63DSC8SZW`). |
| `aliases`   | string[] | Human aliases as emitted by Core identity, e.g. `["adr-027", "adr-027-ci-test-topology"]`. May be empty. |
| `type`      | string   | Artifact family (`decision`, `requirement`, …). Open set; the viewer derives its type filter from the values present. |
| `status`    | string   | Lifecycle status in its authored casing (`Accepted`, `Proposed`, `Superseded`, …). Open set — see case handling below. |
| `title`     | string   | Plain text.                                            |
| `path`      | string   | Source path within the repository. Shown as a muted provenance line on the detail view. |
| `body_html` | string   | The artifact body **rendered to HTML at export time** (see trust model). |

#### Alias display

The opaque `id` is stable but not human-friendly, so the viewer prefers
a **display name**: deterministically, the first alias that differs
from the `id`, else the `id` itself. The display name is used on list
rows, the detail heading, and related-artifact links; the opaque `id`
stays visible on the detail view's provenance line (alongside `path`)
and remains the routing key (`#/artifact/<id>`).

#### Status case handling

Statuses arrive in arbitrary case. The viewer groups the status filter
and applies semantic colouring **case-insensitively** (`Accepted` and
`accepted` are one status) while displaying the first-seen authored
casing. Known statuses get semantic colour (accepted = green,
rejected = error, superseded/deprecated = muted); unknown statuses
render plain.

### `relationships[]` — edges

Each edge is `{ "from": ID, "to": ID-or-alias, "type": string }` and
reads "`from` `type` `to`". Ordered by (from, to). Core emits **only**
`relates-to`; richer edge typing is a future Core decision. `to` may be
an unresolved alias preserved verbatim — the viewer renders those as
"(not in corpus)" rather than dropping them.

The type set stays open for forward compatibility. The viewer keeps
inverse labels for types a future Core might emit (accepted if they
appear, **never emitted today**):

| type         | outbound label | inbound label    | emitted by Core |
| ------------ | -------------- | ---------------- | --------------- |
| `relates-to` | relates to →   | ← related to     | yes             |
| `supersedes` | supersedes →   | ← superseded by  | no — forward-compatible |
| `refines`    | refines →      | ← refined by     | no — forward-compatible |
| `implements` | implements →   | ← implemented by | no — forward-compatible |

Unknown edge types render grouped under their literal type name in both
directions.

## 2. Data injection — how the JSON reaches the viewer

The built artifact must open from `file://` with **zero network
requests**. `fetch()` of a sibling JSON file fails from `file://`, as do
external module scripts, so injection works as follows:

1. **Built single-file artifact** (`dist/viewer/lore-viewer.html`): the
   corpus is embedded in the HTML as

   ```html
   <script type="application/json" id="lore-export">{…}</script>
   ```

   placed before the (inlined) application script. On boot the app reads
   `document.getElementById('lore-export')` and parses its text content.
   All JS and CSS are inlined into the same file; there are no other
   references. `rac export --html` produces exactly this: the vendored
   Portal shell with the corpus JSON substituted into that element
   (escaping `</` as `<\/` and `<!--` as `<\u0021--`, both valid JSON
   escapes, so the payload is `<script>`-safe and parses unchanged).

2. **Dev server / hosted multi-page build**: no inline element exists,
   so the app falls back to fetching the committed sample corpus
   (`src/viewer/sample/lore-export.sample.json`) as a normal asset.

The feature detection is exactly "inline element with non-empty text
content present → use it, else fetch".

### Build pipeline (this repo)

`npm run build:viewer` =

1. `tsc -b` — type-check;
2. `vite build --config vite.config.viewer.ts` — a dedicated
   single-entry build (one JS chunk, one CSS file, `modulePreload`
   disabled, dynamic imports inlined) into `dist/.viewer-build/`;
3. `node scripts/build-viewer-artifact.mjs` — inlines the JS and CSS
   into the HTML, strips file-referencing `@font-face` rules (below),
   injects the corpus, verifies the result has no external `src=`,
   `href=` (other than `#` anchors) or non-`data:` `url()` references,
   and writes `dist/viewer/lore-viewer.html`.

`scripts/build-viewer-artifact.mjs --corpus <path> --out <path>` builds
the artifact around any conforming export.

### Portal shell (`--shell-only`) and vendoring

`scripts/build-viewer-artifact.mjs --shell-only` emits the same
single-file artifact with an **empty** data seam —
`<script type="application/json" id="lore-export"></script>`, no
whitespace inside the element — and writes it to
`dist/viewer/lore-portal-shell.html` by default. `rac export --html`
injects the escaped corpus JSON into that seam; nothing else in the
file changes.

`npm run vendor:shell` (`scripts/vendor-portal-shell.mjs`) builds the
shell and commits it into the RAC package as
`src/rac/templates/portal/lore-portal-shell.html` together with
`provenance.json` (rac-localview source commit, shell hash, viewer
source-tree hash). A drift-guard test on the Python side fails when the
viewer source changes without re-vendoring; the normative hash
algorithm is documented in `scripts/vendor-portal-shell.mjs`.

### Fonts in the single-file artifact

The design system self-hosts JetBrains Mono (~190 KB of woff2). The
single-file artifact deliberately does **not** inline the fonts: the
build strips the file-referencing `@font-face` rules and the artifact
renders in the metric-compatible local mono fallback stack already
defined by the design tokens (`JetBrains Mono Fallback` /
`ui-monospace` / `Courier New`). This keeps the artifact small and
reference-free. The hosted viewer build keeps the real fonts.

## 3. Body-HTML trust model

`body_html` is produced by Core's vendored `markdown-it-py` CommonMark
renderer with **raw HTML escaped**: HTML written in artifact sources is
emitted as escaped text, not markup, so an export of repo-resident
Markdown contains no script elements, no event-handler attributes and
no external resource loads of its own making.

The viewer renders `body_html` **as-is** (assigned to `innerHTML`, then
cited ids and aliases in text nodes are linkified). The viewer performs
**no sanitisation**. The contract is therefore:

- **Escaping happens at export time, in Core.** The corpus is the
  user's own repository content, viewed by the person who exported it —
  the same trust boundary as the repo itself.
- A hand-crafted hostile `body_html` (i.e. a tampered export document,
  not one Core produced from Markdown) could execute script in the
  viewer page. Do not build artifacts from untrusted export documents.
  If exports ever cross a trust boundary (e.g. hosted multi-tenant
  viewing), sanitisation must be added at that boundary; the viewer
  itself stays a dumb renderer.
- Because raw HTML is escaped, bodies cannot load external resources
  (`<img src="http…">` arrives as text), preserving the
  zero-network-request property.

## 4. Viewer behaviour summary

- Read-only; no router dependency — state is hash-based
  (`#/` list, `#/artifact/<id>` detail) so deep links work from
  `file://`.
- List view: every artifact as a row (display name + title + chips);
  filter toggles for type and status derived from the corpus (status
  grouped case-insensitively); debounced substring search over
  id + aliases + title + body text; result count announced via
  `aria-live`.
- Detail view: display name, title, type/status chips, a muted
  provenance line (opaque id + source path), rendered body with cited
  cross-links, and a related-artifacts panel grouped by edge type in
  both directions. List form only — a graph visualisation is an
  explicitly deferred decision.
- Keyboard: `/` focuses search, `Tab` walks rows, `Esc` returns from
  detail to list.
- **Citation linkification**: the viewer builds a case-insensitive
  lookup over every artifact's `id` and every alias. In body text
  nodes, candidate tokens — maximal runs of word characters and hyphens
  starting with a letter, bounded by non-word characters — are linkified
  only when their lowercased form is in the lookup. So
  `RAC-KTQ63DSC8SZW`, `ADR-027` and `adr-027-ci-test-topology` all link
  when they name a corpus artifact; nothing else is touched.
  Linkification never descends into `<a>`, `<code>` or `<pre>`.

## 5. Sample data

`scripts/make-corpus.mjs` (run via `npm run corpus`) generates:

- `src/viewer/sample/lore-export.sample.json` — committed; a
  hand-authored 30-artifact corpus for "ledgerline", a fictional
  mid-size Python billing service, in the v1 shape (opaque ids, human
  aliases, paths, authored-case statuses, `relates-to` edges only —
  including one unresolved alias target). Its `corpus.name` contains
  "SAMPLE DATA" and `sample: true` is set, so every surface that shows
  corpus identity is labelled. Its stable sample source is
  `asdecided/ledgerline-sample`.
- `/tmp/lore-export-500.json` — a deterministic 500-artifact synthetic
  corpus for performance testing. Not committed.

## 6. Editor-host integration (optional)

When the viewer runs inside an editor webview (VS Code / Cursor — the RAC
extension, RAC v0.21.7), it talks to the host over `postMessage`. The bridge is
**feature-detected**: it activates only when `acquireVsCodeApi` is present, so a
standalone Portal opened from `file://` is entirely unaffected — every message
path is inert and behaviour is identical to a hosted-free build.

The protocol is intentionally tiny and carries only the `path`/`id` already in
the payload (§1) — the host derives nothing the export does not already state:

- viewer → host `{ type: "ready" }` — on mount, so the host can reveal the
  artifact for the active editor.
- viewer → host `{ type: "open-artifact", path, id }` — when the user selects an
  artifact (navigates to its detail), so the host can open its file.
- host → viewer `{ type: "reveal-artifact", id }` — navigate the viewer to that
  artifact's detail. A reveal does not echo back as an `open-artifact`.

The bridge lives in `src/viewer/host.ts` and is wired in `src/viewer/App.tsx`;
it is part of the viewer source tree, so a change to it re-hashes the vendored
shell (`viewer_source_sha256`) and requires re-running `npm run vendor:shell`.

## 7. Graph view (v0.21.8)

Alongside the list/detail, the viewer offers a node-link **graph view**
(`#/graph`) drawn entirely from the payload (§1): nodes are `artifacts[]`
(coloured by `type`, sized by link count), edges are `relationships[]`, an
unresolved `to` becomes a dangling node, and retired artifacts (ADR-051) render
muted. Layout is a seeded, dependency-free force simulation computed in the
browser (`src/viewer/graph.ts`) — **no positions are ever read from or written
to the export**, so determinism (§1) is unaffected and only the vendored shell
changes when the view does. It follows Obsidian conventions: a Global graph and
a Local graph rooted on the host's active artifact to a user-set depth (with an
opt-in "follow editor" default), hover-to-highlight with the rest dimmed,
zoom-based label fade, pan/zoom/drag, **force-strength controls** (centre /
repel / link / distance, debounced and seeded so they stay deterministic),
**keyboard navigation** (arrow keys move between nodes, Enter selects, O opens),
**orphans** laid out in a tidy grid rather than piled at the frame edge, and
filters (search, type, status, orphans, unresolved). Selection reuses the §6
host bridge — `open-artifact` on click, `reveal-artifact` roots the local graph —
so no new protocol is added. The list/detail view remains the default.
