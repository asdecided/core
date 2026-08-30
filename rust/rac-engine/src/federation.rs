//! Verified, offline parent-corpus materialisation (ADR-134, ADR-135,
//! ADR-144, ADR-145, and ADR-148).
//!
//! This module owns only the declaration and byte-snapshot boundary. It does
//! not compose artifacts, resolve relationships, or give any read consumer a
//! second directory-overlay path. A successful [`verify_parent`] call returns
//! the exact config and Markdown bytes which were hashed, so later stages can
//! parse the verified snapshot without re-reading mutable parent files.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::markdown::consumed_events;
use crate::sha256::Sha256;

pub const MANIFEST_RELATIVE_PATH: &str = ".decided/corpus.md";
pub const CONFIG_RELATIVE_PATH: &str = ".decided/config.yaml";
pub const DIGEST_PREFIX: &str = "sha256:";
pub const DIGEST_V2_PREFIX: &str = "sha256-v2:";
pub const DIGEST_V2_DOMAIN: &[u8] = b"asdecided-corpus-digest-v2\0";

pub const V2_MAX_MANIFEST_BYTES: usize = 1_048_576;
pub const V2_MAX_CONFIG_BYTES: usize = 1_048_576;
pub const V2_MAX_ALIAS_BYTES: usize = 64;
pub const V2_MAX_SOURCE_BYTES: usize = 255;
pub const V2_MAX_PATH_BYTES: usize = 4_096;
pub const V2_MAX_PATH_COMPONENTS: usize = 64;
pub const V2_MAX_PATH_COMPONENT_BYTES: usize = 255;
pub const V2_MAX_YAML_DEPTH: usize = 32;
pub const V2_MAX_YAML_NODES: usize = 16_384;
pub const V2_MAX_DIRECT_PARENTS: usize = 32;
pub const V2_MAX_INHERITANCE_DEPTH: usize = 16;
pub const V2_MAX_INHERITED_SOURCES: usize = 256;
pub const V2_MAX_EDGES: usize = 1_024;
pub const V2_MAX_OVERRIDES: usize = 4_096;
pub const V2_MAX_INHERITED_FILES: usize = 50_000;
pub const V2_MAX_FILE_BYTES: usize = 16 * 1_048_576;
pub const V2_MAX_LOGICAL_BYTES: usize = 256 * 1_048_576;
pub const V2_MAX_PHYSICAL_BYTES: usize = 512 * 1_048_576;
pub const V2_MAX_VISITED_ENTRIES: usize = 200_000;

/// Fixed domain bytes for parent-corpus digest version 1.
///
/// The complete v1 preimage is:
///
/// ```text
/// "asdecided-corpus-digest-v1\0"
/// frame(0x01, source UTF-8)
/// frame(0x02, raw .decided/config.yaml bytes)
/// for each corpus-relative path in UTF-8 byte order:
///   frame(0x03, path UTF-8)
///   frame(0x04, raw file bytes)
/// ```
///
/// A frame is its one-byte tag, an unsigned 64-bit big-endian payload length,
/// and the payload. Tags plus explicit lengths make every tuple boundary
/// unambiguous; locations, metadata, and timestamps never enter the preimage.
pub const DIGEST_V1_DOMAIN: &[u8] = b"asdecided-corpus-digest-v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentCorpusErrorCode {
    MalformedManifest,
    MultipleParents,
    MaterialisationMissing,
    ParentCorpusMissing,
    ParentConfigMissing,
    ChildConfigMissing,
    InvalidConfig,
    ChildSourceMissing,
    ParentSourceMissing,
    SourceMismatch,
    SourceCollision,
    PathEscape,
    SymlinkTraversal,
    TransitiveInheritance,
    SnapshotFailed,
    DigestMismatch,
    DuplicateParent,
    Cycle,
    DivergentPin,
    OverlappingRoots,
    LimitExceeded,
    UnsupportedFilesystem,
    SnapshotChanged,
}

impl ParentCorpusErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MalformedManifest => "parent-corpus-malformed-manifest",
            Self::MultipleParents => "parent-corpus-multiple-parents",
            Self::MaterialisationMissing => "parent-corpus-materialisation-missing",
            Self::ParentCorpusMissing => "parent-corpus-missing",
            Self::ParentConfigMissing => "parent-corpus-config-missing",
            Self::ChildConfigMissing => "parent-corpus-child-config-missing",
            Self::InvalidConfig => "parent-corpus-config-invalid",
            Self::ChildSourceMissing => "parent-corpus-child-source-missing",
            Self::ParentSourceMissing => "parent-corpus-source-missing",
            Self::SourceMismatch => "parent-corpus-source-mismatch",
            Self::SourceCollision => "parent-corpus-source-collision",
            Self::PathEscape => "parent-corpus-path-escape",
            Self::SymlinkTraversal => "parent-corpus-symlink-traversal",
            Self::TransitiveInheritance => "parent-corpus-transitive-inheritance",
            Self::SnapshotFailed => "parent-corpus-snapshot-failed",
            Self::DigestMismatch => "parent-corpus-digest-mismatch",
            Self::DuplicateParent => "corpus-federation-duplicate-parent",
            Self::Cycle => "corpus-federation-cycle",
            Self::DivergentPin => "corpus-federation-divergent-pin",
            Self::OverlappingRoots => "corpus-federation-overlapping-roots",
            Self::LimitExceeded => "corpus-federation-limit-exceeded",
            Self::UnsupportedFilesystem => "corpus-federation-unsupported-filesystem",
            Self::SnapshotChanged => "corpus-federation-snapshot-changed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentCorpusError {
    pub code: ParentCorpusErrorCode,
    pub message: String,
    pub path: Option<PathBuf>,
}

impl ParentCorpusError {
    fn new(code: ParentCorpusErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            path: None,
        }
    }

    fn at(
        code: ParentCorpusErrorCode,
        path: impl Into<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            path: Some(path.into()),
        }
    }

    pub const fn stable_code(&self) -> &'static str {
        self.code.as_str()
    }
}

impl fmt::Display for ParentCorpusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.stable_code(), self.message)
    }
}

