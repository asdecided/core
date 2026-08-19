//! Verified parent snapshots -> the one source-aware composed read model.
//!
//! A repository without `.decided/corpus.md` returns `Ok(None)` so released
//! command paths remain untouched. A configured repository is snapshotted
//! once: inherited Markdown is parsed only from the exact bytes verified by
//! [`crate::federation::verify_parent`], and the local walk excludes that
//! materialisation subtree before any derived model is built.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::classify::classify;
use crate::composition::{
    ComposedCorpus, OverrideDeclaration, OverrideSyntaxError, ParentIdentity,
    FINDING_INVALID_OVERRIDE,
};
use crate::corpus::{CorpusLayer, PhysicalArtifactLocator, PhysicalCorpusLocator};
use crate::federation::{
    direct_graph_materialisation_roots, load_graph_manifest, verify_federation, verify_parent,
    ParentCorpusError, SnapshotFile, VerifiedParent,
};
use crate::graph_federated_corpus::{
    compose_verified_federation, GraphFederatedCorpusError,
};
use crate::parse::parse_bytes;
use crate::relationships::{
    relationship_severity, validation_from_rows, validation_row_from_item, CorpusItem,
    ISSUE_SCOPE_TARGET_NOT_FOUND,
};
use crate::spec::spec_for;
use crate::validate::{apply_overrides, has_errors, SeverityOverrides};

pub const PARENT_CORPUS_INVALID: &str = "parent-corpus-invalid";
pub const FEDERATED_CORPUS_SNAPSHOT_FAILED: &str = "federated-corpus-snapshot-failed";
pub const FEDERATED_WRITE_TARGET_RESOLUTION_FAILED: &str =
    "federated-write-target-resolution-failed";

/// One stable, displayable failure at the verification/composition boundary.
#[derive(Debug)]
pub enum FederatedCorpusError {
    Parent(ParentCorpusError),
    Graph(GraphFederatedCorpusError),
    ParentInvalid {
        source: String,
        path: PathBuf,
        detail: String,
    },
    LocalSnapshot {
        path: PathBuf,
        message: String,
    },
    WriteTarget {
        path: PathBuf,
        message: String,
    },
    Composition {
        code: &'static str,
        path: PathBuf,
        message: String,
    },
}

impl FederatedCorpusError {
    pub fn stable_code(&self) -> &str {
        match self {
            Self::Parent(error) => error.stable_code(),
            Self::Graph(error) => error.stable_code(),
            Self::ParentInvalid { .. } => PARENT_CORPUS_INVALID,
            Self::LocalSnapshot { .. } => FEDERATED_CORPUS_SNAPSHOT_FAILED,
            Self::WriteTarget { .. } => FEDERATED_WRITE_TARGET_RESOLUTION_FAILED,
            Self::Composition { code, .. } => code,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Parent(error) => error.path.as_deref(),
            Self::Graph(error) => error.relative_path.as_deref().map(Path::new),
            Self::ParentInvalid { path, .. } | Self::Composition { path, .. } => Some(path),
            Self::LocalSnapshot { path, .. } | Self::WriteTarget { path, .. } => Some(path),
        }
    }

    /// Stable source ownership for a graph-topology or inherited-node
    /// validation failure. Version-1 failures which predate this carrier leave
    /// it absent and retain their released rendering path.
    pub fn validation_origin(&self) -> Option<&crate::federation::FederationValidationOrigin> {
        match self {
            Self::Parent(error) => error.validation_origin.as_deref(),
            Self::Graph(error) => error.validation_origin.as_deref(),
            _ => None,
        }
    }

    /// Lexicographically minimal source route for a graph-topology finding.
    pub fn source_route(&self) -> Option<&[String]> {
        match self {
            Self::Parent(error) => error.source_route.as_deref().map(Vec::as_slice),
            Self::Graph(error) => error.source_route.as_deref().map(Vec::as_slice),
            _ => None,
        }
    }

    /// Exact number of verified physical routes represented by the finding.
    pub fn route_count(&self) -> Option<usize> {
        match self {
            Self::Parent(error) => error.route_count.as_deref().copied(),
            Self::Graph(error) => error.route_count.as_deref().copied(),
            _ => None,
        }
    }
}

