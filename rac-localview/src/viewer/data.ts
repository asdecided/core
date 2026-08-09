/**
 * Data loading and indexing for the export viewer.
 *
 * Loading strategy (see VIEWER_CONTRACT.md):
 *   1. If the document contains <script type="application/json"
 *      id="lore-export">, parse it. This is how the built single-file
 *      artifact works from file:// with zero network requests.
 *   2. Otherwise (dev server / hosted multi-page build), fetch the
 *      committed sample corpus as an asset.
 */

import type { Artifact, AsDecidedExport, Relationship } from './types';

export async function loadExport(): Promise<AsDecidedExport> {
  const inline = document.getElementById('lore-export');
  const text = inline?.textContent?.trim();
  if (text) {
    return JSON.parse(text) as AsDecidedExport;
  }
  const url = new URL('./sample/lore-export.sample.json', import.meta.url);
  const res = await fetch(url);
  if (!res.ok) {
    throw new Error(`failed to load sample export (${res.status})`);
  }
  return (await res.json()) as AsDecidedExport;
}

/**
 * Preferred human-facing name for an artifact: the first alias that
 * differs from the opaque id, else the id itself. Deterministic —
 * alias order is as emitted by Core.
 */
export function displayName(artifact: Artifact): string {
  for (const alias of artifact.aliases ?? []) {
    if (alias !== artifact.id) return alias;
  }
  return artifact.id;
}

/** One artifact plus everything precomputed for list/search/detail. */
export interface IndexedArtifact {
  artifact: Artifact;
  /** Lowercased id + aliases + title + body text, for search. */
  haystack: string;
}

export interface CorpusIndex {
  data: AsDecidedExport;
  rows: IndexedArtifact[];
  byId: Map<string, Artifact>;
  /** Distinct artifact types, in first-seen order. */
  types: string[];
  /** Distinct statuses (first-seen casing), deduplicated
   *  case-insensitively. */
  statuses: string[];
  outbound: Map<string, Relationship[]>;
  inbound: Map<string, Relationship[]>;
  /** Lowercased id and alias tokens -> canonical artifact id, for
   *  cited-token linkification. */
  citationLookup: Map<string, string>;
}

const TAG_RE = /<[^>]*>/g;
const WS_RE = /\s+/g;

export function buildIndex(data: AsDecidedExport): CorpusIndex {
  const byId = new Map<string, Artifact>();
  const citationLookup = new Map<string, string>();
  const types: string[] = [];
  const statuses: string[] = [];
  const statusKeys = new Set<string>();
  const rows: IndexedArtifact[] = [];

  for (const artifact of data.artifacts) {
    const aliases = artifact.aliases ?? [];
    byId.set(artifact.id, artifact);
    citationLookup.set(artifact.id.toLowerCase(), artifact.id);
    for (const alias of aliases) {
      const key = alias.toLowerCase();
      if (!citationLookup.has(key)) citationLookup.set(key, artifact.id);
    }
    if (!types.includes(artifact.type)) types.push(artifact.type);
    const statusKey = artifact.status.toLowerCase();
    if (!statusKeys.has(statusKey)) {
      statusKeys.add(statusKey);
      statuses.push(artifact.status);
    }
    const bodyText = artifact.body_html.replace(TAG_RE, ' ').replace(WS_RE, ' ');
    rows.push({
      artifact,
      haystack: `${artifact.id} ${aliases.join(' ')} ${artifact.title} ${bodyText}`.toLowerCase(),
    });
  }

  const outbound = new Map<string, Relationship[]>();
  const inbound = new Map<string, Relationship[]>();
  for (const edge of data.relationships) {
    const out = outbound.get(edge.from);
    if (out) out.push(edge);
    else outbound.set(edge.from, [edge]);
    const inn = inbound.get(edge.to);
    if (inn) inn.push(edge);
    else inbound.set(edge.to, [edge]);
  }

  return {
    data,
    rows,
    byId,
    types,
    statuses,
    outbound,
    inbound,
    citationLookup,
  };
}

/**
 * Replace cited artifact tokens inside the rendered body with links to
 * their detail view. A token is a maximal run of word characters and
 * hyphens starting with a letter, bounded by non-word characters; it is
 * linkified only when its lowercase form is a known id or alias in the
 * corpus (so both "RAC-KTQ63DSC8SZW" and "ADR-027"/"adr-027" link, and
 * nothing else does). Walks text nodes only; text inside <a>, <code>
 * and <pre> is left alone.
 */
const TOKEN_RE = /(?<![\w-])[A-Za-z][\w-]+(?![\w-])/g;
const SKIP_TAGS = new Set(['A', 'CODE', 'PRE']);

export function linkifyCitations(
  root: HTMLElement,
  lookup: Map<string, string>,
): void {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      let el = node.parentElement;
      while (el && el !== root) {
        if (SKIP_TAGS.has(el.tagName)) return NodeFilter.FILTER_REJECT;
        el = el.parentElement;
      }
      return NodeFilter.FILTER_ACCEPT;
    },
  });

  const textNodes: Text[] = [];
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    textNodes.push(n as Text);
  }

  for (const textNode of textNodes) {
    const text = textNode.nodeValue ?? '';
    TOKEN_RE.lastIndex = 0;
    let match: RegExpExecArray | null;
    let last = 0;
    let frag: DocumentFragment | null = null;
    while ((match = TOKEN_RE.exec(text)) !== null) {
      const target = lookup.get(match[0].toLowerCase());
      if (!target) continue;
      frag ??= document.createDocumentFragment();
      if (match.index > last) {
        frag.appendChild(document.createTextNode(text.slice(last, match.index)));
      }
      const a = document.createElement('a');
      a.href = `#/artifact/${encodeURIComponent(target)}`;
      a.textContent = match[0];
      frag.appendChild(a);
      last = match.index + match[0].length;
    }
    if (frag) {
      if (last < text.length) {
        frag.appendChild(document.createTextNode(text.slice(last)));
      }
      textNode.replaceWith(frag);
    }
  }
}
