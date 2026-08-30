/**
 * Graph model and layout for the corpus graph view (v0.21.8).
 *
 * Pure, dependency-free, and deterministic: the same export always produces the
 * same node set, edges, degrees, and — given a fixed seed and viewport — the
 * same coordinates. Positions live only here and in the component; they are
 * never written back to the export (ADR-007 determinism is unaffected).
 */

import type { Artifact, AsDecidedExport } from './types';
import {
  artifactKey,
  displayName,
  relationshipSourceKey,
  relationshipTargetKey,
} from './data';

export interface GraphNode {
  id: string;
  /** Human display name (alias or id). */
  label: string;
  /** Artifact family, or `''` for an unresolved (dangling) target. */
  type: string;
  status: string;
  title: string;
  /** Number of edges touching the node (in + out, resolved + dangling). */
  degree: number;
  /** True for a referenced target that is not itself in the corpus. */
  unresolved: boolean;
  /** Lowercased id + label + title, for the node search filter. */
  haystack: string;
  x: number;
  y: number;
}

export interface GraphEdge {
  from: string;
  to: string;
  /** True when `to` is not a real artifact (a dangling reference). */
  unresolved: boolean;
}

export interface Graph {
  nodes: GraphNode[];
  edges: GraphEdge[];
  byId: Map<string, GraphNode>;
  /** Undirected adjacency, for neighbour highlighting and local-graph BFS. */
  adjacency: Map<string, Set<string>>;
}

const RETIRED = new Set(['superseded', 'deprecated', 'abandoned']);

export function isRetired(status: string): boolean {
  return RETIRED.has(status.toLowerCase());
}

/** Build the graph model from an export payload — nodes, edges, degrees. */
export function buildGraph(data: AsDecidedExport): Graph {
  const byId = new Map<string, GraphNode>();
  const adjacency = new Map<string, Set<string>>();

  const ensure = (id: string, seed?: Artifact): GraphNode => {
    let node = byId.get(id);
    if (!node) {
      node = seed
        ? {
            id,
            label: displayName(seed),
            type: seed.type,
            status: seed.status,
            title: seed.title,
            degree: 0,
            unresolved: false,
            haystack: '',
            x: 0,
            y: 0,
          }
        : {
            id,
            label: id,
            type: '',
            status: '',
            title: '(not in corpus)',
            degree: 0,
            unresolved: true,
            haystack: '',
            x: 0,
            y: 0,
          };
      node.haystack = `${id} ${node.label} ${node.title}`.toLowerCase();
      byId.set(id, node);
      adjacency.set(id, new Set());
    }
    return node;
  };

  for (const artifact of data.artifacts) ensure(artifactKey(artifact), artifact);

  const edges: GraphEdge[] = [];
  for (const rel of data.relationships) {
    const fromId = relationshipSourceKey(rel);
    const toId = relationshipTargetKey(rel);
    const from = byId.get(fromId);
    if (!from) continue; // a from-id outside the corpus cannot be placed
    const target = ensure(toId); // creates a dangling node when unresolved
    const edge: GraphEdge = { from: fromId, to: toId, unresolved: target.unresolved };
    edges.push(edge);
    from.degree += 1;
    target.degree += 1;
    adjacency.get(fromId)!.add(toId);
    adjacency.get(toId)!.add(fromId);
  }

  return { nodes: [...byId.values()], edges, byId, adjacency };
}

/** Ids within `depth` hops of `rootId` (inclusive), over the undirected graph. */
export function neighbourhood(
  adjacency: Map<string, Set<string>>,
  rootId: string,
  depth: number,
): Set<string> {
  const seen = new Set<string>([rootId]);
  let frontier = [rootId];
  for (let d = 0; d < depth; d++) {
    const next: string[] = [];
    for (const id of frontier) {
      for (const nb of adjacency.get(id) ?? []) {
        if (!seen.has(nb)) {
          seen.add(nb);
          next.push(nb);
        }
      }
    }
    frontier = next;
    if (frontier.length === 0) break;
  }
  return seen;
}

export interface LayoutOptions {
  width: number;
  height: number;
  iterations?: number;
  /** Repulsion strength multiplier (default 1). */
  repel?: number;
  /** Edge attraction strength multiplier (default 1). */
  linkForce?: number;
  /** Edge-length multiplier on the ideal separation `sqrt(area / n)` (default 1). */
  linkDistance?: number;
  /** Centering gravity strength (default 0.02). */
  center?: number;
}

/** Default force parameters — these reproduce the unparameterised layout. */
export const DEFAULT_FORCES = {
  repel: 1,
  linkForce: 1,
  linkDistance: 1,
  center: 0.02,
} as const;

/**
 * A seeded Fruchterman–Reingold force layout, mutating node `x`/`y` in place.
 * Deterministic: initial positions are a golden-angle spiral by node order
 * (the export is path-ordered), and the simulation uses no randomness. Force
 * strengths are tunable, with defaults that reproduce the original layout.
 * Orphans (degree 0) are excluded from the simulation and arranged in a tidy
 * grid along the bottom band, so they no longer pile against the frame edge.
 */
export function layout(nodes: GraphNode[], edges: GraphEdge[], opts: LayoutOptions): void {
  const { width, height } = opts;
  const margin = Math.min(width, height) * 0.04;
  const connected = nodes.filter((node) => node.degree > 0);
  const orphans = nodes.filter((node) => node.degree === 0);
  layoutConnected(connected, edges, opts, margin);
  layoutOrphans(orphans, width, height, margin);
}

