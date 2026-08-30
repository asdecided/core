//! Corpus export (`decided.services.export`) — deterministic viewer/graph/documents
//! projections of a corpus. One walk, shared across projections; no timestamps.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use crate::composition::{ComposedCorpus, ComposedProvenance};
use crate::corpus::{ArtifactKey, ArtifactOrigin, ArtifactPath, Layer};
use crate::graph_composition::GraphOverrideProvenance;
use crate::identity::{artifact_identifier, artifact_identifiers};
use crate::markdown::split_frontmatter;
use crate::parse::Artifact;
use crate::pycompat::py_strip;
use crate::relationships::{corpus_items, edge_spec, relationships_from_corpus, CorpusItem};
use crate::scaffold::ScaffoldError;
use crate::spec::ArtifactSpec;
use crate::validate::load_ticketing_provider_with_boundary;

pub const EDGE_TYPE: &str = "relates-to";
pub const STATUS_ABSENT: &str = "unknown";

pub const EXPORT_SCHEMA_NAMES: [&str; 3] = ["viewer", "documents", "graph"];

const ERROR_RECORD_CONFLICT: &str = "federated-export-record-conflict";
const ERROR_SNAPSHOT_MISSING: &str = "federated-export-snapshot-missing";
const ERROR_INVALID_UTF8: &str = "federated-export-invalid-utf8";
const ERROR_MISSING_PIN: &str = "federated-export-missing-pin";
const ERROR_MISSING_PROVENANCE: &str = "federated-export-missing-provenance";
const ERROR_MISSING_CHILD_SOURCE: &str = "federated-export-missing-child-source";

const VIEWER_SCHEMA: &str =
    include_str!("../assets/schemas/export-viewer-v1.schema.json");
const DOCUMENTS_SCHEMA: &str =
    include_str!("../assets/schemas/export-documents-v1.schema.json");
const GRAPH_SCHEMA: &str = include_str!("../assets/schemas/export-graph-v1.schema.json");

/// Released display name; unlike source identity, this remains tied to the
/// caller's directory spelling.
fn corpus_name(directory: &str) -> String {
    crate::corpus::compatible_corpus_name(directory)
}

/// Return the packaged Draft 2020-12 contract for an export projection.
///
/// These bytes are the public resource surfaced by `decided export --schema`;
/// keep the CLI write exact so consumers can compare them directly.
pub fn export_schema(name: &str) -> Option<&'static str> {
    match name {
        "viewer" => Some(VIEWER_SCHEMA),
        "documents" => Some(DOCUMENTS_SCHEMA),
        "graph" => Some(GRAPH_SCHEMA),
        _ => None,
    }
}

/// The one non-federated source derivation shared by every JSON projection:
/// explicit `corpus.source`, then the lower-case repository key, then the
/// released directory-basename fallback (ADR-135).
pub fn corpus_source(directory: &str) -> Result<String, ScaffoldError> {
    crate::corpus::compatible_corpus_source(directory)
}

fn corpus_source_with_fallback(
    directory: &str,
    fallback_directory: &str,
    boundary: Option<&Path>,
) -> Result<String, ScaffoldError> {
    crate::corpus::compatible_corpus_source_with_fallback_and_boundary(
        directory,
        fallback_directory,
        boundary,
    )
}

fn logical_artifact_path(directory: &str, relative_path: &str) -> String {
    let root = crate::walk::normalize_root(directory);
    if root == "." {
        relative_path.to_string()
    } else if root == "/" {
        format!("/{relative_path}")
    } else if root.ends_with('/') {
        format!("{root}{relative_path}")
    } else {
        format!("{root}/{relative_path}")
    }
}

/// A deterministic failure while projecting an already-verified composed
/// corpus. Parent verification and composition findings retain their own
/// stable codes; export-specific snapshot failures use the codes above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedExportError {
    message: String,
}

impl FederatedExportError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for FederatedExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for FederatedExportError {}

impl From<ScaffoldError> for FederatedExportError {
    fn from(error: ScaffoldError) -> Self {
        Self::new(error.message())
    }
}

fn first_line(raw: &str) -> String {
    for line in raw.split('\n') {
        let s = py_strip(line);
        if !s.is_empty() {
            return s.to_string();
        }
    }
    String::new()
}

/// `canonical_value(raw, allowed)`, on this module's `first_line`.
fn canonical_value(raw: &str, allowed: &[String]) -> String {
    crate::spec::canonical_value(&first_line(raw), allowed)
}

/// `_status(product, spec)`.
fn status(artifact: &Artifact, spec: &ArtifactSpec) -> String {
    let body = match artifact.section("status") {
        Some(b) if !b.is_empty() => b,
        _ => return STATUS_ABSENT.to_string(),
    };
    let allowed: &[String] = spec
        .metadata
        .iter()
        .find(|(k, _)| k == "status")
        .map(|(_, v)| v.as_slice())
        .unwrap_or(&[]);
    let value = canonical_value(body, allowed);
    if value.is_empty() {
        STATUS_ABSENT.to_string()
    } else {
        value
    }
}

