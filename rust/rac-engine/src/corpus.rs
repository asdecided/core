//! Stable corpus and artifact identity for the source-aware read model.
//!
//! Stable identity deliberately excludes checkout paths. Runtime filesystem
//! locators are separate types so moving an otherwise identical checkout
//! cannot alter an [`ArtifactKey`] or [`ArtifactPath`] (ADR-135/ADR-138).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::scaffold::{load_repository_identity, ScaffoldError};

/// Whether an artifact belongs to the writable child or a read-only parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    Local,
    Inherited,
}

impl Layer {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Inherited => "inherited",
        }
    }

    pub const fn is_writable(self) -> bool {
        matches!(self, Self::Local)
    }
}

/// Stable identity and provenance shared by every artifact in one layer.
///
/// `pin` and `alias` are absent for the local layer. An inherited layer has
/// both: the full verified digest and the child-local readable alias.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CorpusLayer {
    pub source: String,
    pub layer: Layer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

impl CorpusLayer {
    pub fn local(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            layer: Layer::Local,
            pin: None,
            alias: None,
        }
    }

    pub fn inherited(
        source: impl Into<String>,
        alias: impl Into<String>,
        pin: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            layer: Layer::Inherited,
            pin: Some(pin.into()),
            alias: Some(alias.into()),
        }
    }

    pub fn origin(&self) -> ArtifactOrigin {
        ArtifactOrigin {
            source: self.source.clone(),
            layer: self.layer,
            pin: self.pin.clone(),
            alias: self.alias.clone(),
        }
    }
}

/// Stable global artifact identity: `(source, canonical_id)`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ArtifactKey {
    pub source: String,
    pub canonical_id: String,
}

impl ArtifactKey {
    pub fn new(source: impl Into<String>, canonical_id: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            canonical_id: canonical_id.into(),
        }
    }
}

/// Stable global path identity: `(source, corpus-relative path)`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ArtifactPath {
    pub source: String,
    pub relative_path: String,
}

impl ArtifactPath {
    pub fn new(source: impl Into<String>, relative_path: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            relative_path: relative_path.into(),
        }
    }
}

/// Stable provenance attached to a parsed or derived artifact.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ArtifactOrigin {
    pub source: String,
    pub layer: Layer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

impl ArtifactOrigin {
    pub fn key(&self, canonical_id: impl Into<String>) -> ArtifactKey {
        ArtifactKey::new(self.source.clone(), canonical_id)
    }

    pub fn path(&self, relative_path: impl Into<String>) -> ArtifactPath {
        ArtifactPath::new(self.source.clone(), relative_path)
    }
}

impl From<&ArtifactOrigin> for CorpusLayer {
    fn from(origin: &ArtifactOrigin) -> Self {
        Self {
            source: origin.source.clone(),
            layer: origin.layer,
            pin: origin.pin.clone(),
            alias: origin.alias.clone(),
        }
    }
}

/// Runtime-only filesystem location of one corpus layer.
///
/// These paths must never be serialized as stable identity or used as a
/// deterministic tie-break. Federation verification owns canonicalisation;
/// this substrate only keeps the already-selected locations distinct from
/// provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalCorpusLocator {
    pub repository_root: PathBuf,
    pub corpus_root: PathBuf,
}

impl PhysicalCorpusLocator {
    pub fn new(repository_root: impl Into<PathBuf>, corpus_root: impl Into<PathBuf>) -> Self {
        Self {
            repository_root: repository_root.into(),
            corpus_root: corpus_root.into(),
        }
    }

    pub fn local(corpus_root: impl Into<PathBuf>) -> Self {
        let corpus_root = corpus_root.into();
        Self::new(repository_root_for(&corpus_root), corpus_root)
    }
}

/// Runtime-only filesystem location of one artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalArtifactLocator {
    pub corpus: PhysicalCorpusLocator,
    pub path: PathBuf,
}

impl PhysicalArtifactLocator {
    pub fn new(corpus: PhysicalCorpusLocator, path: impl Into<PathBuf>) -> Self {
        Self {
            corpus,
            path: path.into(),
        }
    }
}

/// The exact non-federated source derivation shared by exports and the local
/// source-aware layer: explicit `corpus.source`, then lower-case
/// `repository_key`, then the released directory-basename fallback.
pub fn compatible_corpus_source(directory: &str) -> Result<String, ScaffoldError> {
    let Some(identity) = load_repository_identity(directory)? else {
        return Ok(compatible_corpus_name(directory));
    };
    if let Some(source) = identity.corpus_source {
        return Ok(source);
    }
    if let Some(repository_key) = identity.repository_key {
        return Ok(repository_key
            .strip_suffix('\n')
            .unwrap_or(&repository_key)
            .to_ascii_lowercase());
    }
    Ok(compatible_corpus_name(directory))
}

