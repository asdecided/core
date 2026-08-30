//! The derived read-model for one corpus snapshot (ADR-099/ADR-103).
//!
//! Port of `services/derived_cache.py` `build_derived_index`: one walk feeds
//! every structure, each a pure function of the sorted-path snapshot, so the
//! whole bundle is content-addressable and the persisted store reproduces a
//! fresh build byte-for-byte (spec/index-contracts.json `derived_cache`).

use serde_json::Value;

use crate::composition::ComposedCorpus;
use crate::corpus::{ArtifactKey, ArtifactOrigin, ArtifactPath, CorpusLayer};
use crate::relationships::{corpus_items, relationships_from_corpus, CorpusItem, Relationship};
use crate::resolve::{entry_from_item, field_tokens_of, is_live_decision, FieldTokens, IndexEntry};
use crate::retrieve::{scope_rows_from_items, ScopeRow};

/// The bundle schema version (`derived_cache.SCHEMA_VERSION`).
pub const SCHEMA_VERSION: &str = "3";

pub(crate) const DECISION_TYPE: &str = "decision";

/// The source-aware identity projection parallel to the searchable rows. The
/// v2 store persists and reconstructs these exact values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAwareArtifact {
    pub key: ArtifactKey,
    pub path: ArtifactPath,
    pub origin: ArtifactOrigin,
    /// Released display path retained independently from stable path identity.
    pub display_path: String,
}

/// One validated canonical redirect retained by a composed generation.
///
/// The declaration's spelling belongs to the manifest/composition boundary;
/// persistence needs only the three stable endpoints which were validated by
/// that boundary. Keeping this projection in the read model lets a warm store
/// reproduce the same override semantics without reparsing raw YAML.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CanonicalRedirect {
    pub parent: ArtifactKey,
    pub replacement: ArtifactKey,
    pub rationale: ArtifactKey,
}

/// Point-resolution state is boxed as one coherent projection. Besides
/// keeping the main derived bundle compact, this makes it difficult for a
/// caller to update authorized aliases without the matching redirect rows.
pub struct ResolutionProjection {
    pub entries: Vec<IndexEntry>,
    pub canonical_redirects: Vec<CanonicalRedirect>,
}

/// The expensive derived structures for one corpus snapshot.
pub struct DerivedIndex {
    /// Stable layer identities represented in this generation.
    pub layers: Vec<CorpusLayer>,
    /// Per-document source identity in the same order as `index_entries`.
    pub source_artifacts: Vec<SourceAwareArtifact>,
    /// The central composition layer's exact point-resolution projection.
    ///
    /// Unlike the searchable effective set, this may retain qualified parent
    /// rows and attach an overridden parent's canonical id to its local
    /// replacement. Its aliases are therefore authoritative for `resolve`.
    pub resolution: Box<ResolutionProjection>,
    /// Repository index rows in walk (sorted-path) order — docid order.
    pub index_entries: Vec<IndexEntry>,
    /// Per-entry BM25F field-token vectors, parallel to `index_entries`.
    /// (The oracle keys by path; docid order carries the same information
    /// without re-keying, and paths are unique within a walk.)
    pub field_tokens: Vec<FieldTokens>,
    pub relationships: Vec<Relationship>,
    /// Stable identities used for liveness filtering in composed stores.
    pub live_decision_keys: Vec<ArtifactKey>,
    /// Released display-path projection retained for single-corpus callers.
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
    let mut inbound: std::collections::HashMap<&ArtifactPath, i64> =
        std::collections::HashMap::new();
    for rel in &relationships {
        if let Some(resolved) = &rel.resolved_artifact {
            *inbound.entry(resolved).or_insert(0) += 1;
        }
    }
    let index_entries: Vec<IndexEntry> = items
        .iter()
        .map(|item| {
            entry_from_item(
                item,
                inbound.get(&item.artifact_path).copied().unwrap_or(0),
            )
        })
        .collect();
    let field_tokens: Vec<FieldTokens> = index_entries.iter().map(field_tokens_of).collect();
    let identity_entries: Vec<IndexEntry> = index_entries
        .iter()
        .cloned()
        .map(|mut entry| {
            entry.search_sections.clear();
            entry.inbound_count = 0;
            entry
        })
        .collect();
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
    let live_decision_keys: Vec<ArtifactKey> = items
        .iter()
        .filter(|item| {
            item.spec.map(|s| s.name == DECISION_TYPE).unwrap_or(false)
                && is_live_decision(&item.artifact)
        })
        .map(|item| item.key.clone())
        .collect();
    let summary = crate::portfolio::portfolio_from_corpus(directory, items, recursive);
    DerivedIndex {
        layers,
        source_artifacts,
        resolution: Box::new(ResolutionProjection {
            entries: identity_entries,
            canonical_redirects: Vec::new(),
        }),
        index_entries,
        field_tokens,
        relationships,
        live_decision_keys,
        live_decision_paths,
        portfolio_summary: crate::output::portfolio_summary_value(&summary),
        scope_rows: scope_rows_from_items(items),
    }
}