/// The Markdown body after the frontmatter envelope, re-read from disk.
///
/// The oracle re-reads in TEXT mode (`open(path, encoding="utf-8")`), which
/// applies universal newlines — `\r\n` and lone `\r` become `\n` — before
/// `split_frontmatter`. Mirror that here.
///
/// The oracle's text-mode read is also STRICT utf-8: a file with invalid
/// bytes CRASHES the oracle uncaught (`UnicodeDecodeError`) even though the
/// classification walk decoded it with `errors="replace"`. Per PORT-CONTRACT
/// decision 3 this port never crashes; export has no per-artifact issue
/// channel, so the divergence-by-design here is "the Rust export simply
/// succeeds" (catalogued in rust/fuzz/pinned/oracle-crashes/).
fn body_markdown(path: &str) -> String {
    let text = crate::pycompat::read_text_universal(path).unwrap_or_default();
    split_frontmatter(&text).body
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

struct ProjectedItem<'a> {
    item: &'a CorpusItem,
    content_digest: String,
    body_markdown: String,
}

fn full_sha256_pin(pin: &str) -> bool {
    full_pin_with_prefix(pin, "sha256:")
}

fn full_graph_corpus_pin(pin: &str) -> bool {
    full_sha256_pin(pin) || full_pin_with_prefix(pin, "sha256-v2:")
}

fn full_pin_with_prefix(pin: &str, prefix: &str) -> bool {
    let hash = pin.strip_prefix(prefix);
    hash.is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn snapshot_text(
    corpus: &ComposedCorpus,
    item: &CorpusItem,
) -> Result<String, FederatedExportError> {
    let bytes = match corpus.content(&item.key) {
        Some(bytes) => bytes.to_vec(),
        None => {
            return Err(FederatedExportError::new(format!(
                "{ERROR_SNAPSHOT_MISSING}: verified content is unavailable for {}::{}",
                item.key.source, item.key.canonical_id
            )))
        }
    };
    let text = String::from_utf8(bytes).map_err(|_| {
        FederatedExportError::new(format!(
            "{ERROR_INVALID_UTF8}: content for {}::{} is not valid UTF-8",
            item.key.source, item.key.canonical_id
        ))
    })?;
    Ok(normalize_newlines(&text))
}

fn projected_items<'a>(
    corpus: &'a ComposedCorpus,
    local_only: bool,
) -> Result<Vec<ProjectedItem<'a>>, FederatedExportError> {
    if let Some(finding) = corpus.findings().first() {
        return Err(FederatedExportError::new(format!(
            "{}: {}",
            finding.code, finding.message
        )));
    }

    let candidates: Vec<&CorpusItem> = if local_only {
        corpus.local_items().collect()
    } else {
        corpus.catalog().collect()
    };
    let mut projected = Vec::new();
    let mut by_key: BTreeMap<ArtifactKey, usize> = BTreeMap::new();
    for item in candidates {
        if item.origin.layer == Layer::Inherited {
            let Some(pin) = item.origin.pin.as_deref() else {
                return Err(FederatedExportError::new(format!(
                    "{ERROR_MISSING_PIN}: inherited record {}::{} has no verified pin",
                    item.key.source, item.key.canonical_id
                )));
            };
            let valid_pin = if corpus.is_graph() {
                full_graph_corpus_pin(pin)
            } else {
                full_sha256_pin(pin)
            };
            if !valid_pin {
                return Err(FederatedExportError::new(format!(
                    "{ERROR_MISSING_PIN}: inherited record {}::{} does not carry a full lowercase SHA-256 pin",
                    item.key.source, item.key.canonical_id
                )));
            }
        }
        let full_text = snapshot_text(corpus, item)?;
        let content_digest = crate::sha256::hexdigest(full_text.as_bytes());
        if let Some(existing_index) = by_key.get(&item.key) {
            let existing: &ProjectedItem<'_> = &projected[*existing_index];
            if existing.item.artifact_path != item.artifact_path
                || existing.content_digest != content_digest
                || existing.item.origin.pin != item.origin.pin
            {
                return Err(FederatedExportError::new(format!(
                    "{ERROR_RECORD_CONFLICT}: {}::{} has disagreeing path, body, or pin",
                    item.key.source, item.key.canonical_id
                )));
            }
            continue;
        }
        let body = split_frontmatter(&full_text).body;
        by_key.insert(item.key.clone(), projected.len());
        projected.push(ProjectedItem {
            item,
            content_digest,
            body_markdown: body,
        });
    }
    Ok(projected)
}

