//! Version-2 source-graph composition (ADR-144, ADR-146, ADR-147).
//!
//! This is the semantic half of federation. It accepts an already verified
//! logical topology and parsed snapshot items; it performs no filesystem,
//! manifest, digest, cache, or network work. The v1 [`crate::composition`]
//! API remains unchanged. A version-2 loader can therefore activate this
//! adapter only after it has authenticated the complete closure.

use std::collections::{BTreeMap, BTreeSet};

use crate::corpus::{ArtifactKey, Layer};
use crate::identity::artifact_identifiers;
use crate::pycompat::py_casefold;
use crate::relationships::{
    edge_spec, extract_relationships_full, CorpusItem, Relationship, ISSUE_SELF_REFERENCE,
    ISSUE_TARGET_AMBIGUOUS, ISSUE_TARGET_NOT_FOUND,
};
use crate::resolve::{
    entry_from_item, identity_entry_from_item, is_live_decision, resolved_from_entry, IndexEntry,
    ResolutionResult, OUTCOME_DUPLICATE, OUTCOME_NOT_FOUND, OUTCOME_RESOLVED,
};

pub const FINDING_INVALID_GRAPH: &str = "corpus-federation-invalid-graph";
pub const FINDING_INVALID_OVERRIDE: &str = "corpus-federation-invalid-override";
pub const FINDING_OVERRIDE_DIVERGENCE: &str = "corpus-federation-override-divergence";

/// One direct, edge-local alias owned by the declaring source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceParentInput {
    pub source: String,
    pub alias: String,
}

impl SourceParentInput {
    pub fn new(source: impl Into<String>, alias: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            alias: alias.into(),
        }
    }
}

/// A unique logical source node. Canonical node digests and physical routes
/// belong to verification/generation and deliberately do not enter this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceNodeInput {
    pub source: String,
    pub direct_parents: Vec<SourceParentInput>,
}

impl SourceNodeInput {
    pub fn new(source: impl Into<String>, direct_parents: Vec<SourceParentInput>) -> Self {
        Self {
            source: source.into(),
            direct_parents,
        }
    }
}

/// One already-parsed version-2 override declaration. The loader turns the
/// manifest's globally qualified target and local canonical operands into
/// stable keys before crossing this boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GraphOverrideDeclaration {
    pub owner_source: String,
    pub target: ArtifactKey,
    pub replacement: ArtifactKey,
    pub rationale: ArtifactKey,
}

impl GraphOverrideDeclaration {
    pub fn new(
        owner_source: impl Into<String>,
        target: ArtifactKey,
        replacement: ArtifactKey,
        rationale: ArtifactKey,
    ) -> Self {
        Self {
            owner_source: owner_source.into(),
            target,
            replacement,
            rationale,
        }
    }
}

/// The filesystem-independent handoff from `VerifiedFederation`.
pub struct GraphCompositionInput {
    pub root_source: String,
    pub nodes: Vec<SourceNodeInput>,
    pub items: Vec<CorpusItem>,
    pub overrides: Vec<GraphOverrideDeclaration>,
}

impl GraphCompositionInput {
    pub fn new(
        root_source: impl Into<String>,
        nodes: Vec<SourceNodeInput>,
        items: Vec<CorpusItem>,
        overrides: Vec<GraphOverrideDeclaration>,
    ) -> Self {
        Self {
            root_source: root_source.into(),
            nodes,
            items,
            overrides,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GraphFindingReason {
    MissingRoot,
    DuplicateSource,
    UnknownParent,
    DuplicateParent,
    DuplicateAlias,
    InvalidAlias,
    Cycle,
    UnreachableSource,
    UnknownItemSource,
    IdentitySourceMismatch,
    DuplicateArtifactKey,
    UnknownOverrideOwner,
    DuplicateOverrideTarget,
    TargetNotInherited,
    ReplacementNotLocal,
    ReplacementNotFound,
    RationaleNotLocal,
    RationaleNotFound,
    RationaleNotDecision,
    RationaleNotLive,
    TypeMismatch,
    SameManifestChain,
    OverrideCycle,
    DivergentTerminal,
}

impl GraphFindingReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingRoot => "missing-root",
            Self::DuplicateSource => "duplicate-source",
            Self::UnknownParent => "unknown-parent",
            Self::DuplicateParent => "duplicate-parent",
            Self::DuplicateAlias => "duplicate-alias",
            Self::InvalidAlias => "invalid-alias",
            Self::Cycle => "cycle",
            Self::UnreachableSource => "unreachable-source",
            Self::UnknownItemSource => "unknown-item-source",
            Self::IdentitySourceMismatch => "identity-source-mismatch",
            Self::DuplicateArtifactKey => "duplicate-artifact-key",
            Self::UnknownOverrideOwner => "unknown-override-owner",
            Self::DuplicateOverrideTarget => "duplicate-override-target",
            Self::TargetNotInherited => "target-not-inherited",
            Self::ReplacementNotLocal => "replacement-not-local",
            Self::ReplacementNotFound => "replacement-not-found",
            Self::RationaleNotLocal => "rationale-not-local",
            Self::RationaleNotFound => "rationale-not-found",
            Self::RationaleNotDecision => "rationale-not-decision",
            Self::RationaleNotLive => "rationale-not-live",
            Self::TypeMismatch => "type-mismatch",
            Self::SameManifestChain => "same-manifest-chain",
            Self::OverrideCycle => "override-cycle",
            Self::DivergentTerminal => "divergent-terminal",
        }
    }
}

/// A deterministic graph-composition blocker. Verification findings carry
/// routes; this semantic layer identifies the owning source and stable keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCompositionFinding {
    pub code: &'static str,
    pub reason: GraphFindingReason,
    pub owner_source: Option<String>,
    pub artifacts: Vec<ArtifactKey>,
    pub message: String,
}

/// Closure visibility and edge-local alias scope. Both tables use bytewise
/// `BTree*` ordering and never contain physical locators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceVisibility {
    visible: BTreeMap<String, BTreeSet<String>>,
    aliases: BTreeMap<(String, String), String>,
}