/// Build the persistable projection from the one authoritative composed
/// corpus and its already-captured governing inputs. This adapter performs no
/// corpus walk and does not rebuild a source-blind relationship overlay.
pub(crate) fn build_derived_index_from_composed(
    stable_directory: &str,
    child_directory: &str,
    recursive: bool,
    layers: &[CorpusLayer],
    child_config_bytes: &[u8],
    composed: &ComposedCorpus,
) -> DerivedIndex {
    let items: Vec<CorpusItem> = composed.effective().cloned().collect();
    let index_entries = composed.effective_index();
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
    let live_decision_paths: Vec<String> = items
        .iter()
        .filter(|item| {
            item.spec.map(|spec| spec.name == DECISION_TYPE).unwrap_or(false)
                && is_live_decision(&item.artifact)
        })
        .map(|item| item.path.clone())
        .collect();
    let live_decision_keys: Vec<ArtifactKey> = items
        .iter()
        .filter(|item| {
            item.spec.map(|spec| spec.name == DECISION_TYPE).unwrap_or(false)
                && is_live_decision(&item.artifact)
        })
        .map(|item| item.key.clone())
        .collect();
    let relationships = composed.relationships();
    let relationship_summary = composed.relationship_summary();
    let relationships_ok = composed
        .validate_relationships(child_directory, recursive)
        .ok();
    let overrides = crate::validate::overrides_from_config_bytes(child_config_bytes);
    let portfolio = crate::portfolio::portfolio_from_corpus_with_analysis(
        stable_directory,
        &items,
        recursive,
        &overrides,
        relationship_summary,
        relationships_ok,
    );
    let canonical_redirects = composed
        .overrides()
        .iter()
        .map(|mapping| CanonicalRedirect {
            parent: mapping.parent.clone(),
            replacement: mapping.replacement.clone(),
            rationale: mapping.rationale.clone(),
        })
        .collect();

    DerivedIndex {
        layers: layers.to_vec(),
        source_artifacts,
        resolution: Box::new(ResolutionProjection {
            entries: composed.identity_index(),
            canonical_redirects,
        }),
        index_entries,
        field_tokens,
        relationships,
        live_decision_keys,
        live_decision_paths,
        portfolio_summary: crate::output::portfolio_summary_value(&portfolio),
        scope_rows: scope_rows_from_items(&items),
    }
}

/// Build the one persistable graph projection from an authenticated semantic
/// closure. Every row comes from `VerifiedGraphCorpus`; no filesystem walk or
/// alternate overlay occurs at this cache boundary.
pub(crate) fn build_derived_index_from_graph(
    corpus: &crate::graph_federated_corpus::VerifiedGraphCorpus,
    recursive: bool,
) -> DerivedIndex {
    let items: Vec<CorpusItem> = corpus.composition.effective().cloned().collect();
    let index_entries = corpus.composition.effective_index();
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
    let live_decision_paths: Vec<String> = items
        .iter()
        .filter(|item| {
            item.spec.map(|spec| spec.name == DECISION_TYPE).unwrap_or(false)
                && is_live_decision(&item.artifact)
        })
        .map(|item| item.path.clone())
        .collect();
    let live_decision_keys: Vec<ArtifactKey> = items
        .iter()
        .filter(|item| {
            item.spec.map(|spec| spec.name == DECISION_TYPE).unwrap_or(false)
                && is_live_decision(&item.artifact)
        })
        .map(|item| item.key.clone())
        .collect();
    let relationships = corpus.composition.relationships();
    let relationship_summary = corpus.composition.relationship_summary();
    let overrides = crate::validate::overrides_from_config_bytes(
        &corpus.federation.root_config_bytes,
    );
    let portfolio = crate::portfolio::portfolio_from_corpus_with_analysis(
        &corpus.federation.root_corpus_path,
        &items,
        recursive,
        &overrides,
        relationship_summary,
        true,
    );
    let canonical_redirects = corpus
        .composition
        .ordered_overrides()
        .iter()
        .map(|mapping| CanonicalRedirect {
            parent: mapping.target.clone(),
            replacement: mapping.replacement.clone(),
            rationale: mapping.rationale.clone(),
        })
        .collect();

    DerivedIndex {
        layers: corpus.canonical_layers.values().cloned().collect(),
        source_artifacts,
        resolution: Box::new(ResolutionProjection {
            entries: corpus.composition.identity_index(),
            canonical_redirects,
        }),
        index_entries,
        field_tokens,
        relationships,
        live_decision_keys,
        live_decision_paths,
        portfolio_summary: crate::output::portfolio_summary_value(&portfolio),
        scope_rows: scope_rows_from_items(&items),
    }
}

/// Build the derived structures fresh from one corpus walk (the miss path).
pub fn build_derived_index(directory: &str, recursive: bool) -> DerivedIndex {
    let items = corpus_items(directory, recursive);
    build_derived_index_from_items(directory, &items, recursive)
}