fn tags_of(artifact: &Artifact) -> Vec<String> {
    artifact
        .metadata
        .as_ref()
        .map(|m| m.tags.clone())
        .unwrap_or_default()
}

fn canonical_by_path(items: &[CorpusItem]) -> std::collections::HashMap<String, String> {
    items
        .iter()
        .map(|it| {
            (
                it.path.clone(),
                artifact_identifier(&it.artifact, it.spec, &it.path),
            )
        })
        .collect()
}

// --- viewer JSON -------------------------------------------------------------

/// Public source-aware endpoint identity. The field stays `id` on the wire so
/// consumers can lift existing export identifiers into the global
/// `(source, id)` namespace without learning the engine's internal key name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExportIdentity {
    pub source: String,
    pub id: String,
}

impl From<&ArtifactKey> for ExportIdentity {
    fn from(key: &ArtifactKey) -> Self {
        Self {
            source: key.source.clone(),
            id: key.canonical_id.clone(),
        }
    }
}

fn composed_provenance(
    corpus: &ComposedCorpus,
    item: &CorpusItem,
) -> Result<ComposedProvenance, FederatedExportError> {
    corpus.provenance_for(&item.key).ok_or_else(|| {
        FederatedExportError::new(format!(
            "{ERROR_MISSING_PROVENANCE}: composed record {}::{} has no provenance",
            item.key.source, item.key.canonical_id
        ))
    })
}

/// Complete version-2 export provenance. The graph-native chain remains one
/// ordered atomic value instead of being flattened into the v1 direct-mapping
/// carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphComposedProvenance {
    pub origin: ArtifactOrigin,
    pub overrides: Vec<GraphOverrideProvenance>,
}

fn graph_composed_provenance(
    corpus: &ComposedCorpus,
    item: &CorpusItem,
) -> Result<GraphComposedProvenance, FederatedExportError> {
    let overrides = corpus
        .graph_provenance_for(&item.key)
        .ok_or_else(|| {
            FederatedExportError::new(format!(
                "{ERROR_MISSING_PROVENANCE}: graph record {}::{} has no provenance",
                item.key.source, item.key.canonical_id
            ))
        })?
        .to_vec();
    Ok(GraphComposedProvenance {
        origin: item.origin.clone(),
        overrides,
    })
}

fn record_provenance(
    corpus: &ComposedCorpus,
    item: &CorpusItem,
) -> Result<(Option<ComposedProvenance>, Option<GraphComposedProvenance>), FederatedExportError> {
    if corpus.is_graph() {
        Ok((None, Some(graph_composed_provenance(corpus, item)?)))
    } else {
        Ok((Some(composed_provenance(corpus, item)?), None))
    }
}

fn composed_child_source(corpus: &ComposedCorpus) -> Result<String, FederatedExportError> {
    corpus.child_source().map(str::to_string).ok_or_else(|| {
        FederatedExportError::new(format!(
            "{ERROR_MISSING_CHILD_SOURCE}: verified composition has no child source"
        ))
    })
}

fn item_by_artifact_path(
    corpus: &ComposedCorpus,
) -> BTreeMap<ArtifactPath, &CorpusItem> {
    let mut items = BTreeMap::new();
    for item in corpus.catalog() {
        items.entry(item.artifact_path.clone()).or_insert(item);
    }
    items
}

fn included_keys(items: &[ProjectedItem<'_>]) -> BTreeSet<ArtifactKey> {
    items.iter().map(|item| item.item.key.clone()).collect()
}

fn relationship_endpoints<'a>(
    relationship: &crate::relationships::Relationship,
    by_path: &'a BTreeMap<ArtifactPath, &'a CorpusItem>,
) -> Result<(&'a CorpusItem, Option<&'a CorpusItem>), FederatedExportError> {
    let source = relationship
        .source_artifact
        .as_ref()
        .and_then(|path| by_path.get(path).copied())
        .ok_or_else(|| {
            FederatedExportError::new(format!(
                "{ERROR_SNAPSHOT_MISSING}: relationship source has no composed artifact identity: {}",
                relationship.source_path
            ))
        })?;
    let target = match &relationship.resolved_artifact {
        Some(path) => Some(by_path.get(path).copied().ok_or_else(|| {
            FederatedExportError::new(format!(
                "{ERROR_SNAPSHOT_MISSING}: resolved relationship target has no composed artifact identity: {}",
                relationship.target
            ))
        })?),
        None => None,
    };
    Ok((source, target))
}

