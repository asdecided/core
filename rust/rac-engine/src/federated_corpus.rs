//! Verified parent snapshots -> the one source-aware composed read model.
//!
//! A repository without `.decided/corpus.md` returns `Ok(None)` so released
//! command paths remain untouched. A configured repository is snapshotted
//! once: inherited Markdown is parsed only from the exact bytes verified by
//! [`crate::federation::verify_parent`], and the local walk excludes that
//! materialisation subtree before any derived model is built.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::classify::classify;
use crate::composition::{
    ComposedCorpus, OverrideDeclaration, OverrideSyntaxError, ParentIdentity,
    FINDING_INVALID_OVERRIDE,
};
use crate::corpus::{CorpusLayer, PhysicalArtifactLocator, PhysicalCorpusLocator};
use crate::federation::{verify_parent, ParentCorpusError, SnapshotFile, VerifiedParent};
use crate::parse::parse_bytes;
use crate::relationships::{
    relationship_severity, validation_from_rows, validation_row_from_item, CorpusItem,
    ISSUE_SCOPE_TARGET_NOT_FOUND,
};
use crate::spec::spec_for;
use crate::validate::{apply_overrides, has_errors, SeverityOverrides};

pub const PARENT_CORPUS_INVALID: &str = "parent-corpus-invalid";
pub const FEDERATED_CORPUS_SNAPSHOT_FAILED: &str = "federated-corpus-snapshot-failed";

/// One stable, displayable failure at the verification/composition boundary.
#[derive(Debug)]
pub enum FederatedCorpusError {
    Parent(ParentCorpusError),
    ParentInvalid {
        source: String,
        path: PathBuf,
        detail: String,
    },
    LocalSnapshot {
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
            Self::ParentInvalid { .. } => PARENT_CORPUS_INVALID,
            Self::LocalSnapshot { .. } => FEDERATED_CORPUS_SNAPSHOT_FAILED,
            Self::Composition { code, .. } => code,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Parent(error) => error.path.as_deref(),
            Self::ParentInvalid { path, .. } | Self::Composition { path, .. } => Some(path),
            Self::LocalSnapshot { path, .. } => Some(path),
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
            Self::ParentInvalid { source, detail, .. } => write!(
                formatter,
                "{PARENT_CORPUS_INVALID}: parent source '{source}' is invalid: {detail}"
            ),
            Self::LocalSnapshot { path, message } => write!(
                formatter,
                "{FEDERATED_CORPUS_SNAPSHOT_FAILED}: cannot snapshot {}: {message}",
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

/// Load the central federated read model for `directory`.
///
/// `Ok(None)` is the deliberate no-manifest compatibility result. Every
/// configured consumer must use the returned composition rather than walking
/// the child and parent independently.
pub fn load_composed_corpus(
    directory: &str,
    recursive: bool,
) -> Result<Option<ComposedCorpus>, FederatedCorpusError> {
    let child_root = crate::validate::repository_root(directory);
    let Some(verified) = verify_parent(&child_root)? else {
        return Ok(None);
    };

    compose_verified_generation(directory, recursive, &verified).map(Some)
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
        Some(corpus) => Ok(corpus.local_items().cloned().collect()),
        None => Ok(crate::relationships::corpus_items(directory, recursive)),
    }
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

/// Whether a mutation target lies in a configured parent's read-only
/// materialisation. The manifest search is path-ancestor based (rather than
/// nearest-config based) so a target inside the parent checkout still finds
/// the child repository's governing manifest.
pub fn is_read_only_materialised_path(
    target: impl AsRef<Path>,
) -> Result<bool, FederatedCorpusError> {
    let target = target.as_ref();
    let absolute = if target.is_absolute() {
        target.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(target)
    };
    let start = if absolute.is_dir() {
        absolute.as_path()
    } else {
        absolute.parent().unwrap_or(absolute.as_path())
    };
    let Some(child_root) = start.ancestors().find(|ancestor| {
        ancestor
            .join(crate::federation::MANIFEST_RELATIVE_PATH)
            .is_file()
    }) else {
        return Ok(false);
    };
    Ok(verify_parent(child_root)?
        .is_some_and(|verified| verified.contains_materialised_path(&absolute)))
}