impl std::error::Error for ParentCorpusError {}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawParentDeclaration {
    version: u32,
    alias: String,
    source: String,
    root: String,
    corpus: String,
    digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGraphManifest {
    version: u32,
    parents: Vec<RawGraphParentDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGraphParentDeclaration {
    alias: String,
    source: String,
    root: String,
    corpus: String,
    digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGraphOverrides {
    version: u32,
    items: Vec<RawGraphOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGraphOverride {
    target: String,
    #[serde(rename = "with")]
    replacement: String,
    rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentDeclaration {
    pub version: u32,
    pub alias: String,
    pub source: String,
    pub root: String,
    pub corpus: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CorpusManifest {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub inherits: ParentDeclaration,
    /// Parsed but not interpreted here. Override resolution belongs to the
    /// central composition layer; retaining the value prevents a second
    /// Markdown section parser from drifting from this manifest boundary.
    pub overrides: Option<serde_yaml::Value>,
    /// Normalised YAML payload of the optional override mapping. The exact
    /// manifest bytes remain authoritative; this separate payload makes the
    /// mapping an explicit logical-generation input (ADR-143).
    pub override_mapping_bytes: Option<Vec<u8>>,
}

/// Strict manifest-v2 declaration. Parent order is retained only because the
/// exact manifest bytes are authenticated; all semantic consumers use
/// [`parents`](Self::parents) in the canonical order produced by the loader.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphCorpusManifest {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub parents: Vec<ParentDeclaration>,
    pub overrides: Option<serde_yaml::Value>,
    pub override_mapping_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotFile {
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentDigest {
    pub source: String,
    pub config_path: PathBuf,
    pub config_bytes: Vec<u8>,
    pub corpus_root: PathBuf,
    pub files: Vec<SnapshotFile>,
    /// Full `sha256:<64 lowercase hex>` value.
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentDigestV2 {
    pub source: String,
    pub config_path: PathBuf,
    pub config_bytes: Vec<u8>,
    pub manifest_path: PathBuf,
    pub manifest_bytes: Option<Vec<u8>>,
    pub corpus_root: PathBuf,
    pub files: Vec<SnapshotFile>,
    /// Full `sha256-v2:<64 lowercase hex>` value.
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedParent {
    pub manifest_path: PathBuf,
    pub manifest_bytes: Vec<u8>,
    pub declaration: ParentDeclaration,
    pub child_repository_root: PathBuf,
    pub child_source: String,
    pub child_config_path: PathBuf,
    pub child_config_bytes: Vec<u8>,
    pub materialisation_root: PathBuf,
    pub corpus_root: PathBuf,
    pub config_path: PathBuf,
    pub config_bytes: Vec<u8>,
    pub files: Vec<SnapshotFile>,
    /// The verified, full `sha256:<64 lowercase hex>` pin.
    pub digest: String,
    /// Parsed but not interpreted by the materialisation boundary.
    pub overrides: Option<serde_yaml::Value>,
    /// Normalised YAML payload of the override mapping, if declared.
    pub override_mapping_bytes: Option<Vec<u8>>,
}

/// One unique logical inherited source in a verified v2 closure.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedFederationNode {
    pub source: String,
    pub digest: String,
    pub config_path: PathBuf,
    pub config_bytes: Vec<u8>,
    pub manifest_path: PathBuf,
    pub manifest_bytes: Option<Vec<u8>>,
    pub manifest_version: Option<u32>,
    /// Parsed source-local override declaration. Composition validates and
    /// lifts a retained v1 hop into source-aware graph keys.
    pub overrides: Option<serde_yaml::Value>,
    pub override_mapping_bytes: Option<Vec<u8>>,
    pub corpus_root: PathBuf,
    pub files: Vec<SnapshotFile>,
}

/// One declared and independently verified physical edge. Equal logical
/// diamond targets therefore still have one row per materialised route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedFederationEdge {
    pub owner_source: String,
    pub alias: String,
    pub target_source: String,
    pub declared_digest: String,
    pub canonical_digest: String,
    pub root: String,
    pub corpus: String,
    pub materialisation_root: PathBuf,
    pub corpus_root: PathBuf,
}

/// Immutable, source-aware graph-verification result. Composition and serving
/// build from these captured bytes and must not reopen inherited public paths.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedFederation {
    pub repository_root: PathBuf,
    pub root_source: String,
    pub root_corpus_path: String,
    pub root_corpus_root: PathBuf,
    pub root_files: Vec<SnapshotFile>,
    pub root_config_path: PathBuf,
    pub root_config_bytes: Vec<u8>,
    pub manifest: GraphCorpusManifest,
    pub nodes: Vec<VerifiedFederationNode>,
    pub edges: Vec<VerifiedFederationEdge>,
    pub materialisation_roots: Vec<PathBuf>,
    pub corpus_roots: Vec<PathBuf>,
}

impl VerifiedFederation {
    pub fn contains_materialised_path(&self, path: &Path) -> bool {
        canonical_or_absolute(path).is_some_and(|candidate| {
            self.materialisation_roots
                .iter()
                .any(|root| candidate == *root || candidate.starts_with(root))
        })
    }

    pub fn node(&self, source: &str) -> Option<&VerifiedFederationNode> {
        self.nodes.iter().find(|node| node.source == source)
    }
}

impl VerifiedParent {
    /// Return the verification-time snapshot row for a stable inherited path.
    /// Callers must not reinterpret `relative_path` beneath the child root.
    pub fn snapshot_file(&self, path: &crate::corpus::ArtifactPath) -> Option<&SnapshotFile> {
        if path.source != self.declaration.source {
            return None;
        }
        self.files
            .iter()
            .find(|file| file.relative_path == path.relative_path)
    }

    /// Exact bytes hashed by parent verification for this inherited artifact.
    pub fn artifact_bytes(&self, path: &crate::corpus::ArtifactPath) -> Option<&[u8]> {
        self.snapshot_file(path).map(|file| file.bytes.as_slice())
    }

    /// Strict UTF-8 plus universal-newline decoding used by `get_artifact`.
    /// Invalid UTF-8 remains unreadable, matching the existing public tool.
    pub fn artifact_text(&self, path: &crate::corpus::ArtifactPath) -> Option<String> {
        let text = std::str::from_utf8(self.artifact_bytes(path)?).ok()?;
        Some(text.replace("\r\n", "\n").replace('\r', "\n"))
    }

    /// True when `path` resolves inside the read-only materialisation subtree.
    /// Local walks use this predicate to prevent the same Markdown byte from
    /// entering both the child and inherited layers.
    pub fn contains_materialised_path(&self, path: &Path) -> bool {
        canonical_or_absolute(path).is_some_and(|candidate| {
            candidate == self.materialisation_root
                || candidate.starts_with(&self.materialisation_root)
        })
    }

    /// Remove entries that resolve beneath the verified parent subtree.
    pub fn exclude_materialisation<T, F>(&self, entries: Vec<T>, path_of: F) -> Vec<T>
    where
        F: Fn(&T) -> &Path,
    {
        entries
            .into_iter()
            .filter(|entry| !self.contains_materialised_path(path_of(entry)))
            .collect()
    }
}

fn malformed(path: &Path, reason: impl Into<String>) -> ParentCorpusError {
    ParentCorpusError::at(
        ParentCorpusErrorCode::MalformedManifest,
        path,
        format!(
            "malformed federation manifest {}: {}",
            path.display(),
            reason.into()
        ),
    )
}

fn limit_error(path: &Path, dimension: &str, limit: usize, observed: usize) -> ParentCorpusError {
    ParentCorpusError::at(
        ParentCorpusErrorCode::LimitExceeded,
        path,
        format!(
            "federation limit exceeded: dimension={dimension}, limit={limit}, observed={observed}"
        ),
    )
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn is_top_level_atx_h2(lines: &[&str], line: i64) -> bool {
    if line < 0 {
        return false;
    }
    let Some(raw) = lines.get(line as usize) else {
        return false;
    };
    let indent = raw.bytes().take_while(|byte| *byte == b' ').count();
    if indent > 3 {
        return false;
    }
    let rest = &raw[indent..];
    rest.starts_with("## ") || rest.starts_with("##\t")
}

fn is_top_level_heading(lines: &[&str], line: i64) -> bool {
    if line < 0 {
        return false;
    }
    let index = line as usize;
    let Some(raw) = lines.get(index) else {
        return false;
    };
    let indent = raw.bytes().take_while(|byte| *byte == b' ').count();
    if indent > 3 {
        return false;
    }
    let rest = &raw[indent..];
    if rest.starts_with('#') {
        return true;
    }
    // Top-level Setext heading. Container-prefixed underlines do not match.
    lines.get(index + 1).is_some_and(|underline| {
        let indent = underline.bytes().take_while(|byte| *byte == b' ').count();
        if indent > 3 {
            return false;
        }
        let underline = underline[indent..].trim_end();
        !underline.is_empty()
            && (underline.bytes().all(|byte| byte == b'=')
                || underline.bytes().all(|byte| byte == b'-'))
    })
}

fn heading_sections(text: &str, name: &str) -> Vec<(usize, usize)> {
    let events = consumed_events(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut sections = Vec::new();
    for (index, event) in events.iter().enumerate() {
        if !event.heading
            || event.tag != "h2"
            || event.content != name
            || !is_top_level_atx_h2(&lines, event.line)
        {
            continue;
        }
        let start = event.line.max(0) as usize + 1;
        let end = events[index + 1..]
            .iter()
            .find(|next| {
                next.heading
                    && next.line >= 0
                    && matches!(next.tag, "h1" | "h2")
                    && is_top_level_heading(&lines, next.line)
            })
            .map_or_else(|| text.lines().count(), |next| next.line as usize);
        sections.push((start, end));
    }
    sections
}

fn reject_misspelled_operational_headings(
    path: &Path,
    text: &str,
) -> Result<(), ParentCorpusError> {
    let lines: Vec<&str> = text.lines().collect();
    for event in consumed_events(text) {
        if !event.heading || event.tag != "h2" || !is_top_level_atx_h2(&lines, event.line) {
            continue;
        }
        if (event.content.eq_ignore_ascii_case("inherits") && event.content != "inherits")
            || (event.content.eq_ignore_ascii_case("overrides") && event.content != "overrides")
        {
            return Err(malformed(
                path,
                format!(
                    "federation heading must use exact lowercase spelling: ## {}",
                    event.content.to_ascii_lowercase()
                ),
            ));
        }
    }
    Ok(())
}

fn fence_open(line: &str) -> Option<(u8, usize, &str)> {
    let indent = line.bytes().take_while(|byte| *byte == b' ').count();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let marker = *rest.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let count = rest.bytes().take_while(|byte| *byte == marker).count();
    if count < 3 {
        return None;
    }
    Some((marker, count, rest[count..].trim()))
}

fn fence_close(line: &str, marker: u8, count: usize) -> bool {
    let indent = line.bytes().take_while(|byte| *byte == b' ').count();
    if indent > 3 {
        return false;
    }
    let rest = &line[indent..];
    let seen = rest.bytes().take_while(|byte| *byte == marker).count();
    seen >= count && rest[seen..].trim().is_empty()
}

fn fenced_yaml_blocks(
    path: &Path,
    text: &str,
    start: usize,
    end: usize,
) -> Result<Vec<String>, ParentCorpusError> {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut index = start;
    while index < end.min(lines.len()) {
        let Some((marker, count, info)) = fence_open(lines[index]) else {
            index += 1;
            continue;
        };
        let content_start = index + 1;
        index += 1;
        while index < end.min(lines.len()) && !fence_close(lines[index], marker, count) {
            index += 1;
        }
        if index >= end.min(lines.len()) {
            return Err(malformed(path, "unterminated fenced block"));
        }
        if info == "yaml" {
            blocks.push(lines[content_start..index].join("\n"));
        }
        index += 1;
    }
    Ok(blocks)
}

fn valid_alias(alias: &str) -> bool {
    let bytes = alias.as_bytes();
    !bytes.is_empty()
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && (bytes[bytes.len() - 1].is_ascii_lowercase() || bytes[bytes.len() - 1].is_ascii_digit())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

fn validate_relative_path(value: &str, field: &str) -> Result<(), String> {
    let path = Path::new(value);
    if value.is_empty() {
        return Err(format!("'{field}' must be a non-empty relative path"));
    }
    if path.is_absolute() {
        return Err(format!("'{field}' must not be absolute"));
    }
    for component in path.components() {
        match component {
            Component::ParentDir => return Err(format!("'{field}' must not contain '..'")),
            Component::Prefix(_) | Component::RootDir => {
                return Err(format!("'{field}' must be repository-relative"))
            }
            Component::Normal(_) | Component::CurDir => {}
        }
    }
    Ok(())
}

fn parse_declaration(path: &Path, yaml: &str) -> Result<ParentDeclaration, ParentCorpusError> {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml).map_err(|error| {
        malformed(
            path,
            format!("## inherits YAML must be one mapping: {error}"),
        )
    })?;
    if value.as_sequence().is_some_and(|parents| parents.len() > 1) {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::MultipleParents,
            path,
            "the first federation increment accepts exactly one direct parent mapping",
        ));
    }
    if !value.is_mapping() {
        return Err(malformed(path, "## inherits YAML must be one mapping"));
    }
    let raw: RawParentDeclaration = serde_yaml::from_value(value).map_err(|error| {
        malformed(
            path,
            format!("## inherits YAML must be one mapping: {error}"),
        )
    })?;
    if raw.version != 1 {
        return Err(malformed(
            path,
            format!("unsupported inheritance manifest version: {}", raw.version),
        ));
    }
    if !valid_alias(&raw.alias) {
        return Err(malformed(
            path,
            "'alias' must be a lowercase local name containing only letters, digits, '.', '_', or '-'",
        ));
    }
    if !crate::scaffold::valid_corpus_source(&raw.source) {
        return Err(malformed(
            path,
            "'source' must be a lower-case slash-namespaced corpus identity",
        ));
    }
    validate_relative_path(&raw.root, "root")
        .map_err(|reason| ParentCorpusError::at(ParentCorpusErrorCode::PathEscape, path, reason))?;
    validate_relative_path(&raw.corpus, "corpus")
        .map_err(|reason| ParentCorpusError::at(ParentCorpusErrorCode::PathEscape, path, reason))?;
    let hash = raw.digest.strip_prefix(DIGEST_PREFIX).unwrap_or("");
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(malformed(
            path,
            "'digest' must be sha256: followed by exactly 64 lowercase hexadecimal characters",
        ));
    }
    Ok(ParentDeclaration {
        version: raw.version,
        alias: raw.alias,
        source: raw.source,
        root: raw.root,
        corpus: raw.corpus,
        digest: raw.digest,
    })
}

/// Parse the fixed operational manifest. Absence means no parent; presence is
/// strict and cannot be partially interpreted.
pub fn load_manifest(repository_root: &Path) -> Result<Option<CorpusManifest>, ParentCorpusError> {
    let path = repository_root.join(MANIFEST_RELATIVE_PATH);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ParentCorpusError::at(
                ParentCorpusErrorCode::MalformedManifest,
                &path,
                format!(
                    "cannot inspect federation manifest {}: {error}",
                    path.display()
                ),
            ));
        }
    };
    ensure_no_symlink_components(repository_root, &path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::SymlinkTraversal,
            &path,
            format!(
                "federation manifest must be a regular file: {}",
                path.display()
            ),
        ));
    }
    let bytes = std::fs::read(&path).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::MalformedManifest,
            &path,
            format!(
                "cannot read federation manifest {}: {error}",
                path.display()
            ),
        )
    })?;
    let raw = std::str::from_utf8(&bytes)
        .map_err(|_| malformed(&path, "manifest must be valid UTF-8"))?;
    let text = normalize_newlines(raw);
    reject_misspelled_operational_headings(&path, &text)?;

    let inherits = heading_sections(&text, "inherits");
    if inherits.len() > 1 {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::MultipleParents,
            &path,
            "the first federation increment accepts exactly one ## inherits declaration",
        ));
    }
    let Some((start, end)) = inherits.first().copied() else {
        return Err(malformed(
            &path,
            "missing exact lowercase heading '## inherits'",
        ));
    };
    let blocks = fenced_yaml_blocks(&path, &text, start, end)?;
    if blocks.len() > 1 {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::MultipleParents,
            &path,
            "## inherits must contain exactly one fenced YAML mapping",
        ));
    }
    let Some(yaml) = blocks.first() else {
        return Err(malformed(
            &path,
            "## inherits must contain exactly one fenced yaml block",
        ));
    };
    let inherits = parse_declaration(&path, yaml)?;

    let override_sections = heading_sections(&text, "overrides");
    if override_sections.len() > 1 {
        return Err(malformed(&path, "## overrides may appear at most once"));
    }
    let (overrides, override_mapping_bytes) =
        if let Some((start, end)) = override_sections.first().copied() {
            let blocks = fenced_yaml_blocks(&path, &text, start, end)?;
            if blocks.len() != 1 {
                return Err(malformed(
                    &path,
                    "## overrides must contain exactly one fenced yaml block",
                ));
            }
            let value: serde_yaml::Value =
                serde_yaml::from_str(&blocks[0]).map_err(|error| {
                    malformed(
                        &path,
                        format!("## overrides YAML must be one mapping: {error}"),
                    )
                })?;
            if !value.is_mapping() {
                return Err(malformed(&path, "## overrides YAML must be one mapping"));
            }
            (Some(value), Some(blocks[0].as_bytes().to_vec()))
        } else {
            (None, None)
        };

    Ok(Some(CorpusManifest {
        path,
        bytes,
        inherits,
        overrides,
        override_mapping_bytes,
    }))
}