pub struct ExportArtifact {
    pub id: String,
    pub aliases: Vec<String>,
    pub artifact_type: String,
    pub status: String,
    pub title: String,
    pub path: String,
    pub body_html: String,
    /// OKF-reserved descriptive labels (ADR-050): carried for the OKF
    /// bundle projection, deliberately NOT in the viewer JSON (ADR-007).
    pub tags: Vec<String>,
    /// Present only when a federation manifest activated the composed model.
    pub provenance: Option<ComposedProvenance>,
    /// Version-2 complete ordered provenance; mutually exclusive with the v1
    /// `provenance` carrier.
    pub graph_provenance: Option<GraphComposedProvenance>,
}

pub struct ExportRelationship {
    pub from: String,
    pub to: String,
    pub edge_type: String,
    pub from_identity: Option<ExportIdentity>,
    pub to_identity: Option<ExportIdentity>,
    pub provenance: Option<ComposedProvenance>,
    pub graph_provenance: Option<GraphComposedProvenance>,
    /// Version-2 catalog endpoint fields. `authored_token` being present is
    /// the mode marker; the candidate set may be empty and the terminal null.
    pub authored_token: Option<String>,
    pub historical_candidates: Vec<ExportIdentity>,
    pub effective_terminal: Option<ExportIdentity>,
}

pub struct CorpusExport {
    pub corpus_name: String,
    pub corpus_source: String,
    pub rac_version: String,
    pub artifacts: Vec<ExportArtifact>,
    pub relationships: Vec<ExportRelationship>,
}

impl CorpusExport {
    pub fn artifact_count(&self) -> usize {
        self.artifacts.len()
    }
}

fn build_corpus_export_inner(
    directory: &str,
    identity_directory: &str,
    rac_version: String,
    include_body_html: bool,
    corpus_source: String,
) -> CorpusExport {
    let items = corpus_items(directory, true);
    let canonical = canonical_by_path(&items);

    let mut artifacts: Vec<ExportArtifact> = Vec::new();
    for it in &items {
        let Some(spec) = it.spec else { continue };
        let canon = canonical[&it.path].clone();
        let logical_path =
            logical_artifact_path(identity_directory, &it.artifact_path.relative_path);
        let title = match &it.artifact.product.title {
            Some(t) if !t.is_empty() => t.clone(),
            _ => canon.clone(),
        };
        artifacts.push(ExportArtifact {
            id: canon,
            aliases: artifact_identifiers(&it.artifact, it.spec, &logical_path),
            artifact_type: spec.name.clone(),
            status: status(&it.artifact, spec),
            title,
            path: logical_path,
            body_html: if include_body_html {
                crate::mdhtml::render(&body_markdown(&it.locator.path.to_string_lossy()))
            } else {
                String::new()
            },
            tags: tags_of(&it.artifact),
            provenance: None,
            graph_provenance: None,
        });
    }

    let mut edges: Vec<ExportRelationship> = relationships_from_corpus(&items)
        .into_iter()
        .map(|rel| {
            let to = match &rel.resolved_path {
                Some(p) => canonical[p].clone(),
                None => rel.target.clone(),
            };
            ExportRelationship {
                from: canonical[&rel.source_path].clone(),
                to,
                edge_type: EDGE_TYPE.to_string(),
                from_identity: None,
                to_identity: None,
                provenance: None,
                graph_provenance: None,
                authored_token: None,
                historical_candidates: Vec::new(),
                effective_terminal: None,
            }
        })
        .collect();
    edges.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));

    CorpusExport {
        corpus_name: corpus_name(identity_directory),
        corpus_source,
        rac_version,
        artifacts,
        relationships: edges,
    }
}

pub fn build_corpus_export(
    directory: &str,
    rac_version: String,
) -> Result<CorpusExport, ScaffoldError> {
    build_corpus_export_for(directory, directory, rac_version, None)
}

pub fn build_corpus_export_for(
    directory: &str,
    identity_directory: &str,
    rac_version: String,
    boundary: Option<&Path>,
) -> Result<CorpusExport, ScaffoldError> {
    Ok(build_corpus_export_inner(
        directory,
        identity_directory,
        rac_version,
        true,
        corpus_source_with_fallback(directory, identity_directory, boundary)?,
    ))
}

/// Build the viewer projection from the one verified composed read model.
/// The catalog retains overridden parent history; `--local-only` selects only
/// the writable child records without constructing a second overlay.
pub fn build_corpus_export_from_composed(
    directory: &str,
    rac_version: String,
    corpus: &ComposedCorpus,
    local_only: bool,
) -> Result<CorpusExport, FederatedExportError> {
    build_corpus_export_from_composed_for(
        directory,
        directory,
        rac_version,
        corpus,
        local_only,
        None,
    )
}

