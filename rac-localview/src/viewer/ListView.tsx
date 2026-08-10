import { memo, useEffect, useMemo, useState } from 'react';
import type { RefObject } from 'react';
import { KeyboardHint } from '../components';
import type { CorpusIndex } from './data';
import { artifactKey, displayName } from './data';
import type { Artifact } from './types';
import { ArtifactChips } from './chips';

export interface ListFilters {
  query: string;
  type: string; // '' = all
  status: string; // '' = all
}

export interface ListViewProps {
  index: CorpusIndex;
  filters: ListFilters;
  onFilters: (next: ListFilters) => void;
  searchRef: RefObject<HTMLInputElement>;
}

/** Debounce a value so filtering does not run on every keystroke. */
function useDebounced<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const t = window.setTimeout(() => setDebounced(value), ms);
    return () => window.clearTimeout(t);
  }, [value, ms]);
  return debounced;
}

/**
 * Memoised row: with stable artifact references, rows only re-render
 * when the filtered set changes, never per keystroke.
 */
const Row = memo(function Row({ artifact }: { artifact: Artifact }) {
  return (
    <li className="viewer-rowitem">
      <a
        className="viewer-row"
        href={`#/artifact/${encodeURIComponent(artifactKey(artifact))}`}
      >
        <span className="viewer-row__id">{displayName(artifact)}</span>
        <span className="viewer-row__title">{artifact.title}</span>
        <ArtifactChips artifact={artifact} />
      </a>
    </li>
  );
});

interface ToggleGroupProps {
  label: string;
  options: string[];
  value: string;
  onChange: (value: string) => void;
}

function ToggleGroup({ label, options, value, onChange }: ToggleGroupProps) {
  return (
    <div className="viewer-filter" role="group" aria-label={label}>
      <span className="viewer-filter__label">{label}</span>
      <button
        type="button"
        className="viewer-toggle"
        aria-pressed={value === ''}
        onClick={() => onChange('')}
      >
        all
      </button>
      {options.map((option) => (
        <button
          key={option}
          type="button"
          className="viewer-toggle"
          aria-pressed={value === option}
          onClick={() => onChange(value === option ? '' : option)}
        >
          {option}
        </button>
      ))}
    </div>
  );
}

export function ListView({ index, filters, onFilters, searchRef }: ListViewProps) {
  const debouncedQuery = useDebounced(filters.query, 150);

  // O(n) scan over precomputed lowercase haystacks, once per debounced
  // change — not per keystroke and not per row render.
  const visible = useMemo(() => {
    const q = debouncedQuery.trim().toLowerCase();
    // Statuses arrive in arbitrary case ("Accepted"); group them
    // case-insensitively while displaying the authored casing.
    const statusKey = filters.status.toLowerCase();
    const out: Artifact[] = [];
    for (const row of index.rows) {
      const a = row.artifact;
      if (filters.type && a.type !== filters.type) continue;
      if (statusKey && a.status.toLowerCase() !== statusKey) continue;
      if (q && !row.haystack.includes(q)) continue;
      out.push(a);
    }
    return out;
  }, [index, debouncedQuery, filters.type, filters.status]);

  return (
    <div className="viewer-list">
      <div className="viewer-search">
        <label className="viewer-search__bar">
          <span className="viewer-search__prefix" aria-hidden="true">
            lore-$
          </span>
          <span className="sr-only">Search artifacts by id, title or body</span>
          <input
            ref={searchRef}
            className="viewer-search__input"
            type="search"
            placeholder="search id, title, body…"
            value={filters.query}
            onChange={(e) => onFilters({ ...filters, query: e.target.value })}
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        <span className="viewer-search__hint">
          <KeyboardHint keys={['/']} /> to search
        </span>
      </div>

      <div className="viewer-filters">
        <ToggleGroup
          label="Filter by type"
          options={index.types}
          value={filters.type}
          onChange={(type) => onFilters({ ...filters, type })}
        />
        <ToggleGroup
          label="Filter by status"
          options={index.statuses}
          value={filters.status}
          onChange={(status) => onFilters({ ...filters, status })}
        />
      </div>

      <p className="viewer-count" aria-live="polite">
        {visible.length} of {index.rows.length} artifacts
      </p>

      {visible.length > 0 ? (
        <ul className="viewer-rows">
          {visible.map((artifact) => (
            <Row key={artifactKey(artifact)} artifact={artifact} />
          ))}
        </ul>
      ) : (
        <p className="viewer-empty">no artifacts match the current filters</p>
      )}
    </div>
  );
}