fn validate_v2_path(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("'{field}' must be a non-empty POSIX-relative path"));
    }
    if value.len() > V2_MAX_PATH_BYTES {
        return Err(format!("'{field}' exceeds {V2_MAX_PATH_BYTES} UTF-8 bytes"));
    }
    if value.starts_with('/')
        || value.starts_with("//")
        || value.contains('\\')
        || value.as_bytes().get(1) == Some(&b':')
    {
        return Err(format!("'{field}' must be POSIX-relative"));
    }
    let components: Vec<&str> = value.split('/').collect();
    if components.len() > V2_MAX_PATH_COMPONENTS {
        return Err(format!(
            "'{field}' exceeds {V2_MAX_PATH_COMPONENTS} path components"
        ));
    }
    for component in components {
        if component.is_empty() || component == "." || component == ".." {
            return Err(format!(
                "'{field}' must not contain empty, '.', or '..' components"
            ));
        }
        if component.len() > V2_MAX_PATH_COMPONENT_BYTES {
            return Err(format!(
                "'{field}' component exceeds {V2_MAX_PATH_COMPONENT_BYTES} UTF-8 bytes"
            ));
        }
    }
    Ok(())
}

fn validate_v2_path_limits(
    path: &Path,
    value: &str,
    field: &str,
) -> Result<(), ParentCorpusError> {
    if value.len() > V2_MAX_PATH_BYTES {
        return Err(limit_error(
            path,
            &format!("{field}-path-bytes"),
            V2_MAX_PATH_BYTES,
            V2_MAX_PATH_BYTES + 1,
        ));
    }
    let components: Vec<&str> = value.split('/').collect();
    if components.len() > V2_MAX_PATH_COMPONENTS {
        return Err(limit_error(
            path,
            &format!("{field}-path-components"),
            V2_MAX_PATH_COMPONENTS,
            V2_MAX_PATH_COMPONENTS + 1,
        ));
    }
    if components
        .iter()
        .any(|component| component.len() > V2_MAX_PATH_COMPONENT_BYTES)
    {
        return Err(limit_error(
            path,
            &format!("{field}-path-component-bytes"),
            V2_MAX_PATH_COMPONENT_BYTES,
            V2_MAX_PATH_COMPONENT_BYTES + 1,
        ));
    }
    Ok(())
}

fn strict_yaml_lexical_check(path: &Path, yaml: &str) -> Result<(), ParentCorpusError> {
    for (line_index, line) in yaml.lines().enumerate() {
        let mut single = false;
        let mut double = false;
        let mut escaped = false;
        let mut plain = String::new();
        for character in line.chars() {
            if escaped {
                escaped = false;
                continue;
            }
            if double && character == '\\' {
                escaped = true;
                continue;
            }
            if !double && character == '\'' {
                single = !single;
                continue;
            }
            if !single && character == '"' {
                double = !double;
                continue;
            }
            if !single && !double && character == '#' {
                break;
            }
            if !single && !double {
                plain.push(character);
            }
        }
        let tokens = plain.split(|character: char| {
            character.is_whitespace() || matches!(character, ':' | ',' | '[' | ']' | '{' | '}')
        });
        if tokens
            .clone()
            .any(|token| token.starts_with('&') || token.starts_with('*') || token.starts_with('!'))
            || plain.trim_start().starts_with("<<:")
            || plain.contains(" <<:")
        {
            return Err(malformed(
                path,
                format!(
                    "version 2 YAML forbids anchors, aliases, custom tags, and merge keys (line {})",
                    line_index + 1
                ),
            ));
        }
    }
    Ok(())
}

fn yaml_shape(value: &serde_yaml::Value, depth: usize, nodes: &mut usize) -> Result<(), String> {
    *nodes += 1;
    if *nodes > V2_MAX_YAML_NODES {
        return Err(format!("version 2 YAML exceeds {V2_MAX_YAML_NODES} nodes"));
    }
    if depth > V2_MAX_YAML_DEPTH {
        return Err(format!("version 2 YAML exceeds {V2_MAX_YAML_DEPTH} levels"));
    }
    match value {
        serde_yaml::Value::Sequence(values) => {
            for value in values {
                yaml_shape(value, depth + 1, nodes)?;
            }
        }
        serde_yaml::Value::Mapping(mapping) => {
            for (key, value) in mapping {
                if !matches!(key, serde_yaml::Value::String(_)) {
                    return Err("version 2 YAML mapping keys must be strings".to_string());
                }
                yaml_shape(key, depth + 1, nodes)?;
                yaml_shape(value, depth + 1, nodes)?;
            }
        }
        serde_yaml::Value::Tagged(_) => {
            return Err("version 2 YAML forbids custom tags".to_string())
        }
        serde_yaml::Value::Null
        | serde_yaml::Value::Bool(_)
        | serde_yaml::Value::Number(_)
        | serde_yaml::Value::String(_) => {}
    }
    Ok(())
}

fn parse_strict_v2_yaml(
    path: &Path,
    yaml: &str,
    section: &str,
) -> Result<serde_yaml::Value, ParentCorpusError> {
    strict_yaml_lexical_check(path, yaml)?;
    let value: serde_yaml::Value = serde_yaml::from_str(yaml)
        .map_err(|error| malformed(path, format!("{section} YAML must be one mapping: {error}")))?;
    if !value.is_mapping() {
        return Err(malformed(
            path,
            format!("{section} YAML must be one mapping"),
        ));
    }
    let mut nodes = 0;
    yaml_shape(&value, 1, &mut nodes).map_err(|reason| {
        if reason.contains("nodes") {
            limit_error(
                path,
                "yaml-nodes",
                V2_MAX_YAML_NODES,
                V2_MAX_YAML_NODES + 1,
            )
        } else if reason.contains("levels") {
            limit_error(
                path,
                "yaml-depth",
                V2_MAX_YAML_DEPTH,
                V2_MAX_YAML_DEPTH + 1,
            )
        } else {
            malformed(path, reason)
        }
    })?;
    Ok(value)
}

fn graph_parent(
    path: &Path,
    raw: RawGraphParentDeclaration,
) -> Result<ParentDeclaration, ParentCorpusError> {
    if raw.alias.len() > V2_MAX_ALIAS_BYTES {
        return Err(limit_error(
            path,
            "alias-bytes",
            V2_MAX_ALIAS_BYTES,
            V2_MAX_ALIAS_BYTES + 1,
        ));
    }
    if !valid_alias(&raw.alias) {
        return Err(malformed(
            path,
            format!(
                "'alias' must match the version 2 lowercase syntax and be at most {V2_MAX_ALIAS_BYTES} bytes"
            ),
        ));
    }
    if raw.source.len() > V2_MAX_SOURCE_BYTES {
        return Err(limit_error(
            path,
            "source-bytes",
            V2_MAX_SOURCE_BYTES,
            V2_MAX_SOURCE_BYTES + 1,
        ));
    }
    if !crate::scaffold::valid_corpus_source(&raw.source) {
        return Err(malformed(
            path,
            format!(
                "'source' must be a lower-case slash-namespaced identity of at most {V2_MAX_SOURCE_BYTES} bytes"
            ),
        ));
    }
    validate_v2_path_limits(path, &raw.root, "root")?;
    validate_v2_path_limits(path, &raw.corpus, "corpus")?;
    validate_v2_path(&raw.root, "root")
        .map_err(|reason| ParentCorpusError::at(ParentCorpusErrorCode::PathEscape, path, reason))?;
    validate_v2_path(&raw.corpus, "corpus")
        .map_err(|reason| ParentCorpusError::at(ParentCorpusErrorCode::PathEscape, path, reason))?;
    let hash = raw.digest.strip_prefix(DIGEST_V2_PREFIX).unwrap_or("");
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(malformed(
            path,
            "'digest' must be sha256-v2: followed by exactly 64 lowercase hexadecimal characters",
        ));
    }
    Ok(ParentDeclaration {
        version: 2,
        alias: raw.alias,
        source: raw.source,
        root: raw.root,
        corpus: raw.corpus,
        digest: raw.digest,
    })
}

/// Parse manifest version 2 without changing the established v1 parser.
/// `None` means either no manifest or a version-1 manifest, allowing existing
/// consumers to retain their exact path.
pub fn load_graph_manifest(
    repository_root: &Path,
) -> Result<Option<GraphCorpusManifest>, ParentCorpusError> {
    let path = repository_root.join(MANIFEST_RELATIVE_PATH);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ParentCorpusError::at(
                ParentCorpusErrorCode::MalformedManifest,
                &path,
                format!(
                    "cannot inspect federation manifest {}: {error}",
                    path.display()
                ),
            ))
        }
    };
    ensure_no_symlink_components(repository_root, &path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::SymlinkTraversal,
            &path,
            format!(
                "federation manifest must be a regular file: {}",
                path.display()
            ),
        ));
    }
    if metadata.len() > V2_MAX_MANIFEST_BYTES as u64 {
        return Err(limit_error(
            &path,
            "manifest-bytes",
            V2_MAX_MANIFEST_BYTES,
            V2_MAX_MANIFEST_BYTES + 1,
        ));
    }
    let bytes = std::fs::read(&path).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::MalformedManifest,
            &path,
            format!(
                "cannot read federation manifest {}: {error}",
                path.display()
            ),
        )
    })?;
    let raw = std::str::from_utf8(&bytes)
        .map_err(|_| malformed(&path, "manifest must be valid UTF-8"))?;
    let text = normalize_newlines(raw);
    reject_misspelled_operational_headings(&path, &text)?;
    let inherits_sections = heading_sections(&text, "inherits");
    if inherits_sections.len() != 1 {
        return Err(malformed(
            &path,
            "manifest must contain exactly one exact lowercase '## inherits' heading",
        ));
    }
    let (start, end) = inherits_sections[0];
    let blocks = fenced_yaml_blocks(&path, &text, start, end)?;
    if blocks.len() != 1 {
        return Err(malformed(
            &path,
            "## inherits must contain exactly one fenced yaml block",
        ));
    }
    // Inspect only the version scalar before selecting the strict v2 mode.
    let probe: serde_yaml::Value = serde_yaml::from_str(&blocks[0]).map_err(|error| {
        malformed(
            &path,
            format!("## inherits YAML must be one mapping: {error}"),
        )
    })?;
    let version = probe
        .as_mapping()
        .and_then(|mapping| mapping.get(serde_yaml::Value::String("version".to_string())))
        .and_then(serde_yaml::Value::as_u64);
    if version == Some(1) {
        return Ok(None);
    }
    let value = parse_strict_v2_yaml(&path, &blocks[0], "## inherits")?;
    let raw: RawGraphManifest = serde_yaml::from_value(value).map_err(|error| {
        malformed(
            &path,
            format!("invalid version 2 inheritance mapping: {error}"),
        )
    })?;
    if raw.version != 2 {
        return Err(malformed(
            &path,
            format!("unsupported inheritance manifest version: {}", raw.version),
        ));
    }
    if raw.parents.is_empty() {
        return Err(malformed(&path, "version 2 parents must not be empty"));
    }
    if raw.parents.len() > V2_MAX_DIRECT_PARENTS {
        return Err(limit_error(
            &path,
            "direct-parents",
            V2_MAX_DIRECT_PARENTS,
            V2_MAX_DIRECT_PARENTS + 1,
        ));
    }
    let mut aliases = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut parents = Vec::with_capacity(raw.parents.len());
    for raw_parent in raw.parents {
        let parent = graph_parent(&path, raw_parent)?;
        if !aliases.insert(parent.alias.clone()) {
            return Err(ParentCorpusError::at(
                ParentCorpusErrorCode::DuplicateParent,
                &path,
                format!("duplicate direct parent alias '{}'", parent.alias),
            ));
        }
        if !sources.insert(parent.source.clone()) {
            return Err(ParentCorpusError::at(
                ParentCorpusErrorCode::DuplicateParent,
                &path,
                format!("duplicate direct parent source '{}'", parent.source),
            ));
        }
        parents.push(parent);
    }
    parents.sort_by(|left, right| {
        (
            &left.source,
            &left.digest,
            &left.alias,
            &left.root,
            &left.corpus,
        )
            .cmp(&(
                &right.source,
                &right.digest,
                &right.alias,
                &right.root,
                &right.corpus,
            ))
    });

    let override_sections = heading_sections(&text, "overrides");
    if override_sections.len() > 1 {
        return Err(malformed(&path, "## overrides may appear at most once"));
    }
    let (overrides, override_mapping_bytes) =
        if let Some((start, end)) = override_sections.first().copied() {
            let blocks = fenced_yaml_blocks(&path, &text, start, end)?;
            if blocks.len() != 1 {
                return Err(malformed(
                    &path,
                    "## overrides must contain exactly one fenced yaml block",
                ));
            }
            let value = parse_strict_v2_yaml(&path, &blocks[0], "## overrides")?;
        let parsed: RawGraphOverrides = serde_yaml::from_value(value.clone()).map_err(|error| {
            malformed(&path, format!("invalid version 2 override mapping: {error}"))
        })?;
        if parsed.version != 2 {
            return Err(malformed(
                &path,
                "## overrides version must match ## inherits version 2",
            ));
        }
        for item in &parsed.items {
            if item.target.is_empty() || item.replacement.is_empty() || item.rationale.is_empty() {
                return Err(malformed(
                    &path,
                    "version 2 override operands must be non-empty canonical references",
                ));
            }
        }
        let item_count = parsed.items.len();
            if item_count > V2_MAX_OVERRIDES {
                return Err(limit_error(
                    &path,
                    "overrides",
                    V2_MAX_OVERRIDES,
                    V2_MAX_OVERRIDES + 1,
                ));
            }
            (Some(value), Some(blocks[0].as_bytes().to_vec()))
        } else {
            (None, None)
        };

    Ok(Some(GraphCorpusManifest {
        path,
        bytes,
        parents,
        overrides,
        override_mapping_bytes,
    }))
}

