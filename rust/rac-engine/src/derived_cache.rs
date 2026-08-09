//! Content-addressed derived-index cache (ADR-099/ADR-112) — port of
//! `services/derived_cache.py` `DerivedIndexCache.load_or_build` plus the
//! stat-manifest freshness rungs of `services/freshness.py` the one-shot
//! path consumes (INDEX-PLAN B3).
//!
//! Every failure mode degrades to a fresh build: enabling the cache can only
//! change latency, never an answer or an exit code (ADR-080).

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use rayon::prelude::*;

use crate::derived::{DerivedIndex, SCHEMA_VERSION};
use crate::index_store::{
    manifest_root_key, open_freshness_manifest, open_store, remove_store, store_dir,
    write_freshness_manifest, write_store, FileState, MmapIndexReader,
};
use crate::walk::find_markdown_files;

pub const CACHE_DIR_ENV: &str = "DECIDED_CACHE_DIR";

/// Whether the persistent cache is active for this invocation (ADR-112):
/// on by default; `--no-cache` per invocation, non-empty `DECIDED_NO_CACHE`
/// environment-wide.
pub fn cache_enabled(cache_flag: bool) -> bool {
    cache_flag && std::env::var("DECIDED_NO_CACHE").unwrap_or_default().is_empty()
}

/// The derived-cache directory ladder: `DECIDED_CACHE_DIR` >
/// `$XDG_CACHE_HOME/decisions/derived` > `~/.cache/decisions/derived` >
/// `<tmp>/decided-cache/decisions/derived` (the homeless floor — never raises).
pub fn default_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(CACHE_DIR_ENV) {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let base = match std::env::var("XDG_CACHE_HOME") {
        Ok(xdg) if !xdg.is_empty() => PathBuf::from(xdg),
        _ => match std::env::var("HOME") {
            Ok(home) if !home.is_empty() => Path::new(&home).join(".cache"),
            _ => std::env::temp_dir().join("decided-cache"),
        },
    };
    base.join("decided").join("derived")
}

// ---------------------------------------------------------------------------
// Freshness rungs (services/freshness.py stat_scan + hash recomposition)
// ---------------------------------------------------------------------------

fn stat_pair(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime_ns = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos() as u64;
    Some((meta.len(), mtime_ns))
}

/// Diff the corpus against `prev_manifest` by stat, content-confirming
/// changes. Returns the rebuilt manifest (scan order) and the changed set.
pub fn stat_scan(
    root_str: &str,
    prev_manifest: &[(String, FileState)],
    content_confirm_all: bool,
    recursive: bool,
) -> (Vec<(String, FileState)>, BTreeSet<String>) {
    let prev: std::collections::HashMap<&str, &FileState> = prev_manifest
        .iter()
        .map(|(rel, state)| (rel.as_str(), state))
        .collect();
    let discovery_started = crate::timing::start();
    let entries = find_markdown_files(root_str, recursive);
    crate::timing::emit_since(
        "stat.discovery",
        discovery_started,
        &[("files", entries.len() as u64)],
    );
    // Metadata probes dominate the warm scan at large corpus sizes and are
    // independent. Indexed parallel collection preserves walk order, which is
    // part of the manifest/hash contract.
    let metadata_started = crate::timing::start();
    let scan_entry = |entry: &crate::walk::WalkEntry| {
        let rel = entry.components.join("/");
        let Some((size, mtime_ns)) = stat_pair(&entry.abs) else {
            return None; // vanished between enumeration and stat
        };
        if !content_confirm_all {
            if let Some(prev_state) = prev.get(rel.as_str()) {
                if prev_state.size == size && prev_state.mtime_ns == mtime_ns {
                    return Some((rel, (*prev_state).clone(), false)); // S5 accepted
                }
            }
        }
        let digest = crate::index_store::content_hash(&entry.abs);
        let changed_content = match prev.get(rel.as_str()) {
            Some(prev_state) => prev_state.content_hash != digest,
            None => true,
        };
        Some((
            rel.clone(),
            FileState {
                content_hash: digest,
                size,
                mtime_ns,
            },
            changed_content,
        ))
    };
    let scanned: Vec<Option<(String, FileState, bool)>> =
        entries.par_iter().map(scan_entry).collect();
    crate::timing::emit_since(
        "stat.metadata",
        metadata_started,
        &[("files", entries.len() as u64)],
    );
    let mut changed: BTreeSet<String> = BTreeSet::new();
    let mut new_manifest: Vec<(String, FileState)> = Vec::with_capacity(scanned.len());
    for (rel, state, changed_content) in scanned.into_iter().flatten() {
        if changed_content {
            changed.insert(rel.clone());
        }
        new_manifest.push((rel, state));
    }
    let present: std::collections::HashSet<&str> =
        new_manifest.iter().map(|(rel, _)| rel.as_str()).collect();
    for (rel, _) in prev_manifest {
        if !present.contains(rel.as_str()) {
            changed.insert(rel.clone()); // removed — enumeration is truth
        }
    }
    (new_manifest, changed)
}

