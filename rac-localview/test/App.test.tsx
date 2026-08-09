import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { act } from 'react';
import { createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { App } from '../src/viewer/App';
import { fixtureExport, HUB_ID } from './fixtures';

// Mount the real viewer in jsdom against an injected export, at each route.
// A smoke net: the v0.21.8 graph bug shipped because nothing mounted the viewer.

let container: HTMLElement;
let root: Root | undefined;

beforeEach(() => {
  const seam = document.createElement('script');
  seam.type = 'application/json';
  seam.id = 'lore-export';
  seam.textContent = JSON.stringify(fixtureExport);
  document.body.appendChild(seam);
  container = document.createElement('div');
  document.body.appendChild(container);
});

afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  document.body.innerHTML = '';
  window.location.hash = '';
});

async function mountAt(hash: string): Promise<void> {
  window.location.hash = hash;
  await act(async () => {
    root = createRoot(container);
    root.render(createElement(App));
    // let loadExport()'s microtask + effects settle
    await new Promise((r) => setTimeout(r, 60));
  });
}

describe('viewer App', () => {
  it('renders the list view without throwing', async () => {
    await mountAt('#/');
    expect(container.querySelector('.viewer')).toBeTruthy();
    expect(container.querySelectorAll('.viewer-row').length).toBeGreaterThan(0);
    expect(container.querySelector('.viewer-meta__line')?.textContent).toContain(
      'AsDecided 0.0.0-test',
    );
  });

  it('renders the graph view with nodes and edges', async () => {
    await mountAt('#/graph');
    expect(container.querySelector('.graph-canvas')).toBeTruthy();
    expect(container.querySelectorAll('.graph-node__dot').length).toBeGreaterThan(0);
    expect(container.querySelectorAll('.graph-edge').length).toBeGreaterThan(0);
  });

  it('renders a detail view for an artifact', async () => {
    await mountAt(`#/artifact/${encodeURIComponent(HUB_ID)}`);
    expect(container.querySelector('.viewer-detail')).toBeTruthy();
    expect(container.textContent).toContain('Hub decision');
  });
});
