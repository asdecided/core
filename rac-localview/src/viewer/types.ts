/**
 * Types for the `decided export` JSON payload the viewer consumes.
 * Reconciled v1 — see rac-localview/VIEWER_CONTRACT.md.
 */

export interface CorpusMeta {
  /** Human-readable corpus name, e.g. the exported directory name. */
  name: string;
  /** Stable corpus provenance identity; absent only in older payloads. */
  source?: string;
  /** Retained v1 key containing the AsDecided CLI version. */
  rac_version?: string;
  /** Number of artifacts in the export. */
  artifact_count?: number;
  /** True when the corpus is demonstration data, not a real repo. */
  sample?: boolean;
}

export interface SourceIdentity {
  source: string;
  id: string;
}

export interface OverrideProvenance {
  state: 'overridden' | 'replacement';
  parent: SourceIdentity;
  replacement: SourceIdentity;
  rationale: SourceIdentity;
}

export interface ArtifactProvenance {
  source: string;
  layer: 'local' | 'inherited';
  pin?: string;
  overrides?: OverrideProvenance[];
}

export interface Artifact {
  /** Opaque stable artifact ID, unique within its owning source. */
  id: string;
  /** Human aliases, e.g. ["adr-027", "adr-027-ci-test-topology"]. */
  aliases: string[];
  /** Artifact family, e.g. "decision", "requirement". Open set. */
  type: string;
  /** Lifecycle status as authored, e.g. "Accepted". Open set; the
   *  viewer groups and colours it case-insensitively. */
  status: string;
  title: string;
  /** Source path within the repository, e.g. "decisions/decisions/adr-027.md". */
  path: string;
  /** Body rendered to HTML at export time. Trusted — see contract. */
  body_html: string;
  /** Present for manifest-backed exports; owns global record identity. */
  provenance?: ArtifactProvenance;
}

export interface Relationship {
  /** Source artifact ID; the edge reads "<from> <type> <to>". */
  from: string;
  /** Target artifact ID, or an unresolved alias kept verbatim. */
  to: string;
  /** Edge type. Core emits only "relates-to"; the set stays open. */
  type: string;
  /** Source-aware endpoints on manifest-backed exports. */
  from_identity?: SourceIdentity;
  to_identity?: SourceIdentity | null;
  /** Provenance of the artifact which declared this edge. */
  provenance?: ArtifactProvenance;
}

export interface AsDecidedExport {
  /** Schema version, a string: "1". */
  schema_version: string;
  corpus: CorpusMeta;
  artifacts: Artifact[];
  relationships: Relationship[];
}