/// Reproduce `corpus_content_hash` from the manifest's cached hashes.
pub fn corpus_hash_from_manifest(
    root_str: &str,
    manifest: &[(String, FileState)],
    recursive: bool,
) -> String {
    let by_rel: std::collections::HashMap<&str, &FileState> = manifest
        .iter()
        .map(|(rel, state)| (rel.as_str(), state))
        .collect();
    let mut hasher = crate::sha256::Sha256::new();
    for entry in find_markdown_files(root_str, recursive) {
        let rel = entry.components.join("/");
        let digest = match by_rel.get(rel.as_str()) {
            Some(state) => state.content_hash.clone(),
            None => crate::index_store::content_hash(&entry.abs),
        };
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(digest.as_bytes());
        hasher.update(b"\0");
    }
    hasher.hexdigest()
}

/// Recompose the corpus hash from a complete scan-order manifest without a
/// second filesystem walk. `stat_scan` always returns exactly this shape.
pub fn corpus_hash_from_complete_manifest(manifest: &[(String, FileState)]) -> String {
    let mut hasher = crate::sha256::Sha256::new();
    for (rel, state) in manifest {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(state.content_hash.as_bytes());
        hasher.update(b"\0");
    }
    hasher.hexdigest()
}

// ---------------------------------------------------------------------------
// Marker file — the fail-closed schema gate beside the store.
// ---------------------------------------------------------------------------

fn marker_path(cache_dir: &Path, corpus_hash: &str) -> PathBuf {
    cache_dir.join(format!("{corpus_hash}.json"))
}

fn marker_valid(cache_dir: &Path, corpus_hash: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(marker_path(cache_dir, corpus_hash)) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    value
        .as_object()
        .and_then(|obj| obj.get("schema_version"))
        .and_then(|v| v.as_str())
        == Some(SCHEMA_VERSION)
}

/// The tracker's compaction gate (INDEX-PLAN B6): write the marker for an
/// already-landed store.
pub fn write_marker_public(cache_dir: &Path, corpus_hash: &str) -> bool {
    write_marker(cache_dir, corpus_hash, true)
}

