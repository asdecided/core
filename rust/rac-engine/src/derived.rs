//! The derived read-model for one corpus snapshot (ADR-099/ADR-103).
//!
//! Port of `services/derived_cache.py` `build_derived_index`: one walk feeds
//! every structure, each a pure function of the sorted-path snapshot, so the
//! whole bundle is content-addressable and the persisted store reproduces a
//! fresh build byte-for-byte (spec/index-contracts.json `derived_cache`).

use serde_json::Value;

use crate::corpus::{ArtifactKey, ArtifactOrigin, ArtifactPath, CorpusLayer};
use crate::relationships::{corpus_items, relationships_from_corpus, CorpusItem, Relationship};
use crate::resolve::{entry_from_item, field_tokens_of, is_live_decision, FieldTokens, IndexEntry};
use crate::retrieve::{scope_rows_from_items, ScopeRow};

/// The bundle schema version (`derived_cache.SCHEMA_VERSION`).
pub const SCHEMA_VERSION: &str = "3";

pub(crate) const DECISION_TYPE: &str = "decision";

/// The source-aware identity projection parallel to the existing searchable
/// rows. It remains in memory while the frozen v1 store is the compatibility
/// format; the versioned store cutover will persist these exact values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAwareArtifact {
    pub key: ArtifactKey,
    pub path: ArtifactPath,
    pub origin: ArtifactOrigin,
    /// Released display path retained independently from stable path identity.
    pub display_path: String,
}

/// The expensive derived structures for one corpus snapshot.
pub struct DerivedIndex {
    /// Stable layer identities represented in this generation.
    pub layers: Vec<CorpusLayer>,
    /// Per-document source identity in the same order as `index_entries`.
    pub source_artifacts: Vec<SourceAwareArtifact>,
    /// Repository index rows in walk (sorted-path) order — docid order.
    pub index_entries: Vec<IndexEntry>,
    /// Per-entry BM25F field-token vectors, parallel to `index_entries`.
    /// (The oracle keys by path; docid order carries the same information
    /// without re-keying, and paths are unique within a walk.)
    pub field_tokens: Vec<FieldTokens>,
    pub relationships: Vec<Relationship>,
    pub live_decision_paths: Vec<String>,
    /// The `get_summary` portfolio dict (ADR-103) — the JSON payload the
    /// store persists verbatim in `portfolio.seg`.
    pub portfolio_summary: Value,
    pub scope_rows: Vec<ScopeRow>,
}

/// Build the derived structures from an already-walked corpus snapshot.
pub fn build_derived_index_from_items(
    directory: &str,
    items: &[CorpusItem],
    recursive: bool,
) -> DerivedIndex {
    // Resolve the graph once; inbound degree is counted off the resolved
    // edges exactly as `inbound_counts_from_relationships` does.
    let relationships = relationships_from_corpus(items);
    let mut inbound: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for rel in &relationships {
        if let Some(resolved) = &rel.resolved_path {
            *inbound.entry(resolved.as_str()).or_insert(0) += 1;
        }
    }
    let index_entries: Vec<IndexEntry> = items
        .iter()
        .map(|item| {
            entry_from_item(item, inbound.get(item.path.as_str()).copied().unwrap_or(0))
        })
        .collect();
    let field_tokens: Vec<FieldTokens> = index_entries.iter().map(field_tokens_of).collect();
    let source_artifacts: Vec<SourceAwareArtifact> = items
        .iter()
        .map(|item| SourceAwareArtifact {
            key: item.key.clone(),
            path: item.artifact_path.clone(),
            origin: item.origin.clone(),
            display_path: item.path.clone(),
        })
        .collect();
    let mut layers: Vec<CorpusLayer> = items
        .iter()
        .map(|item| CorpusLayer::from(&item.origin))
        .collect();
    layers.sort();
    layers.dedup();
    if layers.is_empty() {
        layers.push(crate::corpus::compatible_local_layer(directory));
    }
    let live_decision_paths: Vec<String> = items
        .iter()
        .filter(|item| {
            item.spec.map(|s| s.name == DECISION_TYPE).unwrap_or(false)
                && is_live_decision(&item.artifact)
        })
        .map(|item| item.path.clone())
        .collect();
    let summary = crate::portfolio::portfolio_from_corpus(directory, items, recursive);
    DerivedIndex {
        layers,
        source_artifacts,
        index_entries,
        field_tokens,
        relationships,
        live_decision_paths,
        portfolio_summary: crate::output::portfolio_summary_value(&summary),
        scope_rows: scope_rows_from_items(items),
    }
}

/// Build the derived structures fresh from one corpus walk (the miss path).
pub fn build_derived_index(directory: &str, recursive: bool) -> DerivedIndex {
    let items = corpus_items(directory, recursive);
    build_derived_index_from_items(directory, &items, recursive)
}
