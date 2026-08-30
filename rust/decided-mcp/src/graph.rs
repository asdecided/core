//! Generation-bound `get_related` graph view. Identity, incoming, outgoing,
//! and adjacency indexes are built once per freshness generation, then reused
//! by every graph call until the corpus changes.

use rac_engine::corpus::{ArtifactKey, ArtifactOrigin, ArtifactPath, Layer};
use rac_engine::freshness::TrackerModel;
use rac_engine::relationships::{corpus_items, relationships_from_corpus, Relationship};
use rac_engine::resolve::{index_from_items, IndexEntry, ResolutionResult, ResolvedArtifact};
use rac_engine::spec::{snake, RELATIONSHIP_SECTIONS};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

// Traversal caps (`decided.core.limits`).
pub const MAX_RELATED_EDGES: usize = 1000;
pub const MAX_TRAVERSAL_DEPTH: i64 = 5;
pub const MAX_TRAVERSAL_FRONTIER: usize = 1000;
pub const MAX_TRAVERSAL_WORK: i64 = 10_000;

/// Rank of a snake_case relationship section in the canonical order
/// (`_RELATIONSHIP_ORDER`); unknown sections rank last.
fn relationship_order(section: &str) -> usize {
    for (i, (name, _)) in RELATIONSHIP_SECTIONS.iter().enumerate() {
        if snake(name) == section {
            return i;
        }
    }
    RELATIONSHIP_SECTIONS.len()
}

fn stable_entry_order(left: &IndexEntry, right: &IndexEntry) -> std::cmp::Ordering {
    left.artifact_path
        .cmp(&right.artifact_path)
        .then_with(|| left.path.cmp(&right.path))
        .then_with(|| left.id.cmp(&right.id))
}

fn public_path(entry: &IndexEntry, federated: bool) -> String {
    if federated {
        if let Some(path) = &entry.artifact_path {
            return path.relative_path.clone();
        }
    }
    entry.path.clone()
}

pub struct OutgoingReferences {
    /// Section (snake_case) → raw stored targets, first-seen section order.
    pub by_section: Vec<(String, Vec<String>)>,
    pub total: usize,
}

impl OutgoingReferences {
    pub fn kept(&self) -> usize {
        self.by_section.iter().map(|(_, targets)| targets.len()).sum()
    }

    pub fn to_value(&self) -> Value {
        let mut map = Map::new();
        for (section, targets) in &self.by_section {
            map.insert(section.clone(), json!(targets));
        }
        Value::Object(map)
    }
}

pub struct IncomingReference {
    pub key: Option<ArtifactKey>,
    pub origin: Option<ArtifactOrigin>,
    pub id: String,
    pub artifact_type: String,
    pub title: Option<String>,
    pub path: String,
    pub section: String,
    pub target: String,
}

pub struct IncomingReferences {
    pub items: Vec<IncomingReference>,
    pub total: usize,
}

pub struct NeighborhoodNode {
    pub key: Option<ArtifactKey>,
    pub origin: Option<ArtifactOrigin>,
    pub id: String,
    pub artifact_type: String,
    pub title: Option<String>,
    pub path: String,
    pub hops: i64,
}

pub struct Neighborhood {
    pub nodes: Vec<NeighborhoodNode>,
    pub truncated: bool,
}

struct RelationshipProjection {
    relationships: Vec<Relationship>,
    outgoing_by_source: Vec<Vec<usize>>,
    incoming_by_target: Vec<Vec<usize>>,
    adjacency: Vec<Vec<(usize, usize)>>,
}

/// Immutable graph projection for one logical corpus generation.
pub struct GraphView {
    entries: Vec<IndexEntry>,
    entry_by_path: HashMap<String, usize>,
    entry_by_artifact_path: HashMap<ArtifactPath, usize>,
    effective_graph: RelationshipProjection,
    historical_graph: Option<RelationshipProjection>,
    federated: bool,
}

impl GraphView {
    pub fn from_model(model: &TrackerModel) -> Self {
        match model {
            TrackerModel::View(reader) => Self::new(
                rac_engine::read_model::store_identity_entries(reader),
                reader.relationships().unwrap_or_default(),
            ),
            TrackerModel::Snapshot(derived) => Self::new(
                derived
                    .index_entries
                    .iter()
                    .map(identity_projection)
                    .collect(),
                derived.relationships.clone(),
            ),
            TrackerModel::Delta(generation) => Self::new(
                generation.identity.entries(),
                generation.graph.relationships(),
            ),
        }
    }