fn write_marker(cache_dir: &Path, corpus_hash: &str, store_written: bool) -> bool {
    if !store_written {
        return false;
    }
    if std::fs::create_dir_all(cache_dir).is_err() {
        return false;
    }
    // json.dumps default separators over an insertion-ordered dict.
    let payload =
        format!("{{\"schema_version\": \"{SCHEMA_VERSION}\", \"corpus_hash\": \"{corpus_hash}\"}}");
    let tmp = cache_dir.join(format!(
        ".{corpus_hash}.{}.tmp",
        std::process::id()
    ));
    if std::fs::write(&tmp, payload).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    if std::fs::rename(&tmp, marker_path(cache_dir, corpus_hash)).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// load_or_build — the whole cache surface.
// ---------------------------------------------------------------------------

/// What `load_or_build` returns: a memory-mapped store view (the warm path),
/// or the freshly built structures when the store could not be written or
/// reopened (ADR-080 — never a failure).
pub enum ReadModel {
    View(MmapIndexReader),
    Fresh(Box<DerivedIndex>),
}

impl ReadModel {
    /// Exact point resolution through the central identity projection. The
    /// mapped and fresh arms therefore preserve qualified parent aliases and
    /// canonical override redirects identically.
    pub fn resolve(&self, reference: &str) -> crate::resolve::ResolutionResult {
        match self {
            Self::View(reader) => crate::read_model::store_resolve(reader, reference),
            Self::Fresh(derived) => {
                crate::resolve::resolve_in_index(&derived.resolution.entries, reference)
            }
        }
    }

    /// Catalog identity rows matching one stable `(source, canonical_id)`.
    pub fn identity_entries_for_key(
        &self,
        key: &crate::corpus::ArtifactKey,
    ) -> Result<Vec<crate::resolve::IndexEntry>, crate::index_format::IndexFormatError> {
        match self {
            Self::View(reader) => reader
                .docids_for_key(key)?
                .into_iter()
                .map(|docid| reader.identity_entry(docid))
                .collect(),
            Self::Fresh(derived) => Ok(derived
                .resolution
                .entries
                .iter()
                .filter(|entry| entry.key.as_ref() == Some(key))
                .cloned()
                .collect()),
        }
    }

    pub fn canonical_redirect(
        &self,
        parent: &crate::corpus::ArtifactKey,
    ) -> Result<Option<crate::derived::CanonicalRedirect>, crate::index_format::IndexFormatError>
    {
        match self {
            Self::View(reader) => reader.canonical_redirect(parent),
            Self::Fresh(derived) => Ok(derived
                .resolution
                .canonical_redirects
                .iter()
                .find(|redirect| &redirect.parent == parent)
                .cloned()),
        }
    }
}

pub struct DerivedIndexCache {
    pub cache_dir: PathBuf,
}

impl Default for DerivedIndexCache {
    fn default() -> Self {
        Self {
            cache_dir: default_cache_dir(),
        }
    }
}

impl DerivedIndexCache {
    pub fn load_or_build(&self, directory: &str, recursive: bool, verify: bool) -> ReadModel {
        // Freshness: the key is recomputed every call through the persisted
        // stat manifest (ADR-112); `verify` or a missing manifest forces the
        // content-confirm-all floor, and the rewrite self-heals either way.
        let root_key = manifest_root_key(directory, recursive);
        let prev = if verify {
            None
        } else {
            open_freshness_manifest(&self.cache_dir, &root_key)
        };
        let manifest_missing = prev.is_none();
        let confirm_all = verify || manifest_missing;
        let prev_manifest = prev.unwrap_or_default();
        let scan_started = crate::timing::start();
        let (manifest, changed) = stat_scan(directory, &prev_manifest, confirm_all, recursive);
        crate::timing::emit_since(
            "cache.discovery_stat",
            scan_started,
            &[
                ("files", manifest.len() as u64),
                ("changed", changed.len() as u64),
            ],
        );
        let hash_started = crate::timing::start();
        let corpus_hash = corpus_hash_from_complete_manifest(&manifest);
        crate::timing::emit_since(
            "cache.corpus_hash",
            hash_started,
            &[("files", manifest.len() as u64)],
        );
        // Best-effort persistence: the manifest is a latency structure only.
        let manifest_started = crate::timing::start();
        let manifest_dirty = manifest_missing || manifest != prev_manifest;
        let manifest_written =
            !manifest_dirty || write_freshness_manifest(&self.cache_dir, &root_key, &manifest);
        crate::timing::emit_since(
            "cache.manifest_write",
            manifest_started,
            &[
                ("files", manifest.len() as u64),
                ("dirty", u64::from(manifest_dirty)),
                ("success", u64::from(manifest_written)),
            ],
        );
        if marker_valid(&self.cache_dir, &corpus_hash) {
            let open_started = crate::timing::start();
            if let Some(view) = open_store(&self.cache_dir, &corpus_hash, SCHEMA_VERSION) {
                crate::timing::emit_since(
                    "cache.store_open",
                    open_started,
                    &[("hit", 1), ("documents", u64::from(view.doc_count))],
                );
                return ReadModel::View(view);
            }
            crate::timing::emit_since("cache.store_open", open_started, &[("hit", 0)]);
            // Marker claimed a store but it is unusable: clear it so the
            // rebuild below writes fresh rather than skipping the dead dir.
            remove_store(&self.cache_dir, &corpus_hash);
        }
        // Cold miss: build the store from nothing with the parallel fragment
        // fan-out (ADR-107/108) — byte-identical to the serial build, only
        // faster to produce; the DECIDED_TIMING scorecard line rides here.
        let build_started = crate::timing::start();
        let (derived, mut stats) =
            crate::parallel_build::build_derived_index_parallel(directory, recursive, None);
        crate::timing::emit_since(
            "cache.cold_build",
            build_started,
            &[("documents", derived.index_entries.len() as u64)],
        );
        let write_start = std::time::Instant::now();
        let store_write_started = crate::timing::start();
        let store_written = write_store(&self.cache_dir, &corpus_hash, SCHEMA_VERSION, &derived);
        crate::timing::emit_since(
            "cache.store_write",
            store_write_started,
            &[("written", u64::from(store_written))],
        );
        stats.write_ms = write_start.elapsed().as_secs_f64() * 1000.0;
        crate::parallel_build::emit_build_timing(&stats);
        if write_marker(&self.cache_dir, &corpus_hash, store_written) {
            if let Some(view) = open_store(&self.cache_dir, &corpus_hash, SCHEMA_VERSION) {
                return ReadModel::View(view);
            }
        }
        ReadModel::Fresh(Box::new(derived))
    }

    /// Whether a store directory currently exists for `corpus_hash`.
    pub fn store_present(&self, corpus_hash: &str) -> bool {
        store_dir(&self.cache_dir, corpus_hash).is_dir()
    }
}

// ---------------------------------------------------------------------------
// Federated logical generations and request-boundary freshness (ADR-143).
// ---------------------------------------------------------------------------

/// Domain for the logical composed-generation key. This is independent from
/// the parent pin domain: it identifies all child + declaration + verified
/// parent inputs which can alter the effective read model.
pub const FEDERATED_GENERATION_DOMAIN: &[u8] = b"asdecided-federated-generation-v1\0";

fn generation_frame(hasher: &mut crate::sha256::Sha256, tag: u8, bytes: &[u8]) {
    hasher.update(&[tag]);
    hasher.update(&(bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn legacy_child_corpus_hash(
    directory: &str,
    recursive: bool,
) -> String {
    let mut hasher = crate::sha256::Sha256::new();
    for entry in find_markdown_files(directory, recursive) {
        let rel = entry.components.join("/");
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(crate::index_store::content_hash(&entry.abs).as_bytes());
        hasher.update(b"\0");
    }
    hasher.hexdigest()
}

fn child_snapshot_hash(files: &[crate::federation::SnapshotFile]) -> String {
    let mut hasher = crate::sha256::Sha256::new();
    for file in files {
        hasher.update(file.relative_path.as_bytes());
        hasher.update(b"\0");
        hasher.update(crate::sha256::hexdigest(&file.bytes).as_bytes());
        hasher.update(b"\0");
    }
    hasher.hexdigest()
}

fn stable_child_corpus_path(
    repository_root: &Path,
    corpus_root: &Path,
) -> Result<String, FederatedCacheError> {
    let repository_root = std::fs::canonicalize(repository_root).map_err(|error| {
        FederatedCacheError::ChildSnapshot {
            logical_path: "child repository".to_string(),
            message: error.to_string(),
        }
    })?;
    let corpus_root = std::fs::canonicalize(corpus_root).map_err(|error| {
        FederatedCacheError::ChildSnapshot {
            logical_path: "child corpus".to_string(),
            message: error.to_string(),
        }
    })?;
    let relative = corpus_root.strip_prefix(&repository_root).map_err(|_| {
        FederatedCacheError::ChildSnapshot {
            logical_path: "child corpus".to_string(),
            message: "child corpus is outside the child repository".to_string(),
        }
    })?;
    let path = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    Ok(if path.is_empty() { ".".to_string() } else { path })
}

fn capture_child_snapshot(
    directory: &str,
    recursive: bool,
    parent: &crate::federation::VerifiedParent,
) -> Result<Vec<crate::federation::SnapshotFile>, FederatedCacheError> {
    find_markdown_files(directory, recursive)
        .into_iter()
        .filter(|entry| !parent.contains_materialised_path(&entry.abs))
        .map(|entry| {
            let bytes = std::fs::read(&entry.abs).map_err(|error| {
                FederatedCacheError::ChildSnapshot {
                    logical_path: entry.rel(),
                    message: error.to_string(),
                }
            })?;
            Ok(crate::federation::SnapshotFile {
                relative_path: entry.rel(),
                absolute_path: entry.abs,
                bytes,
            })
        })
        .collect()
}

/// Stable, typed failures at the verified generation/build boundary.
#[derive(Debug)]
pub enum FederatedCacheError {
    Parent(crate::federation::ParentCorpusError),
    ChildSnapshot {
        logical_path: String,
        message: String,
    },
    Composition { code: String, message: String },
    InvalidModel { message: String },
}

impl FederatedCacheError {
    pub fn stable_code(&self) -> &str {
        match self {
            Self::Parent(error) => error.stable_code(),
            Self::ChildSnapshot { .. } => "federated-child-snapshot-failed",
            Self::Composition { code, .. } => code,
            Self::InvalidModel { .. } => "federated-cache-invalid-model",
        }
    }

    /// Adapt the central composition layer's stable finding into the cache
    /// boundary without erasing its code.
    pub fn composition(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Composition {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl From<crate::federation::ParentCorpusError> for FederatedCacheError {
    fn from(error: crate::federation::ParentCorpusError) -> Self {
        Self::Parent(error)
    }
}

impl fmt::Display for FederatedCacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parent(error) => {
                let mut message = error.message.clone();
                if let Some(path) = &error.path {
                    let logical_path = if path.ends_with(crate::federation::MANIFEST_RELATIVE_PATH)
                    {
                        crate::federation::MANIFEST_RELATIVE_PATH
                    } else if path.ends_with(crate::federation::CONFIG_RELATIVE_PATH) {
                        crate::federation::CONFIG_RELATIVE_PATH
                    } else {
                        "federation input"
                    };
                    message = message.replace(&path.display().to_string(), logical_path);
                }
                write!(formatter, "{}: {message}", error.stable_code())
            }
            Self::ChildSnapshot {
                logical_path,
                message,
                ..
            } => {
                write!(formatter, "{}: {logical_path}: {message}", self.stable_code())
            }
            Self::Composition { code, message } => write!(formatter, "{code}: {message}"),
            Self::InvalidModel { message } => {
                write!(formatter, "federated-cache-invalid-model: {message}")
            }
        }
    }
}

impl std::error::Error for FederatedCacheError {}

/// Explicit inputs and stable identities for one verified composed
/// generation. Filesystem locations are observability inputs only and do not
/// enter `cache_key` except through their exact bytes or relative identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedGenerationIdentity {
    pub cache_key: String,
    pub child_corpus_hash: String,
    /// Child-repository-relative corpus root; never a checkout path.
    pub child_corpus_path: String,
    pub recursive: bool,
    pub layers: Vec<crate::corpus::CorpusLayer>,
    pub watched_roots: Vec<PathBuf>,
    pub watched_files: Vec<PathBuf>,
}

/// A logical cache generation. The legacy variant deliberately preserves the
/// released single-corpus key when `.decided/corpus.md` is absent.
#[derive(Debug, Clone, PartialEq)]
pub enum LogicalGeneration {
    Legacy { corpus_hash: String },
    Federated {
        identity: FederatedGenerationIdentity,
        parent: Box<crate::federation::VerifiedParent>,
        child_files: Vec<crate::federation::SnapshotFile>,
    },
}

impl LogicalGeneration {
    pub fn cache_key(&self) -> &str {
        match self {
            Self::Legacy { corpus_hash } => corpus_hash,
            Self::Federated { identity, .. } => &identity.cache_key,
        }
    }

    pub fn verified_parent(&self) -> Option<&crate::federation::VerifiedParent> {
        match self {
            Self::Legacy { .. } => None,
            Self::Federated { parent, .. } => Some(parent),
        }
    }

    pub fn identity(&self) -> Option<&FederatedGenerationIdentity> {
        match self {
            Self::Legacy { .. } => None,
            Self::Federated { identity, .. } => Some(identity),
        }
    }

    /// Exact local bytes which produced this federated generation key. The
    /// central composer must consume these rows rather than walking again.
    pub fn child_files(&self) -> Option<&[crate::federation::SnapshotFile]> {
        match self {
            Self::Legacy { .. } => None,
            Self::Federated { child_files, .. } => Some(child_files),
        }
    }
}

/// Verify the optional parent before constructing a cache key. A federated
/// key cannot be obtained from declaration bytes alone: the loader must first
/// prove that the materialised parent still matches its source and pin.
pub fn capture_logical_generation(
    child_repository_root: impl AsRef<Path>,
    child_corpus: &str,
    recursive: bool,
) -> Result<LogicalGeneration, FederatedCacheError> {
    let verified = crate::federation::verify_parent(child_repository_root)?;
    let Some(parent) = verified else {
        return Ok(LogicalGeneration::Legacy {
            corpus_hash: legacy_child_corpus_hash(child_corpus, recursive),
        });
    };

    capture_federated_generation(parent, child_corpus, recursive)
}

fn capture_federated_generation(
    parent: crate::federation::VerifiedParent,
    child_corpus: &str,
    recursive: bool,
) -> Result<LogicalGeneration, FederatedCacheError> {
    let child_files = capture_child_snapshot(child_corpus, recursive, &parent)?;
    let child_hash = child_snapshot_hash(&child_files);
    let child_corpus_path = stable_child_corpus_path(
        &parent.child_repository_root,
        Path::new(child_corpus),
    )?;

    let local_layer = crate::corpus::CorpusLayer::local(parent.child_source.clone());
    let inherited_layer = crate::corpus::CorpusLayer::inherited(
        parent.declaration.source.clone(),
        parent.declaration.alias.clone(),
        parent.digest.clone(),
    );
    let override_payload = match &parent.override_mapping_bytes {
        Some(bytes) => {
            let mut payload = Vec::with_capacity(bytes.len() + 1);
            payload.push(1);
            payload.extend_from_slice(bytes);
            payload
        }
        None => vec![0],
    };
    let mut hasher = crate::sha256::Sha256::new();
    hasher.update(FEDERATED_GENERATION_DOMAIN);
    generation_frame(&mut hasher, 0x01, child_hash.as_bytes());
    generation_frame(&mut hasher, 0x02, parent.child_source.as_bytes());
    generation_frame(&mut hasher, 0x03, &parent.child_config_bytes);
    generation_frame(&mut hasher, 0x04, &parent.manifest_bytes);
    generation_frame(&mut hasher, 0x05, parent.declaration.source.as_bytes());
    generation_frame(&mut hasher, 0x06, parent.digest.as_bytes());
    generation_frame(&mut hasher, 0x07, &parent.config_bytes);
    generation_frame(&mut hasher, 0x08, parent.declaration.alias.as_bytes());
    generation_frame(&mut hasher, 0x09, &override_payload);
    generation_frame(&mut hasher, 0x0a, local_layer.layer.as_str().as_bytes());
    generation_frame(
        &mut hasher,
        0x0b,
        inherited_layer.layer.as_str().as_bytes(),
    );
    generation_frame(&mut hasher, 0x0c, &[u8::from(recursive)]);
    generation_frame(&mut hasher, 0x0d, child_corpus_path.as_bytes());
    let cache_key = hasher.hexdigest();

    let mut watched_files = Vec::with_capacity(parent.files.len() + child_files.len() + 3);
    watched_files.push(parent.child_config_path.clone());
    watched_files.push(parent.manifest_path.clone());
    watched_files.push(parent.config_path.clone());
    watched_files.extend(child_files.iter().map(|file| file.absolute_path.clone()));
    watched_files.extend(parent.files.iter().map(|file| file.absolute_path.clone()));
    let watched_roots = vec![PathBuf::from(child_corpus), parent.corpus_root.clone()];

    Ok(LogicalGeneration::Federated {
        identity: FederatedGenerationIdentity {
            cache_key,
            child_corpus_hash: child_hash,
            child_corpus_path,
            recursive,
            layers: vec![local_layer, inherited_layer],
            watched_roots,
            watched_files,
        },
        parent: Box::new(parent),
        child_files,
    })
}

/// Why a request produced the returned model. A relevant input change is
/// always a complete recomposition; delta mutation is intentionally not used
/// across a federation boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FederatedCacheRefresh {
    WarmReuse,
    StoreHit,
    Recomposed,
}

/// One central composition plus its persistable projection. A cache miss may
/// persist `model`; every first read (including a store hit) retains
/// `composed` so downstream consumers never reconstruct an overlay.
pub struct FederatedCacheBuild<C> {
    pub composed: C,
    pub model: DerivedIndex,
}

impl<C> FederatedCacheBuild<C> {
    pub fn new(composed: C, model: DerivedIndex) -> Self {
        Self { composed, model }
    }
}

/// One successfully verified read session. The model and exact logical inputs
/// are borrowed together, preventing callers from serving a retained model
/// after a later parent-verification failure.
pub struct FederatedCacheRead<'a, C = ()> {
    pub refresh: FederatedCacheRefresh,
    pub model: &'a ReadModel,
    pub generation: &'a LogicalGeneration,
    /// The authoritative central composition built from this generation's
    /// captured bytes. MCP and other consumers borrow this directly.
    pub composed: &'a C,
}

impl<C> FederatedCacheRead<'_, C> {
    /// Captured inherited bytes addressed by stable source-relative path.
    pub fn inherited_bytes(&self, path: &crate::corpus::ArtifactPath) -> Option<&[u8]> {
        self.generation
            .verified_parent()?
            .artifact_bytes(path)
    }

    /// Existing `get_artifact` decoding over captured bytes, without a second
    /// filesystem read through the public source-relative `path` field.
    pub fn inherited_text(&self, path: &crate::corpus::ArtifactPath) -> Option<String> {
        self.generation.verified_parent()?.artifact_text(path)
    }

    pub fn override_mapping(&self) -> Option<&serde_yaml::Value> {
        self.generation.verified_parent()?.overrides.as_ref()
    }
}

