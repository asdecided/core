//! Corpus export (`decided.services.export`) — deterministic viewer/graph/documents
//! projections of a corpus. One walk, shared across projections; no timestamps.

use crate::identity::{artifact_identifier, artifact_identifiers};
use crate::markdown::split_frontmatter;
use crate::parse::Artifact;
use crate::pycompat::py_strip;
use crate::relationships::{
    corpus_items, edge_spec, relationships_from_corpus, CorpusItem,
};
use crate::spec::ArtifactSpec;
use crate::validate::load_ticketing_provider;

pub const EDGE_TYPE: &str = "relates-to";
pub const STATUS_ABSENT: &str = "unknown";

/// `_corpus_name(directory)`.
fn corpus_name(directory: &str) -> String {
    let trimmed = directory.trim_end_matches('/');
    let name = trimmed.rsplit('/').next().unwrap_or("");
    if name.is_empty() || name == "." || name == ".." {
        directory.to_string()
    } else {
        name.to_string()
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
}

pub struct ExportRelationship {
    pub from: String,
    pub to: String,
    pub edge_type: String,
}

pub struct CorpusExport {
    pub corpus_name: String,
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
    rac_version: String,
    include_body_html: bool,
) -> CorpusExport {
    let items = corpus_items(directory, true);
    let canonical = canonical_by_path(&items);

    let mut artifacts: Vec<ExportArtifact> = Vec::new();
    for it in &items {
        let Some(spec) = it.spec else { continue };
        let canon = canonical[&it.path].clone();
        let title = match &it.artifact.product.title {
            Some(t) if !t.is_empty() => t.clone(),
            _ => canon.clone(),
        };
        artifacts.push(ExportArtifact {
            id: canon,
            aliases: artifact_identifiers(&it.artifact, it.spec, &it.path),
            artifact_type: spec.name.clone(),
            status: status(&it.artifact, spec),
            title,
            path: it.path.clone(),
            body_html: if include_body_html {
                crate::mdhtml::render(&body_markdown(&it.path))
            } else {
                String::new()
            },
            tags: tags_of(&it.artifact),
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
            }
        })
        .collect();
    edges.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));

    CorpusExport {
        corpus_name: corpus_name(directory),
        rac_version,
        artifacts,
        relationships: edges,
    }
}

pub fn build_corpus_export(directory: &str, rac_version: String) -> CorpusExport {
    build_corpus_export_inner(directory, rac_version, true)
}

/// OKF consumes the source Markdown body directly. Avoid an irrelevant HTML
/// render over the whole corpus on this path.
pub fn build_okf_export(directory: &str, rac_version: String) -> CorpusExport {
    build_corpus_export_inner(directory, rac_version, false)
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
}

pub struct DocumentsExport {
    pub corpus_name: String,
    pub documents: Vec<ExportDocument>,
}

pub fn build_documents_export(directory: &str) -> DocumentsExport {
    let items = corpus_items(directory, true);
    let mut documents: Vec<ExportDocument> = Vec::new();
    for it in &items {
        let Some(spec) = it.spec else { continue };
        let canon = artifact_identifier(&it.artifact, it.spec, &it.path);
        let title = match &it.artifact.product.title {
            Some(t) if !t.is_empty() => t.clone(),
            _ => canon.clone(),
        };
        documents.push(ExportDocument {
            id: canon,
            artifact_type: spec.name.clone(),
            status: status(&it.artifact, spec),
            title,
            text: body_markdown(&it.path),
            aliases: artifact_identifiers(&it.artifact, it.spec, &it.path),
            path: it.path.clone(),
            tags: tags_of(&it.artifact),
        });
    }
    DocumentsExport {
        corpus_name: corpus_name(directory),
        documents,
    }
}

// --- graph JSON --------------------------------------------------------------

pub struct GraphNode {
    pub id: String,
    pub artifact_type: String,
    pub status: String,
    pub title: String,
}

pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub edge_type: String,
    pub directed: bool,
    pub resolved: bool,
    pub external: bool,
    pub provider: Option<String>,
}

pub struct GraphExport {
    pub corpus_name: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

pub fn build_graph_export(directory: &str) -> GraphExport {
    let items = corpus_items(directory, true);
    let provider = load_ticketing_provider(directory);
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
        });
    }
    edges.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.edge_type.cmp(&b.edge_type))
            .then(a.target.cmp(&b.target))
    });

    GraphExport {
        corpus_name: corpus_name(directory),
        nodes,
        edges,
    }
}
