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