impl SourceVisibility {
    pub fn is_visible(&self, context: &str, source: &str) -> bool {
        self.visible
            .get(context)
            .is_some_and(|sources| sources.contains(source))
    }

    pub fn visible_from(&self, context: &str) -> Option<&BTreeSet<String>> {
        self.visible.get(context)
    }

    pub fn alias_target(&self, context: &str, alias: &str) -> Option<&str> {
        self.aliases
            .get(&(context.to_string(), alias.to_string()))
            .map(String::as_str)
    }

    pub fn aliases(&self) -> &BTreeMap<(String, String), String> {
        &self.aliases
    }
}

/// One validated mapping with the accepted total owner order embedded.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CompiledOverride {
    pub owner_rank: usize,
    pub owner_source: String,
    pub target: ArtifactKey,
    pub replacement: ArtifactKey,
    pub rationale: ArtifactKey,
}

impl CompiledOverride {
    fn from_declaration(owner_rank: usize, declaration: &GraphOverrideDeclaration) -> Self {
        Self {
            owner_rank,
            owner_source: declaration.owner_source.clone(),
            target: declaration.target.clone(),
            replacement: declaration.replacement.clone(),
            rationale: declaration.rationale.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GraphOverrideState {
    Overridden,
    Replacement,
    Lineage,
}

impl GraphOverrideState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Overridden => "overridden",
            Self::Replacement => "replacement",
            Self::Lineage => "lineage",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphOverrideProvenance {
    pub state: GraphOverrideState,
    pub owner_source: String,
    pub target: ArtifactKey,
    pub replacement: ArtifactKey,
    pub rationale: ArtifactKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphResolution {
    /// Every source-owned record matching the authored token, sorted by key.
    pub historical_candidates: Vec<ArtifactKey>,
    /// Qualified lookup selects history; unqualified lookup selects the one
    /// effective terminal after explicit redirects.
    pub selected: ArtifactKey,
    /// The source-contextual terminal even when `selected` is historical.
    pub effective_terminal: ArtifactKey,
    pub qualified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphLookupError {
    UnknownContext,
    NotFound,
    InvalidQualifiedReference,
    QualifiedCanonicalRequired,
    Ambiguous {
        historical_candidates: Vec<ArtifactKey>,
        effective_candidates: Vec<ArtifactKey>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphRelationshipIssue {
    TargetNotFound,
    TargetAmbiguous,
    SelfReference,
}

/// One catalog relationship endpoint. `historical_candidates` records the
/// immutable authored meaning; `effective_terminal` is a separate live value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRelationship {
    pub source: ArtifactKey,
    pub relationship: String,
    pub authored_token: String,
    pub historical_candidates: Vec<ArtifactKey>,
    pub effective_terminal: Option<ArtifactKey>,
    pub issue: Option<GraphRelationshipIssue>,
    pub external: bool,
}

type OverrideRoute = Vec<CompiledOverride>;

#[derive(Debug, Clone, Default)]
struct OriginRoutes {
    /// A historical key may arrive through several graph branches. Routes to
    /// an equal terminal remain distinct for provenance but do not diverge.
    by_terminal: BTreeMap<ArtifactKey, BTreeSet<OverrideRoute>>,
}

#[derive(Debug, Clone, Default)]
struct EffectiveProjection {
    origins: BTreeMap<ArtifactKey, OriginRoutes>,
}

impl EffectiveProjection {
    fn insert_identity(&mut self, key: ArtifactKey) {
        self.origins
            .entry(key.clone())
            .or_default()
            .by_terminal
            .entry(key)
            .or_default()
            .insert(Vec::new());
    }

    fn merge(&mut self, other: &Self) {
        for (origin, routes) in &other.origins {
            let destination = self.origins.entry(origin.clone()).or_default();
            for (terminal, paths) in &routes.by_terminal {
                destination
                    .by_terminal
                    .entry(terminal.clone())
                    .or_default()
                    .extend(paths.iter().cloned());
            }
        }
    }

    fn terminals(&self) -> BTreeSet<ArtifactKey> {
        self.origins
            .values()
            .flat_map(|routes| routes.by_terminal.keys().cloned())
            .collect()
    }

    fn apply(&mut self, replacements: &BTreeMap<ArtifactKey, CompiledOverride>) {
        for routes in self.origins.values_mut() {
            let previous = std::mem::take(&mut routes.by_terminal);
            for (terminal, paths) in previous {
                if let Some(mapping) = replacements.get(&terminal) {
                    let redirected = routes
                        .by_terminal
                        .entry(mapping.replacement.clone())
                        .or_default();
                    for mut path in paths {
                        path.push(mapping.clone());
                        redirected.insert(path);
                    }
                } else {
                    routes
                        .by_terminal
                        .entry(terminal)
                        .or_default()
                        .extend(paths);
                }
            }
        }
    }

    fn terminal_for(&self, key: &ArtifactKey) -> Option<&ArtifactKey> {
        let terminals = &self.origins.get(key)?.by_terminal;
        (terminals.len() == 1)
            .then(|| terminals.keys().next())
            .flatten()
    }
}

/// The unique catalog plus source-contextual and root-effective projections.
pub struct GraphComposition {
    root_source: String,
    items: Vec<CorpusItem>,
    item_by_key: BTreeMap<ArtifactKey, usize>,
    local_by_source: BTreeMap<String, Vec<usize>>,
    root_local: Vec<usize>,
    root_effective: Vec<usize>,
    visibility: SourceVisibility,
    projections: BTreeMap<String, EffectiveProjection>,
    identifiers: BTreeMap<String, BTreeMap<String, Vec<ArtifactKey>>>,
    canonical: BTreeMap<(String, String), ArtifactKey>,
    topological_sources: Vec<String>,
    ordered_overrides: Vec<CompiledOverride>,
    terminal_redirects: BTreeMap<ArtifactKey, ArtifactKey>,
    provenance: BTreeMap<ArtifactKey, Vec<GraphOverrideProvenance>>,
}

impl GraphComposition {
    pub fn compose(input: GraphCompositionInput) -> Result<Self, Vec<GraphCompositionFinding>> {
        let topology = PreparedTopology::prepare(&input.root_source, input.nodes)?;

        let PreparedItems {
            items,
            item_by_key,
            local_by_source,
            identifiers,
            canonical,
        } = PreparedItems::prepare(&input.root_source, &topology.nodes, input.items)?;

        let declarations =
            prepare_override_declarations(input.overrides, &topology, &item_by_key, &items)?;

        let mut projections: BTreeMap<String, EffectiveProjection> = BTreeMap::new();
        let mut ordered_overrides = Vec::new();
        let mut findings = Vec::new();

        for source in &topology.order {
            let node = &topology.nodes[source];
            let mut projection = EffectiveProjection::default();
            for parent in &node.direct_parents {
                if let Some(parent_projection) = projections.get(&parent.source) {
                    projection.merge(parent_projection);
                }
            }

            let live_inherited = projection.terminals();
            let owner_declarations = declarations.get(source).cloned().unwrap_or_default();
            let mut replacements = BTreeMap::new();
            for declaration in owner_declarations {
                if !live_inherited.contains(&declaration.target) {
                    findings.push(override_finding(
                        GraphFindingReason::TargetNotInherited,
                        &declaration,
                        vec![declaration.target.clone()],
                    ));
                    continue;
                }
                let mapping =
                    CompiledOverride::from_declaration(topology.rank[source], &declaration);
                replacements.insert(mapping.target.clone(), mapping.clone());
                ordered_overrides.push(mapping);
            }

            projection.apply(&replacements);
            if let Some(local) = local_by_source.get(source) {
                for index in local {
                    projection.insert_identity(items[*index].key.clone());
                }
            }

            for (origin, routes) in &projection.origins {
                if routes.by_terminal.len() <= 1 {
                    continue;
                }
                let mut artifacts = vec![origin.clone()];
                artifacts.extend(routes.by_terminal.keys().cloned());
                artifacts.sort();
                artifacts.dedup();
                findings.push(GraphCompositionFinding {
                    code: FINDING_OVERRIDE_DIVERGENCE,
                    reason: GraphFindingReason::DivergentTerminal,
                    owner_source: Some(source.clone()),
                    message: format!(
                        "source '{source}' leaves historical artifact '{}::{}' with {} effective terminals",
                        origin.source,
                        origin.canonical_id,
                        routes.by_terminal.len()
                    ),
                    artifacts,
                });
            }
            projections.insert(source.clone(), projection);
        }

        ordered_overrides.sort();
        if override_graph_has_cycle(&ordered_overrides) {
            findings.push(GraphCompositionFinding {
                code: FINDING_INVALID_OVERRIDE,
                reason: GraphFindingReason::OverrideCycle,
                owner_source: None,
                artifacts: ordered_overrides
                    .iter()
                    .flat_map(|mapping| [mapping.target.clone(), mapping.replacement.clone()])
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                message: "the compiled override graph contains a cycle".to_string(),
            });
        }
        if !findings.is_empty() {
            findings.sort_by(finding_order);
            findings.dedup();
            return Err(findings);
        }

        let root_projection = &projections[&input.root_source];
        let terminal_redirects: BTreeMap<ArtifactKey, ArtifactKey> = root_projection
            .origins
            .iter()
            .filter_map(|(origin, routes)| {
                let terminal = routes.by_terminal.keys().next()?;
                (origin != terminal).then(|| (origin.clone(), terminal.clone()))
            })
            .collect();
        let effective_keys: BTreeSet<ArtifactKey> = root_projection
            .origins
            .values()
            .flat_map(|routes| routes.by_terminal.keys().cloned())
            .collect();
        let root_effective = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| effective_keys.contains(&item.key).then_some(index))
            .collect();
        let root_local = local_by_source
            .get(&input.root_source)
            .cloned()
            .unwrap_or_default();
        let provenance = build_provenance(root_projection, &ordered_overrides);

        Ok(Self {
            root_source: input.root_source,
            items,
            item_by_key,
            local_by_source,
            root_local,
            root_effective,
            visibility: topology.visibility,
            projections,
            identifiers,
            canonical,
            topological_sources: topology.order,
            ordered_overrides,
            terminal_redirects,
            provenance,
        })
    }

    pub fn root_source(&self) -> &str {
        &self.root_source
    }

    pub fn catalog(&self) -> impl ExactSizeIterator<Item = &CorpusItem> {
        self.items.iter()
    }

    pub fn effective(&self) -> impl ExactSizeIterator<Item = &CorpusItem> {
        self.root_effective.iter().map(|index| &self.items[*index])
    }

    pub fn root_local(&self) -> impl ExactSizeIterator<Item = &CorpusItem> {
        self.root_local.iter().map(|index| &self.items[*index])
    }

    /// Existing-shape ranking adapter for command/cache/MCP consumers. The
    /// rows and inbound counts come only from the root-effective projection;
    /// no caller reconstructs a graph overlay.
    pub fn effective_index(&self) -> Vec<IndexEntry> {
        let mut inbound: BTreeMap<ArtifactKey, i64> = BTreeMap::new();
        for relationship in self.effective_relationships() {
            if relationship.issue.is_none() {
                if let Some(target) = relationship.effective_terminal {
                    *inbound.entry(target).or_default() += 1;
                }
            }
        }
        self.effective()
            .map(|item| entry_from_item(item, inbound.get(&item.key).copied().unwrap_or(0)))
            .collect()
    }

    /// Existing-shape exact-identity adapter. Effective unqualified aliases
    /// point at terminals; stable source-qualified and root-direct aliases
    /// continue to select source-owned catalog history.
    pub fn identity_index(&self) -> Vec<IndexEntry> {
        let effective: BTreeSet<&ArtifactKey> = self.effective().map(|item| &item.key).collect();
        let mut entries: Vec<IndexEntry> = self
            .catalog()
            .map(|item| {
                let mut entry = identity_entry_from_item(item);
                if !effective.contains(&item.key) {
                    entry.aliases.clear();
                }
                entry
                    .aliases
                    .push(format!("{}::{}", item.key.source, item.key.canonical_id));
                if let Some(alias) =
                    self.visibility
                        .aliases()
                        .iter()
                        .find_map(|((owner, alias), target)| {
                            (owner == &self.root_source && target == &item.key.source)
                                .then_some(alias.as_str())
                        })
                {
                    entry
                        .aliases
                        .push(format!("{alias}::{}", item.key.canonical_id));
                }
                entry.aliases.sort_by(|left, right| {
                    py_casefold(left)
                        .cmp(&py_casefold(right))
                        .then_with(|| left.cmp(right))
                });
                entry
                    .aliases
                    .dedup_by(|left, right| py_casefold(left) == py_casefold(right));
                entry
            })
            .collect();
        let entry_by_key: BTreeMap<ArtifactKey, usize> = entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.key.clone().map(|key| (key, index)))
            .collect();
        for (historical, terminal) in &self.terminal_redirects {
            let Some(index) = entry_by_key.get(terminal).copied() else {
                continue;
            };
            if !entries[index]
                .aliases
                .iter()
                .any(|alias| py_casefold(alias) == py_casefold(&historical.canonical_id))
            {
                entries[index].aliases.push(historical.canonical_id.clone());
            }
        }
        entries
    }

    pub fn local_to(&self, source: &str) -> impl Iterator<Item = &CorpusItem> {
        self.local_by_source
            .get(source)
            .into_iter()
            .flatten()
            .map(|index| &self.items[*index])
    }

    /// The effective view rooted at an immutable authoring source. This is
    /// the structural/relationship-validation projection for that node; root
    /// serving should use [`Self::effective`] instead.
    pub fn effective_from(&self, source: &str) -> Option<Vec<&CorpusItem>> {
        let projection = self.projections.get(source)?;
        let terminals: BTreeSet<&ArtifactKey> = projection
            .origins
            .values()
            .flat_map(|routes| routes.by_terminal.keys())
            .collect();
        Some(
            self.items
                .iter()
                .filter(|item| terminals.contains(&item.key))
                .collect(),
        )
    }

    pub fn terminal_from(&self, source: &str, key: &ArtifactKey) -> Option<&ArtifactKey> {
        self.projections.get(source)?.terminal_for(key)
    }

    pub fn item(&self, key: &ArtifactKey) -> Option<&CorpusItem> {
        self.item_by_key.get(key).map(|index| &self.items[*index])
    }

    pub fn visibility(&self) -> &SourceVisibility {
        &self.visibility
    }

    pub fn topological_sources(&self) -> &[String] {
        &self.topological_sources
    }

    pub fn ordered_overrides(&self) -> &[CompiledOverride] {
        &self.ordered_overrides
    }

    pub fn terminal_redirects(&self) -> &BTreeMap<ArtifactKey, ArtifactKey> {
        &self.terminal_redirects
    }

    pub fn provenance_for(&self, key: &ArtifactKey) -> Option<&[GraphOverrideProvenance]> {
        self.item_by_key
            .contains_key(key)
            .then(|| self.provenance.get(key).map(Vec::as_slice).unwrap_or(&[]))
    }

    /// Public root lookup. Qualified tokens select source-owned history;
    /// unqualified tokens select the one root-effective terminal.
    pub fn resolve_public(&self, reference: &str) -> Result<GraphResolution, GraphLookupError> {
        self.resolve_from(&self.root_source, reference)
    }

    /// Existing exact-lookup response adapter for public root consumers.
    pub fn resolve_identity(&self, reference: &str) -> ResolutionResult {
        match self.resolve_public(reference) {
            Ok(resolution) => self
                .item(&resolution.selected)
                .map(|item| ResolutionResult {
                    artifact_id: reference.to_string(),
                    outcome: OUTCOME_RESOLVED,
                    artifact: Some(resolved_from_entry(&identity_entry_from_item(item))),
                    duplicate_paths: Vec::new(),
                })
                .unwrap_or_else(|| not_found_resolution(reference)),
            Err(GraphLookupError::Ambiguous {
                historical_candidates,
                ..
            }) => {
                let mut paths: Vec<String> = historical_candidates
                    .iter()
                    .filter_map(|key| self.item(key))
                    .map(|item| {
                        format!(
                            "{}::{}",
                            item.artifact_path.source, item.artifact_path.relative_path
                        )
                    })
                    .collect();
                paths.sort();
                paths.dedup();
                ResolutionResult {
                    artifact_id: reference.to_string(),
                    outcome: OUTCOME_DUPLICATE,
                    artifact: None,
                    duplicate_paths: paths,
                }
            }
            Err(_) => not_found_resolution(reference),
        }
    }

    /// Resolve using the immutable authoring source's visibility and aliases.
    pub fn resolve_from(
        &self,
        context: &str,
        reference: &str,
    ) -> Result<GraphResolution, GraphLookupError> {
        let analyzed = self.analyze_reference(context, reference)?;
        let selected = if analyzed.qualified {
            analyzed.historical_candidates[0].clone()
        } else {
            analyzed.effective_terminal.clone()
        };
        Ok(GraphResolution {
            historical_candidates: analyzed.historical_candidates,
            selected,
            effective_terminal: analyzed.effective_terminal,
            qualified: analyzed.qualified,
        })
    }

    pub fn catalog_relationships(&self) -> Vec<GraphRelationship> {
        self.items
            .iter()
            .flat_map(|item| self.relationships_for(item, false))
            .collect()
    }

    pub fn effective_relationships(&self) -> Vec<GraphRelationship> {
        self.effective()
            .flat_map(|item| self.relationships_for(item, true))
            .collect()
    }

    /// Existing live relationship shape. The richer catalog history remains
    /// available only through [`Self::catalog_relationships`] because the v1
    /// `Relationship` type cannot represent several historical candidates and
    /// a separate terminal without losing information.
    pub fn relationships(&self) -> Vec<Relationship> {
        self.effective_relationships()
            .into_iter()
            .map(|relationship| self.compatible_relationship(relationship))
            .collect()
    }

    fn compatible_relationship(&self, relationship: GraphRelationship) -> Relationship {
        let source_item = self.item(&relationship.source);
        let target_item = relationship
            .effective_terminal
            .as_ref()
            .and_then(|key| self.item(key));
        Relationship {
            source_artifact: source_item.map(|item| item.artifact_path.clone()),
            source_path: source_item
                .map(|item| item.path.clone())
                .unwrap_or_default(),
            relationship: relationship.relationship,
            target: relationship.authored_token,
            resolved_artifact: target_item.map(|item| item.artifact_path.clone()),
            resolved_path: target_item.map(|item| item.path.clone()),
            issue: relationship.issue.map(|issue| match issue {
                GraphRelationshipIssue::TargetNotFound => ISSUE_TARGET_NOT_FOUND.to_string(),
                GraphRelationshipIssue::TargetAmbiguous => ISSUE_TARGET_AMBIGUOUS.to_string(),
                GraphRelationshipIssue::SelfReference => ISSUE_SELF_REFERENCE.to_string(),
            }),
        }
    }

    fn relationships_for(&self, item: &CorpusItem, root_effective: bool) -> Vec<GraphRelationship> {
        let Some(spec) = item.spec else {
            return Vec::new();
        };
        let mut output = Vec::new();
        for (relationship, references) in extract_relationships_full(&item.artifact, spec) {
            let external = edge_spec(&relationship).is_some_and(|edge| edge.external);
            for reference in references {
                if external {
                    output.push(GraphRelationship {
                        source: item.key.clone(),
                        relationship: relationship.clone(),
                        authored_token: reference,
                        historical_candidates: Vec::new(),
                        effective_terminal: None,
                        issue: None,
                        external: true,
                    });
                    continue;
                }
                let analyzed = self.analyze_reference(&item.key.source, &reference);
                let mut endpoint = match analyzed {
                    Ok(analyzed) => GraphRelationship {
                        source: item.key.clone(),
                        relationship: relationship.clone(),
                        authored_token: reference,
                        historical_candidates: analyzed.historical_candidates,
                        effective_terminal: Some(analyzed.effective_terminal),
                        issue: None,
                        external: false,
                    },
                    Err(GraphLookupError::Ambiguous {
                        historical_candidates,
                        ..
                    }) => GraphRelationship {
                        source: item.key.clone(),
                        relationship: relationship.clone(),
                        authored_token: reference,
                        historical_candidates,
                        effective_terminal: None,
                        issue: Some(GraphRelationshipIssue::TargetAmbiguous),
                        external: false,
                    },
                    Err(_) => GraphRelationship {
                        source: item.key.clone(),
                        relationship: relationship.clone(),
                        authored_token: reference,
                        historical_candidates: Vec::new(),
                        effective_terminal: None,
                        issue: Some(GraphRelationshipIssue::TargetNotFound),
                        external: false,
                    },
                };
                if root_effective {
                    if let Some(terminal) = endpoint.effective_terminal.as_ref() {
                        endpoint.effective_terminal = self
                            .projections
                            .get(&self.root_source)
                            .and_then(|projection| projection.terminal_for(terminal))
                            .cloned();
                    }
                }
                if endpoint.effective_terminal.as_ref() == Some(&item.key) {
                    endpoint.effective_terminal = None;
                    endpoint.issue = Some(GraphRelationshipIssue::SelfReference);
                }
                output.push(endpoint);
            }
        }
        output
    }

    fn analyze_reference(
        &self,
        context: &str,
        reference: &str,
    ) -> Result<AnalyzedReference, GraphLookupError> {
        let Some(projection) = self.projections.get(context) else {
            return Err(GraphLookupError::UnknownContext);
        };
        if reference.contains("::") {
            return self.analyze_qualified(context, reference, projection);
        }

        let folded = py_casefold(reference);
        let visible = self
            .visibility
            .visible_from(context)
            .ok_or(GraphLookupError::UnknownContext)?;
        let mut historical_candidates: Vec<ArtifactKey> = visible
            .iter()
            .flat_map(|source| {
                self.identifiers
                    .get(source)
                    .and_then(|identifiers| identifiers.get(&folded))
                    .into_iter()
                    .flatten()
                    .cloned()
            })
            .collect();
        historical_candidates.sort();
        historical_candidates.dedup();
        if historical_candidates.is_empty() {
            return Err(GraphLookupError::NotFound);
        }
        let effective_candidates: BTreeSet<ArtifactKey> = historical_candidates
            .iter()
            .filter_map(|candidate| projection.terminal_for(candidate).cloned())
            .collect();
        if effective_candidates.len() != 1 {
            return Err(GraphLookupError::Ambiguous {
                historical_candidates,
                effective_candidates: effective_candidates.into_iter().collect(),
            });
        }
        Ok(AnalyzedReference {
            historical_candidates,
            effective_terminal: effective_candidates.into_iter().next().unwrap(),
            qualified: false,
        })
    }

    fn analyze_qualified(
        &self,
        context: &str,
        reference: &str,
        projection: &EffectiveProjection,
    ) -> Result<AnalyzedReference, GraphLookupError> {
        let Some((qualifier, canonical_id)) = reference.split_once("::") else {
            return Err(GraphLookupError::InvalidQualifiedReference);
        };
        if qualifier.is_empty() || canonical_id.is_empty() || canonical_id.contains("::") {
            return Err(GraphLookupError::InvalidQualifiedReference);
        }
        let target_source = if qualifier.contains('/') {
            if !self.visibility.is_visible(context, qualifier) {
                return Err(GraphLookupError::NotFound);
            }
            qualifier
        } else {
            self.visibility
                .alias_target(context, qualifier)
                .ok_or(GraphLookupError::NotFound)?
        };
        let folded = py_casefold(canonical_id);
        let Some(key) = self
            .canonical
            .get(&(target_source.to_string(), folded.clone()))
        else {
            let alias_exists = self
                .identifiers
                .get(target_source)
                .and_then(|identifiers| identifiers.get(&folded))
                .is_some();
            return Err(if alias_exists {
                GraphLookupError::QualifiedCanonicalRequired
            } else {
                GraphLookupError::NotFound
            });
        };
        let effective_terminal = projection
            .terminal_for(key)
            .cloned()
            .ok_or(GraphLookupError::NotFound)?;
        Ok(AnalyzedReference {
            historical_candidates: vec![key.clone()],
            effective_terminal,
            qualified: true,
        })
    }
}

struct AnalyzedReference {
    historical_candidates: Vec<ArtifactKey>,
    effective_terminal: ArtifactKey,
    qualified: bool,
}

struct PreparedTopology {
    nodes: BTreeMap<String, SourceNodeInput>,
    order: Vec<String>,
    rank: BTreeMap<String, usize>,
    visibility: SourceVisibility,
}

impl PreparedTopology {
    fn prepare(
        root_source: &str,
        nodes: Vec<SourceNodeInput>,
    ) -> Result<Self, Vec<GraphCompositionFinding>> {
        let mut findings = Vec::new();
        let mut by_source = BTreeMap::new();
        for mut node in nodes {
            node.direct_parents.sort();
            if by_source.contains_key(&node.source) {
                findings.push(graph_finding(
                    GraphFindingReason::DuplicateSource,
                    Some(node.source.clone()),
                    format!(
                        "logical source '{}' is declared more than once",
                        node.source
                    ),
                ));
            } else {
                by_source.insert(node.source.clone(), node);
            }
        }
        if !by_source.contains_key(root_source) {
            findings.push(graph_finding(
                GraphFindingReason::MissingRoot,
                Some(root_source.to_string()),
                format!("root source '{root_source}' is absent from the logical graph"),
            ));
        }

        for node in by_source.values() {
            let mut sources = BTreeSet::new();
            let mut aliases = BTreeSet::new();
            for parent in &node.direct_parents {
                if !valid_source_alias(&parent.alias) {
                    findings.push(graph_finding(
                        GraphFindingReason::InvalidAlias,
                        Some(node.source.clone()),
                        format!(
                            "source '{}' declares invalid edge alias '{}'",
                            node.source, parent.alias
                        ),
                    ));
                }
                if !by_source.contains_key(&parent.source) {
                    findings.push(graph_finding(
                        GraphFindingReason::UnknownParent,
                        Some(node.source.clone()),
                        format!(
                            "source '{}' declares unknown parent '{}'",
                            node.source, parent.source
                        ),
                    ));
                }
                if !sources.insert(parent.source.clone()) {
                    findings.push(graph_finding(
                        GraphFindingReason::DuplicateParent,
                        Some(node.source.clone()),
                        format!(
                            "source '{}' declares parent '{}' more than once",
                            node.source, parent.source
                        ),
                    ));
                }
                if !aliases.insert(parent.alias.clone()) {
                    findings.push(graph_finding(
                        GraphFindingReason::DuplicateAlias,
                        Some(node.source.clone()),
                        format!(
                            "source '{}' declares alias '{}' more than once",
                            node.source, parent.alias
                        ),
                    ));
                }
            }
        }
        if !findings.is_empty() {
            findings.sort_by(finding_order);
            findings.dedup();
            return Err(findings);
        }

        let mut reachable = BTreeSet::new();
        let mut pending = vec![root_source.to_string()];
        while let Some(source) = pending.pop() {
            if !reachable.insert(source.clone()) {
                continue;
            }
            pending.extend(
                by_source[&source]
                    .direct_parents
                    .iter()
                    .map(|parent| parent.source.clone()),
            );
        }
        for source in by_source.keys() {
            if !reachable.contains(source) {
                findings.push(graph_finding(
                    GraphFindingReason::UnreachableSource,
                    Some(source.clone()),
                    format!("source '{source}' is not reachable from root '{root_source}'"),
                ));
            }
        }

        let mut indegree: BTreeMap<String, usize> = by_source
            .iter()
            .map(|(source, node)| (source.clone(), node.direct_parents.len()))
            .collect();
        let mut children: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for node in by_source.values() {
            for parent in &node.direct_parents {
                children
                    .entry(parent.source.clone())
                    .or_default()
                    .insert(node.source.clone());
            }
        }
        let mut ready: BTreeSet<String> = indegree
            .iter()
            .filter_map(|(source, count)| (*count == 0).then_some(source.clone()))
            .collect();
        let mut order = Vec::with_capacity(by_source.len());
        while let Some(source) = ready.pop_first() {
            order.push(source.clone());
            for child in children.get(&source).into_iter().flatten() {
                let remaining = indegree.get_mut(child).unwrap();
                *remaining -= 1;
                if *remaining == 0 {
                    ready.insert(child.clone());
                }
            }
        }
        if order.len() != by_source.len() {
            let cyclic: Vec<String> = indegree
                .into_iter()
                .filter_map(|(source, count)| (count != 0).then_some(source))
                .collect();
            findings.push(GraphCompositionFinding {
                code: FINDING_INVALID_GRAPH,
                reason: GraphFindingReason::Cycle,
                owner_source: cyclic.first().cloned(),
                artifacts: Vec::new(),
                message: format!(
                    "source graph contains a cycle through {}",
                    cyclic.join(", ")
                ),
            });
        }
        if !findings.is_empty() {
            findings.sort_by(finding_order);
            findings.dedup();
            return Err(findings);
        }

        let rank = order
            .iter()
            .enumerate()
            .map(|(rank, source)| (source.clone(), rank))
            .collect();
        let mut visible: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut aliases = BTreeMap::new();
        for source in &order {
            let mut sources = BTreeSet::from([source.clone()]);
            for parent in &by_source[source].direct_parents {
                sources.extend(visible[&parent.source].iter().cloned());
                aliases.insert(
                    (source.clone(), parent.alias.clone()),
                    parent.source.clone(),
                );
            }
            visible.insert(source.clone(), sources);
        }
        Ok(Self {
            nodes: by_source,
            order,
            rank,
            visibility: SourceVisibility { visible, aliases },
        })
    }
}

struct PreparedItems {
    items: Vec<CorpusItem>,
    item_by_key: BTreeMap<ArtifactKey, usize>,
    local_by_source: BTreeMap<String, Vec<usize>>,
    identifiers: BTreeMap<String, BTreeMap<String, Vec<ArtifactKey>>>,
    canonical: BTreeMap<(String, String), ArtifactKey>,
}

impl PreparedItems {
    fn prepare(
        root_source: &str,
        nodes: &BTreeMap<String, SourceNodeInput>,
        mut items: Vec<CorpusItem>,
    ) -> Result<Self, Vec<GraphCompositionFinding>> {
        items.sort_by(|left, right| {
            left.artifact_path
                .cmp(&right.artifact_path)
                .then_with(|| left.key.cmp(&right.key))
        });
        let mut findings = Vec::new();
        let mut item_by_key = BTreeMap::new();
        let mut local_by_source: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut identifiers: BTreeMap<String, BTreeMap<String, Vec<ArtifactKey>>> = BTreeMap::new();
        let mut canonical = BTreeMap::new();

        for (index, item) in items.iter().enumerate() {
            if !nodes.contains_key(&item.key.source) {
                findings.push(item_finding(
                    GraphFindingReason::UnknownItemSource,
                    item,
                    format!("artifact belongs to unknown source '{}'", item.key.source),
                ));
                continue;
            }
            if item.key.source != item.origin.source || item.key.source != item.artifact_path.source
            {
                findings.push(item_finding(
                    GraphFindingReason::IdentitySourceMismatch,
                    item,
                    "artifact key, path, and origin sources differ".to_string(),
                ));
                continue;
            }
            let expected_layer = if item.key.source == root_source {
                Layer::Local
            } else {
                Layer::Inherited
            };
            if item.origin.layer != expected_layer {
                findings.push(item_finding(
                    GraphFindingReason::IdentitySourceMismatch,
                    item,
                    format!(
                        "source '{}' has layer '{}' but graph root semantics require '{}'",
                        item.key.source,
                        item.origin.layer.as_str(),
                        expected_layer.as_str()
                    ),
                ));
                continue;
            }
            if item_by_key.insert(item.key.clone(), index).is_some() {
                findings.push(item_finding(
                    GraphFindingReason::DuplicateArtifactKey,
                    item,
                    format!(
                        "artifact key '{}::{}' occurs more than once",
                        item.key.source, item.key.canonical_id
                    ),
                ));
                continue;
            }
            local_by_source
                .entry(item.key.source.clone())
                .or_default()
                .push(index);
            let canonical_key = (item.key.source.clone(), py_casefold(&item.key.canonical_id));
            if let Some(existing) = canonical.insert(canonical_key, item.key.clone()) {
                findings.push(GraphCompositionFinding {
                    code: FINDING_INVALID_GRAPH,
                    reason: GraphFindingReason::DuplicateArtifactKey,
                    owner_source: Some(item.key.source.clone()),
                    artifacts: vec![existing, item.key.clone()],
                    message: format!(
                        "source '{}' contains duplicate canonical id '{}'",
                        item.key.source, item.key.canonical_id
                    ),
                });
                continue;
            }
            for identifier in artifact_identifiers(&item.artifact, item.spec, &item.path) {
                identifiers
                    .entry(item.key.source.clone())
                    .or_default()
                    .entry(py_casefold(&identifier))
                    .or_default()
                    .push(item.key.clone());
            }
        }
        for source_identifiers in identifiers.values_mut() {
            for keys in source_identifiers.values_mut() {
                keys.sort();
                keys.dedup();
            }
        }
        if !findings.is_empty() {
            findings.sort_by(finding_order);
            findings.dedup();
            return Err(findings);
        }
        Ok(Self {
            items,
            item_by_key,
            local_by_source,
            identifiers,
            canonical,
        })
    }
}

fn prepare_override_declarations(
    mut declarations: Vec<GraphOverrideDeclaration>,
    topology: &PreparedTopology,
    item_by_key: &BTreeMap<ArtifactKey, usize>,
    items: &[CorpusItem],
) -> Result<BTreeMap<String, Vec<GraphOverrideDeclaration>>, Vec<GraphCompositionFinding>> {
    declarations.sort();
    let mut findings = Vec::new();
    let mut by_owner: BTreeMap<String, Vec<GraphOverrideDeclaration>> = BTreeMap::new();
    let mut targets = BTreeSet::new();
    for declaration in declarations {
        let owner = declaration.owner_source.clone();
        if !topology.nodes.contains_key(&owner) {
            findings.push(override_finding(
                GraphFindingReason::UnknownOverrideOwner,
                &declaration,
                Vec::new(),
            ));
            continue;
        }
        if !targets.insert((owner.clone(), declaration.target.clone())) {
            findings.push(override_finding(
                GraphFindingReason::DuplicateOverrideTarget,
                &declaration,
                vec![declaration.target.clone()],
            ));
            continue;
        }
        if declaration.replacement.source != owner {
            findings.push(override_finding(
                GraphFindingReason::ReplacementNotLocal,
                &declaration,
                vec![declaration.replacement.clone()],
            ));
            continue;
        }
        if declaration.rationale.source != owner {
            findings.push(override_finding(
                GraphFindingReason::RationaleNotLocal,
                &declaration,
                vec![declaration.rationale.clone()],
            ));
            continue;
        }
        let Some(replacement_index) = item_by_key.get(&declaration.replacement).copied() else {
            findings.push(override_finding(
                GraphFindingReason::ReplacementNotFound,
                &declaration,
                vec![declaration.replacement.clone()],
            ));
            continue;
        };
        let Some(rationale_index) = item_by_key.get(&declaration.rationale).copied() else {
            findings.push(override_finding(
                GraphFindingReason::RationaleNotFound,
                &declaration,
                vec![declaration.rationale.clone()],
            ));
            continue;
        };
        let Some(target_index) = item_by_key.get(&declaration.target).copied() else {
            findings.push(override_finding(
                GraphFindingReason::TargetNotInherited,
                &declaration,
                vec![declaration.target.clone()],
            ));
            continue;
        };
        let replacement = &items[replacement_index];
        let rationale = &items[rationale_index];
        let target = &items[target_index];
        if rationale.spec.map(|spec| spec.name.as_str()) != Some("decision") {
            findings.push(override_finding(
                GraphFindingReason::RationaleNotDecision,
                &declaration,
                vec![declaration.rationale.clone()],
            ));
            continue;
        }
        if !is_live_decision(&rationale.artifact) {
            findings.push(override_finding(
                GraphFindingReason::RationaleNotLive,
                &declaration,
                vec![declaration.rationale.clone()],
            ));
            continue;
        }
        let target_type = target.spec.map(|spec| spec.name.as_str());
        let replacement_type = replacement.spec.map(|spec| spec.name.as_str());
        if target_type.is_none() || target_type != replacement_type {
            findings.push(override_finding(
                GraphFindingReason::TypeMismatch,
                &declaration,
                vec![declaration.target.clone(), declaration.replacement.clone()],
            ));
            continue;
        }
        by_owner.entry(owner).or_default().push(declaration);
    }

    for (owner, owner_declarations) in &by_owner {
        let targets: BTreeSet<&ArtifactKey> = owner_declarations
            .iter()
            .map(|declaration| &declaration.target)
            .collect();
        for declaration in owner_declarations {
            if targets.contains(&declaration.replacement) {
                findings.push(override_finding(
                    GraphFindingReason::SameManifestChain,
                    declaration,
                    vec![declaration.target.clone(), declaration.replacement.clone()],
                ));
            }
        }
        if owner_declarations
            .iter()
            .any(|declaration| declaration.target.source == *owner)
        {
            // A target local to its declaring source is also caught by the
            // bottom-up inherited-terminal check. Record it here so malformed
            // input cannot become order-dependent.
            for declaration in owner_declarations
                .iter()
                .filter(|declaration| declaration.target.source == *owner)
            {
                findings.push(override_finding(
                    GraphFindingReason::TargetNotInherited,
                    declaration,
                    vec![declaration.target.clone()],
                ));
            }
        }
    }

    if !findings.is_empty() {
        findings.sort_by(finding_order);
        findings.dedup();
        return Err(findings);
    }
    Ok(by_owner)
}

fn build_provenance(
    root: &EffectiveProjection,
    ordered: &[CompiledOverride],
) -> BTreeMap<ArtifactKey, Vec<GraphOverrideProvenance>> {
    let mut states: BTreeMap<ArtifactKey, BTreeMap<CompiledOverride, GraphOverrideState>> =
        BTreeMap::new();
    for (origin, routes) in &root.origins {
        for paths in routes.by_terminal.values() {
            for path in paths {
                let mut carrying = BTreeSet::from([origin.clone()]);
                carrying.extend(path.iter().map(|mapping| mapping.replacement.clone()));
                for artifact in carrying {
                    let artifact_states = states.entry(artifact.clone()).or_default();
                    for mapping in path {
                        let state = if artifact == mapping.target {
                            GraphOverrideState::Overridden
                        } else if artifact == mapping.replacement {
                            GraphOverrideState::Replacement
                        } else {
                            GraphOverrideState::Lineage
                        };
                        artifact_states.entry(mapping.clone()).or_insert(state);
                    }
                }
            }
        }
    }
    states
        .into_iter()
        .map(|(artifact, mappings)| {
            let entries = ordered
                .iter()
                .filter_map(|mapping| {
                    mappings
                        .get(mapping)
                        .copied()
                        .map(|state| GraphOverrideProvenance {
                            state,
                            owner_source: mapping.owner_source.clone(),
                            target: mapping.target.clone(),
                            replacement: mapping.replacement.clone(),
                            rationale: mapping.rationale.clone(),
                        })
                })
                .collect();
            (artifact, entries)
        })
        .collect()
}

fn override_graph_has_cycle(mappings: &[CompiledOverride]) -> bool {
    let outgoing: BTreeMap<&ArtifactKey, &ArtifactKey> = mappings
        .iter()
        .map(|mapping| (&mapping.target, &mapping.replacement))
        .collect();
    for start in outgoing.keys() {
        let mut seen = BTreeSet::new();
        let mut current = *start;
        while let Some(next) = outgoing.get(current) {
            if !seen.insert(current) {
                return true;
            }
            current = next;
        }
    }
    false
}

fn valid_source_alias(alias: &str) -> bool {
    let mut characters = alias.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_' | '.')
        })
}

fn not_found_resolution(reference: &str) -> ResolutionResult {
    ResolutionResult {
        artifact_id: reference.to_string(),
        outcome: OUTCOME_NOT_FOUND,
        artifact: None,
        duplicate_paths: Vec::new(),
    }
}

fn graph_finding(
    reason: GraphFindingReason,
    owner_source: Option<String>,
    message: String,
) -> GraphCompositionFinding {
    GraphCompositionFinding {
        code: FINDING_INVALID_GRAPH,
        reason,
        owner_source,
        artifacts: Vec::new(),
        message,
    }
}

fn item_finding(
    reason: GraphFindingReason,
    item: &CorpusItem,
    message: String,
) -> GraphCompositionFinding {
    GraphCompositionFinding {
        code: FINDING_INVALID_GRAPH,
        reason,
        owner_source: Some(item.key.source.clone()),
        artifacts: vec![item.key.clone()],
        message,
    }
}

fn override_finding(
    reason: GraphFindingReason,
    declaration: &GraphOverrideDeclaration,
    mut artifacts: Vec<ArtifactKey>,
) -> GraphCompositionFinding {
    artifacts.sort();
    artifacts.dedup();
    GraphCompositionFinding {
        code: FINDING_INVALID_OVERRIDE,
        reason,
        owner_source: Some(declaration.owner_source.clone()),
        artifacts,
        message: format!(
            "override '{}::{}' -> '{}::{}' in source '{}' is invalid: {}",
            declaration.target.source,
            declaration.target.canonical_id,
            declaration.replacement.source,
            declaration.replacement.canonical_id,
            declaration.owner_source,
            reason.as_str()
        ),
    }
}

fn finding_order(
    left: &GraphCompositionFinding,
    right: &GraphCompositionFinding,
) -> std::cmp::Ordering {
    left.code
        .cmp(right.code)
        .then_with(|| left.owner_source.cmp(&right.owner_source))
        .then_with(|| left.artifacts.cmp(&right.artifacts))
        .then_with(|| left.reason.cmp(&right.reason))
        .then_with(|| left.message.cmp(&right.message))
}