pub fn build_corpus_export_from_composed_for(
    directory: &str,
    identity_directory: &str,
    rac_version: String,
    corpus: &ComposedCorpus,
    local_only: bool,
    boundary: Option<&Path>,
) -> Result<CorpusExport, FederatedExportError> {
    if !corpus.is_federated() {
        return build_corpus_export_for(directory, identity_directory, rac_version, boundary)
            .map_err(Into::into);
    }

    let projected = projected_items(corpus, local_only)?;
    let mut artifacts = Vec::new();
    for projected_item in &projected {
        let item = projected_item.item;
        let Some(spec) = item.spec else { continue };
        let (provenance, graph_provenance) = record_provenance(corpus, item)?;
        let id = item.key.canonical_id.clone();
        let title = match &item.artifact.product.title {
            Some(title) if !title.is_empty() => title.clone(),
            _ => id.clone(),
        };
        artifacts.push(ExportArtifact {
            id,
            aliases: artifact_identifiers(
                &item.artifact,
                item.spec,
                &item.artifact_path.relative_path,
            ),
            artifact_type: spec.name.clone(),
            status: status(&item.artifact, spec),
            title,
            path: item.artifact_path.relative_path.clone(),
            body_html: crate::mdhtml::render(&projected_item.body_markdown),
            tags: tags_of(&item.artifact),
            provenance,
            graph_provenance,
        });
    }

    let included = included_keys(&projected);
    if let Some(graph) = corpus.graph() {
        let mut relationships = Vec::new();
        for relationship in graph.catalog_relationships() {
            let source = graph.item(&relationship.source).ok_or_else(|| {
                FederatedExportError::new(format!(
                    "{ERROR_SNAPSHOT_MISSING}: relationship source has no graph artifact identity: {}::{}",
                    relationship.source.source, relationship.source.canonical_id
                ))
            })?;
            if !included.contains(&source.key) {
                continue;
            }
            let (_, graph_provenance) = record_provenance(corpus, source)?;
            let effective_key = relationship.effective_terminal.as_ref().map(|terminal| {
                graph.terminal_redirects().get(terminal).unwrap_or(terminal)
            });
            let effective_terminal = effective_key.map(ExportIdentity::from);
            relationships.push(ExportRelationship {
                from: source.key.canonical_id.clone(),
                to: effective_key
                    .map(|key| key.canonical_id.clone())
                    .unwrap_or_else(|| relationship.authored_token.clone()),
                edge_type: EDGE_TYPE.to_string(),
                from_identity: Some(ExportIdentity::from(&source.key)),
                to_identity: effective_terminal.clone(),
                provenance: None,
                graph_provenance,
                authored_token: Some(relationship.authored_token),
                historical_candidates: relationship
                    .historical_candidates
                    .iter()
                    .map(ExportIdentity::from)
                    .collect(),
                effective_terminal,
            });
        }
        relationships.sort_by(|left, right| {
            left.from_identity
                .cmp(&right.from_identity)
                .then(left.authored_token.cmp(&right.authored_token))
                .then(left.historical_candidates.cmp(&right.historical_candidates))
                .then(left.effective_terminal.cmp(&right.effective_terminal))
        });
        return Ok(CorpusExport {
            corpus_name: corpus_name(identity_directory),
            corpus_source: composed_child_source(corpus)?,
            rac_version,
            artifacts,
            relationships,
        });
    }

    let by_path = item_by_artifact_path(corpus);
    let mut relationships = Vec::new();
    for relationship in corpus.catalog_relationships() {
        let (source, target) = relationship_endpoints(&relationship, &by_path)?;
        if !included.contains(&source.key) {
            continue;
        }
        let target_id = target
            .map(|item| item.key.canonical_id.clone())
            .unwrap_or_else(|| relationship.target.clone());
        relationships.push(ExportRelationship {
            from: source.key.canonical_id.clone(),
            to: target_id,
            edge_type: EDGE_TYPE.to_string(),
            from_identity: Some(ExportIdentity::from(&source.key)),
            to_identity: target.map(|item| ExportIdentity::from(&item.key)),
            provenance: Some(composed_provenance(corpus, source)?),
            graph_provenance: None,
            authored_token: None,
            historical_candidates: Vec::new(),
            effective_terminal: None,
        });
    }
    relationships.sort_by(|left, right| {
        left.from_identity
            .cmp(&right.from_identity)
            .then(left.to_identity.cmp(&right.to_identity))
            .then(left.to.cmp(&right.to))
    });

    Ok(CorpusExport {
        corpus_name: corpus_name(identity_directory),
        corpus_source: composed_child_source(corpus)?,
        rac_version,
        artifacts,
        relationships,
    })
}

/// OKF consumes the source Markdown body directly. Avoid an irrelevant HTML
/// render over the whole corpus on this path.
pub fn build_okf_export(directory: &str, rac_version: String) -> CorpusExport {
    build_corpus_export_inner(
        directory,
        directory,
        rac_version,
        false,
        corpus_name(directory),
    )
}