fn lexical_components(path: &Path) -> Result<Vec<&std::ffi::OsStr>, ()> {
    let mut out = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => out.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return Err(()),
        }
    }
    Ok(out)
}

fn checked_relative_join(
    boundary: &Path,
    relative: &str,
    field: &str,
) -> Result<PathBuf, ParentCorpusError> {
    let components = lexical_components(Path::new(relative)).map_err(|_| {
        ParentCorpusError::new(
            ParentCorpusErrorCode::PathEscape,
            format!("parent {field} must be a relative path without '..': {relative}"),
        )
    })?;
    if components.is_empty() && relative != "." {
        return Err(ParentCorpusError::new(
            ParentCorpusErrorCode::PathEscape,
            format!("parent {field} must be a non-empty relative path"),
        ));
    }
    let mut joined = boundary.to_path_buf();
    for component in components {
        joined.push(component);
    }
    Ok(joined)
}

fn ensure_no_symlink_components(boundary: &Path, target: &Path) -> Result<(), ParentCorpusError> {
    let relative = target.strip_prefix(boundary).map_err(|_| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::PathEscape,
            target,
            format!(
                "parent path escapes repository boundary: {}",
                target.display()
            ),
        )
    })?;
    let mut current = boundary.to_path_buf();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            continue;
        };
        current.push(value);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ParentCorpusError::at(
                    ParentCorpusErrorCode::SymlinkTraversal,
                    &current,
                    format!("parent path traverses a symlink: {}", current.display()),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(ParentCorpusError::at(
                    ParentCorpusErrorCode::SnapshotFailed,
                    &current,
                    format!("cannot inspect parent path {}: {error}", current.display()),
                ));
            }
        }
    }
    Ok(())
}

fn canonical_confined(
    boundary: &Path,
    candidate: &Path,
    missing_code: ParentCorpusErrorCode,
    kind: &str,
) -> Result<PathBuf, ParentCorpusError> {
    ensure_no_symlink_components(boundary, candidate)?;
    let canonical = std::fs::canonicalize(candidate).map_err(|error| {
        ParentCorpusError::at(
            missing_code,
            candidate,
            format!(
                "parent {kind} is unavailable at {}: {error}",
                candidate.display()
            ),
        )
    })?;
    if canonical != boundary && !canonical.starts_with(boundary) {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::PathEscape,
            candidate,
            format!(
                "parent {kind} escapes repository boundary: {}",
                candidate.display()
            ),
        ));
    }
    Ok(canonical)
}

fn canonical_or_absolute(path: &Path) -> Option<PathBuf> {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return Some(canonical);
    }
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        std::env::current_dir().ok().map(|cwd| cwd.join(path))
    }
}

#[cfg(target_os = "linux")]
fn ensure_no_mount_boundary(boundary: &Path, target: &Path) -> Result<(), ParentCorpusError> {
    fn unescape_mount_path(value: &str) -> String {
        value
            .replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\012", "\n")
            .replace("\\134", "\\")
    }
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::UnsupportedFilesystem,
            target,
            format!("cannot inspect Linux mount identity: {error}"),
        )
    })?;
    for line in mountinfo.lines() {
        let Some(raw_mount) = line.split_whitespace().nth(4) else {
            return Err(ParentCorpusError::at(
                ParentCorpusErrorCode::UnsupportedFilesystem,
                target,
                "Linux mount identity record is malformed",
            ));
        };
        let mount = PathBuf::from(unescape_mount_path(raw_mount));
        if mount != boundary
            && mount.starts_with(boundary)
            && (target == mount || target.starts_with(&mount))
        {
            return Err(ParentCorpusError::at(
                ParentCorpusErrorCode::UnsupportedFilesystem,
                target,
                format!(
                    "federation path crosses mount boundary at {}",
                    mount.display()
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn ensure_no_mount_boundary(_boundary: &Path, _target: &Path) -> Result<(), ParentCorpusError> {
    // Stable device/volume identities are checked at every directory and file
    // on these platforms. Linux additionally exposes bind-mount identity.
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StableFileIdentity {
    device: u64,
    inode: u64,
    links: u64,
    length: u64,
    changed_seconds: i64,
    changed_nanos: i64,
}

#[cfg(unix)]
fn stable_file_identity(
    _path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<StableFileIdentity, ()> {
    use std::os::unix::fs::MetadataExt;
    Ok(StableFileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        links: metadata.nlink(),
        length: metadata.len(),
        changed_seconds: metadata.ctime(),
        changed_nanos: metadata.ctime_nsec(),
    })
}

#[cfg(windows)]
fn stable_file_identity(
    path: &Path,
    _metadata: &std::fs::Metadata,
) -> Result<StableFileIdentity, ()> {
    use std::ffi::c_void;
    use std::mem::MaybeUninit;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[repr(C)]
    struct ByHandleFileInformation {
        _file_attributes: u32,
        _creation_time: FileTime,
        _last_access_time: FileTime,
        last_write_time: FileTime,
        volume_serial_number: u32,
        file_size_high: u32,
        file_size_low: u32,
        number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
    }

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandle(
            file: *mut c_void,
            information: *mut ByHandleFileInformation,
        ) -> i32;
    }

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|_| ())?;
    let mut information = MaybeUninit::<ByHandleFileInformation>::uninit();
    // SAFETY: `file` keeps a valid owned handle alive for the call and the
    // Win32 function initializes the complete output structure on success.
    let succeeded = unsafe {
        GetFileInformationByHandle(
            file.as_raw_handle().cast::<c_void>(),
            information.as_mut_ptr(),
        )
    };
    if succeeded == 0 {
        return Err(());
    }
    // SAFETY: the successful Win32 call above initialized every field.
    let information = unsafe { information.assume_init() };
    let last_write = ((information.last_write_time.high as u64) << 32)
        | information.last_write_time.low as u64;
    Ok(StableFileIdentity {
        device: information.volume_serial_number as u64,
        inode: ((information.file_index_high as u64) << 32)
            | information.file_index_low as u64,
        links: information.number_of_links as u64,
        length: ((information.file_size_high as u64) << 32)
            | information.file_size_low as u64,
        changed_seconds: (last_write / 10_000_000) as i64,
        changed_nanos: ((last_write % 10_000_000) * 100) as i64,
    })
}

#[cfg(not(any(unix, windows)))]
fn stable_file_identity(
    _path: &Path,
    _metadata: &std::fs::Metadata,
) -> Result<StableFileIdentity, ()> {
    Err(())
}

fn read_stable_regular(
    path: &Path,
    maximum: usize,
    dimension: &str,
    inherited: bool,
    expected_device: Option<u64>,
) -> Result<Vec<u8>, ParentCorpusError> {
    let before = std::fs::symlink_metadata(path).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotFailed,
            path,
            format!(
                "cannot inspect federation input {}: {error}",
                path.display()
            ),
        )
    })?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::SymlinkTraversal,
            path,
            format!(
                "federation input must be a real regular file: {}",
                path.display()
            ),
        ));
    }
    let before_identity = stable_file_identity(path, &before).map_err(|_| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::UnsupportedFilesystem,
            path,
            format!("stable file identity is unavailable for {}", path.display()),
        )
    })?;
    if inherited && before_identity.links != 1 {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::UnsupportedFilesystem,
            path,
            format!(
                "inherited files must have exactly one hard link: {}",
                path.display()
            ),
        ));
    }
    if expected_device.is_some_and(|device| device != before_identity.device) {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::UnsupportedFilesystem,
            path,
            format!(
                "federation input crosses a filesystem boundary: {}",
                path.display()
            ),
        ));
    }
    if before_identity.length > maximum as u64 {
        return Err(limit_error(path, dimension, maximum, maximum + 1));
    }

    #[cfg(unix)]
    let bytes = {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|error| {
                ParentCorpusError::at(
                    ParentCorpusErrorCode::SnapshotFailed,
                    path,
                    format!("cannot open federation input {}: {error}", path.display()),
                )
            })?;
        let mut bytes = Vec::with_capacity(before_identity.length as usize);
        file.read_to_end(&mut bytes).map_err(|error| {
            ParentCorpusError::at(
                ParentCorpusErrorCode::SnapshotFailed,
                path,
                format!("cannot read federation input {}: {error}", path.display()),
            )
        })?;
        bytes
    };
    #[cfg(not(unix))]
    let bytes = std::fs::read(path).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotFailed,
            path,
            format!("cannot read federation input {}: {error}", path.display()),
        )
    })?;

    let after = std::fs::symlink_metadata(path).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotChanged,
            path,
            format!(
                "federation input changed during capture {}: {error}",
                path.display()
            ),
        )
    })?;
    let after_identity = stable_file_identity(path, &after).map_err(|_| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::UnsupportedFilesystem,
            path,
            format!("stable file identity is unavailable for {}", path.display()),
        )
    })?;
    if before_identity != after_identity || bytes.len() as u64 != before_identity.length {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotChanged,
            path,
            format!(
                "federation input changed during capture: {}",
                path.display()
            ),
        ));
    }
    Ok(bytes)
}

#[derive(Debug, Default)]
struct GraphCounters {
    edges: usize,
    overrides: usize,
    physical_bytes: usize,
    visited_entries: usize,
    logical_bytes: usize,
    logical_files: usize,
}

fn add_limited(
    current: &mut usize,
    amount: usize,
    limit: usize,
    dimension: &str,
    path: &Path,
) -> Result<(), ParentCorpusError> {
    let next = current.saturating_add(amount);
    if next > limit {
        return Err(limit_error(path, dimension, limit, limit + 1));
    }
    *current = next;
    Ok(())
}

