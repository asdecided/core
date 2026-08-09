import { useEffect, useMemo, useRef, useState } from 'react';
import { KeyboardHint, TerminalFrame } from '../components';
import { buildIndex, loadExport } from './data';
import type { CorpusIndex } from './data';
import type { AsDecidedExport } from './types';
import { ListView } from './ListView';
import type { ListFilters } from './ListView';
import { DetailView } from './DetailView';
import { GraphView } from './GraphView';
import { hasHost, onRevealArtifact, postOpenArtifact, postReady } from './host';
import './viewer.css';

/** Hash-based routing so links work from file:// — no router dep. */
type Route = { view: 'list' } | { view: 'graph' } | { view: 'detail'; id: string };

function parseHash(hash: string): Route {
  const match = /^#\/artifact\/(.+)$/.exec(hash);
  if (match) return { view: 'detail', id: decodeURIComponent(match[1]) };
  if (hash === '#/graph') return { view: 'graph' };
  return { view: 'list' };
}

function useRoute(): Route {
  const [route, setRoute] = useState<Route>(() =>
    parseHash(window.location.hash),
  );
  useEffect(() => {
    const onHashChange = () => setRoute(parseHash(window.location.hash));
    window.addEventListener('hashchange', onHashChange);
    return () => window.removeEventListener('hashchange', onHashChange);
  }, []);
  return route;
}

export function App() {
  const route = useRoute();
  const [data, setData] = useState<AsDecidedExport | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Filters live here so they survive list -> detail -> list.
  const [filters, setFilters] = useState<ListFilters>({
    query: '',
    type: '',
    status: '',
  });
  const searchRef = useRef<HTMLInputElement>(null);
  // Set when a reveal from the host navigates us, so the navigation it causes
  // is not echoed straight back as an open-artifact (which would re-open the
  // file the editor is already on). Consumed by the outbound effect below.
  const revealedRef = useRef<string | null>(null);
  // The host/editor's active artifact — roots the local graph and is
  // highlighted in the graph view.
  const [activeId, setActiveId] = useState<string | null>(null);
  // Mirror the current view so the once-bound reveal handler can branch on it.
  const viewRef = useRef(route.view);
  viewRef.current = route.view;

  useEffect(() => {
    loadExport().then(setData, (err: unknown) => {
      setError(err instanceof Error ? err.message : String(err));
    });
  }, []);

  const index: CorpusIndex | null = useMemo(
    () => (data ? buildIndex(data) : null),
    [data],
  );

  // Editor-host bridge (v0.21.7): announce readiness and apply the host's
  // reveal requests. Inert in a standalone Portal (no host).
  useEffect(() => {
    const unsubscribe = onRevealArtifact((id) => {
      setActiveId(id);
      // In the graph view a reveal just roots/highlights the node; it does not
      // navigate away. Elsewhere it opens the detail page, as before.
      if (viewRef.current === 'graph') return;
      const target = `#/artifact/${encodeURIComponent(id)}`;
      if (window.location.hash === target) {
        revealedRef.current = null; // already here — nothing to suppress
        return;
      }
      revealedRef.current = id;
      window.location.hash = target;
    });
    postReady();
    return unsubscribe;
  }, []);

  // Graph-view selection: open the file in a host (and root the local graph);
  // double-click opens the detail page.
  const selectInGraph = (id: string) => {
    setActiveId(id);
    const artifact = index?.byId.get(id);
    if (artifact && hasHost()) postOpenArtifact(artifact.path, artifact.id);
  };
  const openDetail = (id: string) => {
    window.location.hash = `#/artifact/${encodeURIComponent(id)}`;
  };

  // Report the user's selection to the host so it can open the file. A reveal
  // the host itself requested is consumed here rather than echoed back.
  useEffect(() => {
    if (route.view !== 'detail') return;
    const artifact = index?.byId.get(route.id);
    if (!artifact) return;
    if (revealedRef.current === route.id) {
      revealedRef.current = null;
      return;
    }
    postOpenArtifact(artifact.path, artifact.id);
  }, [route, index]);

  // Keyboard: "/" focuses search on the list view; Escape returns from
  // detail to list. Both ignored while typing in a form control.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const typing =
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement;
      if (event.key === '/' && !typing && route.view === 'list') {
        event.preventDefault();
        searchRef.current?.focus();
      } else if (event.key === 'Escape' && route.view === 'detail') {
        window.location.hash = '#/';
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [route]);

  const corpus = data?.corpus;

  return (
    <main className="viewer">
      <TerminalFrame title="Lore — export viewer">
        {corpus ? (
          <header className="viewer-meta">
            <p className="viewer-meta__name">
              {corpus.name}
              {corpus.sample ? (
                <span className="viewer-chip viewer-chip--sample">
                  SAMPLE DATA
                </span>
              ) : null}
            </p>
            <p className="viewer-meta__line">
              {corpus.rac_version ? (
                <>
                  AsDecided {corpus.rac_version} {'·'}{' '}
                </>
              ) : null}
              {corpus.artifact_count ?? data?.artifacts.length ?? 0} artifacts{' '}
              {'·'} read-only
              {route.view !== 'graph' ? (
                <>
                  {' '}
                  {'·'} <a className="viewer-metalink" href="#/graph">graph view →</a>
                </>
              ) : null}
            </p>
          </header>
        ) : null}

        {error ? (
          <p className="viewer-error" role="alert">
            could not load the export: {error}
          </p>
        ) : null}

        {!error && !index ? <p className="viewer-empty">loading corpus…</p> : null}

        {index && route.view === 'list' ? (
          <ListView
            index={index}
            filters={filters}
            onFilters={setFilters}
            searchRef={searchRef}
          />
        ) : null}

        {index && route.view === 'graph' ? (
          <GraphView
            index={index}
            activeId={activeId}
            onSelect={selectInGraph}
            onOpenDetail={openDetail}
          />
        ) : null}

        {index && route.view === 'detail' ? (
          <DetailView index={index} id={route.id} />
        ) : null}
      </TerminalFrame>

      <footer className="viewer-foot">
        <span>
          <KeyboardHint keys={['/']} /> search {'·'}{' '}
          <KeyboardHint keys={['Tab']} /> move through rows {'·'}{' '}
          <KeyboardHint keys={['Esc']} /> back to list
        </span>
        {corpus?.sample ? (
          <span className="viewer-foot__sample">
            sample data — fictional corpus for demonstration
          </span>
        ) : null}
      </footer>
    </main>
  );
}