/// Keep the first federation increment's OKF carrier local-only while using
/// the verified composition to exclude a vendored parent beneath `directory`.
/// Physical child paths are retained because the OKF writer copies their
/// Markdown bodies and derives repository-local bundle paths.
pub fn build_okf_export_from_composed(
    directory: &str,
    rac_version: String,
    corpus: &ComposedCorpus,
) -> CorpusExport {
    if !corpus.is_federated() {
        return build_okf_export(directory, rac_version);
    }

    let items = crate::federated_corpus::local_writable_projection(directory, corpus);
    let canonical = canonical_by_path(&items);
    let mut artifacts = Vec::new();
    for item in &items {
        let Some(spec) = item.spec else { continue };
        let id = canonical[&item.path].clone();
        let title = match &item.artifact.product.title {
            Some(title) if !title.is_empty() => title.clone(),
            _ => id.clone(),
        };
        artifacts.push(ExportArtifact {
            id,
            aliases: artifact_identifiers(&item.artifact, item.spec, &item.path),
            artifact_type: spec.name.clone(),
            status: status(&item.artifact, spec),
            title,
            path: item.path.clone(),
            body_html: String::new(),
            tags: tags_of(&item.artifact),
            provenance: None,
            graph_provenance: None,
        });
    }

    let mut relationships = Vec::new();
    for relationship in corpus.catalog_relationships() {
        let Some(from) = canonical.get(&relationship.source_path) else {
            continue;
        };
        let to = relationship
            .resolved_path
            .as_ref()
            .and_then(|path| canonical.get(path))
            .cloned()
            .unwrap_or(relationship.target);
        relationships.push(ExportRelationship {
            from: from.clone(),
            to,
            edge_type: EDGE_TYPE.to_string(),
            from_identity: None,
            to_identity: None,
            provenance: None,
            graph_provenance: None,
            authored_token: None,
            historical_candidates: Vec::new(),
            effective_terminal: None,
        });
    }
    relationships.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then(left.to.cmp(&right.to))
    });

    CorpusExport {
        corpus_name: corpus_name(directory),
        corpus_source: corpus_name(directory),
        rac_version,
        artifacts,
        relationships,
    }
}

// --- documents JSONL ---------------------------------------------------------

pub struct ExportDocument {
    pub id: String,
    pub artifact_type: String,
    pub status: String,
    pub title: String,
    pub text: String,
    pub aliases: Vec<String>,
    pub path: String,
    pub tags: Vec<String>,
    pub provenance: Option<ComposedProvenance>,
    pub graph_provenance: Option<GraphComposedProvenance>,
}

pub struct DocumentsExport {
    pub corpus_name: String,
    pub corpus_source: String,
    pub documents: Vec<ExportDocument>,
}

pub fn build_documents_export(directory: &str) -> Result<DocumentsExport, ScaffoldError> {
    build_documents_export_for(directory, directory, None)
}

pub fn build_documents_export_for(
    directory: &str,
    identity_directory: &str,
    boundary: Option<&Path>,
) -> Result<DocumentsExport, ScaffoldError> {
    let source = corpus_source_with_fallback(directory, identity_directory, boundary)?;
    let items = corpus_items(directory, true);
    let mut documents: Vec<ExportDocument> = Vec::new();
    for it in &items {
        let Some(spec) = it.spec else { continue };
        let canon = artifact_identifier(&it.artifact, it.spec, &it.path);
        let logical_path =
            logical_artifact_path(identity_directory, &it.artifact_path.relative_path);
        let title = match &it.artifact.product.title {
            Some(t) if !t.is_empty() => t.clone(),
            _ => canon.clone(),
        };
        documents.push(ExportDocument {
            id: canon,
            artifact_type: spec.name.clone(),
            status: status(&it.artifact, spec),
            title,
            text: body_markdown(&it.locator.path.to_string_lossy()),
            aliases: artifact_identifiers(&it.artifact, it.spec, &logical_path),
            path: logical_path,
            tags: tags_of(&it.artifact),
            provenance: None,
            graph_provenance: None,
        });
    }
    Ok(DocumentsExport {
        corpus_name: corpus_name(identity_directory),
        corpus_source: source,
        documents,
    })
}

pub fn build_documents_export_from_composed(
    directory: &str,
    corpus: &ComposedCorpus,
    local_only: bool,
) -> Result<DocumentsExport, FederatedExportError> {
    build_documents_export_from_composed_for(directory, directory, corpus, local_only, None)
}