/// Server-lifetime cache gate for a composed corpus. Every request captures a
/// fresh verified generation before it can return the resident model. A
/// verification error leaves the old model retained but inaccessible: callers
/// receive the error and therefore cannot serve a stale formerly-valid parent.
pub struct FederatedCacheTracker<C = ()> {
    cache: DerivedIndexCache,
    current_key: Option<String>,
    model: Option<ReadModel>,
    generation: Option<LogicalGeneration>,
    composed: Option<C>,
}

impl<C> FederatedCacheTracker<C> {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache: DerivedIndexCache { cache_dir },
            current_key: None,
            model: None,
            generation: None,
            composed: None,
        }
    }

    fn open_generation(&self, cache_key: &str) -> Option<ReadModel> {
        if !marker_valid(&self.cache.cache_dir, cache_key) {
            return None;
        }
        match open_store(&self.cache.cache_dir, cache_key, SCHEMA_VERSION) {
            Some(view) => Some(ReadModel::View(view)),
            None => {
                remove_store(&self.cache.cache_dir, cache_key);
                None
            }
        }
    }

    fn persist(&self, cache_key: &str, derived: DerivedIndex) -> ReadModel {
        let written = write_store(&self.cache.cache_dir, cache_key, SCHEMA_VERSION, &derived);
        if write_marker(&self.cache.cache_dir, cache_key, written) {
            if let Some(view) = open_store(&self.cache.cache_dir, cache_key, SCHEMA_VERSION) {
                return ReadModel::View(view);
            }
        }
        ReadModel::Fresh(Box::new(derived))
    }

    pub(crate) fn read_or_recompose<F>(
        &mut self,
        child_repository_root: impl AsRef<Path>,
        child_corpus: &str,
        recursive: bool,
        build: F,
    ) -> Result<FederatedCacheRead<'_, C>, FederatedCacheError>
    where
        F: FnOnce(&LogicalGeneration) -> Result<FederatedCacheBuild<C>, FederatedCacheError>,
    {
        // Reverification and exact input capture happen before either the
        // resident-model fast path or a persistent-store lookup.
        let generation =
            capture_logical_generation(child_repository_root, child_corpus, recursive)?;
        let cache_key = generation.cache_key().to_string();
        if self.current_key.as_deref() == Some(cache_key.as_str()) {
            if self.generation.as_ref() != Some(&generation) {
                let built = build(&generation)?;
                validate_federated_model(&generation, &built.model)?;
                self.composed = Some(built.composed);
            }
            // Replace even an equal generation with the just-verified strict
            // snapshot so the returned content handle is request-current.
            self.generation = Some(generation);
            return Ok(FederatedCacheRead {
                refresh: FederatedCacheRefresh::WarmReuse,
                model: self.model.as_ref().expect("current key has a model"),
                generation: self.generation.as_ref().expect("generation installed"),
                composed: self.composed.as_ref().expect("current key has a composition"),
            });
        }

        // A process-local miss and a persistent store hit both compose once
        // from this exact verified generation. The store can replace only the
        // expensive derived projection, never the authoritative corpus.
        let built = build(&generation)?;
        validate_federated_model(&generation, &built.model)?;
        let cold = self.current_key.is_none();
        let (refresh, model) = if cold {
            match self.open_generation(&cache_key) {
                Some(model) => (FederatedCacheRefresh::StoreHit, model),
                None => (
                    FederatedCacheRefresh::Recomposed,
                    self.persist(&cache_key, built.model),
                ),
            }
        } else {
            // A changed manifest/config/corpus/pin/override is a federation
            // topology change. Rebuild the complete composed model rather
            // than applying a source-blind document delta.
            (
                FederatedCacheRefresh::Recomposed,
                self.persist(&cache_key, built.model),
            )
        };
        self.current_key = Some(cache_key);
        self.model = Some(model);
        self.generation = Some(generation);
        self.composed = Some(built.composed);
        Ok(FederatedCacheRead {
            refresh,
            model: self.model.as_ref().expect("model just installed"),
            generation: self.generation.as_ref().expect("generation just installed"),
            composed: self.composed.as_ref().expect("composition just installed"),
        })
    }

    pub fn current_key(&self) -> Option<&str> {
        self.current_key.as_deref()
    }
}