    pub fn fresh(root: &str) -> Self {
        let corpus = corpus_items(root, true);
        Self::new(index_from_items(&corpus), relationships_from_corpus(&corpus))
    }

    pub fn from_composed(corpus: &rac_engine::composition::ComposedCorpus) -> Self {
        Self::new_with_history(
            corpus.identity_index(),
            corpus.relationships(),
            Some(corpus.catalog_relationships()),
        )
    }

    pub fn new(entries: Vec<IndexEntry>, relationships: Vec<Relationship>) -> Self {
        Self::new_with_history(entries, relationships, None)
    }

    fn new_with_history(
        entries: Vec<IndexEntry>,
        relationships: Vec<Relationship>,
        historical_relationships: Option<Vec<Relationship>>,
    ) -> Self {
        let mut entry_by_path = HashMap::with_capacity(entries.len());
        let mut entry_by_artifact_path = HashMap::with_capacity(entries.len());
        let federated = entries.iter().any(|entry| {
            entry
                .origin
                .as_ref()
                .is_some_and(|origin| origin.layer == Layer::Inherited)
        });
        for (index, entry) in entries.iter().enumerate() {
            entry_by_path.insert(entry.path.clone(), index);
            if let Some(path) = &entry.artifact_path {
                entry_by_artifact_path.insert(path.clone(), index);
            }
        }

        let effective_graph = Self::relationship_projection(
            entries.len(),
            &entry_by_path,
            &entry_by_artifact_path,
            relationships,
        );
        let historical_graph = historical_relationships.map(|relationships| {
            Self::relationship_projection(
                entries.len(),
                &entry_by_path,
                &entry_by_artifact_path,
                relationships,
            )
        });

        Self {
            entries,
            entry_by_path,
            entry_by_artifact_path,
            effective_graph,
            historical_graph,
            federated,
        }
    }

    fn relationship_projection(
        entry_count: usize,
        entry_by_path: &HashMap<String, usize>,
        entry_by_artifact_path: &HashMap<ArtifactPath, usize>,
        relationships: Vec<Relationship>,
    ) -> RelationshipProjection {
        let mut outgoing_by_source = vec![Vec::new(); entry_count];
        let mut incoming_by_target = vec![Vec::new(); entry_count];
        let mut adjacency = vec![Vec::new(); entry_count];
        for (index, relationship) in relationships.iter().enumerate() {
            let source_index = relationship
                .source_artifact
                .as_ref()
                .and_then(|path| entry_by_artifact_path.get(path))
                .or_else(|| entry_by_path.get(&relationship.source_path))
                .copied();
            let Some(source_index) = source_index else {
                continue;
            };
            outgoing_by_source[source_index].push(index);
            let target_index = relationship
                .resolved_artifact
                .as_ref()
                .and_then(|path| entry_by_artifact_path.get(path))
                .copied()
                .or_else(|| {
                    relationship
                        .resolved_path
                        .as_deref()
                        .and_then(|path| entry_by_path.get(path).copied())
                });
            let Some(target_index) = target_index else {
                continue;
            };
            incoming_by_target[target_index].push(index);
            if source_index == target_index {
                continue;
            }
            let rank = relationship_order(&relationship.relationship);
            adjacency[source_index].push((target_index, rank));
            adjacency[target_index].push((source_index, rank));
        }

        RelationshipProjection {
            relationships,
            outgoing_by_source,
            incoming_by_target,
            adjacency,
        }
    }

    fn graph(&self, historical: bool) -> &RelationshipProjection {
        if historical {
            self.historical_graph
                .as_ref()
                .unwrap_or(&self.effective_graph)
        } else {
            &self.effective_graph
        }
    }

    fn entry_index(&self, artifact: &ResolvedArtifact) -> Option<usize> {
        artifact
            .artifact_path
            .as_ref()
            .and_then(|path| self.entry_by_artifact_path.get(path))
            .copied()
            .or_else(|| self.entry_by_path.get(&artifact.path).copied())
    }

    pub fn resolve(&self, artifact_id: &str) -> ResolutionResult {
        rac_engine::resolve::resolve_in_index(&self.entries, artifact_id)
    }

    pub fn outgoing(
        &self,
        artifact: &ResolvedArtifact,
        historical: bool,
    ) -> OutgoingReferences {
        let graph = self.graph(historical);
        let indexes = self
            .entry_index(artifact)
            .map(|index| graph.outgoing_by_source[index].as_slice())
            .unwrap_or(&[]);
        let mut by_section: Vec<(String, Vec<String>)> = Vec::new();
        for index in indexes.iter().take(MAX_RELATED_EDGES) {
            let relationship = &graph.relationships[*index];
            match by_section
                .iter_mut()
                .find(|(section, _)| *section == relationship.relationship)
            {
                Some((_, targets)) => targets.push(relationship.target.clone()),
                None => by_section.push((
                    relationship.relationship.clone(),
                    vec![relationship.target.clone()],
                )),
            }
        }
        OutgoingReferences {
            by_section,
            total: indexes.len(),
        }
    }