fn directory_device(path: &Path) -> Result<u64, ParentCorpusError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotFailed,
            path,
            format!(
                "cannot inspect federation directory {}: {error}",
                path.display()
            ),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::SymlinkTraversal,
            path,
            format!(
                "federation directory must be a real directory: {}",
                path.display()
            ),
        ));
    }
    stable_file_identity(path, &metadata)
        .map(|identity| identity.device)
        .map_err(|_| {
            ParentCorpusError::at(
                ParentCorpusErrorCode::UnsupportedFilesystem,
                path,
                format!(
                    "stable filesystem identity is unavailable for {}",
                    path.display()
                ),
            )
        })
}

#[allow(clippy::too_many_arguments)] // The explicit bounds/context prevent accidental v1 reuse.
fn snapshot_directory_v2(
    corpus_root: &Path,
    directory: &Path,
    components: &mut Vec<String>,
    exclusions: &[PathBuf],
    expected_device: u64,
    inherited_files: bool,
    charge_limits: bool,
    counters: &mut GraphCounters,
    output: &mut Vec<SnapshotFile>,
) -> Result<(), ParentCorpusError> {
    let directory_before = std::fs::symlink_metadata(directory).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotFailed,
            directory,
            format!(
                "cannot inspect inherited directory {}: {error}",
                directory.display()
            ),
        )
    })?;
    let directory_identity = stable_file_identity(directory, &directory_before).map_err(|_| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::UnsupportedFilesystem,
            directory,
            format!(
                "stable directory identity is unavailable for {}",
                directory.display()
            ),
        )
    })?;
    if directory_identity.device != expected_device {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::UnsupportedFilesystem,
            directory,
            format!(
                "federation directory crosses a filesystem boundary: {}",
                directory.display()
            ),
        ));
    }
    let entries = std::fs::read_dir(directory).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotFailed,
            directory,
            format!(
                "cannot read inherited corpus directory {}: {error}",
                directory.display()
            ),
        )
    })?;
    let mut entries = entries.collect::<Result<Vec<_>, _>>().map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotFailed,
            directory,
            format!(
                "cannot enumerate inherited corpus directory {}: {error}",
                directory.display()
            ),
        )
    })?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if charge_limits {
            add_limited(
                &mut counters.visited_entries,
                1,
                V2_MAX_VISITED_ENTRIES,
                "visited-entries",
                directory,
            )?;
        }
        let path = entry.path();
        if exclusions
            .iter()
            .any(|excluded| path == *excluded || path.starts_with(excluded))
        {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            ParentCorpusError::at(
                ParentCorpusErrorCode::SnapshotFailed,
                &path,
                format!("cannot inspect inherited entry {}: {error}", path.display()),
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(ParentCorpusError::at(
                ParentCorpusErrorCode::SymlinkTraversal,
                &path,
                format!(
                    "inherited corpus must not contain symlinks: {}",
                    path.display()
                ),
            ));
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        components.push(name.clone());
        if metadata.is_dir() {
            snapshot_directory_v2(
                corpus_root,
                &path,
                components,
                exclusions,
                expected_device,
                inherited_files,
                charge_limits,
                counters,
                output,
            )?;
        } else if name.ends_with(".md") {
            if !metadata.is_file() {
                return Err(ParentCorpusError::at(
                    ParentCorpusErrorCode::SnapshotFailed,
                    &path,
                    format!(
                        "inherited Markdown is not a regular file: {}",
                        path.display()
                    ),
                ));
            }
            let bytes = read_stable_regular(
                &path,
                V2_MAX_FILE_BYTES,
                "file-bytes",
                inherited_files,
                Some(expected_device),
            )?;
            if charge_limits {
                add_limited(
                    &mut counters.physical_bytes,
                    bytes.len(),
                    V2_MAX_PHYSICAL_BYTES,
                    "physical-bytes",
                    &path,
                )?;
            }
            output.push(SnapshotFile {
                relative_path: components.join("/"),
                absolute_path: path,
                bytes,
            });
        }
        components.pop();
    }
    let directory_after = std::fs::symlink_metadata(directory).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotChanged,
            directory,
            format!(
                "inherited directory changed during capture {}: {error}",
                directory.display()
            ),
        )
    })?;
    let after_identity = stable_file_identity(directory, &directory_after).map_err(|_| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::UnsupportedFilesystem,
            directory,
            format!(
                "stable directory identity is unavailable for {}",
                directory.display()
            ),
        )
    })?;
    if directory_identity != after_identity {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotChanged,
            directory,
            format!(
                "inherited directory changed during capture: {}",
                directory.display()
            ),
        ));
    }
    output.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    // Every emitted path came from the confined recursive walk.
    debug_assert!(output
        .iter()
        .all(|file| file.absolute_path.starts_with(corpus_root)));
    Ok(())
}

fn snapshot_directory(
    corpus_root: &Path,
    directory: &Path,
    components: &mut Vec<String>,
    output: &mut Vec<SnapshotFile>,
) -> Result<(), ParentCorpusError> {
    let entries = std::fs::read_dir(directory).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotFailed,
            directory,
            format!(
                "cannot read parent corpus directory {}: {error}",
                directory.display()
            ),
        )
    })?;
    let mut entries = entries.collect::<Result<Vec<_>, _>>().map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotFailed,
            directory,
            format!(
                "cannot enumerate parent corpus directory {}: {error}",
                directory.display()
            ),
        )
    })?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        // Match the engine's corpus discovery boundary: paths which cannot be
        // represented as UTF-8 are not discovered as artifacts and therefore
        // do not enter the digest.
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            ParentCorpusError::at(
                ParentCorpusErrorCode::SnapshotFailed,
                &path,
                format!(
                    "cannot inspect parent corpus entry {}: {error}",
                    path.display()
                ),
            )
        })?;
        if metadata.file_type().is_symlink() {
            if name.ends_with(".md") {
                return Err(ParentCorpusError::at(
                    ParentCorpusErrorCode::SymlinkTraversal,
                    &path,
                    format!(
                        "parent Markdown artifact must not be a symlink: {}",
                        path.display()
                    ),
                ));
            }
            // A symlinked directory is never traversed and therefore cannot
            // contribute bytes to the snapshot.
            continue;
        }
        components.push(name.clone());
        if metadata.is_dir() {
            snapshot_directory(corpus_root, &path, components, output)?;
        } else if name.ends_with(".md") {
            if !metadata.is_file() {
                return Err(ParentCorpusError::at(
                    ParentCorpusErrorCode::SnapshotFailed,
                    &path,
                    format!(
                        "parent Markdown artifact is not a regular file: {}",
                        path.display()
                    ),
                ));
            }
            let canonical = std::fs::canonicalize(&path).map_err(|error| {
                ParentCorpusError::at(
                    ParentCorpusErrorCode::SnapshotFailed,
                    &path,
                    format!("cannot resolve parent artifact {}: {error}", path.display()),
                )
            })?;
            if !canonical.starts_with(corpus_root) {
                return Err(ParentCorpusError::at(
                    ParentCorpusErrorCode::PathEscape,
                    &path,
                    format!("parent artifact escapes corpus root: {}", path.display()),
                ));
            }
            let bytes = std::fs::read(&canonical).map_err(|error| {
                ParentCorpusError::at(
                    ParentCorpusErrorCode::SnapshotFailed,
                    &path,
                    format!("cannot read parent artifact {}: {error}", path.display()),
                )
            })?;
            output.push(SnapshotFile {
                relative_path: components.join("/"),
                absolute_path: canonical,
                bytes,
            });
        }
        components.pop();
    }
    Ok(())
}

fn write_frame(hasher: &mut Sha256, tag: u8, payload: &[u8]) {
    hasher.update(&[tag]);
    hasher.update(&(payload.len() as u64).to_be_bytes());
    hasher.update(payload);
}

/// Pure digest over an already captured byte snapshot. File order at the API
/// boundary is ignored; paths are always folded in canonical UTF-8 order.
pub fn digest_snapshot(source: &str, config_bytes: &[u8], files: &[SnapshotFile]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DIGEST_V1_DOMAIN);
    write_frame(&mut hasher, 0x01, source.as_bytes());
    write_frame(&mut hasher, 0x02, config_bytes);
    let mut files: Vec<&SnapshotFile> = files.iter().collect();
    files.sort_by_key(|file| file.relative_path.as_str());
    for file in files {
        write_frame(&mut hasher, 0x03, file.relative_path.as_bytes());
        write_frame(&mut hasher, 0x04, &file.bytes);
    }
    format!("{DIGEST_PREFIX}{}", hasher.hexdigest())
}

/// Pure canonical version-2 digest over one captured node snapshot.
pub fn digest_snapshot_v2(
    source: &str,
    config_bytes: &[u8],
    manifest_bytes: Option<&[u8]>,
    files: &[SnapshotFile],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DIGEST_V2_DOMAIN);
    write_frame(&mut hasher, 0x01, source.as_bytes());
    write_frame(&mut hasher, 0x02, config_bytes);
    write_frame(
        &mut hasher,
        0x03,
        if manifest_bytes.is_some() {
            &[0x01]
        } else {
            &[0x00]
        },
    );
    if let Some(manifest) = manifest_bytes {
        write_frame(&mut hasher, 0x04, manifest);
    }
    let mut files: Vec<&SnapshotFile> = files.iter().collect();
    files.sort_by_key(|file| file.relative_path.as_str());
    for file in files {
        write_frame(&mut hasher, 0x05, file.relative_path.as_bytes());
        write_frame(&mut hasher, 0x06, &file.bytes);
    }
    format!("{DIGEST_V2_PREFIX}{}", hasher.hexdigest())
}

fn read_bounded_source(config_path: &Path) -> Result<(String, Vec<u8>), ParentCorpusError> {
    let bytes = std::fs::read(config_path).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::InvalidConfig,
            config_path,
            format!(
                "cannot read parent config {}: {error}",
                config_path.display()
            ),
        )
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::InvalidConfig,
            config_path,
            format!(
                "parent config must be valid UTF-8: {}",
                config_path.display()
            ),
        )
    })?;
    // Parse the exact file rather than ancestor-walking: the materialisation
    // root is the provenance boundary chosen by the manifest/operator.
    let identity = crate::scaffold::parse_identity_config(&config_path.to_string_lossy(), text)
        .map_err(|error| {
            ParentCorpusError::at(
                ParentCorpusErrorCode::InvalidConfig,
                config_path,
                error.message().to_string(),
            )
        })?;
    let source = identity.corpus_source.ok_or_else(|| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::ParentSourceMissing,
            config_path,
            format!(
                "parent config {} must declare an explicit corpus.source",
                config_path.display()
            ),
        )
    })?;
    Ok((source, bytes))
}

fn snapshot_at(root: &Path, corpus_relative: &str) -> Result<ParentDigest, ParentCorpusError> {
    let root_input = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                ParentCorpusError::new(
                    ParentCorpusErrorCode::SnapshotFailed,
                    format!("cannot determine current directory: {error}"),
                )
            })?
            .join(root)
    };
    if std::fs::symlink_metadata(&root_input)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::SymlinkTraversal,
            &root_input,
            format!(
                "parent materialisation root must not be a symlink: {}",
                root_input.display()
            ),
        ));
    }
    let root = std::fs::canonicalize(&root_input).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::MaterialisationMissing,
            &root_input,
            format!(
                "parent materialisation is unavailable at {}: {error}",
                root_input.display()
            ),
        )
    })?;
    if !root.is_dir() {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::MaterialisationMissing,
            &root,
            format!(
                "parent materialisation is not a directory: {}",
                root.display()
            ),
        ));
    }
    validate_relative_path(corpus_relative, "corpus")
        .map_err(|reason| ParentCorpusError::new(ParentCorpusErrorCode::PathEscape, reason))?;

    let config_candidate = root.join(CONFIG_RELATIVE_PATH);
    let config_path = canonical_confined(
        &root,
        &config_candidate,
        ParentCorpusErrorCode::ParentConfigMissing,
        "config",
    )?;
    if !config_path.is_file() {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::ParentConfigMissing,
            &config_path,
            format!(
                "parent config is not a regular file: {}",
                config_path.display()
            ),
        ));
    }
    let corpus_candidate = checked_relative_join(&root, corpus_relative, "corpus")?;
    let corpus_root = canonical_confined(
        &root,
        &corpus_candidate,
        ParentCorpusErrorCode::ParentCorpusMissing,
        "corpus",
    )?;
    if !corpus_root.is_dir() {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::ParentCorpusMissing,
            &corpus_root,
            format!(
                "parent corpus is not a directory: {}",
                corpus_root.display()
            ),
        ));
    }

    let (source, config_bytes) = read_bounded_source(&config_path)?;
    let mut files = Vec::new();
    snapshot_directory(&corpus_root, &corpus_root, &mut Vec::new(), &mut files)?;
    // Directory recursion is already component-sorted. Pin this explicitly so
    // a future traversal refactor cannot alter the digest contract.
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let digest = digest_snapshot(&source, &config_bytes, &files);
    Ok(ParentDigest {
        source,
        config_path,
        config_bytes,
        corpus_root,
        files,
        digest,
    })
}

