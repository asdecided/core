//! Read-only operator views over the verified version-2 federation graph.
//!
//! The reports in this module are projections only. They consume the exact
//! [`VerifiedGraphCorpus`](crate::graph_federated_corpus::VerifiedGraphCorpus)
//! used by serving, validation, export, and enforcement; they never walk a
//! second corpus, contact a forge, mutate a checkout, or reinterpret pins.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::corpus::{ArtifactKey, Layer};
use crate::graph_composition::{GraphLookupError, GraphOverrideProvenance};
use crate::graph_federated_corpus::VerifiedGraphCorpus;

pub const FEDERATION_REPORT_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationStatusSummary {
    pub sources: usize,
    pub inherited_sources: usize,
    pub edges: usize,
    pub physical_routes: usize,
    pub max_depth: usize,
    pub catalog_artifacts: usize,
    pub effective_artifacts: usize,
    pub root_local_artifacts: usize,
    pub overrides: usize,
    pub read_only_roots: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationSourceStatus {
    pub source: String,
    pub layer: Layer,
    pub pin: Option<String>,
    pub manifest_version: Option<u32>,
    pub source_route: Vec<String>,
    pub route_count: usize,
    pub config_path: String,
    pub manifest_path: Option<String>,
    pub corpus_path: String,
    pub artifact_count: usize,
    pub effective_artifact_count: usize,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationEdgeStatus {
    pub owner_source: String,
    pub alias: String,
    pub target_source: String,
    pub declared_pin: String,
    pub canonical_pin: String,
    pub root: String,
    pub corpus: String,
    pub materialisation_root: String,
    pub corpus_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationStatusReport {
    pub root_source: String,
    pub root_corpus: String,
    pub summary: FederationStatusSummary,
    pub sources: Vec<FederationSourceStatus>,
    pub edges: Vec<FederationEdgeStatus>,
    pub read_only_roots: Vec<String>,
}

impl FederationStatusReport {
    pub fn from_corpus(corpus: &VerifiedGraphCorpus) -> Self {
        let federation = &corpus.federation;
        let root = &federation.repository_root;
        let mut catalog_by_source: BTreeMap<String, usize> = BTreeMap::new();
        for item in corpus.composition.catalog() {
            *catalog_by_source
                .entry(item.key.source.clone())
                .or_default() += 1;
        }
        let mut effective_by_source: BTreeMap<String, usize> = BTreeMap::new();
        for item in corpus.composition.effective() {
            *effective_by_source
                .entry(item.key.source.clone())
                .or_default() += 1;
        }

        let mut sources = vec![FederationSourceStatus {
            source: federation.root_source.clone(),
            layer: Layer::Local,
            pin: None,
            manifest_version: Some(2),
            source_route: vec![federation.root_source.clone()],
            route_count: 1,
            config_path: relative_display(root, &federation.root_config_path),
            manifest_path: Some(relative_display(root, &federation.manifest.path)),
            corpus_path: relative_display(root, &federation.root_corpus_root),
            artifact_count: catalog_by_source
                .get(&federation.root_source)
                .copied()
                .unwrap_or(0),
            effective_artifact_count: effective_by_source
                .get(&federation.root_source)
                .copied()
                .unwrap_or(0),
            writable: true,
        }];
        sources.extend(federation.nodes.iter().map(|node| {
            FederationSourceStatus {
                source: node.source.clone(),
                layer: Layer::Inherited,
                pin: Some(node.digest.clone()),
                manifest_version: node.manifest_version,
                source_route: node.source_route.clone(),
                route_count: node.route_count,
                config_path: relative_display(root, &node.config_path),
                manifest_path: node
                    .manifest_bytes
                    .as_ref()
                    .map(|_| relative_display(root, &node.manifest_path)),
                corpus_path: relative_display(root, &node.corpus_root),
                artifact_count: catalog_by_source.get(&node.source).copied().unwrap_or(0),
                effective_artifact_count: effective_by_source
                    .get(&node.source)
                    .copied()
                    .unwrap_or(0),
                writable: false,
            }
        }));
        sources.sort_by(|left, right| {
            left.layer
                .cmp(&right.layer)
                .then_with(|| left.source.cmp(&right.source))
        });

        let mut edges: Vec<FederationEdgeStatus> = federation
            .edges
            .iter()
            .map(|edge| FederationEdgeStatus {
                owner_source: edge.owner_source.clone(),
                alias: edge.alias.clone(),
                target_source: edge.target_source.clone(),
                declared_pin: edge.declared_digest.clone(),
                canonical_pin: edge.canonical_digest.clone(),
                root: edge.root.clone(),
                corpus: edge.corpus.clone(),
                materialisation_root: relative_display(root, &edge.materialisation_root),
                corpus_path: relative_display(root, &edge.corpus_root),
            })
            .collect();
        edges.sort_by(|left, right| {
            (
                &left.owner_source,
                &left.alias,
                &left.target_source,
                &left.materialisation_root,
            )
                .cmp(&(
                    &right.owner_source,
                    &right.alias,
                    &right.target_source,
                    &right.materialisation_root,
                ))
        });

        let read_only_roots: Vec<String> = federation
            .materialisation_roots
            .iter()
            .map(|path| relative_display(root, path))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let summary = FederationStatusSummary {
            sources: sources.len(),
            inherited_sources: federation.nodes.len(),
            edges: edges.len(),
            physical_routes: federation.materialisation_roots.len(),
            max_depth: federation
                .nodes
                .iter()
                .map(|node| node.source_route.len().saturating_sub(1))
                .max()
                .unwrap_or(0),
            catalog_artifacts: corpus.composition.catalog().len(),
            effective_artifacts: corpus.composition.effective().len(),
            root_local_artifacts: corpus.composition.root_local().len(),
            overrides: corpus.composition.ordered_overrides().len(),
            read_only_roots: read_only_roots.len(),
        };
        Self {
            root_source: federation.root_source.clone(),
            root_corpus: federation.root_corpus_path.clone(),
            summary,
            sources,
            edges,
            read_only_roots,
        }
    }

    pub fn render_human(&self) -> String {
        let mut output = String::new();
        let _ = writeln!(output, "PASS  Federation verified");
        let _ = writeln!(output, "Root: {} ({})", self.root_source, self.root_corpus);
        let _ = writeln!(
            output,
            "Sources: {} ({} inherited) | edges: {} | physical routes: {} | max depth: {}",
            self.summary.sources,
            self.summary.inherited_sources,
            self.summary.edges,
            self.summary.physical_routes,
            self.summary.max_depth
        );
        let _ = writeln!(
            output,
            "Artifacts: {} catalog | {} effective | {} root-local | overrides: {}",
            self.summary.catalog_artifacts,
            self.summary.effective_artifacts,
            self.summary.root_local_artifacts,
            self.summary.overrides
        );
        let _ = writeln!(
            output,
            "Pins: exact and verified from local bytes (offline)"
        );
        let _ = writeln!(output, "\nSources:");
        for source in &self.sources {
            let route = source.source_route.join(" -> ");
            let _ = writeln!(
                output,
                "- {} [{}] artifacts {}/{}; route {} ({} physical route{}); {}",
                source.source,
                source.layer.as_str(),
                source.effective_artifact_count,
                source.artifact_count,
                route,
                source.route_count,
                if source.route_count == 1 { "" } else { "s" },
                if source.writable {
                    "writable"
                } else {
                    "read-only"
                }
            );
            if let Some(pin) = &source.pin {
                let _ = writeln!(output, "  pin: {pin}");
            }
            let _ = writeln!(output, "  corpus: {}", source.corpus_path);
        }
        let _ = writeln!(output, "\nEdges:");
        for edge in &self.edges {
            let _ = writeln!(
                output,
                "- {} --{}--> {} ({}/{})",
                edge.owner_source, edge.alias, edge.target_source, edge.root, edge.corpus
            );
            let _ = writeln!(output, "  pin: {}", edge.canonical_pin);
            let _ = writeln!(output, "  materialised: {}", edge.materialisation_root);
        }
        let _ = write!(
            output,
            "\nRead-only boundaries: {} verified materialisation root{}",
            self.summary.read_only_roots,
            if self.summary.read_only_roots == 1 {
                ""
            } else {
                "s"
            }
        );
        output
    }

    pub fn render_json(&self) -> String {
        crate::pyjson::dumps_indent2(&self.json_value())
    }

    pub fn json_value(&self) -> Value {
        let summary = json!({
            "sources": self.summary.sources,
            "inherited_sources": self.summary.inherited_sources,
            "edges": self.summary.edges,
            "physical_routes": self.summary.physical_routes,
            "max_depth": self.summary.max_depth,
            "catalog_artifacts": self.summary.catalog_artifacts,
            "effective_artifacts": self.summary.effective_artifacts,
            "root_local_artifacts": self.summary.root_local_artifacts,
            "overrides": self.summary.overrides,
            "read_only_roots": self.summary.read_only_roots,
        });
        let sources = self.sources.iter().map(source_value).collect::<Vec<_>>();
        let edges = self.edges.iter().map(edge_value).collect::<Vec<_>>();
        json!({
            "schema_version": FEDERATION_REPORT_SCHEMA_VERSION,
            "status": "verified",
            "manifest_version": 2,
            "root_source": self.root_source,
            "root_corpus": self.root_corpus,
            "verification": {
                "network_access": false,
                "pins_verified": true,
                "immutable_snapshots": true,
            },
            "summary": summary,
            "sources": sources,
            "edges": edges,
            "read_only_roots": self.read_only_roots,
        })
    }
}

fn source_value(source: &FederationSourceStatus) -> Value {
    let mut value = Map::new();
    value.insert("source".into(), json!(source.source));
    value.insert("layer".into(), json!(source.layer.as_str()));
    value.insert("writable".into(), json!(source.writable));
    if let Some(pin) = &source.pin {
        value.insert("pin".into(), json!(pin));
    }
    value.insert("manifest_version".into(), json!(source.manifest_version));
    value.insert("source_route".into(), json!(source.source_route));
    value.insert("route_count".into(), json!(source.route_count));
    value.insert("config_path".into(), json!(source.config_path));
    if let Some(manifest_path) = &source.manifest_path {
        value.insert("manifest_path".into(), json!(manifest_path));
    }
    value.insert("corpus_path".into(), json!(source.corpus_path));
    value.insert("artifact_count".into(), json!(source.artifact_count));
    value.insert(
        "effective_artifact_count".into(),
        json!(source.effective_artifact_count),
    );
    Value::Object(value)
}

fn edge_value(edge: &FederationEdgeStatus) -> Value {
    json!({
        "owner_source": edge.owner_source,
        "alias": edge.alias,
        "target_source": edge.target_source,
        "declared_pin": edge.declared_pin,
        "canonical_pin": edge.canonical_pin,
        "declared_pin_verified": true,
        "root": edge.root,
        "corpus": edge.corpus,
        "materialisation_root": edge.materialisation_root,
        "corpus_path": edge.corpus_path,
        "read_only": true,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainedArtifact {
    pub key: ArtifactKey,
    pub title: Option<String>,
    pub artifact_type: Option<String>,
    pub path: String,
    pub layer: Layer,
    pub pin: Option<String>,
    pub alias: Option<String>,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainAlias {
    pub alias: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationExplainReport {
    pub reference: String,
    pub context: String,
    pub outcome: &'static str,
    pub message: String,
    pub qualified: Option<bool>,
    pub visible_sources: Vec<String>,
    pub aliases: Vec<ExplainAlias>,
    pub historical_candidates: Vec<ExplainedArtifact>,
    pub effective_candidates: Vec<ExplainedArtifact>,
    pub selected: Option<ExplainedArtifact>,
    pub effective_terminal: Option<ExplainedArtifact>,
    pub override_provenance: Vec<GraphOverrideProvenance>,
}

impl FederationExplainReport {
    pub fn from_corpus(corpus: &VerifiedGraphCorpus, context: &str, reference: &str) -> Self {
        let visible_sources: Vec<String> = corpus
            .composition
            .visibility()
            .visible_from(context)
            .map(|sources| sources.iter().cloned().collect())
            .unwrap_or_default();
        let aliases: Vec<ExplainAlias> = corpus
            .composition
            .visibility()
            .aliases()
            .iter()
            .filter(|((owner, _), _)| owner == context)
            .map(|((_, alias), source)| ExplainAlias {
                alias: alias.clone(),
                source: source.clone(),
            })
            .collect();
        let base = |outcome, message| Self {
            reference: reference.to_string(),
            context: context.to_string(),
            outcome,
            message,
            qualified: None,
            visible_sources: visible_sources.clone(),
            aliases: aliases.clone(),
            historical_candidates: Vec::new(),
            effective_candidates: Vec::new(),
            selected: None,
            effective_terminal: None,
            override_provenance: Vec::new(),
        };

        match corpus.composition.resolve_from(context, reference) {
            Ok(resolution) => {
                let historical_candidates = resolution
                    .historical_candidates
                    .iter()
                    .filter_map(|key| artifact_view(corpus, key))
                    .collect();
                let selected = artifact_view(corpus, &resolution.selected);
                let effective_terminal = artifact_view(corpus, &resolution.effective_terminal);
                let provenance_key = if corpus
                    .composition
                    .provenance_for(&resolution.selected)
                    .is_some_and(|entries| !entries.is_empty())
                {
                    &resolution.selected
                } else {
                    &resolution.effective_terminal
                };
                let override_provenance = corpus
                    .composition
                    .provenance_for(provenance_key)
                    .unwrap_or(&[])
                    .to_vec();
                let message = if resolution.qualified
                    && resolution.selected != resolution.effective_terminal
                {
                    "the qualified reference selects source-owned history; the effective projection redirects it through explicit overrides"
                } else if !resolution.qualified && resolution.historical_candidates.len() > 1 {
                    "the unqualified reference has multiple historical records which reconverge on one effective terminal"
                } else if resolution.selected != resolution.effective_terminal {
                    "the reference resolves through explicit overrides to the effective terminal"
                } else {
                    "the reference selects the effective source-owned artifact directly"
                };
                Self {
                    reference: reference.to_string(),
                    context: context.to_string(),
                    outcome: "resolved",
                    message: message.to_string(),
                    qualified: Some(resolution.qualified),
                    visible_sources,
                    aliases,
                    historical_candidates,
                    effective_candidates: Vec::new(),
                    selected,
                    effective_terminal,
                    override_provenance,
                }
            }
            Err(GraphLookupError::Ambiguous {
                historical_candidates,
                effective_candidates,
            }) => {
                let mut report = base(
                    "ambiguous",
                    "the visible historical records do not converge on one effective terminal; use a source-qualified canonical ID".to_string(),
                );
                report.historical_candidates = historical_candidates
                    .iter()
                    .filter_map(|key| artifact_view(corpus, key))
                    .collect();
                report.effective_candidates = effective_candidates
                    .iter()
                    .filter_map(|key| artifact_view(corpus, key))
                    .collect();
                report
            }
            Err(GraphLookupError::UnknownContext) => base(
                "unknown-context",
                "the requested source context is not part of the verified closure".to_string(),
            ),
            Err(GraphLookupError::NotFound) => base(
                "not-found",
                "no visible artifact matches this reference in the requested source context"
                    .to_string(),
            ),
            Err(GraphLookupError::InvalidQualifiedReference) => base(
                "invalid-reference",
                "a qualified reference must contain exactly one qualifier and one canonical ID"
                    .to_string(),
            ),
            Err(GraphLookupError::QualifiedCanonicalRequired) => base(
                "canonical-id-required",
                "qualified lookup accepts only the source-owned canonical ID, not an alias"
                    .to_string(),
            ),
        }
    }

    pub fn ok(&self) -> bool {
        self.outcome == "resolved"
    }

    pub fn render_human(&self) -> String {
        let mut output = String::new();
        let _ = writeln!(
            output,
            "{}  {}",
            self.outcome.to_ascii_uppercase(),
            self.reference
        );
        let _ = writeln!(output, "Context: {}", self.context);
        let _ = writeln!(output, "Reason: {}", self.message);
        if let Some(selected) = &self.selected {
            let _ = writeln!(output, "Selected: {}", artifact_label(selected));
        }
        if let Some(terminal) = &self.effective_terminal {
            let _ = writeln!(output, "Effective: {}", artifact_label(terminal));
        }
        if !self.historical_candidates.is_empty() {
            let _ = writeln!(output, "\nHistorical candidates:");
            for candidate in &self.historical_candidates {
                let _ = writeln!(output, "- {}", artifact_label(candidate));
            }
        }
        if !self.effective_candidates.is_empty() {
            let _ = writeln!(output, "\nEffective candidates:");
            for candidate in &self.effective_candidates {
                let _ = writeln!(output, "- {}", artifact_label(candidate));
            }
        }
        if self.override_provenance.is_empty() {
            let _ = writeln!(output, "\nOverrides: none");
        } else {
            let _ = writeln!(output, "\nOverride provenance:");
            for (index, mapping) in self.override_provenance.iter().enumerate() {
                let _ = writeln!(
                    output,
                    "{}. {} [{}]: {} -> {} (rationale {})",
                    index + 1,
                    mapping.owner_source,
                    mapping.state.as_str(),
                    key_label(&mapping.target),
                    key_label(&mapping.replacement),
                    key_label(&mapping.rationale)
                );
            }
        }
        if !self.aliases.is_empty() {
            let _ = writeln!(output, "\nDirect aliases from {}:", self.context);
            for alias in &self.aliases {
                let _ = writeln!(output, "- {} -> {}", alias.alias, alias.source);
            }
        }
        output.trim_end().to_string()
    }

    pub fn render_json(&self) -> String {
        crate::pyjson::dumps_indent2(&self.json_value())
    }

    pub fn json_value(&self) -> Value {
        let mut value = Map::new();
        value.insert(
            "schema_version".into(),
            json!(FEDERATION_REPORT_SCHEMA_VERSION),
        );
        value.insert("outcome".into(), json!(self.outcome));
        value.insert("reference".into(), json!(self.reference));
        value.insert("context".into(), json!(self.context));
        value.insert("message".into(), json!(self.message));
        if let Some(qualified) = self.qualified {
            value.insert("qualified".into(), json!(qualified));
        }
        value.insert("visible_sources".into(), json!(self.visible_sources));
        value.insert(
            "aliases".into(),
            Value::Array(
                self.aliases
                    .iter()
                    .map(|alias| json!({"alias": alias.alias, "source": alias.source}))
                    .collect(),
            ),
        );
        value.insert(
            "historical_candidates".into(),
            Value::Array(
                self.historical_candidates
                    .iter()
                    .map(artifact_value)
                    .collect(),
            ),
        );
        value.insert(
            "effective_candidates".into(),
            Value::Array(
                self.effective_candidates
                    .iter()
                    .map(artifact_value)
                    .collect(),
            ),
        );
        if let Some(selected) = &self.selected {
            value.insert("selected".into(), artifact_value(selected));
        }
        if let Some(terminal) = &self.effective_terminal {
            value.insert("effective_terminal".into(), artifact_value(terminal));
        }
        value.insert(
            "override_provenance".into(),
            Value::Array(
                self.override_provenance
                    .iter()
                    .map(override_value)
                    .collect(),
            ),
        );
        Value::Object(value)
    }
}

fn artifact_view(corpus: &VerifiedGraphCorpus, key: &ArtifactKey) -> Option<ExplainedArtifact> {
    let item = corpus.composition.item(key)?;
    Some(ExplainedArtifact {
        key: item.key.clone(),
        title: item.artifact.product.title.clone(),
        artifact_type: item.spec.map(|spec| spec.name.clone()),
        path: item.artifact_path.relative_path.clone(),
        layer: item.origin.layer,
        pin: item.origin.pin.clone(),
        alias: item.origin.alias.clone(),
        writable: item.origin.layer.is_writable(),
    })
}

fn artifact_value(artifact: &ExplainedArtifact) -> Value {
    let mut provenance = Map::new();
    provenance.insert("source".into(), json!(artifact.key.source));
    provenance.insert("layer".into(), json!(artifact.layer.as_str()));
    provenance.insert("writable".into(), json!(artifact.writable));
    if let Some(pin) = &artifact.pin {
        provenance.insert("pin".into(), json!(pin));
    }
    if let Some(alias) = &artifact.alias {
        provenance.insert("alias".into(), json!(alias));
    }
    let mut value = Map::new();
    value.insert("key".into(), key_value(&artifact.key));
    value.insert("title".into(), json!(artifact.title));
    value.insert("type".into(), json!(artifact.artifact_type));
    value.insert("path".into(), json!(artifact.path));
    value.insert("provenance".into(), Value::Object(provenance));
    Value::Object(value)
}

fn override_value(mapping: &GraphOverrideProvenance) -> Value {
    json!({
        "state": mapping.state.as_str(),
        "owner_source": mapping.owner_source,
        "target": key_value(&mapping.target),
        "replacement": key_value(&mapping.replacement),
        "rationale": key_value(&mapping.rationale),
    })
}

fn key_value(key: &ArtifactKey) -> Value {
    json!({
        "source": key.source,
        "canonical_id": key.canonical_id,
        "qualified_id": key_label(key),
    })
}

fn artifact_label(artifact: &ExplainedArtifact) -> String {
    format!(
        "{} ({}, {}, {})",
        key_label(&artifact.key),
        artifact.artifact_type.as_deref().unwrap_or("unknown type"),
        artifact.layer.as_str(),
        artifact.path
    )
}

fn key_label(key: &ArtifactKey) -> String {
    format!("{}::{}", key.source, key.canonical_id)
}

fn relative_display(root: &Path, path: &Path) -> String {
    let relative: PathBuf = path
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf());
    let text = relative.to_string_lossy().replace('\\', "/");
    if text.is_empty() {
        ".".to_string()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_labels_are_source_qualified() {
        assert_eq!(
            key_label(&ArtifactKey::new("acme/standards", "ADR-001")),
            "acme/standards::ADR-001"
        );
    }

    #[test]
    fn relative_paths_are_posix_and_checkout_independent() {
        let root = Path::new("checkout");
        assert_eq!(
            relative_display(root, Path::new("checkout/vendor/standards")),
            "vendor/standards"
        );
    }

    #[test]
    fn every_override_state_has_a_stable_word() {
        assert_eq!(
            crate::graph_composition::GraphOverrideState::Overridden.as_str(),
            "overridden"
        );
        assert_eq!(
            crate::graph_composition::GraphOverrideState::Replacement.as_str(),
            "replacement"
        );
        assert_eq!(
            crate::graph_composition::GraphOverrideState::Lineage.as_str(),
            "lineage"
        );
    }
}
