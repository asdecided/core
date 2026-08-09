//! Verified, offline parent-corpus materialisation (ADR-133 through ADR-135).
//!
//! This module owns only the declaration and byte-snapshot boundary. It does
//! not compose artifacts, resolve relationships, or give any read consumer a
//! second directory-overlay path. A successful [`verify_parent`] call returns
//! the exact config and Markdown bytes which were hashed, so later stages can
//! parse the verified snapshot without re-reading mutable parent files.

use serde::Deserialize;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::markdown::consumed_events;
use crate::sha256::Sha256;

pub const MANIFEST_RELATIVE_PATH: &str = ".decided/corpus.md";
pub const CONFIG_RELATIVE_PATH: &str = ".decided/config.yaml";
pub const DIGEST_PREFIX: &str = "sha256:";

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

impl VerifiedParent {
    /// Return the verification-time snapshot row for a stable inherited path.
    /// Callers must not reinterpret `relative_path` beneath the child root.
    pub fn snapshot_file(
        &self,
        path: &crate::corpus::ArtifactPath,
    ) -> Option<&SnapshotFile> {
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