impl fmt::Display for FederatedCorpusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parent(error) => {
                let mut message = error.message.clone();
                if let Some(path) = &error.path {
                    let stable = if path.ends_with(crate::federation::MANIFEST_RELATIVE_PATH) {
                        crate::federation::MANIFEST_RELATIVE_PATH
                    } else if path.ends_with(crate::federation::CONFIG_RELATIVE_PATH) {
                        crate::federation::CONFIG_RELATIVE_PATH
                    } else {
                        "declared parent materialisation"
                    };
                    message = message.replace(&path.display().to_string(), stable);
                }
                write!(formatter, "{}: {message}", error.stable_code())
            }
            Self::Graph(error) => write!(formatter, "{error}"),
            Self::ParentInvalid { source, detail, .. } => write!(
                formatter,
                "{PARENT_CORPUS_INVALID}: parent source '{source}' is invalid: {detail}"
            ),
            Self::LocalSnapshot { path, message } => write!(
                formatter,
                "{FEDERATED_CORPUS_SNAPSHOT_FAILED}: cannot snapshot {}: {message}",
                path.display()
            ),
            Self::WriteTarget { path, message } => write!(
                formatter,
                "{FEDERATED_WRITE_TARGET_RESOLUTION_FAILED}: cannot safely resolve write target {}: {message}",
                path.display()
            ),
            Self::Composition { code, message, .. } => write!(formatter, "{code}: {message}"),
        }
    }
}

impl std::error::Error for FederatedCorpusError {}

impl From<ParentCorpusError> for FederatedCorpusError {
    fn from(error: ParentCorpusError) -> Self {
        Self::Parent(error)
    }
}

