//! Repository index — `decided index` (services/index.py, INDEX-PLAN B1).
//!
//! One walk, one parse per file, entries in sorted-path order. Identity-only
//! JSON contract (id/type/title/path/aliases); the command never consumes or
//! writes the derived cache (spec/index-contracts.json `index-command`).

use crate::classify::classify;
use crate::corpus::ArtifactOrigin;
use crate::identity::{artifact_identifier, artifact_identifiers};
use crate::relationships::{corpus_items, CorpusItem};

/// One row in the repository manifest: structural identity only.
pub struct IndexEntry {
    pub id: String,
    pub artifact_type: String,
    pub title: Option<String>,
    pub path: String,
    pub aliases: Vec<String>,
    /// Present only when the row came from an explicit composed projection.
    pub origin: Option<ArtifactOrigin>,
}

/// Deterministic inventory of every artifact in a repository.
pub struct RepositoryIndex {
    pub directory: String,
    pub recursive: bool,
    pub artifacts: Vec<IndexEntry>,
}

fn repository_index_from_items(
    directory: &str,
    items: &[CorpusItem],
    recursive: bool,
    include_origin: bool,
) -> RepositoryIndex {
    let artifacts = items
        .iter()
        .map(|it| IndexEntry {
            id: artifact_identifier(&it.artifact, it.spec, &it.path),
            artifact_type: classify(&it.artifact).artifact_type,
            title: it.artifact.product.title.clone(),
            path: it.path.clone(),
            aliases: artifact_identifiers(&it.artifact, it.spec, &it.path),
            origin: include_origin.then(|| it.origin.clone()),
        })
        .collect();
    RepositoryIndex {
        directory: directory.to_string(),
        recursive,
        artifacts,
    }
}

/// Deterministic inventory over a caller-selected projection. Federation
/// passes the effective items from its authoritative composition here; this
/// adapter never walks or constructs an overlay independently.
pub fn build_repository_index_from_items(
    directory: &str,
    items: &[CorpusItem],
    recursive: bool,
) -> RepositoryIndex {
    repository_index_from_items(directory, items, recursive, true)
}

pub fn build_repository_index(directory: &str, recursive: bool) -> RepositoryIndex {
    let items = corpus_items(directory, recursive);
    repository_index_from_items(directory, &items, recursive, false)
}
