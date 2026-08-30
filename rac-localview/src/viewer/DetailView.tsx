import { useEffect, useRef } from 'react';
import { KeyboardHint, Panel } from '../components';
import type { CorpusIndex } from './data';
import {
  displayName,
  linkifyCitations,
  relationshipSourceKey,
  relationshipTargetKey,
} from './data';
import type { Relationship } from './types';
import { ArtifactChips } from './chips';

/** Human labels per edge type: [outbound, inbound]. */
const EDGE_LABELS: Record<string, [string, string]> = {
  supersedes: ['supersedes', 'superseded by'],
  refines: ['refines', 'refined by'],
  'relates-to': ['relates to', 'related to'],
  implements: ['implements', 'implemented by'],
};

function edgeLabel(type: string, direction: 'out' | 'in'): string {
  const labels = EDGE_LABELS[type];
  if (labels) return direction === 'out' ? labels[0] : labels[1];
  return direction === 'out' ? type : `${type} (inbound)`;
}

/**
 * Rendered body HTML. The export is trusted (escaped at export time —
 * see VIEWER_CONTRACT.md); the viewer renders it as-is, then linkifies
 * cited artifact ids and aliases that exist in the corpus.
 */
function ArtifactBody({
  html,
  lookup,
}: {
  html: string;
  lookup: Map<string, string>;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.innerHTML = html;
    linkifyCitations(el, lookup);
  }, [html, lookup]);
  return <div className="viewer-body" ref={ref} />;
}

interface RelatedGroupProps {
  heading: string;
  edges: Relationship[];
  index: CorpusIndex;
  direction: 'out' | 'in';
}

function RelatedGroup({ heading, edges, index, direction }: RelatedGroupProps) {
  return (
    <div className="viewer-related__group">
      <h3 className="viewer-related__heading">{heading}</h3>
      <ul className="viewer-related__list">
      {edges.map((edge) => {
        const otherId = direction === 'out' ? edge.to : edge.from;
        const otherKey =
          direction === 'out'
            ? relationshipTargetKey(edge)
            : relationshipSourceKey(edge);
        const other = index.byId.get(otherKey);
        return (
          <li key={`${relationshipSourceKey(edge)}-${edge.type}-${relationshipTargetKey(edge)}`}>
            <a href={`#/artifact/${encodeURIComponent(otherKey)}`}>
              {other ? displayName(other) : otherId}
            </a>{' '}
            <span className="viewer-related__title">
              {other ? other.title : '(not in corpus)'}
            </span>
          </li>
        );
      })}
      </ul>
    </div>
  );
}

export interface DetailViewProps {
  index: CorpusIndex;
  id: string;
}

export function DetailView({ index, id }: DetailViewProps) {
  const artifact = index.byId.get(id);

  if (!artifact) {
    return (
      <div className="viewer-detail">
        <p className="viewer-backline">
          <a href="#/">{'←'} all artifacts</a>
        </p>
        <p className="viewer-empty">
          no artifact with id <strong>{id}</strong> in this corpus
        </p>
      </div>
    );
  }

  // Group typed edges by edge type, both directions.
  const groups: { key: string; heading: string; edges: Relationship[]; direction: 'out' | 'in' }[] = [];
  const outbound = index.outbound.get(id) ?? [];
  const inbound = index.inbound.get(id) ?? [];
  const outTypes = [...new Set(outbound.map((e) => e.type))];
  const inTypes = [...new Set(inbound.map((e) => e.type))];
  for (const type of outTypes) {
    groups.push({
      key: `out-${type}`,
      heading: `${edgeLabel(type, 'out')} →`,
      edges: outbound.filter((e) => e.type === type),
      direction: 'out',
    });
  }
  for (const type of inTypes) {
    groups.push({
      key: `in-${type}`,
      heading: `← ${edgeLabel(type, 'in')}`,
      edges: inbound.filter((e) => e.type === type),
      direction: 'in',
    });
  }

  return (
    <div className="viewer-detail">
      <p className="viewer-backline">
        <a href="#/">{'←'} all artifacts</a>{' '}
        <span className="viewer-backline__hint">
          <KeyboardHint keys={['Esc']} /> back to list
        </span>
      </p>

      <header className="viewer-detail__head">
        <p className="viewer-detail__id">{displayName(artifact)}</p>
        <h2 className="viewer-detail__title">{artifact.title}</h2>
        <ArtifactChips artifact={artifact} />
        <p className="viewer-detail__provenance">
          {displayName(artifact) !== artifact.id ? (
            <>
              {artifact.id} {'·'}{' '}
            </>
          ) : null}
          {artifact.provenance ? <>{artifact.provenance.source} {'·'} </> : null}
          {artifact.path}
        </p>
      </header>

      <ArtifactBody html={artifact.body_html} lookup={index.citationLookup} />

      {groups.length > 0 ? (
        <Panel title="Related artifacts" className="viewer-related">
          {groups.map((group) => (
            <RelatedGroup
              key={group.key}
              heading={group.heading}
              edges={group.edges}
              index={index}
              direction={group.direction}
            />
          ))}
        </Panel>
      ) : null}
    </div>
  );
}
