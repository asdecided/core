import { describe, it, expect } from 'vitest';
import { buildGraph, layout, bounds, neighbourhood, nodeRadius } from '../src/viewer/graph';
import { fixtureExport, HUB_ID } from './fixtures';

const W = 1000;
const H = 720;

describe('corpus graph model', () => {
  it('builds nodes, degrees, and a dangling node for an unresolved target', () => {
    const g = buildGraph(fixtureExport);
    // 6 real artifacts + 1 unresolved/dangling target
    expect(g.nodes).toHaveLength(7);
    const dangling = g.nodes.filter((n) => n.unresolved);
    expect(dangling).toHaveLength(1);
    expect(dangling[0].id).toBe('RAC-GHOST0000001');
    // the hub is referenced by three artifacts and points at one — degree 4
    expect(g.byId.get(HUB_ID)!.degree).toBe(4);
  });

  it('neighbourhood BFS respects depth', () => {
    const g = buildGraph(fixtureExport);
    const d1 = neighbourhood(g.adjacency, HUB_ID, 1);
    const d2 = neighbourhood(g.adjacency, HUB_ID, 2);
    expect(d1.has(HUB_ID)).toBe(true);
    expect(d2.size).toBeGreaterThanOrEqual(d1.size);
  });

  it('node radius grows with degree', () => {
    expect(nodeRadius(8)).toBeGreaterThan(nodeRadius(0));
  });
});

describe('graph layout', () => {
  // Regression guard for the v0.21.8 bug: the force layout had no frame
  // containment, so nodes drifted far outside W×H and fit-to-view collapsed the
  // whole graph to ~15% scale (an unreadable blob). Assert containment + spread.
  it('keeps every node inside the logical frame', () => {
    const g = buildGraph(fixtureExport);
    layout(g.nodes, g.edges, { width: W, height: H });
    for (const n of g.nodes) {
      expect(Number.isFinite(n.x) && Number.isFinite(n.y)).toBe(true);
      expect(n.x).toBeGreaterThanOrEqual(0);
      expect(n.x).toBeLessThanOrEqual(W);
      expect(n.y).toBeGreaterThanOrEqual(0);
      expect(n.y).toBeLessThanOrEqual(H);
    }
  });

  it('spreads across the frame without exceeding it', () => {
    const g = buildGraph(fixtureExport);
    layout(g.nodes, g.edges, { width: W, height: H });
    const b = bounds(g.nodes);
    // not collapsed to a point…
    expect((b.maxX - b.minX) / W).toBeGreaterThan(0.3);
    expect((b.maxY - b.minY) / H).toBeGreaterThan(0.3);
    // …and not exploded outside the frame (the v0.21.8 failure mode)
    expect(b.minX).toBeGreaterThanOrEqual(0);
    expect(b.maxX).toBeLessThanOrEqual(W);
    expect(b.minY).toBeGreaterThanOrEqual(0);
    expect(b.maxY).toBeLessThanOrEqual(H);
  });

  it('is deterministic — the same corpus yields the same layout', () => {
    const snapshot = () => {
      const g = buildGraph(fixtureExport);
      layout(g.nodes, g.edges, { width: W, height: H });
      return g.nodes.map((n) => `${n.id}:${n.x.toFixed(3)},${n.y.toFixed(3)}`).join('|');
    };
    expect(snapshot()).toBe(snapshot());
  });
});

import type { AsDecidedExport } from '../src/viewer/types';

describe('force controls', () => {
  const snapshot = (opts: Partial<{ repel: number; linkForce: number; linkDistance: number; center: number }>) => {
    const g = buildGraph(fixtureExport);
    layout(g.nodes, g.edges, { width: W, height: H, ...opts });
    return g.nodes.map((n) => `${n.x.toFixed(2)},${n.y.toFixed(2)}`).join('|');
  };

  it('is deterministic for a given set of forces', () => {
    expect(snapshot({ repel: 1.8 })).toBe(snapshot({ repel: 1.8 }));
  });

  it('different forces produce a different layout', () => {
    expect(snapshot({ repel: 0.3 })).not.toBe(snapshot({ repel: 3 }));
  });

  it('stays inside the frame at extreme forces', () => {
    const g = buildGraph(fixtureExport);
    layout(g.nodes, g.edges, { width: W, height: H, repel: 3, linkForce: 3, linkDistance: 2.5 });
    for (const n of g.nodes) {
      expect(n.x).toBeGreaterThanOrEqual(0);
      expect(n.x).toBeLessThanOrEqual(W);
      expect(n.y).toBeGreaterThanOrEqual(0);
      expect(n.y).toBeLessThanOrEqual(H);
    }
  });
});

describe('orphan layout', () => {
  const withOrphans: AsDecidedExport = {
    schema_version: '1',
    corpus: { name: 'orphans' },
    artifacts: [
      { id: 'A', aliases: ['a'], type: 'decision', status: 'Accepted', title: 'A', path: 'a.md', body_html: '' },
      { id: 'B', aliases: ['b'], type: 'decision', status: 'Accepted', title: 'B', path: 'b.md', body_html: '' },
      { id: 'O1', aliases: ['o1'], type: 'requirement', status: 'Active', title: 'O1', path: 'o1.md', body_html: '' },
      { id: 'O2', aliases: ['o2'], type: 'requirement', status: 'Active', title: 'O2', path: 'o2.md', body_html: '' },
      { id: 'O3', aliases: ['o3'], type: 'requirement', status: 'Active', title: 'O3', path: 'o3.md', body_html: '' },
    ],
    relationships: [{ from: 'A', to: 'B', type: 'relates-to' }],
  };

  it('places orphans inside the frame, spread out (not piled in one spot)', () => {
    const g = buildGraph(withOrphans);
    layout(g.nodes, g.edges, { width: W, height: H });
    const orphans = g.nodes.filter((n) => n.degree === 0);
    expect(orphans).toHaveLength(3);
    for (const o of orphans) {
      expect(o.x).toBeGreaterThanOrEqual(0);
      expect(o.x).toBeLessThanOrEqual(W);
      expect(o.y).toBeGreaterThanOrEqual(0);
      expect(o.y).toBeLessThanOrEqual(H);
    }
    // spread horizontally rather than collapsed onto one column
    const xs = new Set(orphans.map((o) => Math.round(o.x)));
    expect(xs.size).toBeGreaterThan(1);
  });
});
