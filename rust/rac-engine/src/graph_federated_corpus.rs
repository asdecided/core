//! Exact verified graph snapshots -> one source-aware semantic composition.
//!
//! This adapter is deliberately downstream of federation verification. It
//! parses only captured byte buffers, never reopens a corpus path, and keeps
//! version-1 roots on the established [`crate::federated_corpus`] path.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::classify::classify;
use crate::composition::{ComposedCorpus, OverrideDeclaration, ParentIdentity};
use crate::corpus::{
    ArtifactKey, ArtifactOrigin, ArtifactPath, CorpusLayer, Layer, PhysicalArtifactLocator,
    PhysicalCorpusLocator,
};
use crate::federation::{
    FederationValidationOrigin, SnapshotFile, VerifiedFederation, VerifiedFederationNode,
    MANIFEST_RELATIVE_PATH,
};
use crate::graph_composition::{
    GraphComposition, GraphCompositionFinding, GraphCompositionInput, GraphOverrideDeclaration,
    GraphRelationshipIssue, SourceNodeInput, SourceParentInput,
};
use crate::parse::parse_bytes;
use crate::relationships::{
    edge_spec, relationship_severity, CorpusItem, ISSUE_RELATIONSHIP_CYCLE, ISSUE_TARGET_AMBIGUOUS,
    ISSUE_TARGET_NOT_FOUND, ISSUE_TARGET_TYPE_MISMATCH,
};
use crate::spec::spec_for;
use crate::validate::{
    apply_overrides, check_okf_conformance, resolve_severity, OkfEntry, SeverityOverrides,
};

pub const GRAPH_CORPUS_INVALID_NODE: &str = "corpus-federation-invalid-node";
pub const GRAPH_CORPUS_INVALID_MANIFEST: &str = "corpus-federation-invalid-manifest";

/// Exact captured content retained beside the parsed catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedGraphContent {
    pub artifact_path: ArtifactPath,
    pub bytes: Vec<u8>,
}

/// One deterministic failure at the verified-byte/semantic boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphFederatedCorpusError {
    pub code: &'static str,
    pub source: Option<String>,
    pub relative_path: Option<String>,
    pub message: String,
    pub composition_findings: Box<Vec<GraphCompositionFinding>>,
    pub validation_origin: Option<Box<FederationValidationOrigin>>,
    pub source_route: Option<Box<Vec<String>>>,
    pub route_count: Option<Box<usize>>,
}

impl GraphFederatedCorpusError {
    pub fn stable_code(&self) -> &'static str {
        self.code
    }

    fn sourced(
        code: &'static str,
        source: impl Into<String>,
        relative_path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            source: Some(source.into()),
            relative_path: Some(relative_path.into()),
            message: message.into(),
            composition_findings: Box::new(Vec::new()),
            validation_origin: None,
            source_route: None,
            route_count: None,
        }
    }

    fn composition(findings: Vec<GraphCompositionFinding>) -> Self {
        let first = findings
            .first()
            .expect("graph composition reports at least one finding");
        Self {
            code: first.code,
            source: first.owner_source.clone(),
            relative_path: None,
            message: first.message.clone(),
            composition_findings: Box::new(findings),
            validation_origin: None,
            source_route: None,
            route_count: None,
        }
    }

    fn attach_federation_context(&mut self, federation: &VerifiedFederation) {
        if !self.composition_findings.is_empty() {
            self.validation_origin = Some(Box::new(FederationValidationOrigin {
                source: federation.root_source.clone(),
                layer: Layer::Local,
                pin: None,
            }));
            self.source_route = Some(Box::new(vec![federation.root_source.clone()]));
            self.route_count = Some(Box::new(1));
            return;
        }
        let source = self.source.as_deref().unwrap_or(&federation.root_source);
        if source == federation.root_source {
            self.validation_origin = Some(Box::new(FederationValidationOrigin {
                source: federation.root_source.clone(),
                layer: Layer::Local,
                pin: None,
            }));
        } else if let Some(node) = federation.node(source) {
            self.validation_origin = Some(Box::new(FederationValidationOrigin {
                source: node.source.clone(),
                layer: Layer::Inherited,
                pin: Some(node.digest.clone()),
            }));
            self.source_route = Some(Box::new(node.source_route.clone()));
            self.route_count = Some(Box::new(node.route_count));
        }
    }
}