pub fn build_documents_export_from_composed_for(
    directory: &str,
    identity_directory: &str,
    corpus: &ComposedCorpus,
    local_only: bool,
    boundary: Option<&Path>,
) -> Result<DocumentsExport, FederatedExportError> {
    if !corpus.is_federated() {
        return build_documents_export_for(directory, identity_directory, boundary)
            .map_err(Into::into);
    }

    let projected = projected_items(corpus, local_only)?;
    let mut documents = Vec::new();
    for projected_item in projected {
        let item = projected_item.item;
        let Some(spec) = item.spec else { continue };
        let (provenance, graph_provenance) = record_provenance(corpus, item)?;
        let id = item.key.canonical_id.clone();
        let title = match &item.artifact.product.title {
            Some(title) if !title.is_empty() => title.clone(),
            _ => id.clone(),
        };
        documents.push(ExportDocument {
            id,
            artifact_type: spec.name.clone(),
            status: status(&item.artifact, spec),
            title,
            text: projected_item.body_markdown,
            aliases: artifact_identifiers(
                &item.artifact,
                item.spec,
                &item.artifact_path.relative_path,
            ),
            path: item.artifact_path.relative_path.clone(),
            tags: tags_of(&item.artifact),
            provenance,
            graph_provenance,
        });
    }
    Ok(DocumentsExport {
        corpus_name: corpus_name(identity_directory),
        corpus_source: composed_child_source(corpus)?,
        documents,
    })
}

// --- graph JSON --------------------------------------------------------------

pub struct GraphNode {
    pub id: String,
    pub artifact_type: String,
    pub status: String,
    pub title: String,
    pub provenance: Option<ComposedProvenance>,
    pub graph_provenance: Option<GraphComposedProvenance>,
}

pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub edge_type: String,
    pub directed: bool,
    pub resolved: bool,
    pub external: bool,
    pub provider: Option<String>,
    pub source_identity: Option<ExportIdentity>,
    pub target_identity: Option<ExportIdentity>,
    pub provenance: Option<ComposedProvenance>,
    pub graph_provenance: Option<GraphComposedProvenance>,
    pub authored_token: Option<String>,
    pub historical_candidates: Vec<ExportIdentity>,
    pub effective_terminal: Option<ExportIdentity>,
}

pub struct GraphExport {
    pub corpus_name: String,
    pub corpus_source: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

pub fn build_graph_export(directory: &str) -> Result<GraphExport, ScaffoldError> {
    build_graph_export_for(directory, directory, None)
}

pub fn build_graph_export_for(
    directory: &str,
    identity_directory: &str,
    boundary: Option<&Path>,
) -> Result<GraphExport, ScaffoldError> {
    let source = corpus_source_with_fallback(directory, identity_directory, boundary)?;
    let items = corpus_items(directory, true);
    let provider = load_ticketing_provider_with_boundary(directory, boundary);
    let canonical = canonical_by_path(&items);

    let mut nodes: Vec<GraphNode> = Vec::new();
    for it in &items {
        let Some(spec) = it.spec else { continue };
        let canon = canonical[&it.path].clone();
        let title = match &it.artifact.product.title {
            Some(t) if !t.is_empty() => t.clone(),
            _ => canon.clone(),
        };
        nodes.push(GraphNode {
            id: canon,
            artifact_type: spec.name.clone(),
            status: status(&it.artifact, spec),
            title,
            provenance: None,
            graph_provenance: None,
        });
    }

    let mut edges: Vec<GraphEdge> = Vec::new();
    for rel in relationships_from_corpus(&items) {
        let kind = edge_spec(&rel.relationship);
        let external = kind.map(|k| k.external).unwrap_or(false);
        let target = match &rel.resolved_path {
            Some(p) => canonical[p].clone(),
            None => rel.target.clone(),
        };
        let provider_tag = match kind {
            Some(k) if k.external_provider => provider.clone(),
            _ => None,
        };
        edges.push(GraphEdge {
            source: canonical[&rel.source_path].clone(),
            target,
            edge_type: rel.relationship.clone(),
            directed: kind.map(|k| k.directional).unwrap_or(false),
            resolved: rel.resolved_path.is_some(),
            external,
            provider: provider_tag,
            source_identity: None,
            target_identity: None,
            provenance: None,
            graph_provenance: None,
            authored_token: None,
            historical_candidates: Vec::new(),
            effective_terminal: None,
        });
    }
    edges.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.edge_type.cmp(&b.edge_type))
            .then(a.target.cmp(&b.target))
    });

    Ok(GraphExport {
        corpus_name: corpus_name(identity_directory),
        corpus_source: source,
        nodes,
        edges,
    })
}

pub fn build_graph_export_from_composed(
    directory: &str,
    corpus: &ComposedCorpus,
    local_only: bool,
) -> Result<GraphExport, FederatedExportError> {
    build_graph_export_from_composed_for(directory, directory, corpus, local_only, None)
}