    pub fn incoming(
        &self,
        artifact: &ResolvedArtifact,
        historical: bool,
    ) -> IncomingReferences {
        let graph = self.graph(historical);
        let target_index = self.entry_index(artifact);
        let indexes = target_index
            .map(|index| graph.incoming_by_target[index].as_slice())
            .unwrap_or(&[]);
        let mut incoming = Vec::new();
        let mut total = 0usize;
        for index in indexes {
            let relationship = &graph.relationships[*index];
            let entry_index = relationship
                .source_artifact
                .as_ref()
                .and_then(|path| self.entry_by_artifact_path.get(path))
                .or_else(|| self.entry_by_path.get(&relationship.source_path))
                .copied();
            let Some(entry_index) = entry_index else {
                continue;
            };
            if Some(entry_index) == target_index {
                continue;
            }
            total += 1;
            if incoming.len() < MAX_RELATED_EDGES {
                let entry = &self.entries[entry_index];
                incoming.push(IncomingReference {
                    key: entry.key.clone(),
                    origin: entry.origin.clone(),
                    id: entry.id.clone(),
                    artifact_type: entry.artifact_type.clone(),
                    title: entry.title.clone(),
                    path: public_path(entry, self.federated),
                    section: relationship.relationship.clone(),
                    target: relationship.target.clone(),
                });
            }
        }
        let mut decorated: Vec<(usize, IncomingReference)> = incoming
            .into_iter()
            .map(|reference| (relationship_order(&reference.section), reference))
            .collect();
        decorated.sort_by(|a, b| {
            (a.0, &a.1.id, &a.1.path).cmp(&(b.0, &b.1.id, &b.1.path))
        });
        IncomingReferences {
            items: decorated.into_iter().map(|(_, reference)| reference).collect(),
            total,
        }
    }

