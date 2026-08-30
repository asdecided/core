//! Generation-bound `get_related` graph view. Identity, incoming, outgoing,
//! and adjacency indexes are built once per freshness generation, then reused
//! by every graph call until the corpus changes.

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

/// Immutable graph projection for one logical corpus generation.
pub struct GraphView {
    entries: Vec<IndexEntry>,
    relationships: Vec<Relationship>,
    aliases: HashMap<String, Vec<usize>>,
    entry_by_path: HashMap<String, usize>,
    outgoing_by_source: Vec<Vec<usize>>,
    incoming_by_target: Vec<Vec<usize>>,
    adjacency: Vec<Vec<(usize, usize)>>,
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

    pub fn new(entries: Vec<IndexEntry>, relationships: Vec<Relationship>) -> Self {
        let mut aliases: HashMap<String, Vec<usize>> = HashMap::new();
        let mut entry_by_path = HashMap::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            entry_by_path.insert(entry.path.clone(), index);
            for alias in &entry.aliases {
                let targets = aliases
                    .entry(rac_engine::pycompat::py_casefold(alias))
                    .or_default();
                if !targets.contains(&index) {
                    targets.push(index);
                }
            }
        }

        let mut outgoing_by_source = vec![Vec::new(); entries.len()];
        let mut incoming_by_target = vec![Vec::new(); entries.len()];
        let mut adjacency = vec![Vec::new(); entries.len()];
        for (index, relationship) in relationships.iter().enumerate() {
            let Some(&source_index) = entry_by_path.get(&relationship.source_path) else {
                continue;
            };
            outgoing_by_source[source_index].push(index);
            let Some(target) = relationship.resolved_path.as_deref() else {
                continue;
            };
            let Some(&target_index) = entry_by_path.get(target) else {
                continue;
            };
            incoming_by_target[target_index].push(index);
            if relationship.source_path == target {
                continue;
            }
            let rank = relationship_order(&relationship.relationship);
            adjacency[source_index].push((target_index, rank));
            adjacency[target_index].push((source_index, rank));
        }

        Self {
            entries,
            relationships,
            aliases,
            entry_by_path,
            outgoing_by_source,
            incoming_by_target,
            adjacency,
        }
    }

    pub fn resolve(&self, artifact_id: &str) -> ResolutionResult {
        use rac_engine::resolve::{OUTCOME_DUPLICATE, OUTCOME_NOT_FOUND, OUTCOME_RESOLVED};

        let wanted = rac_engine::pycompat::py_casefold(rac_engine::pycompat::py_strip(artifact_id));
        let matches = self.aliases.get(&wanted).map(Vec::as_slice).unwrap_or(&[]);
        if matches.is_empty() {
            return ResolutionResult {
                artifact_id: artifact_id.to_string(),
                outcome: OUTCOME_NOT_FOUND,
                artifact: None,
                duplicate_paths: Vec::new(),
            };
        }
        if matches.len() > 1 {
            let mut paths: Vec<String> = matches
                .iter()
                .map(|index| self.entries[*index].path.clone())
                .collect();
            paths.sort();
            return ResolutionResult {
                artifact_id: artifact_id.to_string(),
                outcome: OUTCOME_DUPLICATE,
                artifact: None,
                duplicate_paths: paths,
            };
        }
        let entry = &self.entries[matches[0]];
        ResolutionResult {
            artifact_id: artifact_id.to_string(),
            outcome: OUTCOME_RESOLVED,
            artifact: Some(ResolvedArtifact {
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
                tags: entry.tags.clone(),
            }),
            duplicate_paths: Vec::new(),
        }
    }

    pub fn outgoing(&self, source_path: &str) -> OutgoingReferences {
        let indexes = self
            .entry_by_path
            .get(source_path)
            .map(|index| self.outgoing_by_source[*index].as_slice())
            .unwrap_or(&[]);
        let mut by_section: Vec<(String, Vec<String>)> = Vec::new();
        for index in indexes.iter().take(MAX_RELATED_EDGES) {
            let relationship = &self.relationships[*index];
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

    pub fn incoming(&self, target_path: &str) -> IncomingReferences {
        let indexes = self
            .entry_by_path
            .get(target_path)
            .map(|index| self.incoming_by_target[*index].as_slice())
            .unwrap_or(&[]);
        let mut incoming = Vec::new();
        let mut total = 0usize;
        for index in indexes {
            let relationship = &self.relationships[*index];
            if relationship.source_path == target_path {
                continue;
            }
            let Some(entry_index) = self.entry_by_path.get(&relationship.source_path) else {
                continue;
            };
            total += 1;
            if incoming.len() < MAX_RELATED_EDGES {
                let entry = &self.entries[*entry_index];
                incoming.push(IncomingReference {
                    id: entry.id.clone(),
                    artifact_type: entry.artifact_type.clone(),
                    title: entry.title.clone(),
                    path: relationship.source_path.clone(),
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

    pub fn neighborhood(&self, origin_path: &str, depth: i64) -> Neighborhood {
        let depth = depth.clamp(0, MAX_TRAVERSAL_DEPTH);
        let Some(&origin_index) = self.entry_by_path.get(origin_path) else {
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
            sorted_frontier.sort_by(|a, b| self.entries[*a].path.cmp(&self.entries[*b].path));
            for entry_index in &sorted_frontier {
                let mut neighbors = self.adjacency[*entry_index].clone();
                neighbors.sort_by(|a, b| {
                    (&self.entries[a.0].path, a.1).cmp(&(&self.entries[b.0].path, b.1))
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
            (a.0, a.1, &a.2, &self.entries[a.3].path)
                .cmp(&(b.0, b.1, &b.2, &self.entries[b.3].path))
        });
        let mut nodes: Vec<NeighborhoodNode> = discovered
            .into_iter()
            .map(|(hops, _rank, _id, entry_index)| {
                let entry = &self.entries[entry_index];
                NeighborhoodNode {
                    id: entry.id.clone(),
                    artifact_type: entry.artifact_type.clone(),
                    title: entry.title.clone(),
                    path: entry.path.clone(),
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
        self.relationships.len()
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
            })
            .sum();
        let relationship_bytes: usize = self
            .relationships
            .iter()
            .map(|relationship| {
                relationship.source_path.len()
                    + relationship.relationship.len()
                    + relationship.target.len()
                    + relationship.resolved_path.as_ref().map_or(0, String::len)
                    + relationship.issue.as_ref().map_or(0, String::len)
            })
            .sum();
        let map_key_bytes = self.aliases.keys().map(String::len).sum::<usize>()
            + self.entry_by_path.keys().map(String::len).sum::<usize>();
        let vector_payload_bytes = self
            .outgoing_by_source
            .iter()
            .map(|indexes| indexes.len() * std::mem::size_of::<usize>())
            .sum::<usize>()
            + self
                .incoming_by_target
                .iter()
                .map(|indexes| indexes.len() * std::mem::size_of::<usize>())
                .sum::<usize>()
            + self
                .adjacency
                .iter()
                .map(|neighbors| neighbors.len() * std::mem::size_of::<(usize, usize)>())
                .sum::<usize>();
        entry_bytes + relationship_bytes + map_key_bytes + vector_payload_bytes
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
    view: Option<GraphView>,
    builds: u64,
}

impl GraphCache {
    pub fn view_for(&mut self, generation: u64, model: &TrackerModel) -> &GraphView {
        if self.generation != Some(generation) || self.view.is_none() {
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
            self.builds += 1;
        }
        self.view.as_ref().expect("graph view built")
    }

    #[cfg(test)]
    pub fn builds(&self) -> u64 {
        self.builds
    }
}