pub fn build_graph_export_from_composed_for(
    directory: &str,
    identity_directory: &str,
    corpus: &ComposedCorpus,
    local_only: bool,
    boundary: Option<&Path>,
) -> Result<GraphExport, FederatedExportError> {
    if !corpus.is_federated() {
        return build_graph_export_for(directory, identity_directory, boundary).map_err(Into::into);
    }

    let projected = projected_items(corpus, local_only)?;
    let mut nodes = Vec::new();
    for projected_item in &projected {
        let item = projected_item.item;
        let Some(spec) = item.spec else { continue };
        let (provenance, graph_provenance) = record_provenance(corpus, item)?;
        let id = item.key.canonical_id.clone();
        let title = match &item.artifact.product.title {
            Some(title) if !title.is_empty() => title.clone(),
            _ => id.clone(),
        };
        nodes.push(GraphNode {
            id,
            artifact_type: spec.name.clone(),
            status: status(&item.artifact, spec),
            title,
            provenance,
            graph_provenance,
        });
    }

    let included = included_keys(&projected);
    if let Some(graph) = corpus.graph() {
        let provider = load_ticketing_provider_with_boundary(directory, boundary);
        let mut edges = Vec::new();
        for relationship in graph.catalog_relationships() {
            let source = graph.item(&relationship.source).ok_or_else(|| {
                FederatedExportError::new(format!(
                    "{ERROR_SNAPSHOT_MISSING}: relationship source has no graph artifact identity: {}::{}",
                    relationship.source.source, relationship.source.canonical_id
                ))
            })?;
            if !included.contains(&source.key) {
                continue;
            }
            let (_, graph_provenance) = record_provenance(corpus, source)?;
            let kind = edge_spec(&relationship.relationship);
            let effective_key = relationship.effective_terminal.as_ref().map(|terminal| {
                graph.terminal_redirects().get(terminal).unwrap_or(terminal)
            });
            let effective_terminal = effective_key.map(ExportIdentity::from);
            edges.push(GraphEdge {
                source: source.key.canonical_id.clone(),
                target: effective_key
                    .map(|key| key.canonical_id.clone())
                    .unwrap_or_else(|| relationship.authored_token.clone()),
                edge_type: relationship.relationship,
                directed: kind.map(|spec| spec.directional).unwrap_or(false),
                resolved: effective_key.is_some(),
                external: relationship.external,
                provider: match kind {
                    Some(spec) if spec.external_provider => provider.clone(),
                    _ => None,
                },
                source_identity: Some(ExportIdentity::from(&source.key)),
                target_identity: effective_terminal.clone(),
                provenance: None,
                graph_provenance,
                authored_token: Some(relationship.authored_token),
                historical_candidates: relationship
                    .historical_candidates
                    .iter()
                    .map(ExportIdentity::from)
                    .collect(),
                effective_terminal,
            });
        }
        edges.sort_by(|left, right| {
            left.source_identity
                .cmp(&right.source_identity)
                .then(left.edge_type.cmp(&right.edge_type))
                .then(left.authored_token.cmp(&right.authored_token))
                .then(left.historical_candidates.cmp(&right.historical_candidates))
                .then(left.effective_terminal.cmp(&right.effective_terminal))
        });
        return Ok(GraphExport {
            corpus_name: corpus_name(identity_directory),
            corpus_source: composed_child_source(corpus)?,
            nodes,
            edges,
        });
    }

    let by_path = item_by_artifact_path(corpus);
    let provider = load_ticketing_provider_with_boundary(directory, boundary);
    let mut edges = Vec::new();
    for relationship in corpus.catalog_relationships() {
        let (source, target) = relationship_endpoints(&relationship, &by_path)?;
        if !included.contains(&source.key) {
            continue;
        }
        let kind = edge_spec(&relationship.relationship);
        let target_id = target
            .map(|item| item.key.canonical_id.clone())
            .unwrap_or_else(|| relationship.target.clone());
        edges.push(GraphEdge {
            source: source.key.canonical_id.clone(),
            target: target_id,
            edge_type: relationship.relationship,
            directed: kind.map(|spec| spec.directional).unwrap_or(false),
            resolved: target.is_some(),
            external: kind.map(|spec| spec.external).unwrap_or(false),
            provider: match kind {
                Some(spec) if spec.external_provider => provider.clone(),
                _ => None,
            },
            source_identity: Some(ExportIdentity::from(&source.key)),
            target_identity: target.map(|item| ExportIdentity::from(&item.key)),
            provenance: Some(composed_provenance(corpus, source)?),
            graph_provenance: None,
            authored_token: None,
            historical_candidates: Vec::new(),
            effective_terminal: None,
        });
    }
    edges.sort_by(|left, right| {
        left.source_identity
            .cmp(&right.source_identity)
            .then(left.edge_type.cmp(&right.edge_type))
            .then(left.target_identity.cmp(&right.target_identity))
            .then(left.target.cmp(&right.target))
    });

    Ok(GraphExport {
        corpus_name: corpus_name(identity_directory),
        corpus_source: composed_child_source(corpus)?,
        nodes,
        edges,
    })
}
