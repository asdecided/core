import { describe, expect, it } from 'vitest';
import { buildIndex } from '../src/viewer/data';
import { buildGraph } from '../src/viewer/graph';
import {
  DIFFERENT_PARENT_ID,
  DIFFERENT_REPLACEMENT_KEY,
  federatedDifferentIdOverrideExport,
  federatedOverrideExport,
  fixtureExport,
  HUB_ID,
  LOCAL_SHARED_KEY,
  PARENT_SHARED_KEY,
  SHARED_ID,
} from './fixtures';

describe('federated viewer identity', () => {
  it('retains both sides of a same-id override under source-aware keys', () => {
    const index = buildIndex(federatedOverrideExport);

    expect(index.rows).toHaveLength(3);
    expect(index.byId.size).toBe(3);
    expect(index.byId.get(LOCAL_SHARED_KEY)?.title).toBe('Local replacement');
    expect(index.byId.get(PARENT_SHARED_KEY)?.title).toBe(
      'Inherited parent policy',
    );
    expect(index.citationLookup.get(SHARED_ID.toLowerCase())).toBe(
      LOCAL_SHARED_KEY,
    );
    expect(index.outbound.get(LOCAL_SHARED_KEY)).toHaveLength(1);
    expect(index.outbound.get(PARENT_SHARED_KEY)).toHaveLength(1);
  });

  it('builds distinct graph nodes and source-aware edges', () => {
    const graph = buildGraph(federatedOverrideExport);

    expect(graph.nodes).toHaveLength(3);
    expect(graph.byId.get(LOCAL_SHARED_KEY)?.title).toBe('Local replacement');
    expect(graph.byId.get(PARENT_SHARED_KEY)?.title).toBe(
      'Inherited parent policy',
    );
    expect(graph.edges.filter((edge) => edge.from === LOCAL_SHARED_KEY)).toHaveLength(1);
    expect(graph.edges.filter((edge) => edge.from === PARENT_SHARED_KEY)).toHaveLength(1);
  });

  it('redirects an overridden parent canonical id to a different-id replacement', () => {
    const index = buildIndex(federatedDifferentIdOverrideExport);

    expect(index.citationLookup.get(DIFFERENT_PARENT_ID.toLowerCase())).toBe(
      DIFFERENT_REPLACEMENT_KEY,
    );
    expect(index.citationLookup.has('parent-policy-alias')).toBe(false);
  });

  it('keeps bare-id keys exact for a legacy payload', () => {
    const index = buildIndex(fixtureExport);
    expect(index.byId.get(HUB_ID)?.title).toBe('Hub decision');

    const graph = buildGraph(fixtureExport);
    expect(graph.byId.get(HUB_ID)?.title).toBe('Hub decision');
  });
});