impl FederatedCacheTracker<crate::composition::ComposedCorpus> {
    /// Read the authoritative composed corpus and its persisted projection
    /// from one exact verified generation. Callers cannot supply an alternate
    /// overlay or reopen either corpus between cache-key capture and build.
    pub fn read_composed(
        &mut self,
        child_repository_root: impl AsRef<Path>,
        child_corpus: &str,
        recursive: bool,
    ) -> Result<FederatedCacheRead<'_, crate::composition::ComposedCorpus>, FederatedCacheError>
    {
        self.read_or_recompose(
            child_repository_root,
            child_corpus,
            recursive,
            |generation| {
                let Some(identity) = generation.identity() else {
                    return Err(FederatedCacheError::InvalidModel {
                        message: "composed cache reads require .decided/corpus.md".to_string(),
                    });
                };
                let parent = generation
                    .verified_parent()
                    .expect("federated identity has a verified parent");
                let child_files = generation
                    .child_files()
                    .expect("federated identity has captured child files");
                let composed = crate::federated_corpus::compose_verified_generation_from_snapshot(
                    child_corpus,
                    parent,
                    child_files,
                )
                .map_err(|error| {
                    FederatedCacheError::composition(error.stable_code(), error.to_string())
                })?;
                let model = crate::derived::build_derived_index_from_composed(
                    &identity.child_corpus_path,
                    child_corpus,
                    identity.recursive,
                    &identity.layers,
                    &parent.child_config_bytes,
                    &composed,
                );
                Ok(FederatedCacheBuild::new(composed, model))
            },
        )
    }
}

