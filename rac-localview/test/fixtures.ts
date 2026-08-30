import type { AsDecidedExport } from '../src/viewer/types';

/**
 * A small but representative export: a hub decision, several families, a
 * retired (Superseded) decision, and one reference to an artifact that is not
 * in the corpus (an unresolved/dangling target).
 */
export const fixtureExport: AsDecidedExport = {
  schema_version: '1',
  corpus: { name: 'fixture', source: 'asdecided/test-fixture', rac_version: '0.0.0-test', artifact_count: 6 },
  artifacts: [
    { id: 'RAC-HUB000000001', aliases: ['adr-hub'], type: 'decision', status: 'Accepted', title: 'Hub decision', path: 'decisions/decisions/adr-hub.md', body_html: '<h1>Hub</h1><p>core decision</p>' },
    { id: 'RAC-REQ000000001', aliases: ['req-001'], type: 'requirement', status: 'Active', title: 'A requirement', path: 'decisions/requirements/req-001.md', body_html: '<p>req</p>' },
    { id: 'RAC-RMP000000001', aliases: ['v0.1.0'], type: 'roadmap', status: 'Planned', title: 'A roadmap', path: 'decisions/roadmaps/v0.1.0.md', body_html: '<p>roadmap</p>' },
    { id: 'RAC-OLD000000001', aliases: ['adr-old'], type: 'decision', status: 'Superseded', title: 'Retired decision', path: 'decisions/decisions/adr-old.md', body_html: '<p>old</p>' },
    { id: 'RAC-PRM000000001', aliases: ['prompt-x'], type: 'prompt', status: 'Active', title: 'A prompt', path: 'decisions/prompts/prompt-x.md', body_html: '<p>prompt</p>' },
    { id: 'RAC-DSN000000001', aliases: ['design-x'], type: 'design', status: 'Active', title: 'A design', path: 'decisions/designs/design-x.md', body_html: '<p>design</p>' },
  ],
  relationships: [
    { from: 'RAC-REQ000000001', to: 'RAC-HUB000000001', type: 'relates-to' },
    { from: 'RAC-RMP000000001', to: 'RAC-HUB000000001', type: 'relates-to' },
    { from: 'RAC-PRM000000001', to: 'RAC-HUB000000001', type: 'relates-to' },
    { from: 'RAC-DSN000000001', to: 'RAC-RMP000000001', type: 'relates-to' },
    { from: 'RAC-HUB000000001', to: 'RAC-OLD000000001', type: 'relates-to' },
    { from: 'RAC-RMP000000001', to: 'RAC-GHOST0000001', type: 'relates-to' }, // unresolved target
  ],
};

export const HUB_ID = 'RAC-HUB000000001';

export const SHARED_ID = 'RAC-SHARED000001';
export const LOCAL_SHARED_KEY = `acme/app::${SHARED_ID}`;
export const PARENT_SHARED_KEY = `acme/standards::${SHARED_ID}`;
const RATIONALE_ID = 'RAC-RATIONALE001';
const PIN = 'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef';

const override = {
  parent: { source: 'acme/standards', id: SHARED_ID },
  replacement: { source: 'acme/app', id: SHARED_ID },
  rationale: { source: 'acme/app', id: RATIONALE_ID },
} as const;

/** Same canonical id in two sources, authorised by an explicit override. */
export const federatedOverrideExport: AsDecidedExport = {
  schema_version: '1',
  corpus: {
    name: 'federated-fixture',
    source: 'acme/app',
    rac_version: '0.0.0-test',
    artifact_count: 3,
  },
  artifacts: [
    {
      id: SHARED_ID,
      aliases: ['local-policy'],
      type: 'decision',
      status: 'Accepted',
      title: 'Local replacement',
      path: 'replacement.md',
      body_html: '<p>local replacement</p>',
      provenance: {
        source: 'acme/app',
        layer: 'local',
        overrides: [{ state: 'replacement', ...override }],
      },
    },
    {
      id: RATIONALE_ID,
      aliases: ['override-rationale'],
      type: 'decision',
      status: 'Accepted',
      title: 'Override rationale',
      path: 'rationale.md',
      body_html: `<p>${SHARED_ID}</p>`,
      provenance: { source: 'acme/app', layer: 'local' },
    },
    {
      id: SHARED_ID,
      aliases: ['parent-policy'],
      type: 'decision',
      status: 'Accepted',
      title: 'Inherited parent policy',
      path: 'policy.md',
      body_html: '<p>parent history</p>',
      provenance: {
        source: 'acme/standards',
        layer: 'inherited',
        pin: PIN,
        overrides: [{ state: 'overridden', ...override }],
      },
    },
  ],
  relationships: [
    {
      from: SHARED_ID,
      to: RATIONALE_ID,
      type: 'relates-to',
      from_identity: { source: 'acme/app', id: SHARED_ID },
      to_identity: { source: 'acme/app', id: RATIONALE_ID },
      provenance: {
        source: 'acme/app',
        layer: 'local',
        overrides: [{ state: 'replacement', ...override }],
      },
    },
    {
      from: SHARED_ID,
      to: RATIONALE_ID,
      type: 'relates-to',
      from_identity: { source: 'acme/standards', id: SHARED_ID },
      to_identity: { source: 'acme/app', id: RATIONALE_ID },
      provenance: {
        source: 'acme/standards',
        layer: 'inherited',
        pin: PIN,
        overrides: [{ state: 'overridden', ...override }],
      },
    },
  ],
};

export const DIFFERENT_PARENT_ID = 'RAC-PARENT000001';
export const DIFFERENT_REPLACEMENT_ID = 'RAC-LOCAL0000001';
export const DIFFERENT_REPLACEMENT_KEY =
  `acme/app::${DIFFERENT_REPLACEMENT_ID}`;

const differentIdOverride = {
  parent: { source: 'acme/standards', id: DIFFERENT_PARENT_ID },
  replacement: { source: 'acme/app', id: DIFFERENT_REPLACEMENT_ID },
  rationale: { source: 'acme/app', id: RATIONALE_ID },
} as const;

/** A parent canonical id redirected to a differently-named local decision. */
export const federatedDifferentIdOverrideExport: AsDecidedExport = {
  schema_version: '1',
  corpus: {
    name: 'federated-different-id-fixture',
    source: 'acme/app',
    rac_version: '0.0.0-test',
    artifact_count: 3,
  },
  artifacts: [
    {
      id: DIFFERENT_REPLACEMENT_ID,
      aliases: ['local-exception'],
      type: 'decision',
      status: 'Accepted',
      title: 'Different-id replacement',
      path: 'replacement.md',
      body_html: '<p>local replacement</p>',
      provenance: {
        source: 'acme/app',
        layer: 'local',
        overrides: [{ state: 'replacement', ...differentIdOverride }],
      },
    },
    {
      id: RATIONALE_ID,
      aliases: ['override-rationale'],
      type: 'decision',
      status: 'Accepted',
      title: 'Override rationale',
      path: 'rationale.md',
      body_html: `<p>${DIFFERENT_PARENT_ID}</p>`,
      provenance: { source: 'acme/app', layer: 'local' },
    },
    {
      id: DIFFERENT_PARENT_ID,
      aliases: ['parent-policy-alias'],
      type: 'decision',
      status: 'Accepted',
      title: 'Inherited parent policy',
      path: 'policy.md',
      body_html: '<p>parent history</p>',
      provenance: {
        source: 'acme/standards',
        layer: 'inherited',
        pin: PIN,
        overrides: [{ state: 'overridden', ...differentIdOverride }],
      },
    },
  ],
  relationships: [],
};