/// Calculate a parent pin from a bounded materialisation root and a relative
/// corpus directory. This is the pure operator surface behind
/// `decided corpus digest`; it never writes, fetches, or updates a pin.
pub fn calculate_parent_digest(
    root: impl AsRef<Path>,
    corpus_relative: &str,
) -> Result<ParentDigest, ParentCorpusError> {
    snapshot_at(root.as_ref(), corpus_relative)
}

#[derive(Debug, Clone, PartialEq)]
enum CapturedManifest {
    None,
    V1(CorpusManifest),
    V2(GraphCorpusManifest),
}

impl CapturedManifest {
    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::None => None,
            Self::V1(manifest) => Some(&manifest.bytes),
            Self::V2(manifest) => Some(&manifest.bytes),
        }
    }

    fn direct_parents(&self) -> Vec<ParentDeclaration> {
        match self {
            Self::None => Vec::new(),
            Self::V1(manifest) => vec![manifest.inherits.clone()],
            Self::V2(manifest) => manifest.parents.clone(),
        }
    }


    fn version(&self) -> Option<u32> {
        match self {
            Self::None => None,
            Self::V1(_) => Some(1),
            Self::V2(_) => Some(2),
        }
    }

    fn overrides(&self) -> Option<&serde_yaml::Value> {
        match self {
            Self::None => None,
            Self::V1(manifest) => manifest.overrides.as_ref(),
            Self::V2(manifest) => manifest.overrides.as_ref(),
        }
    }

    fn override_mapping_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::None => None,
            Self::V1(manifest) => manifest.override_mapping_bytes.as_deref(),
            Self::V2(manifest) => manifest.override_mapping_bytes.as_deref(),
        }
    }

    fn override_count(&self) -> usize {
        self.overrides()
            .and_then(serde_yaml::Value::as_mapping)
            .and_then(|mapping| {
                mapping.get(serde_yaml::Value::String("items".to_string()))
            })
            .and_then(serde_yaml::Value::as_sequence)
            .map_or(0, Vec::len)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CapturedNode {
    repository_root: PathBuf,
    source: String,
    config_path: PathBuf,
    config_bytes: Vec<u8>,
    manifest_path: PathBuf,
    manifest: CapturedManifest,
    corpus_root: PathBuf,
    files: Vec<SnapshotFile>,
    digest_v2: String,
}

fn captured_manifest(repository_root: &Path) -> Result<CapturedManifest, ParentCorpusError> {
    if let Some(manifest) = load_graph_manifest(repository_root)? {
        return Ok(CapturedManifest::V2(manifest));
    }
    match load_manifest(repository_root)? {
        Some(manifest) => Ok(CapturedManifest::V1(manifest)),
        None => Ok(CapturedManifest::None),
    }
}

fn capture_v2_node(
    repository_root: &Path,
    corpus_relative: &str,
    invocation_root: &Path,
    counters: &mut GraphCounters,
) -> Result<CapturedNode, ParentCorpusError> {
    let repository_root = std::fs::canonicalize(repository_root).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::MaterialisationMissing,
            repository_root,
            format!("parent materialisation is unavailable: {error}"),
        )
    })?;
    if repository_root == invocation_root || !repository_root.starts_with(invocation_root) {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::PathEscape,
            &repository_root,
            "inherited materialisation must be strictly contained by the invocation repository",
        ));
    }
    let device = directory_device(&repository_root)?;
    let config_candidate = repository_root.join(CONFIG_RELATIVE_PATH);
    ensure_no_symlink_components(&repository_root, &config_candidate)?;
    let config_path = canonical_confined(
        &repository_root,
        &config_candidate,
        ParentCorpusErrorCode::ParentConfigMissing,
        "config",
    )?;
    let config_bytes = read_stable_regular(
        &config_path,
        V2_MAX_CONFIG_BYTES,
        "config-bytes",
        true,
        Some(device),
    )?;
    add_limited(
        &mut counters.physical_bytes,
        config_bytes.len(),
        V2_MAX_PHYSICAL_BYTES,
        "physical-bytes",
        &config_path,
    )?;
    let config_text = std::str::from_utf8(&config_bytes).map_err(|_| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::InvalidConfig,
            &config_path,
            "version 2 governing config must be valid UTF-8",
        )
    })?;
    parse_strict_v2_yaml(&config_path, config_text, "governing config")?;
    let identity =
        crate::scaffold::parse_identity_config(&config_path.to_string_lossy(), config_text)
            .map_err(|error| {
                ParentCorpusError::at(
                    ParentCorpusErrorCode::InvalidConfig,
                    &config_path,
                    error.message().to_string(),
                )
            })?;
    let source = identity.corpus_source.ok_or_else(|| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::ParentSourceMissing,
            &config_path,
            "version 2 graph nodes must declare corpus.source",
        )
    })?;
    if source.len() > V2_MAX_SOURCE_BYTES {
        return Err(limit_error(
            &config_path,
            "source-bytes",
            V2_MAX_SOURCE_BYTES,
            V2_MAX_SOURCE_BYTES + 1,
        ));
    }

    let manifest_path = repository_root.join(MANIFEST_RELATIVE_PATH);
    ensure_no_symlink_components(&repository_root, &manifest_path)?;
    let manifest_bytes = match std::fs::symlink_metadata(&manifest_path) {
        Ok(_) => Some(read_stable_regular(
            &manifest_path,
            V2_MAX_MANIFEST_BYTES,
            "manifest-bytes",
            true,
            Some(device),
        )?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(ParentCorpusError::at(
                ParentCorpusErrorCode::SnapshotFailed,
                &manifest_path,
                format!("cannot inspect inherited manifest: {error}"),
            ))
        }
    };
    if let Some(bytes) = &manifest_bytes {
        add_limited(
            &mut counters.physical_bytes,
            bytes.len(),
            V2_MAX_PHYSICAL_BYTES,
            "physical-bytes",
            &manifest_path,
        )?;
    }
    let manifest = captured_manifest(&repository_root)?;
    if manifest.bytes() != manifest_bytes.as_deref() {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotChanged,
            &manifest_path,
            "federation manifest changed while it was parsed",
        ));
    }
    add_limited(
        &mut counters.overrides,
        manifest.override_count(),
        V2_MAX_OVERRIDES,
        "overrides",
        &manifest_path,
    )?;

    let corpus_candidate = checked_relative_join(&repository_root, corpus_relative, "corpus")?;
    let corpus_root = canonical_confined(
        &repository_root,
        &corpus_candidate,
        ParentCorpusErrorCode::ParentCorpusMissing,
        "corpus",
    )?;
    ensure_no_mount_boundary(&repository_root, &corpus_root)?;
    if !corpus_root.is_dir() || directory_device(&corpus_root)? != device {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::UnsupportedFilesystem,
            &corpus_root,
            "inherited corpus must be a real directory on the materialisation filesystem",
        ));
    }
    let exclusions = manifest
        .direct_parents()
        .iter()
        .filter_map(|parent| checked_relative_join(&repository_root, &parent.root, "root").ok())
        .collect::<Vec<_>>();
    let mut files = Vec::new();
    snapshot_directory_v2(
        &corpus_root,
        &corpus_root,
        &mut Vec::new(),
        &exclusions,
        device,
        true,
        true,
        counters,
        &mut files,
    )?;
    let digest_v2 = digest_snapshot_v2(&source, &config_bytes, manifest_bytes.as_deref(), &files);
    Ok(CapturedNode {
        repository_root,
        source,
        config_path,
        config_bytes,
        manifest_path,
        manifest,
        corpus_root,
        files,
        digest_v2,
    })
}

/// Calculate a manifest-bound graph pin. The existing v1 function and output
/// remain unchanged; callers enter this contract explicitly.
pub fn calculate_parent_digest_v2(
    root: impl AsRef<Path>,
    corpus_relative: &str,
) -> Result<ParentDigestV2, ParentCorpusError> {
    validate_v2_path_limits(root.as_ref(), corpus_relative, "corpus")?;
    validate_v2_path(corpus_relative, "corpus")
        .map_err(|reason| ParentCorpusError::new(ParentCorpusErrorCode::PathEscape, reason))?;
    let input = root.as_ref();
    let root = std::fs::canonicalize(input).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::MaterialisationMissing,
            input,
            format!("parent materialisation is unavailable: {error}"),
        )
    })?;
    // `capture_v2_node` requires strict descent because graph nodes are
    // inherited. Give the operator capture a synthetic lexical parent while
    // retaining the real root as the only filesystem boundary.
    let invocation = root.parent().unwrap_or(&root).to_path_buf();
    let mut counters = GraphCounters::default();
    let captured = capture_v2_node(&root, corpus_relative, &invocation, &mut counters)?;
    Ok(ParentDigestV2 {
        source: captured.source,
        config_path: captured.config_path,
        config_bytes: captured.config_bytes,
        manifest_path: captured.manifest_path,
        manifest_bytes: captured.manifest.bytes().map(ToOwned::to_owned),
        corpus_root: captured.corpus_root,
        files: captured.files,
        digest: captured.digest_v2,
    })
}

struct GraphVerification {
    invocation_root: PathBuf,
    counters: GraphCounters,
    physical_captures: BTreeMap<(PathBuf, String), CapturedNode>,
    expanded_physical: BTreeSet<(PathBuf, String)>,
    logical_nodes: BTreeMap<String, VerifiedFederationNode>,
    edges: Vec<VerifiedFederationEdge>,
    materialisation_roots: BTreeSet<PathBuf>,
    corpus_roots: BTreeSet<PathBuf>,
}

impl GraphVerification {
    fn resolve_materialisation(
        &self,
        owner_root: &Path,
        declaration: &ParentDeclaration,
    ) -> Result<PathBuf, ParentCorpusError> {
        let candidate = checked_relative_join(owner_root, &declaration.root, "root")?;
        let root = canonical_confined(
            owner_root,
            &candidate,
            ParentCorpusErrorCode::MaterialisationMissing,
            "materialisation",
        )?;
        if root == owner_root || !root.is_dir() || !root.starts_with(&self.invocation_root) {
            return Err(ParentCorpusError::at(
                ParentCorpusErrorCode::PathEscape,
                &candidate,
                "parent materialisation must be a directory strictly inside its declaring repository and the invocation repository",
            ));
        }
        ensure_no_mount_boundary(owner_root, &root)?;
        Ok(root)
    }