fn stable_public_path(path: &str) -> bool {
    !path.is_empty()
        && !Path::new(path).is_absolute()
        && !Path::new(path)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
}

fn validate_federated_model(
    generation: &LogicalGeneration,
    derived: &DerivedIndex,
) -> Result<(), FederatedCacheError> {
    let Some(identity) = generation.identity() else {
        return Ok(());
    };
    let mut expected_layers = identity.layers.clone();
    expected_layers.sort();
    let mut actual_layers = derived.layers.clone();
    actual_layers.sort();
    if actual_layers != expected_layers {
        return Err(FederatedCacheError::InvalidModel {
            message: "derived layers do not match the verified generation".to_string(),
        });
    }
    let expected_sources: std::collections::BTreeSet<&str> = expected_layers
        .iter()
        .map(|layer| layer.source.as_str())
        .collect();
    let expected_by_source: std::collections::BTreeMap<&str, &crate::corpus::CorpusLayer> =
        expected_layers
            .iter()
            .map(|layer| (layer.source.as_str(), layer))
            .collect();
    let invalid_path = |source: &str, relative_path: &str, display_path: &str| {
        !expected_sources.contains(source)
            || !stable_public_path(relative_path)
            || display_path != relative_path
    };
    if derived.source_artifacts.len() != derived.index_entries.len()
        || derived
            .source_artifacts
            .iter()
            .zip(&derived.index_entries)
            .any(|(artifact, entry)| {
                invalid_path(
                    &artifact.path.source,
                    &artifact.path.relative_path,
                    &artifact.display_path,
                ) || artifact.key.source != artifact.origin.source
                    || artifact.path.source != artifact.origin.source
                    || expected_by_source.get(artifact.origin.source.as_str()).copied()
                        != Some(&crate::corpus::CorpusLayer::from(&artifact.origin))
                    || entry.key.as_ref() != Some(&artifact.key)
                    || entry.artifact_path.as_ref() != Some(&artifact.path)
                    || entry.origin.as_ref() != Some(&artifact.origin)
                    || entry.path != artifact.display_path
            })
    {
        return Err(FederatedCacheError::InvalidModel {
            message: "search rows contain missing, physical, or mismatched source identity"
                .to_string(),
        });
    }
    let mut resolution_sources = std::collections::BTreeSet::new();
    for entry in &derived.resolution.entries {
        let (Some(key), Some(path), Some(origin)) =
            (&entry.key, &entry.artifact_path, &entry.origin)
        else {
            return Err(FederatedCacheError::InvalidModel {
                message: "identity projection is missing source provenance".to_string(),
            });
        };
        if invalid_path(&path.source, &path.relative_path, &entry.path)
            || key.source != origin.source
            || path.source != origin.source
            || expected_by_source.get(origin.source.as_str()).copied()
                != Some(&crate::corpus::CorpusLayer::from(origin))
        {
            return Err(FederatedCacheError::InvalidModel {
                message: "identity projection contains physical or mismatched paths".to_string(),
            });
        }
        resolution_sources.insert(key.source.as_str());
    }
    if resolution_sources != expected_sources {
        return Err(FederatedCacheError::InvalidModel {
            message: "identity projection does not retain both verified sources".to_string(),
        });
    }
    if derived.live_decision_keys.len() != derived.live_decision_paths.len()
        || derived
            .live_decision_keys
            .iter()
            .any(|key| !expected_sources.contains(key.source.as_str()))
        || derived.scope_rows.iter().any(|row| {
            row.key
                .as_ref()
                .is_none_or(|key| !expected_sources.contains(key.source.as_str()))
                || row.artifact_path.as_ref().is_none_or(|path| {
                    !expected_sources.contains(path.source.as_str())
                        || !stable_public_path(&path.relative_path)
                        || row.path != path.relative_path
                })
                || row.origin.as_ref().is_none_or(|origin| {
                    row.key.as_ref().is_none_or(|key| key.source != origin.source)
                        || row
                            .artifact_path
                            .as_ref()
                            .is_none_or(|path| path.source != origin.source)
                        || expected_by_source.get(origin.source.as_str()).copied()
                            != Some(&crate::corpus::CorpusLayer::from(origin))
                })
        })
    {
        return Err(FederatedCacheError::InvalidModel {
            message: "scope or liveness projection is not source-aware".to_string(),
        });
    }
    let identity_keys: std::collections::BTreeSet<&crate::corpus::ArtifactKey> = derived
        .resolution
        .entries
        .iter()
        .filter_map(|entry| entry.key.as_ref())
        .collect();
    if derived.resolution.canonical_redirects.iter().any(|redirect| {
        !identity_keys.contains(&redirect.parent)
            || !identity_keys.contains(&redirect.replacement)
            || !identity_keys.contains(&redirect.rationale)
    }) {
        return Err(FederatedCacheError::InvalidModel {
            message: "canonical redirect endpoints are absent from the identity projection"
                .to_string(),
        });
    }
    for relationship in &derived.relationships {
        for endpoint in [
            relationship.source_artifact.as_ref(),
            relationship.resolved_artifact.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if !expected_sources.contains(endpoint.source.as_str())
                || !stable_public_path(&endpoint.relative_path)
            {
                return Err(FederatedCacheError::InvalidModel {
                    message: "relationship projection contains an invalid endpoint".to_string(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_manifest_hash_uses_scan_order_and_content_only() {
        let manifest = vec![
            (
                "a.md".to_string(),
                FileState {
                    content_hash: "hash-a".to_string(),
                    size: 10,
                    mtime_ns: 20,
                },
            ),
            (
                "nested/b.md".to_string(),
                FileState {
                    content_hash: "hash-b".to_string(),
                    size: 30,
                    mtime_ns: 40,
                },
            ),
        ];

        assert_eq!(
            corpus_hash_from_complete_manifest(&manifest),
            crate::sha256::hexdigest(b"a.md\0hash-a\0nested/b.md\0hash-b\0")
        );

        let mut stat_only_change = manifest.clone();
        stat_only_change[0].1.size += 1;
        stat_only_change[0].1.mtime_ns += 1;
        assert_eq!(
            corpus_hash_from_complete_manifest(&manifest),
            corpus_hash_from_complete_manifest(&stat_only_change)
        );
    }
}