/// Build the dormant single local layer without making configuration errors
/// newly observable on legacy read paths. Strict federation loading uses its
/// own fallible verification gate before composition.
pub fn compatible_local_layer(directory: &str) -> CorpusLayer {
    let source =
        compatible_corpus_source(directory).unwrap_or_else(|_| compatible_corpus_name(directory));
    CorpusLayer::local(source)
}

pub(crate) fn compatible_corpus_name(directory: &str) -> String {
    let normalized = crate::walk::normalize_root(directory);
    let trimmed = normalized.trim_end_matches('/');
    let name = trimmed.rsplit('/').next().unwrap_or("");
    if name.is_empty() || name == "." || name == ".." {
        directory.to_string()
    } else {
        name.to_string()
    }
}

fn repository_root_for(corpus_root: &Path) -> PathBuf {
    let start = if corpus_root.is_dir() {
        corpus_root
    } else {
        corpus_root.parent().unwrap_or(corpus_root)
    };
    crate::validate::repository_root(&start.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_keys_order_by_source_then_local_identity() {
        let mut keys = [
            ArtifactKey::new("zeta/standards", "ADR-001"),
            ArtifactKey::new("acme/app", "ADR-002"),
            ArtifactKey::new("acme/app", "ADR-001"),
        ];
        keys.sort();
        assert_eq!(
            keys.iter()
                .map(|key| (key.source.as_str(), key.canonical_id.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("acme/app", "ADR-001"),
                ("acme/app", "ADR-002"),
                ("zeta/standards", "ADR-001"),
            ]
        );

        let mut paths = [
            ArtifactPath::new("zeta/standards", "a.md"),
            ArtifactPath::new("acme/app", "z.md"),
            ArtifactPath::new("acme/app", "a.md"),
        ];
        paths.sort();
        assert_eq!(paths[0], ArtifactPath::new("acme/app", "a.md"));
        assert_eq!(paths[2], ArtifactPath::new("zeta/standards", "a.md"));
    }

    #[test]
    fn provenance_serde_is_stable_and_omits_absent_local_fields() {
        assert_eq!(
            serde_json::to_string(&ArtifactKey::new("acme/app", "APP-001")).unwrap(),
            r#"{"source":"acme/app","canonical_id":"APP-001"}"#
        );
        assert_eq!(
            serde_json::to_string(&ArtifactPath::new("acme/app", "decisions/adr-001.md")).unwrap(),
            r#"{"source":"acme/app","relative_path":"decisions/adr-001.md"}"#
        );
        let local = CorpusLayer::local("acme/app").origin();
        assert_eq!(
            serde_json::to_string(&local).unwrap(),
            r#"{"source":"acme/app","layer":"local"}"#
        );
        assert_eq!(
            serde_json::from_str::<ArtifactOrigin>(r#"{"source":"acme/app","layer":"local"}"#)
                .unwrap(),
            local
        );

        let inherited =
            CorpusLayer::inherited("acme/standards", "standards", "sha256:0123456789abcdef")
                .origin();
        assert_eq!(
            serde_json::to_string(&inherited).unwrap(),
            r#"{"source":"acme/standards","layer":"inherited","pin":"sha256:0123456789abcdef","alias":"standards"}"#
        );
        assert_eq!(
            serde_json::from_str::<ArtifactOrigin>(&serde_json::to_string(&inherited).unwrap())
                .unwrap(),
            inherited
        );
    }

    #[test]
    fn stable_identity_does_not_include_clone_location() {
        let layer =
            CorpusLayer::inherited("acme/standards", "standards", "sha256:0123456789abcdef");
        let left = PhysicalArtifactLocator::new(
            PhysicalCorpusLocator::new("/clone-a", "/clone-a/vendor/standards/decisions"),
            "/clone-a/vendor/standards/decisions/decisions/adr-001.md",
        );
        let right = PhysicalArtifactLocator::new(
            PhysicalCorpusLocator::new("/clone-b", "/clone-b/vendor/standards/decisions"),
            "/clone-b/vendor/standards/decisions/decisions/adr-001.md",
        );

        assert_ne!(left, right);
        assert_eq!(
            layer.origin().key("ADR-001"),
            ArtifactKey::new("acme/standards", "ADR-001")
        );
        assert_eq!(
            layer.origin().path("decisions/adr-001.md"),
            ArtifactPath::new("acme/standards", "decisions/adr-001.md")
        );
        assert!(!serde_json::to_string(&layer).unwrap().contains("clone-"));
    }
}