    fn verify_edges(
        &mut self,
        owner_source: &str,
        owner_root: &Path,
        declarations: &[ParentDeclaration],
        depth: usize,
        active_sources: &mut Vec<String>,
        owner_is_v1: bool,
    ) -> Result<(), ParentCorpusError> {
        if depth > V2_MAX_INHERITANCE_DEPTH {
            return Err(limit_error(
                owner_root,
                "depth",
                V2_MAX_INHERITANCE_DEPTH,
                V2_MAX_INHERITANCE_DEPTH + 1,
            ));
        }
        let mut siblings: Vec<(PathBuf, &ParentDeclaration)> = Vec::new();
        for declaration in declarations {
            add_limited(
                &mut self.counters.edges,
                1,
                V2_MAX_EDGES,
                "edges",
                owner_root,
            )?;
            let materialisation_root = self.resolve_materialisation(owner_root, declaration)?;
            for (other_root, other) in &siblings {
                let overlap = materialisation_root == *other_root
                    || materialisation_root.starts_with(other_root)
                    || other_root.starts_with(&materialisation_root);
                if overlap
                    && !(materialisation_root == *other_root
                        && declaration.source == other.source
                        && declaration.digest == other.digest)
                {
                    return Err(ParentCorpusError::at(
                        ParentCorpusErrorCode::OverlappingRoots,
                        &materialisation_root,
                        format!(
                            "sibling parent roots overlap for '{}' and '{}'",
                            other.source, declaration.source
                        ),
                    ));
                }
            }
            siblings.push((materialisation_root.clone(), declaration));

            let physical_key = (materialisation_root.clone(), declaration.corpus.clone());
            let captured = if let Some(captured) = self.physical_captures.get(&physical_key) {
                captured.clone()
            } else {
                let captured = capture_v2_node(
                    &materialisation_root,
                    &declaration.corpus,
                    &self.invocation_root,
                    &mut self.counters,
                )?;
                self.physical_captures
                    .insert(physical_key.clone(), captured.clone());
                captured
            };
            if captured.source != declaration.source {
                return Err(ParentCorpusError::at(
                    ParentCorpusErrorCode::SourceMismatch,
                    &captured.config_path,
                    format!(
                        "manifest source '{}' does not match inherited corpus.source '{}'",
                        declaration.source, captured.source
                    ),
                ));
            }
            let verified_pin = if declaration.version == 1 {
                digest_snapshot(&captured.source, &captured.config_bytes, &captured.files)
            } else {
                captured.digest_v2.clone()
            };
            if verified_pin != declaration.digest {
                return Err(ParentCorpusError::at(
                    ParentCorpusErrorCode::DigestMismatch,
                    owner_root.join(MANIFEST_RELATIVE_PATH),
                    format!(
                        "declared parent digest '{}' does not match verified digest '{}'",
                        declaration.digest, verified_pin
                    ),
                ));
            }
            if owner_is_v1 && !matches!(captured.manifest, CapturedManifest::None) {
                return Err(ParentCorpusError::at(
                    ParentCorpusErrorCode::TransitiveInheritance,
                    &captured.manifest_path,
                    format!(
                        "version-1 parent source '{}' declares its own inheritance",
                        captured.source
                    ),
                ));
            }
            if active_sources
                .iter()
                .any(|source| source == &captured.source)
            {
                let mut route = active_sources.clone();
                route.push(captured.source.clone());
                return Err(ParentCorpusError::at(
                    ParentCorpusErrorCode::Cycle,
                    owner_root.join(MANIFEST_RELATIVE_PATH),
                    format!("federation source cycle: {}", route.join(" -> ")),
                ));
            }
            if let Some(existing) = self.logical_nodes.get(&captured.source) {
                if existing.digest != captured.digest_v2 {
                    return Err(ParentCorpusError::at(
                        ParentCorpusErrorCode::DivergentPin,
                        owner_root.join(MANIFEST_RELATIVE_PATH),
                        format!(
                            "source '{}' verified with divergent pins '{}' and '{}'",
                            captured.source, existing.digest, captured.digest_v2
                        ),
                    ));
                }
            }

            self.materialisation_roots
                .insert(materialisation_root.clone());
            self.corpus_roots.insert(captured.corpus_root.clone());
            self.edges.push(VerifiedFederationEdge {
                owner_source: owner_source.to_string(),
                alias: declaration.alias.clone(),
                target_source: captured.source.clone(),
                declared_digest: declaration.digest.clone(),
                canonical_digest: captured.digest_v2.clone(),
                root: declaration.root.clone(),
                corpus: declaration.corpus.clone(),
                materialisation_root,
                corpus_root: captured.corpus_root.clone(),
            });

            active_sources.push(captured.source.clone());
            if !self.expanded_physical.contains(&physical_key) {
                match &captured.manifest {
                    CapturedManifest::None => {}
                    CapturedManifest::V1(manifest) => self.verify_edges(
                        &captured.source,
                        &captured.repository_root,
                        std::slice::from_ref(&manifest.inherits),
                        depth + 1,
                        active_sources,
                        true,
                    )?,
                    CapturedManifest::V2(manifest) => self.verify_edges(
                        &captured.source,
                        &captured.repository_root,
                        &manifest.parents,
                        depth + 1,
                        active_sources,
                        false,
                    )?,
                }
                self.expanded_physical.insert(physical_key);
            }
            active_sources.pop();

            if !self.logical_nodes.contains_key(&captured.source) {
                if self.logical_nodes.len() >= V2_MAX_INHERITED_SOURCES {
                    return Err(limit_error(
                        &captured.config_path,
                        "unique-inherited-sources",
                        V2_MAX_INHERITED_SOURCES,
                        V2_MAX_INHERITED_SOURCES + 1,
                    ));
                }
                add_limited(
                    &mut self.counters.logical_files,
                    captured.files.len(),
                    V2_MAX_INHERITED_FILES,
                    "inherited-files",
                    &captured.corpus_root,
                )?;
                let logical_bytes = captured.config_bytes.len()
                    + captured.manifest.bytes().map_or(0, <[u8]>::len)
                    + captured
                        .files
                        .iter()
                        .map(|file| file.bytes.len())
                        .sum::<usize>();
                add_limited(
                    &mut self.counters.logical_bytes,
                    logical_bytes,
                    V2_MAX_LOGICAL_BYTES,
                    "logical-bytes",
                    &captured.corpus_root,
                )?;
                self.logical_nodes.insert(
                    captured.source.clone(),
                    VerifiedFederationNode {
                        source: captured.source,
                        digest: captured.digest_v2,
                        config_path: captured.config_path,
                        config_bytes: captured.config_bytes,
                        manifest_path: captured.manifest_path,
                        manifest_bytes: captured.manifest.bytes().map(ToOwned::to_owned),
                        manifest_version: captured.manifest.version(),
                        overrides: captured.manifest.overrides().cloned(),
                        override_mapping_bytes: captured
                            .manifest
                            .override_mapping_bytes()
                            .map(ToOwned::to_owned),
                        corpus_root: captured.corpus_root,
                        files: captured.files,
                    },
                );
            }
        }
        Ok(())
    }
}

/// Verify a version-2 federation graph. Absence and version 1 deliberately
/// return `None` so their established loader and observable behavior are not
/// routed through graph semantics.
pub fn verify_federation(
    repository_root: impl AsRef<Path>,
    root_corpus_relative: &str,
) -> Result<Option<VerifiedFederation>, ParentCorpusError> {
    let input = repository_root.as_ref();
    let repository_root = std::fs::canonicalize(input).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::PathEscape,
            input,
            format!("repository root is unavailable: {error}"),
        )
    })?;
    let Some(manifest) = load_graph_manifest(&repository_root)? else {
        return Ok(None);
    };
    validate_v2_path_limits(&repository_root, root_corpus_relative, "corpus")?;
    validate_v2_path(root_corpus_relative, "corpus")
        .map_err(|reason| ParentCorpusError::new(ParentCorpusErrorCode::PathEscape, reason))?;
    let root_config_path = repository_root.join(CONFIG_RELATIVE_PATH);
    ensure_no_symlink_components(&repository_root, &root_config_path)?;
    let root_config_bytes = read_stable_regular(
        &root_config_path,
        V2_MAX_CONFIG_BYTES,
        "config-bytes",
        false,
        Some(directory_device(&repository_root)?),
    )?;
    let config_text = std::str::from_utf8(&root_config_bytes).map_err(|_| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::InvalidConfig,
            &root_config_path,
            "version 2 root config must be valid UTF-8",
        )
    })?;
    parse_strict_v2_yaml(&root_config_path, config_text, "governing config")?;
    let identity =
        crate::scaffold::parse_identity_config(&root_config_path.to_string_lossy(), config_text)
            .map_err(|error| {
                ParentCorpusError::at(
                    ParentCorpusErrorCode::InvalidConfig,
                    &root_config_path,
                    error.message().to_string(),
                )
            })?;
    let root_source = identity.corpus_source.ok_or_else(|| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::ChildSourceMissing,
            &root_config_path,
            "version 2 root config must declare corpus.source",
        )
    })?;
    if root_source.len() > V2_MAX_SOURCE_BYTES {
        return Err(limit_error(
            &root_config_path,
            "source-bytes",
            V2_MAX_SOURCE_BYTES,
            V2_MAX_SOURCE_BYTES + 1,
        ));
    }
    let root_corpus_candidate =
        checked_relative_join(&repository_root, root_corpus_relative, "corpus")?;
    let root_corpus_root = canonical_confined(
        &repository_root,
        &root_corpus_candidate,
        ParentCorpusErrorCode::ParentCorpusMissing,
        "root corpus",
    )?;
    let root_device = directory_device(&repository_root)?;
    if directory_device(&root_corpus_root)? != root_device {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::UnsupportedFilesystem,
            &root_corpus_root,
            "root corpus crosses a filesystem boundary",
        ));
    }
    let root_exclusions = manifest
        .parents
        .iter()
        .map(|parent| checked_relative_join(&repository_root, &parent.root, "root"))
        .collect::<Result<Vec<_>, _>>()?;
    let mut root_files = Vec::new();
    let mut root_snapshot_counters = GraphCounters::default();
    snapshot_directory_v2(
        &root_corpus_root,
        &root_corpus_root,
        &mut Vec::new(),
        &root_exclusions,
        root_device,
        false,
        false,
        &mut root_snapshot_counters,
        &mut root_files,
    )?;
    let stable_manifest_bytes = read_stable_regular(
        &manifest.path,
        V2_MAX_MANIFEST_BYTES,
        "manifest-bytes",
        false,
        Some(root_device),
    )?;
    if stable_manifest_bytes != manifest.bytes {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::SnapshotChanged,
            &manifest.path,
            "root federation manifest changed while it was parsed",
        ));
    }
    let root_override_count = manifest
        .overrides
        .as_ref()
        .and_then(serde_yaml::Value::as_mapping)
        .and_then(|mapping| mapping.get(serde_yaml::Value::String("items".to_string())))
        .and_then(serde_yaml::Value::as_sequence)
        .map_or(0, Vec::len);
    let mut verification = GraphVerification {
        invocation_root: repository_root.clone(),
        counters: GraphCounters {
            overrides: root_override_count,
            ..GraphCounters::default()
        },
        physical_captures: BTreeMap::new(),
        expanded_physical: BTreeSet::new(),
        logical_nodes: BTreeMap::new(),
        edges: Vec::new(),
        materialisation_roots: BTreeSet::new(),
        corpus_roots: BTreeSet::new(),
    };
    verification.verify_edges(
        &root_source,
        &repository_root,
        &manifest.parents,
        1,
        &mut vec![root_source.clone()],
        false,
    )?;
    verification.edges.sort_by(|left, right| {
        (
            &left.owner_source,
            &left.target_source,
            &left.declared_digest,
            &left.alias,
            &left.root,
            &left.corpus,
            &left.materialisation_root,
        )
            .cmp(&(
                &right.owner_source,
                &right.target_source,
                &right.declared_digest,
                &right.alias,
                &right.root,
                &right.corpus,
                &right.materialisation_root,
            ))
    });
    Ok(Some(VerifiedFederation {
        repository_root,
        root_source,
        root_corpus_path: root_corpus_relative.to_string(),
        root_corpus_root,
        root_files,
        root_config_path,
        root_config_bytes,
        manifest,
        nodes: verification.logical_nodes.into_values().collect(),
        edges: verification.edges,
        materialisation_roots: verification.materialisation_roots.into_iter().collect(),
        corpus_roots: verification.corpus_roots.into_iter().collect(),
    }))
}