function layoutConnected(
  nodes: GraphNode[],
  edges: GraphEdge[],
  opts: LayoutOptions,
  margin: number,
): void {
  const n = nodes.length;
  if (n === 0) return;
  const { width, height, iterations } = opts;
  const repel = opts.repel ?? DEFAULT_FORCES.repel;
  const linkForce = opts.linkForce ?? DEFAULT_FORCES.linkForce;
  const center = opts.center ?? DEFAULT_FORCES.center;
  const cx = width / 2;
  const cy = height / 2;
  const k = Math.sqrt((width * height) / n) * (opts.linkDistance ?? 1); // ideal separation
  const golden = Math.PI * (3 - Math.sqrt(5));
  const radius = Math.min(width, height) * 0.46;
  nodes.forEach((node, i) => {
    const r = (Math.sqrt(i + 0.5) / Math.sqrt(n)) * radius;
    const a = i * golden;
    node.x = cx + r * Math.cos(a);
    node.y = cy + r * Math.sin(a);
  });
  if (n === 1) return;

  const index = new Map(nodes.map((node, i) => [node.id, i]));
  const iters = iterations ?? Math.round(Math.max(90, Math.min(320, 5200 / Math.sqrt(n))));
  const disp = nodes.map(() => ({ x: 0, y: 0 }));
  let temp = Math.min(width, height) * 0.12;
  const cool = temp / (iters + 1);

  for (let it = 0; it < iters; it++) {
    for (let i = 0; i < n; i++) {
      disp[i].x = 0;
      disp[i].y = 0;
    }
    // repulsion between every pair
    for (let i = 0; i < n; i++) {
      for (let j = i + 1; j < n; j++) {
        let dx = nodes[i].x - nodes[j].x;
        let dy = nodes[i].y - nodes[j].y;
        let d2 = dx * dx + dy * dy;
        if (d2 < 0.01) {
          dx = (i - j) % 7 || 1;
          dy = (i + j) % 5 || 1;
          d2 = dx * dx + dy * dy;
        }
        const d = Math.sqrt(d2);
        const f = ((k * k) / d) * repel;
        const ux = dx / d;
        const uy = dy / d;
        disp[i].x += ux * f;
        disp[i].y += uy * f;
        disp[j].x -= ux * f;
        disp[j].y -= uy * f;
      }
    }
    // attraction along edges
    for (const edge of edges) {
      const a = index.get(edge.from);
      const b = index.get(edge.to);
      if (a === undefined || b === undefined) continue;
      const dx = nodes[a].x - nodes[b].x;
      const dy = nodes[a].y - nodes[b].y;
      const d = Math.sqrt(dx * dx + dy * dy) || 0.01;
      const f = ((d * d) / k) * linkForce;
      const ux = dx / d;
      const uy = dy / d;
      disp[a].x -= ux * f;
      disp[a].y -= uy * f;
      disp[b].x += ux * f;
      disp[b].y += uy * f;
    }
    // gentle gravity keeps the cluster centred
    for (let i = 0; i < n; i++) {
      disp[i].x += (cx - nodes[i].x) * center;
      disp[i].y += (cy - nodes[i].y) * center;
    }
    // integrate, capped by the cooling temperature, and keep nodes inside the
    // frame (classic FR containment) so the graph stays compact and the
    // fit-to-view scale is sensible rather than zoomed far out.
    for (let i = 0; i < n; i++) {
      const len = Math.sqrt(disp[i].x * disp[i].x + disp[i].y * disp[i].y) || 0.01;
      const m = Math.min(len, temp);
      nodes[i].x += (disp[i].x / len) * m;
      nodes[i].y += (disp[i].y / len) * m;
      nodes[i].x = Math.min(width - margin, Math.max(margin, nodes[i].x));
      nodes[i].y = Math.min(height - margin, Math.max(margin, nodes[i].y));
    }
    temp = Math.max(temp - cool, 0.01);
  }
}

/** Lay out orphans (no edges) in a tidy grid along the bottom band. */
function layoutOrphans(orphans: GraphNode[], width: number, height: number, margin: number): void {
  const n = orphans.length;
  if (n === 0) return;
  const cols = Math.max(1, Math.min(n, Math.ceil(Math.sqrt(n) * 1.6)));
  const cellW = (width - 2 * margin) / cols;
  const cellH = Math.min(cellW, height * 0.08);
  orphans.forEach((node, i) => {
    const col = i % cols;
    const row = Math.floor(i / cols);
    node.x = margin + cellW * (col + 0.5);
    node.y = Math.max(margin, height - margin - cellH * (row + 0.5));
  });
}

/** Node radius from degree — hubs read bigger. */
export function nodeRadius(degree: number): number {
  return 5 + Math.min(Math.sqrt(degree) * 2.6, 16);
}

/** CSS custom-property name for an artifact family's node colour. */
export function typeColorVar(type: string): string {
  switch (type) {
    case 'requirement':
      return 'var(--type-requirement)';
    case 'decision':
      return 'var(--type-decision)';
    case 'roadmap':
      return 'var(--type-roadmap)';
    case 'prompt':
      return 'var(--type-prompt)';
    case 'design':
      return 'var(--type-design)';
    default:
      return 'var(--type-default)';
  }
}

export const GRAPH_TYPES = ['requirement', 'decision', 'roadmap', 'prompt', 'design'] as const;

/** Axis-aligned bounding box of a set of points, for fit-to-view. */
export function bounds(
  nodes: { x: number; y: number }[],
): { minX: number; minY: number; maxX: number; maxY: number } {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const node of nodes) {
    if (node.x < minX) minX = node.x;
    if (node.y < minY) minY = node.y;
    if (node.x > maxX) maxX = node.x;
    if (node.y > maxY) maxY = node.y;
  }
  if (!Number.isFinite(minX)) return { minX: 0, minY: 0, maxX: 1, maxY: 1 };
  return { minX, minY, maxX, maxY };
}