    pub fn neighborhood(
        &self,
        artifact: &ResolvedArtifact,
        depth: i64,
        historical: bool,
    ) -> Neighborhood {
        let graph = self.graph(historical);
        let depth = depth.clamp(0, MAX_TRAVERSAL_DEPTH);
        let Some(origin_index) = self.entry_index(artifact) else {
            return Neighborhood {
                nodes: Vec::new(),
                truncated: false,
            };
        };
        let mut visited: HashSet<usize> = HashSet::new();
        visited.insert(origin_index);
        // (hops, rank, id, path) — Python tuple sort.
        let mut discovered: Vec<(i64, usize, String, usize)> = Vec::new();
        let mut frontier = vec![origin_index];
        let mut work = 0i64;
        let mut truncated = false;

        for current_depth in 1..=depth {
            let mut next_frontier = Vec::new();
            let mut sorted_frontier = frontier.clone();
            sorted_frontier.sort_by(|a, b| {
                stable_entry_order(&self.entries[*a], &self.entries[*b])
            });
            for entry_index in &sorted_frontier {
                let mut neighbors = graph.adjacency[*entry_index].clone();
                neighbors.sort_by(|a, b| {
                    stable_entry_order(&self.entries[a.0], &self.entries[b.0])
                        .then_with(|| a.1.cmp(&b.1))
                });
                neighbors.dedup();
                for (neighbor_index, rank) in neighbors {
                    work += 1;
                    if work > MAX_TRAVERSAL_WORK {
                        truncated = true;
                        break;
                    }
                    if visited.contains(&neighbor_index) {
                        continue;
                    }
                    visited.insert(neighbor_index);
                    let id = self.entries[neighbor_index].id.clone();
                    discovered.push((current_depth, rank, id, neighbor_index));
                    if next_frontier.len() >= MAX_TRAVERSAL_FRONTIER {
                        truncated = true;
                    } else {
                        next_frontier.push(neighbor_index);
                    }
                }
                if truncated && work > MAX_TRAVERSAL_WORK {
                    break;
                }
            }
            frontier = next_frontier;
            if frontier.is_empty() {
                break;
            }
        }

        discovered.sort_by(|a, b| {
            (a.0, a.1, &a.2)
                .cmp(&(b.0, b.1, &b.2))
                .then_with(|| stable_entry_order(&self.entries[a.3], &self.entries[b.3]))
        });
        let mut nodes: Vec<NeighborhoodNode> = discovered
            .into_iter()
            .map(|(hops, _rank, _id, entry_index)| {
                let entry = &self.entries[entry_index];
                NeighborhoodNode {
                    key: entry.key.clone(),
                    origin: entry.origin.clone(),
                    id: entry.id.clone(),
                    artifact_type: entry.artifact_type.clone(),
                    title: entry.title.clone(),
                    path: public_path(entry, self.federated),
                    hops,
                }
            })
            .collect();
        nodes.sort_by(|a, b| {
            (a.hops, &a.artifact_type, &a.id).cmp(&(b.hops, &b.artifact_type, &b.id))
        });
        Neighborhood { nodes, truncated }
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn relationship_count(&self) -> usize {
        self.effective_graph.relationships.len()
    }

    pub fn is_federated(&self) -> bool {
        self.federated
    }

    /// Approximate owned heap payload, excluding hash-table control bytes.
    pub fn estimated_payload_bytes(&self) -> usize {
        let entry_bytes: usize = self
            .entries
            .iter()
            .map(|entry| {
                entry.id.len()
                    + entry.artifact_type.len()
                    + entry.title.as_ref().map_or(0, String::len)
                    + entry.path.len()
                    + entry.aliases.iter().map(String::len).sum::<usize>()
                    + entry.tags.iter().map(String::len).sum::<usize>()
                    + entry
                        .artifact_path
                        .as_ref()
                        .map_or(0, |path| path.source.len() + path.relative_path.len())
                    + entry.origin.as_ref().map_or(0, |origin| {
                        origin.source.len()
                            + origin.pin.as_ref().map_or(0, String::len)
                            + origin.alias.as_ref().map_or(0, String::len)
                    })
            })
            .sum();
        let relationship_bytes = |relationships: &[Relationship]| -> usize {
            relationships
            .iter()
            .map(|relationship| {
                relationship.source_path.len()
                    + relationship.relationship.len()
                    + relationship.target.len()
                    + relationship.resolved_path.as_ref().map_or(0, String::len)
                    + relationship.issue.as_ref().map_or(0, String::len)
                    + relationship
                        .source_artifact
                        .as_ref()
                        .map_or(0, |path| path.source.len() + path.relative_path.len())
                    + relationship
                        .resolved_artifact
                        .as_ref()
                        .map_or(0, |path| path.source.len() + path.relative_path.len())
            })
            .sum()
        };
        let relationship_bytes = relationship_bytes(&self.effective_graph.relationships)
            + self.historical_graph.as_ref().map_or(0, |graph| {
                relationship_bytes(&graph.relationships)
            });
        let map_key_bytes = self.entry_by_path.keys().map(String::len).sum::<usize>()
            + self
                .entry_by_artifact_path
                .keys()
                .map(|path| path.source.len() + path.relative_path.len())
                .sum::<usize>();
        let projection_payload = |graph: &RelationshipProjection| {
            graph
            .outgoing_by_source
            .iter()
            .map(|indexes| indexes.len() * std::mem::size_of::<usize>())
            .sum::<usize>()
            + graph
                .incoming_by_target
                .iter()
                .map(|indexes| indexes.len() * std::mem::size_of::<usize>())
                .sum::<usize>()
            + graph
                .adjacency
                .iter()
                .map(|neighbors| neighbors.len() * std::mem::size_of::<(usize, usize)>())
                .sum::<usize>()
        };
        let vector_payload_bytes = projection_payload(&self.effective_graph)
            + self
                .historical_graph
                .as_ref()
                .map_or(0, projection_payload);
        entry_bytes + relationship_bytes + map_key_bytes + vector_payload_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rac_engine::corpus::{ArtifactKey, CorpusLayer};

    fn entry(
        layer: CorpusLayer,
        id: &str,
        relative_path: &str,
        physical_path: &str,
    ) -> IndexEntry {
        let origin = layer.origin();
        IndexEntry {
            key: Some(ArtifactKey::new(&origin.source, id)),
            artifact_path: Some(origin.path(relative_path)),
            origin: Some(origin),
            id: id.to_string(),
            artifact_type: "Decision".to_string(),
            title: None,
            path: physical_path.to_string(),
            aliases: vec![id.to_string()],
            search_sections: Vec::new(),
            inbound_count: 0,
            tags: Vec::new(),
        }
    }

    fn resolved(entry: &IndexEntry) -> ResolvedArtifact {
        ResolvedArtifact {
            key: entry.key.clone(),
            artifact_path: entry.artifact_path.clone(),
            origin: entry.origin.clone(),
            id: entry.id.clone(),
            artifact_type: entry.artifact_type.clone(),
            title: entry.title.clone(),
            path: entry.path.clone(),
            section: None,
            snippet: None,
            evidence: None,
            recency: None,
            tags: Vec::new(),
        }
    }

    #[test]
    fn source_aware_endpoints_do_not_alias_physical_or_display_paths() {
        let parent = entry(
            CorpusLayer::inherited("acme/standards", "standards", "sha256:0123"),
            "ADR-PARENT",
            "decisions/shared.md",
            "/checkout/vendor/decisions/shared.md",
        );
        let local = entry(
            CorpusLayer::local("acme/app"),
            "ADR-LOCAL",
            "decisions/shared.md",
            "/checkout/decisions/shared.md",
        );
        let relationship = Relationship {
            source_artifact: parent.artifact_path.clone(),
            source_path: parent.path.clone(),
            relationship: "depends_on".to_string(),
            target: "ADR-LOCAL".to_string(),
            resolved_artifact: local.artifact_path.clone(),
            resolved_path: Some(local.path.clone()),
            issue: None,
        };
        let parent_artifact = resolved(&parent);
        let local_artifact = resolved(&local);
        let view = GraphView::new(vec![parent, local], vec![relationship]);

        assert!(view.is_federated());
        assert_eq!(view.outgoing(&parent_artifact, false).total, 1);
        assert_eq!(view.outgoing(&local_artifact, false).total, 0);

        let incoming = view.incoming(&local_artifact, false);
        assert_eq!(incoming.total, 1);
        assert_eq!(incoming.items[0].id, "ADR-PARENT");
        assert_eq!(incoming.items[0].path, "decisions/shared.md");
        assert_eq!(
            incoming.items[0]
                .origin
                .as_ref()
                .map(|origin| origin.source.as_str()),
            Some("acme/standards")
        );
    }

}

fn identity_projection(entry: &IndexEntry) -> IndexEntry {
    IndexEntry {
        key: entry.key.clone(),
        artifact_path: entry.artifact_path.clone(),
        origin: entry.origin.clone(),
        id: entry.id.clone(),
        artifact_type: entry.artifact_type.clone(),
        title: entry.title.clone(),
        path: entry.path.clone(),
        aliases: entry.aliases.clone(),
        search_sections: Vec::new(),
        inbound_count: 0,
        tags: Vec::new(),
    }
}

/// Server-lifetime cache. Publication is atomic at the single-threaded MCP
/// request boundary: build the complete replacement, then swap generation and
/// view together.
#[derive(Default)]
pub struct GraphCache {
    generation: Option<u64>,
    federated_generation: Option<String>,
    view: Option<GraphView>,
    builds: u64,
}

impl GraphCache {
    pub fn view_for(&mut self, generation: u64, model: &TrackerModel) -> &GraphView {
        if self.generation != Some(generation)
            || self.federated_generation.is_some()
            || self.view.is_none()
        {
            let started = rac_engine::timing::start();
            let replacement = GraphView::from_model(model);
            rac_engine::timing::emit_since(
                "graph.view_build",
                started,
                &[
                    ("entries", replacement.entry_count() as u64),
                    ("relationships", replacement.relationship_count() as u64),
                    ("payload_bytes", replacement.estimated_payload_bytes() as u64),
                ],
            );
            self.view = Some(replacement);
            self.generation = Some(generation);
            self.federated_generation = None;
            self.builds += 1;
        }
        self.view.as_ref().expect("graph view built")
    }

    pub fn view_for_composed(
        &mut self,
        generation: &str,
        corpus: &rac_engine::composition::ComposedCorpus,
    ) -> &GraphView {
        if self.federated_generation.as_deref() != Some(generation)
            || self.generation.is_some()
            || self.view.is_none()
        {
            let started = rac_engine::timing::start();
            let replacement = GraphView::from_composed(corpus);
            rac_engine::timing::emit_since(
                "graph.view_build",
                started,
                &[
                    ("entries", replacement.entry_count() as u64),
                    ("relationships", replacement.relationship_count() as u64),
                    ("payload_bytes", replacement.estimated_payload_bytes() as u64),
                ],
            );
            self.view = Some(replacement);
            self.generation = None;
            self.federated_generation = Some(generation.to_string());
            self.builds += 1;
        }
        self.view.as_ref().expect("federated graph view built")
    }

    #[cfg(test)]
    pub fn builds(&self) -> u64 {
        self.builds
    }
}