fn exact_config_source(
    config_path: &Path,
    missing_config: ParentCorpusErrorCode,
    missing_source: ParentCorpusErrorCode,
    owner: &str,
) -> Result<(String, Vec<u8>), ParentCorpusError> {
    if !config_path.is_file() {
        return Err(ParentCorpusError::at(
            missing_config,
            config_path,
            format!("{owner} config is missing: {}", config_path.display()),
        ));
    }
    let bytes = std::fs::read(config_path).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::InvalidConfig,
            config_path,
            format!(
                "cannot read {owner} config {}: {error}",
                config_path.display()
            ),
        )
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::InvalidConfig,
            config_path,
            format!(
                "{owner} config must be valid UTF-8: {}",
                config_path.display()
            ),
        )
    })?;
    let identity = crate::scaffold::parse_identity_config(&config_path.to_string_lossy(), text)
        .map_err(|error| {
            ParentCorpusError::at(
                ParentCorpusErrorCode::InvalidConfig,
                config_path,
                error.message().to_string(),
            )
        })?;
    let source = identity.corpus_source.ok_or_else(|| {
        ParentCorpusError::at(
            missing_source,
            config_path,
            format!("{owner} config must declare an explicit corpus.source"),
        )
    })?;
    Ok((source, bytes))
}

/// Verify the optional direct parent rooted at `child_repository_root`.
/// Nothing from the parent is returned until containment, topology, source,
/// and digest checks have all succeeded.
pub fn verify_parent(
    child_repository_root: impl AsRef<Path>,
) -> Result<Option<VerifiedParent>, ParentCorpusError> {
    let input = child_repository_root.as_ref();
    let child_root = std::fs::canonicalize(input).map_err(|error| {
        ParentCorpusError::at(
            ParentCorpusErrorCode::PathEscape,
            input,
            format!(
                "child repository root is unavailable at {}: {error}",
                input.display()
            ),
        )
    })?;
    if !child_root.is_dir() {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::PathEscape,
            &child_root,
            format!(
                "child repository root is not a directory: {}",
                child_root.display()
            ),
        ));
    }
    let Some(manifest) = load_manifest(&child_root)? else {
        return Ok(None);
    };

    let child_config = child_root.join(CONFIG_RELATIVE_PATH);
    ensure_no_symlink_components(&child_root, &child_config)?;
    let (child_source, child_config_bytes) = exact_config_source(
        &child_config,
        ParentCorpusErrorCode::ChildConfigMissing,
        ParentCorpusErrorCode::ChildSourceMissing,
        "child",
    )?;

    let materialisation_candidate =
        checked_relative_join(&child_root, &manifest.inherits.root, "root")?;
    let materialisation_root = canonical_confined(
        &child_root,
        &materialisation_candidate,
        ParentCorpusErrorCode::MaterialisationMissing,
        "materialisation",
    )?;
    if materialisation_root == child_root || !materialisation_root.is_dir() {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::PathEscape,
            &materialisation_candidate,
            "parent materialisation root must be a directory strictly inside the child repository",
        ));
    }

    let parent_manifest_path = materialisation_root.join(MANIFEST_RELATIVE_PATH);
    ensure_no_symlink_components(&materialisation_root, &parent_manifest_path)?;
    if load_manifest(&materialisation_root)?.is_some() {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::TransitiveInheritance,
            &parent_manifest_path,
            format!(
                "parent source '{}' declares its own inheritance; transitive federation is not supported",
                manifest.inherits.source
            ),
        ));
    }

    let digest = snapshot_at(&materialisation_root, &manifest.inherits.corpus)?;
    if digest.source != manifest.inherits.source {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::SourceMismatch,
            &digest.config_path,
            format!(
                "manifest source '{}' does not match parent corpus.source '{}'",
                manifest.inherits.source, digest.source
            ),
        ));
    }
    if child_source == digest.source {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::SourceCollision,
            &manifest.path,
            format!(
                "child and parent must declare distinct corpus.source values; both are '{}'",
                child_source
            ),
        ));
    }
    if digest.digest != manifest.inherits.digest {
        return Err(ParentCorpusError::at(
            ParentCorpusErrorCode::DigestMismatch,
            &manifest.path,
            format!(
                "declared parent digest '{}' does not match verified digest '{}'",
                manifest.inherits.digest, digest.digest
            ),
        ));
    }

    Ok(Some(VerifiedParent {
        manifest_path: manifest.path,
        manifest_bytes: manifest.bytes,
        declaration: manifest.inherits,
        child_repository_root: child_root,
        child_source,
        child_config_path: child_config,
        child_config_bytes,
        materialisation_root,
        corpus_root: digest.corpus_root,
        config_path: digest.config_path,
        config_bytes: digest.config_bytes,
        files: digest.files,
        digest: digest.digest,
        overrides: manifest.overrides,
        override_mapping_bytes: manifest.override_mapping_bytes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn scratch(name: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "asdecided-federation-{name}-{}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_parent(root: &Path) {
        fs::create_dir_all(root.join(".decided")).unwrap();
        fs::create_dir_all(root.join("decisions/sub")).unwrap();
        fs::write(
            root.join(".decided/config.yaml"),
            b"repository_key: STD\ncorpus:\n  source: acme/standards\n",
        )
        .unwrap();
        fs::write(root.join("decisions/a.md"), b"alpha\n").unwrap();
        fs::write(root.join("decisions/sub/b.md"), b"beta\r\n").unwrap();
        fs::write(root.join("decisions/ignored.MD"), b"ignored\n").unwrap();
        fs::create_dir_all(root.join("decisions/.hidden")).unwrap();
        fs::write(root.join("decisions/.hidden/secret.md"), b"hidden\n").unwrap();
    }

    fn write_child(root: &Path, digest: &str) {
        fs::create_dir_all(root.join(".decided")).unwrap();
        fs::write(
            root.join(".decided/config.yaml"),
            b"repository_key: APP\ncorpus:\n  source: acme/app\n",
        )
        .unwrap();
        fs::write(
            root.join(".decided/corpus.md"),
            format!(
                "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: standards\nsource: acme/standards\nroot: vendor/standards\ncorpus: decisions\ndigest: {digest}\n```\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn digest_v1_known_vector_and_exact_snapshot() {
        let root = scratch("digest-vector");
        write_parent(&root);
        let result = calculate_parent_digest(&root, "decisions").unwrap();
        assert_eq!(result.source, "acme/standards");
        assert_eq!(
            result
                .files
                .iter()
                .map(|file| file.relative_path.as_str())
                .collect::<Vec<_>>(),
            ["a.md", "sub/b.md"]
        );
        assert_eq!(result.files[1].bytes, b"beta\r\n");
        assert_eq!(
            result.digest,
            "sha256:899d5cdfa52b90a157b018dceb20f4f2901e0d56c91b089c12286c0b8b7b3325"
        );
        let mut reversed = result.files.clone();
        reversed.reverse();
        assert_eq!(
            digest_snapshot(&result.source, &result.config_bytes, &reversed),
            result.digest
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manifest_requires_exact_lowercase_heading_and_one_mapping() {
        let root = scratch("manifest");
        fs::create_dir_all(root.join(".decided")).unwrap();
        fs::write(
            root.join(".decided/corpus.md"),
            "# Corpus\n\n## Inherits\n\n```yaml\nversion: 1\n```\n",
        )
        .unwrap();
        let error = load_manifest(&root).unwrap_err();
        assert_eq!(error.code, ParentCorpusErrorCode::MalformedManifest);
        assert!(error.message.contains("exact lowercase"));

        fs::write(
            root.join(".decided/corpus.md"),
            "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\n```\n\n```yaml\nversion: 1\n```\n",
        )
        .unwrap();
        assert_eq!(
            load_manifest(&root).unwrap_err().code,
            ParentCorpusErrorCode::MultipleParents
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verifies_one_direct_parent_and_excludes_its_tree() {
        let child = scratch("verify");
        let parent = child.join("vendor/standards");
        write_parent(&parent);
        let pin = calculate_parent_digest(&parent, "decisions")
            .unwrap()
            .digest;
        write_child(&child, &pin);
        let verified = verify_parent(&child).unwrap().unwrap();
        assert_eq!(verified.digest, pin);
        assert_eq!(verified.child_source, "acme/app");
        assert!(verified.contains_materialised_path(&parent.join("decisions/a.md")));
        assert!(!verified.contains_materialised_path(&child.join("local.md")));
        let entries = vec![child.join("local.md"), parent.join("decisions/a.md")];
        let retained = verified.exclude_materialisation(entries, |path| path.as_path());
        assert_eq!(retained, [child.join("local.md")]);
        fs::remove_dir_all(child).unwrap();
    }

    #[test]
    fn stale_pin_and_source_mismatch_are_distinct() {
        let child = scratch("mismatch");
        let parent = child.join("vendor/standards");
        write_parent(&parent);
        let pin = calculate_parent_digest(&parent, "decisions")
            .unwrap()
            .digest;
        write_child(&child, &pin);
        fs::write(parent.join("decisions/a.md"), b"changed\n").unwrap();
        assert_eq!(
            verify_parent(&child).unwrap_err().code,
            ParentCorpusErrorCode::DigestMismatch
        );

        write_child(
            &child,
            &calculate_parent_digest(&parent, "decisions")
                .unwrap()
                .digest,
        );
        let manifest = fs::read_to_string(child.join(".decided/corpus.md"))
            .unwrap()
            .replace("source: acme/standards", "source: acme/other");
        fs::write(child.join(".decided/corpus.md"), manifest).unwrap();
        assert_eq!(
            verify_parent(&child).unwrap_err().code,
            ParentCorpusErrorCode::SourceMismatch
        );
        fs::remove_dir_all(child).unwrap();
    }

    #[test]
    fn transitive_parent_is_rejected_before_overlay() {
        let child = scratch("transitive");
        let parent = child.join("vendor/standards");
        write_parent(&parent);
        let pin = calculate_parent_digest(&parent, "decisions")
            .unwrap()
            .digest;
        write_child(&child, &pin);
        fs::write(
            parent.join(".decided/corpus.md"),
            "# Corpus\n\n## inherits\n\n```yaml\nversion: 1\nalias: upstream\nsource: acme/upstream\nroot: vendor/upstream\ncorpus: decisions\ndigest: sha256:0000000000000000000000000000000000000000000000000000000000000000\n```\n",
        )
        .unwrap();
        assert_eq!(
            verify_parent(&child).unwrap_err().code,
            ParentCorpusErrorCode::TransitiveInheritance
        );
        fs::remove_dir_all(child).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_materialisation_and_artifact_are_rejected() {
        use std::os::unix::fs::symlink;

        let child = scratch("symlink-root");
        let outside = scratch("outside");
        write_parent(&outside);
        fs::create_dir_all(child.join("vendor")).unwrap();
        symlink(&outside, child.join("vendor/standards")).unwrap();
        write_child(
            &child,
            &calculate_parent_digest(&outside, "decisions")
                .unwrap()
                .digest,
        );
        assert_eq!(
            verify_parent(&child).unwrap_err().code,
            ParentCorpusErrorCode::SymlinkTraversal
        );
        fs::remove_dir_all(&child).unwrap();
        fs::remove_dir_all(&outside).unwrap();

        let root = scratch("symlink-file");
        write_parent(&root);
        fs::write(root.join("target.md"), "target").unwrap();
        symlink("../target.md", root.join("decisions/link.md")).unwrap();
        assert_eq!(
            calculate_parent_digest(&root, "decisions")
                .unwrap_err()
                .code,
            ParentCorpusErrorCode::SymlinkTraversal
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_manifest_is_inert() {
        let root = scratch("no-manifest");
        assert!(verify_parent(&root).unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