impl fmt::Display for GraphFederatedCorpusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.code)?;
        if let Some(source) = &self.source {
            write!(formatter, ": source '{source}'")?;
        }
        if let Some(path) = &self.relative_path {
            write!(formatter, " at {path}")?;
        }
        write!(formatter, ": {}", self.message)
    }
}

impl std::error::Error for GraphFederatedCorpusError {}

/// The semantic graph and every immutable input needed by later generations.
///
/// `federation` owns the exact config, manifest, Markdown, physical-route, and
/// root records. The other fields are projections of those bytes; no consumer
/// needs to reopen an inherited path to reconstruct them.
pub struct VerifiedGraphCorpus {
    pub federation: VerifiedFederation,
    pub composition: GraphComposition,
    pub captured_content: BTreeMap<ArtifactKey, CapturedGraphContent>,
    pub canonical_layers: BTreeMap<String, CorpusLayer>,
    pub read_only_materialisation_roots: Vec<PathBuf>,
    pub read_only_corpus_roots: Vec<PathBuf>,
}

impl VerifiedGraphCorpus {
    pub fn content(&self, key: &ArtifactKey) -> Option<&[u8]> {
        self.captured_content
            .get(key)
            .map(|content| content.bytes.as_slice())
    }

    /// Move this verified semantic closure behind the shared read facade.
    /// The facade owns the exact captured bytes and every read-only physical
    /// root; no filesystem path is reopened during the conversion.
    pub fn into_composed_corpus(self) -> ComposedCorpus {
        ComposedCorpus::from_graph(
            self.composition,
            self.captured_content
                .into_iter()
                .map(|(key, content)| (key, content.bytes)),
            self.read_only_materialisation_roots,
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawV2Overrides {
    version: u32,
    items: Vec<RawV2Override>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawV2Override {
    target: String,
    #[serde(rename = "with")]
    replacement: String,
    rationale: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawV1Overrides {
    version: u32,
    items: Vec<RawV1Override>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawV1Override {
    parent: String,
    #[serde(rename = "with")]
    replacement: String,
    rationale: String,
}

#[derive(Default)]
struct LiftedOverrides {
    graph: Vec<GraphOverrideDeclaration>,
    v1: BTreeMap<String, Vec<OverrideDeclaration>>,
}

struct ParsedSnapshots {
    items: Vec<CorpusItem>,
    content: Vec<(ArtifactKey, CapturedGraphContent)>,
    policies: BTreeMap<String, SeverityOverrides>,
}

type SemanticCompositionResult = (
    GraphComposition,
    BTreeMap<ArtifactKey, CapturedGraphContent>,
    BTreeMap<String, CorpusLayer>,
);

/// Parse, intrinsically validate, and compose a verified version-2 closure.
///
/// This function consumes the verification result so the returned model owns
/// all authenticated byte buffers and read-only roots. It performs no
/// filesystem or network operation.
pub fn compose_verified_federation(
    federation: VerifiedFederation,
) -> Result<VerifiedGraphCorpus, GraphFederatedCorpusError> {
    let result: Result<SemanticCompositionResult, GraphFederatedCorpusError> = (|| {
        let topology = topology(&federation);
        let aliases = alias_tables(&topology);
        let lifted = lift_overrides(&federation, &aliases)?;
        let parsed = parse_and_validate_snapshots(&federation)?;

        validate_nested_v1(
            &federation,
            &topology,
            &parsed.items,
            &lifted.v1,
            &parsed.policies,
        )?;

        let composition = GraphComposition::compose(GraphCompositionInput::new(
            federation.root_source.clone(),
            topology,
            parsed.items,
            lifted.graph,
        ))
        .map_err(GraphFederatedCorpusError::composition)?;
        validate_graph_relationships(&composition, &parsed.policies)?;

        let captured_content = parsed.content.into_iter().collect();
        let mut canonical_layers = BTreeMap::new();
        canonical_layers.insert(
            federation.root_source.clone(),
            CorpusLayer::local(federation.root_source.clone()),
        );
        for node in &federation.nodes {
            canonical_layers.insert(
                node.source.clone(),
                CorpusLayer {
                    source: node.source.clone(),
                    layer: Layer::Inherited,
                    pin: Some(node.digest.clone()),
                    alias: None,
                },
            );
        }
        Ok((composition, captured_content, canonical_layers))
    })();
    let (composition, captured_content, canonical_layers) = match result {
        Ok(result) => result,
        Err(mut error) => {
            error.attach_federation_context(&federation);
            return Err(error);
        }
    };

    Ok(VerifiedGraphCorpus {
        read_only_materialisation_roots: federation.materialisation_roots.clone(),
        read_only_corpus_roots: federation.corpus_roots.clone(),
        federation,
        composition,
        captured_content,
        canonical_layers,
    })
}

fn topology(federation: &VerifiedFederation) -> Vec<SourceNodeInput> {
    let mut by_owner: BTreeMap<String, BTreeSet<SourceParentInput>> = BTreeMap::new();
    for edge in &federation.edges {
        by_owner
            .entry(edge.owner_source.clone())
            .or_default()
            .insert(SourceParentInput::new(
                edge.target_source.clone(),
                edge.alias.clone(),
            ));
    }
    let mut sources: BTreeSet<String> = federation
        .nodes
        .iter()
        .map(|node| node.source.clone())
        .collect();
    sources.insert(federation.root_source.clone());
    sources
        .into_iter()
        .map(|source| {
            SourceNodeInput::new(
                source.clone(),
                by_owner
                    .remove(&source)
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
            )
        })
        .collect()
}

fn alias_tables(topology: &[SourceNodeInput]) -> BTreeMap<String, BTreeMap<String, String>> {
    topology
        .iter()
        .map(|node| {
            (
                node.source.clone(),
                node.direct_parents
                    .iter()
                    .map(|parent| (parent.alias.clone(), parent.source.clone()))
                    .collect(),
            )
        })
        .collect()
}

fn lift_overrides(
    federation: &VerifiedFederation,
    aliases: &BTreeMap<String, BTreeMap<String, String>>,
) -> Result<LiftedOverrides, GraphFederatedCorpusError> {
    let mut lifted = LiftedOverrides::default();
    lifted.graph.extend(parse_v2_overrides(
        &federation.root_source,
        federation.manifest.overrides.clone(),
    )?);
    for node in &federation.nodes {
        match node.manifest_version {
            None => {
                if node.overrides.is_some() {
                    return Err(invalid_manifest(
                        &node.source,
                        "a manifest-free node retained override declarations",
                    ));
                }
            }
            Some(1) => {
                let (graph, v1) = parse_v1_overrides(
                    &node.source,
                    node.overrides.clone(),
                    aliases.get(&node.source),
                )?;
                lifted.graph.extend(graph);
                lifted.v1.insert(node.source.clone(), v1);
            }
            Some(2) => lifted
                .graph
                .extend(parse_v2_overrides(&node.source, node.overrides.clone())?),
            Some(version) => {
                return Err(invalid_manifest(
                    &node.source,
                    format!("unsupported retained manifest version {version}"),
                ));
            }
        }
    }
    lifted.graph.sort();
    Ok(lifted)
}

fn parse_v2_overrides(
    owner: &str,
    value: Option<serde_yaml::Value>,
) -> Result<Vec<GraphOverrideDeclaration>, GraphFederatedCorpusError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let raw: RawV2Overrides = serde_yaml::from_value(value).map_err(|error| {
        invalid_manifest(owner, format!("version-2 overrides are malformed: {error}"))
    })?;
    if raw.version != 2 {
        return Err(invalid_manifest(
            owner,
            format!(
                "override version {} does not match manifest version 2",
                raw.version
            ),
        ));
    }
    raw.items
        .into_iter()
        .map(|item| {
            let (target_source, target_id) = global_target(&item.target).map_err(|message| {
                invalid_manifest(
                    owner,
                    format!("override target '{}': {message}", item.target),
                )
            })?;
            let replacement = local_operand(&item.replacement).map_err(|message| {
                invalid_manifest(
                    owner,
                    format!("override replacement '{}': {message}", item.replacement),
                )
            })?;
            let rationale = local_operand(&item.rationale).map_err(|message| {
                invalid_manifest(
                    owner,
                    format!("override rationale '{}': {message}", item.rationale),
                )
            })?;
            Ok(GraphOverrideDeclaration::new(
                owner,
                ArtifactKey::new(target_source, target_id),
                ArtifactKey::new(owner, replacement),
                ArtifactKey::new(owner, rationale),
            ))
        })
        .collect()
}

fn parse_v1_overrides(
    owner: &str,
    value: Option<serde_yaml::Value>,
    aliases: Option<&BTreeMap<String, String>>,
) -> Result<(Vec<GraphOverrideDeclaration>, Vec<OverrideDeclaration>), GraphFederatedCorpusError> {
    let Some(value) = value else {
        return Ok((Vec::new(), Vec::new()));
    };
    let raw: RawV1Overrides = serde_yaml::from_value(value).map_err(|error| {
        invalid_manifest(owner, format!("version-1 overrides are malformed: {error}"))
    })?;
    if raw.version != 1 {
        return Err(invalid_manifest(
            owner,
            format!(
                "override version {} does not match manifest version 1",
                raw.version
            ),
        ));
    }
    let aliases =
        aliases.ok_or_else(|| invalid_manifest(owner, "missing version-1 alias table"))?;
    let mut graph = Vec::new();
    let mut legacy = Vec::new();
    for item in raw.items {
        let declaration =
            OverrideDeclaration::parse(&item.parent, &item.replacement, &item.rationale).map_err(
                |error| {
                    invalid_manifest(
                        owner,
                        format!("version-1 override target '{}': {error}", item.parent),
                    )
                },
            )?;
        let target_source = aliases.get(declaration.parent.alias()).ok_or_else(|| {
            invalid_manifest(
                owner,
                format!(
                    "version-1 override alias '{}' is not a direct parent",
                    declaration.parent.alias()
                ),
            )
        })?;
        graph.push(GraphOverrideDeclaration::new(
            owner,
            ArtifactKey::new(target_source, declaration.parent.canonical_id().as_str()),
            ArtifactKey::new(owner, declaration.replacement.as_str()),
            ArtifactKey::new(owner, declaration.rationale.as_str()),
        ));
        legacy.push(declaration);
    }
    Ok((graph, legacy))
}

fn global_target(value: &str) -> Result<(&str, &str), &'static str> {
    if value.trim() != value {
        return Err("must be unpadded");
    }
    let Some((source, canonical_id)) = value.split_once("::") else {
        return Err("must be globally source-qualified");
    };
    if source.is_empty()
        || canonical_id.is_empty()
        || source.trim() != source
        || canonical_id.trim() != canonical_id
        || canonical_id.contains("::")
        || !source.contains('/')
    {
        return Err("must contain one source/canonical-id delimiter");
    }
    Ok((source, canonical_id))
}

fn local_operand(value: &str) -> Result<&str, &'static str> {
    if value.is_empty() || value.trim() != value {
        return Err("canonical id must be non-empty and unpadded");
    }
    if value.contains("::") {
        return Err("local canonical id must not be qualified");
    }
    Ok(value)
}

fn invalid_manifest(source: &str, message: impl Into<String>) -> GraphFederatedCorpusError {
    GraphFederatedCorpusError::sourced(
        GRAPH_CORPUS_INVALID_MANIFEST,
        source,
        MANIFEST_RELATIVE_PATH,
        message,
    )
}

fn parse_and_validate_snapshots(
    federation: &VerifiedFederation,
) -> Result<ParsedSnapshots, GraphFederatedCorpusError> {
    let mut parsed = ParsedSnapshots {
        items: Vec::new(),
        content: Vec::new(),
        policies: BTreeMap::new(),
    };
    parse_source(
        &federation.root_source,
        CorpusLayer::local(&federation.root_source),
        &federation.repository_root,
        &federation.root_corpus_root,
        &federation.root_files,
        &federation.root_config_bytes,
        &mut parsed,
    )?;
    for node in &federation.nodes {
        let repository_root = repository_root_for_node(node).ok_or_else(|| {
            GraphFederatedCorpusError::sourced(
                GRAPH_CORPUS_INVALID_NODE,
                &node.source,
                ".decided/config.yaml",
                "verified config path has no repository parent",
            )
        })?;
        parse_source(
            &node.source,
            CorpusLayer {
                source: node.source.clone(),
                layer: Layer::Inherited,
                pin: Some(node.digest.clone()),
                alias: None,
            },
            repository_root,
            &node.corpus_root,
            &node.files,
            &node.config_bytes,
            &mut parsed,
        )?;
    }
    Ok(parsed)
}

#[allow(clippy::too_many_arguments)]
fn parse_source(
    source: &str,
    layer: CorpusLayer,
    repository_root: &Path,
    corpus_root: &Path,
    files: &[SnapshotFile],
    config_bytes: &[u8],
    parsed: &mut ParsedSnapshots,
) -> Result<(), GraphFederatedCorpusError> {
    let (provider, policy) = source_policy(config_bytes);
    let locator = PhysicalCorpusLocator::new(repository_root, corpus_root);
    let start = parsed.items.len();
    for file in files {
        let stable_path = file.relative_path.clone();
        let artifact = parse_bytes(&file.bytes, &stable_path);
        let artifact_type = classify(&artifact).artifact_type;
        let spec = spec_for(&artifact_type);
        let item = CorpusItem::new(
            stable_path,
            file.relative_path.clone(),
            artifact,
            spec,
            layer.origin(),
            PhysicalArtifactLocator::new(locator.clone(), file.absolute_path.clone()),
        );
        parsed.content.push((
            item.key.clone(),
            CapturedGraphContent {
                artifact_path: item.artifact_path.clone(),
                bytes: file.bytes.clone(),
            },
        ));
        parsed.items.push(item);
    }

    let source_items = &parsed.items[start..];
    for item in source_items {
        let Some(spec) = item.spec else {
            continue;
        };
        let issues = apply_overrides(
            crate::validate::validate(&item.artifact, provider.as_deref(), Some(&spec.name)),
            &spec.name,
            &policy,
        );
        if let Some(issue) = issues.iter().find(|issue| issue.severity == "error") {
            return Err(GraphFederatedCorpusError::sourced(
                GRAPH_CORPUS_INVALID_NODE,
                source,
                &item.artifact_path.relative_path,
                format!("structural error {}: {}", issue.code, issue.message),
            ));
        }
    }
    let entries: Vec<OkfEntry<'_>> = source_items
        .iter()
        .map(|item| OkfEntry {
            path: &item.artifact_path.relative_path,
            artifact_type: item.spec.map_or("unknown", |spec| spec.name.as_str()),
            file_name: item
                .artifact_path
                .relative_path
                .rsplit('/')
                .next()
                .unwrap_or(&item.artifact_path.relative_path),
        })
        .collect();
    if let Some(finding) = check_okf_conformance(&entries, &policy)
        .findings
        .into_iter()
        .find(|finding| finding.severity == "error")
    {
        return Err(GraphFederatedCorpusError::sourced(
            GRAPH_CORPUS_INVALID_NODE,
            source,
            finding.path,
            format!("OKF error {}: {}", finding.code, finding.message),
        ));
    }
    parsed.policies.insert(source.to_string(), policy);
    Ok(())
}

fn repository_root_for_node(node: &VerifiedFederationNode) -> Option<&Path> {
    node.config_path.parent()?.parent()
}

fn yaml_string(value: &serde_yaml::Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

fn severity(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::Bool(false) => Some("off".to_string()),
        serde_yaml::Value::Bool(true) => Some("on".to_string()),
        _ => yaml_string(value),
    }
}

fn source_policy(config: &[u8]) -> (Option<String>, SeverityOverrides) {
    let Ok(value) = serde_yaml::from_slice::<serde_yaml::Value>(config) else {
        return (None, SeverityOverrides::default());
    };
    let provider = value
        .get("ticketing")
        .and_then(|section| section.get("provider"))
        .and_then(yaml_string);
    let mut overrides = SeverityOverrides::default();
    if let Some(rules) = value
        .get("validation")
        .and_then(|section| section.get("rules"))
        .and_then(serde_yaml::Value::as_mapping)
    {
        for (key, value) in rules {
            let (Some(key), Some(value)) = (key.as_str(), severity(value)) else {
                continue;
            };
            if matches!(value.as_str(), "error" | "warning" | "off") {
                overrides.rules.push((key.to_string(), value));
            }
        }
    }
    if let Some(types) = value
        .get("validation")
        .and_then(|section| section.get("types"))
        .and_then(serde_yaml::Value::as_mapping)
    {
        for (key, value) in types {
            let (Some(key), Some(value)) = (key.as_str(), severity(value)) else {
                continue;
            };
            if matches!(value.as_str(), "error" | "warning") {
                overrides.types.push((key.to_string(), value));
            }
        }
    }
    (provider, overrides)
}

fn validate_nested_v1(
    federation: &VerifiedFederation,
    topology: &[SourceNodeInput],
    items: &[CorpusItem],
    overrides: &BTreeMap<String, Vec<OverrideDeclaration>>,
    policies: &BTreeMap<String, SeverityOverrides>,
) -> Result<(), GraphFederatedCorpusError> {
    for node in federation
        .nodes
        .iter()
        .filter(|node| node.manifest_version == Some(1))
    {
        let graph_node = topology
            .iter()
            .find(|candidate| candidate.source == node.source)
            .expect("verified graph node is present in topology");
        let [parent] = graph_node.direct_parents.as_slice() else {
            return Err(invalid_manifest(
                &node.source,
                "version-1 node must retain exactly one direct parent",
            ));
        };
        let parent_node = federation.node(&parent.source).ok_or_else(|| {
            invalid_manifest(
                &node.source,
                "version-1 direct parent is not in the verified closure",
            )
        })?;
        let parent_identity = ParentIdentity::new(&parent.source, &parent.alias)
            .map_err(|error| invalid_manifest(&node.source, error.to_string()))?;
        let local = items
            .iter()
            .filter(|item| item.key.source == node.source)
            .cloned()
            .map(|item| reorigin_v1(item, Layer::Local, None, None))
            .collect();
        let inherited = items
            .iter()
            .filter(|item| item.key.source == parent.source)
            .cloned()
            .map(|item| {
                reorigin_v1(
                    item,
                    Layer::Inherited,
                    Some(parent_node.digest.clone()),
                    Some(parent.alias.clone()),
                )
            })
            .collect();
        let composition = ComposedCorpus::compose(
            local,
            inherited,
            parent_identity,
            overrides.get(&node.source).cloned().unwrap_or_default(),
        );
        if let Some(finding) = composition.findings().first() {
            return Err(GraphFederatedCorpusError::sourced(
                finding.code,
                &node.source,
                MANIFEST_RELATIVE_PATH,
                finding.message.clone(),
            ));
        }
        let policy = policies
            .get(&node.source)
            .expect("source policy was parsed");
        for relationship in composition.local_relationships() {
            let source_path = relationship
                .source_artifact
                .as_ref()
                .map(|path| path.relative_path.as_str())
                .unwrap_or(relationship.source_path.as_str());
            if let Some(code) = relationship.issue.as_deref() {
                if relationship_blocks(policy, &composition, &relationship.source_artifact, code) {
                    return Err(GraphFederatedCorpusError::sourced(
                        GRAPH_CORPUS_INVALID_NODE,
                        &node.source,
                        source_path,
                        format!("relationship error {code} for '{}'", relationship.target),
                    ));
                }
            }
            let Some(target_path) = &relationship.resolved_artifact else {
                continue;
            };
            let Some(edge) = edge_spec(&relationship.relationship) else {
                continue;
            };
            let target = composition
                .catalog()
                .find(|item| &item.artifact_path == target_path);
            if target
                .and_then(|item| item.spec)
                .is_some_and(|spec| !edge.range.contains(&spec.name.as_str()))
                && relationship_blocks(
                    policy,
                    &composition,
                    &relationship.source_artifact,
                    ISSUE_TARGET_TYPE_MISMATCH,
                )
            {
                return Err(GraphFederatedCorpusError::sourced(
                    GRAPH_CORPUS_INVALID_NODE,
                    &node.source,
                    source_path,
                    format!(
                        "relationship error {ISSUE_TARGET_TYPE_MISMATCH} for '{}'",
                        relationship.target
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn reorigin_v1(
    mut item: CorpusItem,
    layer: Layer,
    pin: Option<String>,
    alias: Option<String>,
) -> CorpusItem {
    item.origin = ArtifactOrigin {
        source: item.key.source.clone(),
        layer,
        pin,
        alias,
    };
    item
}

fn relationship_blocks(
    policy: &SeverityOverrides,
    composition: &ComposedCorpus,
    source_path: &Option<ArtifactPath>,
    code: &str,
) -> bool {
    let artifact_type = source_path
        .as_ref()
        .and_then(|path| {
            composition
                .catalog()
                .find(|item| item.artifact_path == *path)
        })
        .and_then(|item| item.spec)
        .map_or("unknown", |spec| spec.name.as_str());
    resolve_severity(relationship_severity(code), code, artifact_type, policy) == "error"
}

fn validate_graph_relationships(
    composition: &GraphComposition,
    policies: &BTreeMap<String, SeverityOverrides>,
) -> Result<(), GraphFederatedCorpusError> {
    let mut acyclic: BTreeMap<ArtifactKey, BTreeSet<ArtifactKey>> = BTreeMap::new();
    for relationship in composition.catalog_relationships() {
        let Some(source_item) = composition.item(&relationship.source) else {
            continue;
        };
        let policy = policies
            .get(&relationship.source.source)
            .expect("every composition source has parsed policy");
        let source_type = source_item
            .spec
            .map_or("unknown", |spec| spec.name.as_str());
        let code = match relationship.issue {
            Some(GraphRelationshipIssue::TargetNotFound) => Some(ISSUE_TARGET_NOT_FOUND),
            Some(GraphRelationshipIssue::TargetAmbiguous) => Some(ISSUE_TARGET_AMBIGUOUS),
            Some(GraphRelationshipIssue::SelfReference) | None => None,
        };
        if let Some(code) = code {
            if resolve_severity(relationship_severity(code), code, source_type, policy) == "error" {
                return Err(relationship_error(
                    source_item,
                    code,
                    &relationship.authored_token,
                ));
            }
        }
        let Some(target_key) = relationship.effective_terminal else {
            continue;
        };
        let Some(edge) = edge_spec(&relationship.relationship) else {
            continue;
        };
        let target_type = composition
            .item(&target_key)
            .and_then(|item| item.spec)
            .map(|spec| spec.name.as_str());
        if target_type.is_some_and(|target_type| !edge.range.contains(&target_type))
            && resolve_severity(
                relationship_severity(ISSUE_TARGET_TYPE_MISMATCH),
                ISSUE_TARGET_TYPE_MISMATCH,
                source_type,
                policy,
            ) == "error"
        {
            return Err(relationship_error(
                source_item,
                ISSUE_TARGET_TYPE_MISMATCH,
                &relationship.authored_token,
            ));
        }
        if edge.acyclic && !relationship.external && target_key != relationship.source {
            acyclic
                .entry(relationship.source)
                .or_default()
                .insert(target_key);
        }
    }
    if let Some(key) = cycle_source(&acyclic) {
        let item = composition
            .item(&key)
            .expect("relationship cycle keys are catalog items");
        let policy = policies
            .get(&key.source)
            .expect("cycle source has parsed policy");
        let artifact_type = item.spec.map_or("unknown", |spec| spec.name.as_str());
        if resolve_severity(
            relationship_severity(ISSUE_RELATIONSHIP_CYCLE),
            ISSUE_RELATIONSHIP_CYCLE,
            artifact_type,
            policy,
        ) == "error"
        {
            return Err(relationship_error(item, ISSUE_RELATIONSHIP_CYCLE, "cycle"));
        }
    }
    Ok(())
}

fn relationship_error(item: &CorpusItem, code: &str, target: &str) -> GraphFederatedCorpusError {
    GraphFederatedCorpusError::sourced(
        GRAPH_CORPUS_INVALID_NODE,
        &item.key.source,
        &item.artifact_path.relative_path,
        format!("relationship error {code} for '{target}'"),
    )
}

fn cycle_source(adjacency: &BTreeMap<ArtifactKey, BTreeSet<ArtifactKey>>) -> Option<ArtifactKey> {
    fn visit(
        node: &ArtifactKey,
        adjacency: &BTreeMap<ArtifactKey, BTreeSet<ArtifactKey>>,
        states: &mut BTreeMap<ArtifactKey, u8>,
    ) -> Option<ArtifactKey> {
        states.insert(node.clone(), 1);
        if let Some(targets) = adjacency.get(node) {
            for target in targets {
                match states.get(target).copied().unwrap_or(0) {
                    1 => return Some(node.clone()),
                    0 => {
                        if let Some(found) = visit(target, adjacency, states) {
                            return Some(found);
                        }
                    }
                    _ => {}
                }
            }
        }
        states.insert(node.clone(), 2);
        None
    }

    let mut states = BTreeMap::new();
    for node in adjacency.keys() {
        if states.get(node).copied().unwrap_or(0) == 0 {
            if let Some(found) = visit(node, adjacency, &mut states) {
                return Some(found);
            }
        }
    }
    None
}