impl From<GraphFederatedCorpusError> for FederatedCorpusError {
    fn from(error: GraphFederatedCorpusError) -> Self {
        Self::Graph(error)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOverrides {
    version: u32,
    items: Vec<RawOverride>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOverride {
    parent: String,
    #[serde(rename = "with")]
    replacement: String,
    rationale: String,
}

fn invalid_override(verified: &VerifiedParent, message: impl Into<String>) -> FederatedCorpusError {
    FederatedCorpusError::Composition {
        code: FINDING_INVALID_OVERRIDE,
        path: verified.manifest_path.clone(),
        message: message.into(),
    }
}

fn parse_overrides(
    verified: &VerifiedParent,
) -> Result<Vec<OverrideDeclaration>, FederatedCorpusError> {
    let Some(value) = verified.overrides.clone() else {
        return Ok(Vec::new());
    };
    let raw: RawOverrides = serde_yaml::from_value(value).map_err(|error| {
        invalid_override(
            verified,
            format!("override declarations are malformed: {error}"),
        )
    })?;
    if raw.version != 1 {
        return Err(invalid_override(
            verified,
            format!(
                "override declarations use unsupported version {}; expected 1",
                raw.version
            ),
        ));
    }
    raw.items
        .into_iter()
        .map(|item| {
            OverrideDeclaration::parse(&item.parent, &item.replacement, &item.rationale)
                .map_err(|error| override_syntax_error(verified, &item.parent, error))
        })
        .collect()
}

fn override_syntax_error(
    verified: &VerifiedParent,
    parent: &str,
    error: OverrideSyntaxError,
) -> FederatedCorpusError {
    invalid_override(
        verified,
        format!("override for '{parent}' is invalid: {error}"),
    )
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

fn parent_policy(config: &[u8]) -> (Option<String>, SeverityOverrides) {
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

fn parent_invalid(verified: &VerifiedParent, detail: impl Into<String>) -> FederatedCorpusError {
    let corpus = format!(
        "{}/{}",
        verified.declaration.root.trim_end_matches('/'),
        verified.declaration.corpus.trim_start_matches('/')
    );
    FederatedCorpusError::ParentInvalid {
        source: verified.declaration.source.clone(),
        path: verified.manifest_path.clone(),
        detail: format!(
            "{}; validate the parent directly with `decided validate {corpus}` and \
             `decided relationships {corpus} --validate`",
            detail.into()
        ),
    }
}

fn validate_parent(
    verified: &VerifiedParent,
    items: &[CorpusItem],
) -> Result<(), FederatedCorpusError> {
    let (provider, overrides) = parent_policy(&verified.config_bytes);
    for item in items {
        let Some(spec) = item.spec else {
            continue;
        };
        let issues = apply_overrides(
            crate::validate::validate(&item.artifact, provider.as_deref(), Some(&spec.name)),
            &spec.name,
            &overrides,
        );
        if let Some(issue) = issues.iter().find(|issue| issue.severity == "error") {
            return Err(parent_invalid(
                verified,
                format!(
                    "structural error {} in {}: {}",
                    issue.code, item.artifact_path.relative_path, issue.message
                ),
            ));
        }
        debug_assert!(!has_errors(&issues));
    }

    let okf_entries: Vec<crate::validate::OkfEntry<'_>> = items
        .iter()
        .map(|item| crate::validate::OkfEntry {
            path: &item.path,
            artifact_type: item
                .spec
                .map(|spec| spec.name.as_str())
                .unwrap_or("unknown"),
            file_name: item.path.rsplit('/').next().unwrap_or(&item.path),
        })
        .collect();
    let okf = crate::validate::check_okf_conformance(&okf_entries, &overrides);
    if let Some(finding) = okf
        .findings
        .iter()
        .find(|finding| finding.severity == "error")
    {
        return Err(parent_invalid(
            verified,
            format!("OKF error {} in {}", finding.code, finding.path),
        ));
    }

    let rows: Vec<_> = items.iter().map(validation_row_from_item).collect();
    let corpus_root = verified.corpus_root.to_string_lossy();
    let relationships = validation_from_rows(&corpus_root, &rows, true);
    if let Some(issue) = relationships.issues.iter().find(|issue| {
        issue.code != ISSUE_SCOPE_TARGET_NOT_FOUND && relationship_severity(&issue.code) == "error"
    }) {
        return Err(parent_invalid(
            verified,
            format!("relationship error {}", issue.code),
        ));
    }
    Ok(())
}

fn inherited_items(verified: &VerifiedParent) -> Vec<CorpusItem> {
    let origin = CorpusLayer::inherited(
        verified.declaration.source.clone(),
        verified.declaration.alias.clone(),
        verified.digest.clone(),
    )
    .origin();
    let corpus_locator = PhysicalCorpusLocator::new(
        verified.materialisation_root.clone(),
        verified.corpus_root.clone(),
    );
    verified
        .files
        .iter()
        .map(|file| {
            let artifact = parse_bytes(&file.bytes, &file.relative_path);
            let spec = spec_for(&classify(&artifact).artifact_type);
            CorpusItem::new(
                file.relative_path.clone(),
                file.relative_path.clone(),
                artifact,
                spec,
                origin.clone(),
                PhysicalArtifactLocator::new(corpus_locator.clone(), file.absolute_path.clone()),
            )
        })
        .collect()
}

fn capture_local_files(
    directory: &str,
    recursive: bool,
    verified: &VerifiedParent,
) -> Result<Vec<SnapshotFile>, FederatedCorpusError> {
    let mut files = Vec::new();
    for entry in crate::walk::find_markdown_files(directory, recursive)
        .into_iter()
        .filter(|entry| !verified.contains_materialised_path(&entry.abs))
    {
        let relative_path = entry.rel();
        let bytes =
            std::fs::read(&entry.abs).map_err(|error| FederatedCorpusError::LocalSnapshot {
                path: PathBuf::from(&relative_path),
                message: error.to_string(),
            })?;
        files.push(SnapshotFile {
            relative_path,
            absolute_path: entry.abs,
            bytes,
        });
    }
    Ok(files)
}

fn local_items_from_snapshot(
    directory: &str,
    verified: &VerifiedParent,
    files: &[SnapshotFile],
) -> (Vec<CorpusItem>, Vec<(crate::corpus::ArtifactKey, Vec<u8>)>) {
    let origin = CorpusLayer::local(verified.child_source.clone()).origin();
    let corpus_locator = PhysicalCorpusLocator::local(directory);
    let mut contents = Vec::with_capacity(files.len());
    let mut items = Vec::with_capacity(files.len());
    for file in files {
        let artifact = parse_bytes(&file.bytes, &file.relative_path);
        let spec = spec_for(&classify(&artifact).artifact_type);
        let item = CorpusItem::new(
            file.relative_path.clone(),
            file.relative_path.clone(),
            artifact,
            spec,
            origin.clone(),
            PhysicalArtifactLocator::new(corpus_locator.clone(), file.absolute_path.clone()),
        );
        contents.push((item.key.clone(), file.bytes.clone()));
        items.push(item);
    }
    (items, contents)
}

/// Locate a configured repository even when its config is invalid or has
/// disappeared. A present manifest is authoritative topology and must not be
/// bypassed by falling back to a local-only corpus walk.
fn composition_repository_root(directory: &str) -> PathBuf {
    let input = Path::new(directory);
    let absolute = if input.is_absolute() {
        input.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|current| current.join(input))
            .unwrap_or_else(|_| input.to_path_buf())
    };
    let search = std::fs::canonicalize(&absolute).unwrap_or(absolute);
    for ancestor in search.ancestors() {
        let manifest = ancestor.join(crate::federation::MANIFEST_RELATIVE_PATH);
        let config = ancestor.join(crate::federation::CONFIG_RELATIVE_PATH);
        if std::fs::symlink_metadata(&manifest).is_ok()
            || std::fs::symlink_metadata(&config).is_ok()
        {
            return ancestor.to_path_buf();
        }
    }
    crate::validate::repository_root(directory)
}

fn graph_corpus_relative(
    directory: &str,
    repository_root: &Path,
) -> Result<String, FederatedCorpusError> {
    let corpus = std::fs::canonicalize(directory).map_err(|error| {
        FederatedCorpusError::LocalSnapshot {
            path: PathBuf::from(directory),
            message: error.to_string(),
        }
    })?;
    let root = std::fs::canonicalize(repository_root).map_err(|error| {
        FederatedCorpusError::LocalSnapshot {
            path: repository_root.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    let relative = corpus.strip_prefix(&root).map_err(|_| {
        FederatedCorpusError::LocalSnapshot {
            path: PathBuf::from(directory),
            message: "corpus path escapes the configured repository root".to_string(),
        }
    })?;
    let text = relative.to_string_lossy().replace('\\', "/");
    if text.is_empty() {
        return Err(FederatedCorpusError::LocalSnapshot {
            path: PathBuf::from(directory),
            message: "version-2 federation requires a repository-relative corpus path"
                .to_string(),
        });
    }
    Ok(text)
}

/// Locate a configured version-2 graph for cache-backed read consumers.
///
/// `None` preserves the released no-manifest and manifest-v1 paths. A present
/// malformed v2 manifest remains an error, and callers receive the exact
/// repository/corpus pair consumed by [`verify_federation`].
pub fn graph_cache_location(
    directory: &str,
) -> Result<Option<(PathBuf, String)>, FederatedCorpusError> {
    let repository_root = composition_repository_root(directory);
    if load_graph_manifest(&repository_root)?.is_none() {
        return Ok(None);
    }
    let corpus_relative = graph_corpus_relative(directory, &repository_root)?;
    Ok(Some((repository_root, corpus_relative)))
}

/// Load the central federated read model for `directory`.
///
/// `Ok(None)` is the deliberate no-manifest compatibility result. Every
/// configured consumer must use the returned composition rather than walking
/// the child and parent independently.
pub fn load_composed_corpus(
    directory: &str,
    recursive: bool,
) -> Result<Option<ComposedCorpus>, FederatedCorpusError> {
    if let Some(corpus) = load_graph_composed_corpus(directory, recursive)? {
        return Ok(Some(corpus));
    }
    let child_root = composition_repository_root(directory);
    let Some(verified) = verify_parent(&child_root)? else {
        return Ok(None);
    };

    compose_verified_generation(directory, recursive, &verified).map(Some)
}

/// Load only a version-2 graph composition. A version-1 or unconfigured
/// repository returns `Ok(None)` without entering its verification path, so
/// consumers which are additive only for graph federation can preserve the
/// released version-1/no-manifest behaviour byte for byte.
pub fn load_graph_composed_corpus(
    directory: &str,
    _recursive: bool,
) -> Result<Option<ComposedCorpus>, FederatedCorpusError> {
    let child_root = composition_repository_root(directory);
    if load_graph_manifest(&child_root)?.is_none() {
        return Ok(None);
    }
    let corpus_relative = graph_corpus_relative(directory, &child_root)?;
    let verified = verify_federation(&child_root, &corpus_relative)?.ok_or_else(|| {
        FederatedCorpusError::LocalSnapshot {
            path: child_root.join(crate::federation::MANIFEST_RELATIVE_PATH),
            message: "version-2 manifest changed during verification".to_string(),
        }
    })?;
    compose_verified_federation(verified)
        .map(|corpus| Some(corpus.into_composed_corpus()))
        .map_err(Into::into)
}

/// Return the writable local layer for a configured corpus, or the released
/// single-corpus walk when no federation manifest exists. Mutation and
/// local-only projection code uses this boundary so a repository-root walk
/// can never treat a vendored parent as writable child input.
pub fn local_writable_items(
    directory: &str,
    recursive: bool,
) -> Result<Vec<CorpusItem>, FederatedCorpusError> {
    match load_composed_corpus(directory, recursive)? {
        Some(corpus) => Ok(local_writable_projection(directory, &corpus)),
        None => Ok(crate::relationships::corpus_items(directory, recursive)),
    }
}

/// Adapt the composed child layer to the released writable-path convention.
/// Stable `artifact_path` identity stays corpus-relative, while mutation and
/// source-copy consumers receive the same root-prefixed `path` that a direct
/// local walk would have produced. Physical provenance remains in `locator`.
pub fn local_writable_projection(
    directory: &str,
    corpus: &ComposedCorpus,
) -> Vec<CorpusItem> {
    corpus
        .local_items()
        .cloned()
        .map(|mut item| {
            item.path = crate::walk::py_join(
                directory,
                &[item.artifact_path.relative_path.as_str()],
            );
            item
        })
        .collect()
}

/// Compose from an already-verified logical generation without re-running
/// verification or reopening inherited paths. Cache/freshness readers use
/// this handoff to keep one pin check and one byte snapshot per generation.
pub fn compose_verified_generation(
    directory: &str,
    recursive: bool,
    verified: &VerifiedParent,
) -> Result<ComposedCorpus, FederatedCorpusError> {
    let child_files = capture_local_files(directory, recursive, verified)?;
    compose_verified_generation_from_snapshot(directory, verified, &child_files)
}

/// Compose a logical generation whose child and parent bytes were already
/// captured. Neither layer is reopened by this adapter.
pub fn compose_verified_generation_from_snapshot(
    directory: &str,
    verified: &VerifiedParent,
    child_files: &[SnapshotFile],
) -> Result<ComposedCorpus, FederatedCorpusError> {
    let overrides = parse_overrides(verified)?;
    let inherited = inherited_items(verified);
    validate_parent(verified, &inherited)?;
    let (local, mut captured) = local_items_from_snapshot(directory, verified, child_files);
    captured.extend(
        inherited
            .iter()
            .zip(&verified.files)
            .map(|(item, file)| (item.key.clone(), file.bytes.clone())),
    );
    let parent = ParentIdentity::new(
        verified.declaration.source.clone(),
        verified.declaration.alias.clone(),
    )
    .map_err(|error| override_syntax_error(verified, &verified.declaration.alias, error))?;
    let corpus = ComposedCorpus::compose_verified(
        local,
        inherited,
        verified.child_source.clone(),
        verified.materialisation_root.clone(),
        parent,
        overrides,
        captured,
    );
    if let Some(finding) = corpus.findings().first() {
        return Err(FederatedCorpusError::Composition {
            code: finding.code,
            path: verified.manifest_path.clone(),
            message: format!(
                "{} (child source '{}', parent source '{}')",
                finding.message, verified.child_source, verified.declaration.source
            ),
        });
    }
    Ok(corpus)
}

fn absolute_write_target(path: &Path) -> Result<PathBuf, FederatedCorpusError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|error| FederatedCorpusError::WriteTarget {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?
            .join(path))
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn manifest_roots(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    let start = match std::fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_dir() => path.parent().unwrap_or(path),
        _ => path,
    };
    for ancestor in start.ancestors() {
        let manifest = ancestor.join(crate::federation::MANIFEST_RELATIVE_PATH);
        match std::fs::symlink_metadata(&manifest) {
            Ok(_) => roots.push(ancestor.to_path_buf()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot inspect federation manifest {}: {error}",
                    manifest.display()
                ))
            }
        }
    }
    Ok(roots)
}

/// Resolve existing path components in traversal order so a symlink followed
/// by `..` has filesystem semantics, while still retaining a normalized
/// suffix for a not-yet-created output. A `..` after a missing component is
/// deliberately ambiguous and therefore rejected for configured corpora.
fn resolve_write_target(path: &Path) -> Result<PathBuf, String> {
    let mut resolved = PathBuf::new();
    let mut missing_components = 0usize;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => resolved.push(prefix.as_os_str()),
            Component::RootDir => resolved.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if missing_components != 0 {
                    return Err(
                        "a '..' component follows a nonexistent path component".to_string()
                    );
                }
                resolved.pop();
            }
            Component::Normal(part) if missing_components != 0 => {
                resolved.push(part);
                missing_components += 1;
            }
            Component::Normal(part) => {
                let candidate = resolved.join(part);
                match std::fs::symlink_metadata(&candidate) {
                    Ok(_) => {
                        resolved = std::fs::canonicalize(&candidate)
                            .map_err(|error| error.to_string())?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        resolved.push(part);
                        missing_components = 1;
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
        }
    }
    Ok(resolved)
}

/// Traversal-safe containment for an output path against already-verified
/// read-only roots. Ambiguous nonexistent `..` traversals are errors so cache
/// callers can safely degrade to a non-persistent in-memory model.
pub(crate) fn write_target_within_roots(
    target: impl AsRef<Path>,
    roots: &[PathBuf],
) -> Result<bool, FederatedCorpusError> {
    let target = target.as_ref();
    let absolute = absolute_write_target(target)?;
    let lexical = lexical_normalize(&absolute);
    let resolved = resolve_write_target(&absolute).map_err(|message| {
        FederatedCorpusError::WriteTarget {
            path: target.to_path_buf(),
            message,
        }
    })?;
    Ok(roots.iter().any(|root| {
        resolved == *root
            || resolved.starts_with(root)
            || lexical == *root
            || lexical.starts_with(root)
    }))
}

/// Whether a diagnostic target is inside any manifest-v2 materialisation.
///
/// Version 1 is deliberately excluded so its released physical `inspect`
/// behaviour stays unchanged. In version 2, a nearer parent config must not
/// hide the governing child graph when a user names `vendor/parent/...`
/// directly.
pub fn is_read_only_graph_materialised_path(
    target: impl AsRef<Path>,
) -> Result<bool, FederatedCorpusError> {
    let target = target.as_ref();
    let absolute = absolute_write_target(target)?;
    let lexical = lexical_normalize(&absolute);
    let resolved = resolve_write_target(&absolute).map_err(|message| {
        FederatedCorpusError::WriteTarget {
            path: target.to_path_buf(),
            message,
        }
    })?;
    let discover = |path: &Path| {
        manifest_roots(path).map_err(|message| FederatedCorpusError::WriteTarget {
            path: target.to_path_buf(),
            message,
        })
    };
    let mut child_roots = discover(&resolved)?;
    for root in discover(&lexical)? {
        if !child_roots.iter().any(|existing| existing == &root) {
            child_roots.push(root);
        }
    }
    for child_root in child_roots {
        let Some(roots) = direct_graph_materialisation_roots(&child_root)? else {
            continue;
        };
        if roots.iter().any(|root| {
            resolved == *root
                || resolved.starts_with(root)
                || lexical == *root
                || lexical.starts_with(root)
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether a mutation target lies in a configured parent's read-only
/// materialisation. Existing ancestors are canonicalized before containment
/// is checked, and missing suffixes remain lexical, so neither `..` nor a
/// symlink ancestor can disguise a new output inside the inherited tree.
pub fn is_read_only_materialised_path(
    target: impl AsRef<Path>,
) -> Result<bool, FederatedCorpusError> {
    let target = target.as_ref();
    let absolute = absolute_write_target(target)?;
    let lexical = lexical_normalize(&absolute);
    let resolved = match resolve_write_target(&absolute) {
        Ok(resolved) => resolved,
        Err(message) => {
            let roots = manifest_roots(&lexical).map_err(|manifest_error| {
                FederatedCorpusError::WriteTarget {
                    path: target.to_path_buf(),
                    message: manifest_error,
                }
            })?;
            if roots.is_empty() {
                return Ok(false);
            }
            return Err(FederatedCorpusError::WriteTarget {
                path: target.to_path_buf(),
                message,
            });
        }
    };
    let discover = |path: &Path| {
        manifest_roots(path).map_err(|message| FederatedCorpusError::WriteTarget {
            path: target.to_path_buf(),
            message,
        })
    };
    let mut child_roots = discover(&resolved)?;
    for root in discover(&lexical)? {
        if !child_roots.iter().any(|existing| existing == &root) {
            child_roots.push(root);
        }
    }
    if child_roots.is_empty() {
        return Ok(false);
    }
    let mut read_only = false;
    for child_root in child_roots {
        if let Some(roots) = direct_graph_materialisation_roots(&child_root)? {
            read_only |= roots.iter().any(|root| {
                resolved == *root
                    || resolved.starts_with(root)
                    || lexical == *root
                    || lexical.starts_with(root)
            });
        } else if let Some(verified) = verify_parent(&child_root)? {
            read_only |= resolved == verified.materialisation_root
                || resolved.starts_with(&verified.materialisation_root)
                || lexical == verified.materialisation_root
                || lexical.starts_with(&verified.materialisation_root);
        }
    }
    Ok(read_only)
}
