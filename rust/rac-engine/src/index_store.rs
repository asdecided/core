//! Persistent memory-mapped index store (ADR-104) — port of
//! `services/index_store.py` per `rust/spec/index-store-format.md`.
//!
//! Store byte-identity is the parity surface: for the same corpus bytes this
//! writer must produce a segment directory byte-identical to the oracle's.
//! Every reader-side failure degrades to a miss (`None`), never an answer
//! change; every writer-side failure degrades to "not written" (ADR-080).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use memmap2::Mmap;
use serde_json::Value;

use crate::corpus::{ArtifactKey, ArtifactOrigin, ArtifactPath, CorpusLayer, Layer};
use crate::derived::{CanonicalRedirect, DerivedIndex, SourceAwareArtifact};
use crate::federation_generation::{GenerationMapping, GenerationRedirect, GRAPH_CONTRACT};
use crate::index_format::{
    encode_segment, segment_payload, write_indexed, IndexFormatError, IndexedSegment, Reader,
    Writer,
};
use crate::pycompat::py_casefold;
use crate::relationships::Relationship;
use crate::resolve::{FieldTokens, IndexEntry};
use crate::walk::find_markdown_files;

// The scorable field families in the exact BM25F iteration order (ADR-078).
pub const FIELDS: [&str; 6] = ["id", "title", "path", "heading", "body", "tags"];

pub const STORE_DIRNAME: &str = "store";
/// The source-aware store layout. The pre-federation `store/v1` tree is
/// deliberately never opened as v2; it therefore degrades to an ordinary
/// cache miss instead of being reinterpreted without provenance (ADR-143).
pub const STORE_LAYOUT_VERSION: &str = "v2";
/// Graph-federation state is never decoded from the single-parent v2 tree.
/// The segment schema is extended at the graph integration boundary, while
/// this distinct namespace already guarantees old segments are cache misses.
pub const GRAPH_STORE_LAYOUT_VERSION: &str = "v3";

const SEG_HEADER: &str = "header.seg";
const SEG_ENTRIES: &str = "entries.seg";
const SEG_IDENTITIES: &str = "identities.seg";
const SEG_SECTIONS: &str = "sections.seg";
const SEG_TOKENS: &str = "tokens.seg";
const SEG_TERMDICT: &str = "termdict.seg";
const SEG_POSTINGS: &str = "postings.seg";
const SEG_RELATIONSHIPS: &str = "relationships.seg";
const SEG_LIVE: &str = "live.seg";
const SEG_SCOPE: &str = "scope.seg";
const SEG_PORTFOLIO: &str = "portfolio.seg";
const SEG_ALIASMAP: &str = "aliasmap.seg";
const SEG_PATHMAP: &str = "pathmap.seg";
const SEG_KEYMAP: &str = "keymap.seg";
const SEG_ARTIFACTPATHMAP: &str = "artifactpathmap.seg";
const SEG_LAYERS: &str = "layers.seg";
const SEG_REDIRECTS: &str = "redirects.seg";
const SEG_GRAPH: &str = "graph.seg";

const ALL_SEGMENTS: [&str; 17] = [
    SEG_HEADER,
    SEG_ENTRIES,
    SEG_IDENTITIES,
    SEG_SECTIONS,
    SEG_TOKENS,
    SEG_TERMDICT,
    SEG_POSTINGS,
    SEG_RELATIONSHIPS,
    SEG_LIVE,
    SEG_SCOPE,
    SEG_PORTFOLIO,
    SEG_ALIASMAP,
    SEG_PATHMAP,
    SEG_KEYMAP,
    SEG_ARTIFACTPATHMAP,
    SEG_LAYERS,
    SEG_REDIRECTS,
];

/// The pinned scoring-constant fingerprint (spec/index-store-format.md §3.1).
/// Must track `resolve`'s BM25F constants; the golden-vector test pins the
/// exact string against the oracle's `scoring_fingerprint()`.
pub fn scoring_fingerprint() -> &'static str {
    "id=4.0|title=3.0|path=2.0|heading=1.5|body=1.0|tags=2.5|k1=1.2|b=0.75|rrf=60|graph=0.5|graph_floor=0.85"
}

// ---------------------------------------------------------------------------
// Corpus hash (spec §6)
// ---------------------------------------------------------------------------

/// SHA-256 of a file's bytes; unreadable files hash a stable sentinel.
pub fn content_hash(path: &Path) -> String {
    match fs::read(path) {
        Ok(bytes) => crate::sha256::hexdigest(&bytes),
        Err(_) => crate::sha256::hexdigest(b"\x00rac-unreadable-artifact"),
    }
}

/// `corpus_content_hash(directory, recursive)` — fold of the sorted
/// `(rel_posix, content_hash)` pairs.
pub fn corpus_content_hash(directory: &str, recursive: bool) -> String {
    let mut hasher = crate::sha256::Sha256::new();
    for entry in find_markdown_files(directory, recursive) {
        let rel = entry.components.join("/");
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(content_hash(&entry.abs).as_bytes());
        hasher.update(b"\0");
    }
    hasher.hexdigest()
}

// ---------------------------------------------------------------------------
// Store layout paths
// ---------------------------------------------------------------------------

pub fn store_root(cache_dir: &Path) -> PathBuf {
    store_root_for_layout(cache_dir, STORE_LAYOUT_VERSION)
}

pub fn store_dir(cache_dir: &Path, corpus_hash: &str) -> PathBuf {
    store_dir_for_layout(cache_dir, STORE_LAYOUT_VERSION, corpus_hash)
}

pub fn graph_store_root(cache_dir: &Path) -> PathBuf {
    store_root_for_layout(cache_dir, GRAPH_STORE_LAYOUT_VERSION)
}

pub fn graph_store_dir(cache_dir: &Path, generation: &str) -> Option<PathBuf> {
    Some(store_dir_for_layout(
        cache_dir,
        GRAPH_STORE_LAYOUT_VERSION,
        graph_store_directory_key(generation)?,
    ))
}

fn graph_store_directory_key(generation: &str) -> Option<&str> {
    valid_versioned_digest(generation, "sha256-v3:")
}

fn valid_versioned_digest<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value.strip_prefix(prefix).filter(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn store_root_for_layout(cache_dir: &Path, layout: &str) -> PathBuf {
    cache_dir.join(STORE_DIRNAME).join(layout)
}

fn store_dir_for_layout(cache_dir: &Path, layout: &str, key: &str) -> PathBuf {
    store_root_for_layout(cache_dir, layout).join(key)
}

// ---------------------------------------------------------------------------
// Writer — one DerivedIndex -> a directory of segment files, atomically.
// ---------------------------------------------------------------------------

fn write_presence(writer: &mut Writer, present: bool) -> Result<(), IndexFormatError> {
    writer.u32(u64::from(present))
}

fn read_presence(reader: &mut Reader<'_>, field: &str) -> Result<bool, IndexFormatError> {
    match reader.u32()? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(IndexFormatError(format!(
            "invalid {field} presence flag: {value}"
        ))),
    }
}

fn write_artifact_key(
    writer: &mut Writer,
    key: Option<&ArtifactKey>,
) -> Result<(), IndexFormatError> {
    write_presence(writer, key.is_some())?;
    if let Some(key) = key {
        writer.text(&key.source)?;
        writer.text(&key.canonical_id)?;
    }
    Ok(())
}

fn read_artifact_key(reader: &mut Reader<'_>) -> Result<Option<ArtifactKey>, IndexFormatError> {
    if !read_presence(reader, "artifact-key")? {
        return Ok(None);
    }
    Ok(Some(ArtifactKey::new(reader.text()?, reader.text()?)))
}

fn write_artifact_path(
    writer: &mut Writer,
    path: Option<&ArtifactPath>,
) -> Result<(), IndexFormatError> {
    write_presence(writer, path.is_some())?;
    if let Some(path) = path {
        writer.text(&path.source)?;
        writer.text(&path.relative_path)?;
    }
    Ok(())
}

fn read_artifact_path(reader: &mut Reader<'_>) -> Result<Option<ArtifactPath>, IndexFormatError> {
    if !read_presence(reader, "artifact-path")? {
        return Ok(None);
    }
    Ok(Some(ArtifactPath::new(reader.text()?, reader.text()?)))
}

fn write_origin(
    writer: &mut Writer,
    origin: Option<&ArtifactOrigin>,
) -> Result<(), IndexFormatError> {
    write_presence(writer, origin.is_some())?;
    if let Some(origin) = origin {
        writer.text(&origin.source)?;
        writer.text(origin.layer.as_str())?;
        writer.opt_text(origin.pin.as_deref())?;
        writer.opt_text(origin.alias.as_deref())?;
    }
    Ok(())
}

fn read_origin(reader: &mut Reader<'_>) -> Result<Option<ArtifactOrigin>, IndexFormatError> {
    if !read_presence(reader, "artifact-origin")? {
        return Ok(None);
    }
    let source = reader.text()?;
    let layer = match reader.text()?.as_str() {
        "local" => Layer::Local,
        "inherited" => Layer::Inherited,
        other => return Err(IndexFormatError(format!("invalid corpus layer: {other}"))),
    };
    Ok(Some(ArtifactOrigin {
        source,
        layer,
        pin: reader.opt_text()?,
        alias: reader.opt_text()?,
    }))
}

fn write_layer(writer: &mut Writer, layer: &CorpusLayer) -> Result<(), IndexFormatError> {
    writer.text(&layer.source)?;
    writer.text(layer.layer.as_str())?;
    writer.opt_text(layer.pin.as_deref())?;
    writer.opt_text(layer.alias.as_deref())?;
    Ok(())
}

fn read_layer(reader: &mut Reader<'_>) -> Result<CorpusLayer, IndexFormatError> {
    let source = reader.text()?;
    let layer = match reader.text()?.as_str() {
        "local" => Layer::Local,
        "inherited" => Layer::Inherited,
        other => return Err(IndexFormatError(format!("invalid corpus layer: {other}"))),
    };
    Ok(CorpusLayer {
        source,
        layer,
        pin: reader.opt_text()?,
        alias: reader.opt_text()?,
    })
}

fn write_identity_entry(
    writer: &mut Writer,
    entry: &IndexEntry,
) -> Result<(), IndexFormatError> {
    writer.text(&entry.id)?;
    writer.text(&entry.artifact_type)?;
    writer.opt_text(entry.title.as_deref())?;
    writer.text(&entry.path)?;
    writer.text_list(&entry.aliases)?;
    writer.text_list(&entry.tags)?;
    write_artifact_key(writer, entry.key.as_ref())?;
    write_artifact_path(writer, entry.artifact_path.as_ref())?;
    write_origin(writer, entry.origin.as_ref())?;
    Ok(())
}

fn read_identity_entry(reader: &mut Reader<'_>) -> Result<IndexEntry, IndexFormatError> {
    Ok(IndexEntry {
        id: reader.text()?,
        artifact_type: reader.text()?,
        title: reader.opt_text()?,
        path: reader.text()?,
        aliases: reader.text_list()?,
        tags: reader.text_list()?,
        key: read_artifact_key(reader)?,
        artifact_path: read_artifact_path(reader)?,
        origin: read_origin(reader)?,
        search_sections: Vec::new(),
        inbound_count: 0,
    })
}

fn encode_segments(
    corpus_hash: &str,
    bundle_version: &str,
    derived: &DerivedIndex,
) -> Result<Vec<(&'static str, Vec<u8>)>, IndexFormatError> {
    let entries = &derived.index_entries;
    let field_tokens = &derived.field_tokens;
    if derived.source_artifacts.len() != entries.len() {
        return Err(IndexFormatError(
            "source-aware artifact rows are not parallel to index entries".into(),
        ));
    }
    for entry in &derived.resolution.entries {
        if entry.key.is_none() || entry.artifact_path.is_none() || entry.origin.is_none() {
            return Err(IndexFormatError(
                "v2 identity rows require key, artifact path, and origin".into(),
            ));
        }
    }
    if derived.live_decision_keys.len() != derived.live_decision_paths.len() {
        return Err(IndexFormatError(
            "live decision keys are not parallel to display paths".into(),
        ));
    }

    // Global vocabulary -> sorted term dictionary -> term id (code-point
    // order — BTreeMap keys iterate sorted).
    let mut term_id: BTreeMap<&str, u32> = BTreeMap::new();
    for fields in field_tokens {
        for name in FIELDS {
            for token in fields.get(name) {
                term_id.insert(token.as_str(), 0);
            }
        }
    }
    let termdict: Vec<&str> = term_id.keys().copied().collect();
    for (i, term) in termdict.iter().enumerate() {
        *term_id.get_mut(*term).expect("present") = i as u32;
    }

    let mut length_sums = [0u64; 6];
    let mut entry_rows: Vec<Vec<u8>> = Vec::with_capacity(entries.len());
    let mut section_rows: Vec<Vec<u8>> = Vec::with_capacity(entries.len());
    let mut token_rows: Vec<Vec<u8>> = Vec::with_capacity(entries.len());
    let mut postings_lists: Vec<Vec<u32>> = vec![Vec::new(); termdict.len()];
    for (docid, entry) in entries.iter().enumerate() {
        let source_artifact = &derived.source_artifacts[docid];
        if source_artifact.display_path != entry.path {
            return Err(IndexFormatError(
                "source-aware display path does not match index entry".into(),
            ));
        }
        let docid = docid as u32;
        let fields = &field_tokens[docid as usize];
        let lengths: Vec<u64> = FIELDS
            .iter()
            .map(|name| fields.get(name).len() as u64)
            .collect();
        for (i, value) in lengths.iter().enumerate() {
            length_sums[i] += value;
        }

        let mut row = Writer::new();
        row.text(&entry.id)?;
        row.text(&entry.artifact_type)?;
        row.opt_text(entry.title.as_deref())?;
        row.text(&entry.path)?;
        row.text_list(&entry.aliases)?;
        row.text_list(&entry.tags)?;
        row.u32(entry.inbound_count.max(0) as u64)?;
        for value in &lengths {
            row.u32(*value)?;
        }
        write_artifact_key(&mut row, Some(&source_artifact.key))?;
        write_artifact_path(&mut row, Some(&source_artifact.path))?;
        write_origin(&mut row, Some(&source_artifact.origin))?;
        entry_rows.push(row.payload());

        let mut sec = Writer::new();
        sec.u32(entry.search_sections.len() as u64)?;
        for section in &entry.search_sections {
            sec.text(&section.heading)?;
            sec.text_list(&section.lines)?;
        }
        section_rows.push(sec.payload());

        let mut doc_term_ids: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        let mut tok = Writer::new();
        for name in FIELDS {
            let ids: Vec<u32> = fields
                .get(name)
                .iter()
                .map(|token| term_id[token.as_str()])
                .collect();
            tok.u32_list(&ids)?;
            doc_term_ids.extend(ids);
        }
        token_rows.push(tok.payload());
        for tid in doc_term_ids {
            postings_lists[tid as usize].push(docid);
        }
    }

    let n_entries = entries.len() as u64;
    let n_terms = termdict.len() as u64;

    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::with_capacity(17);
    out.push((SEG_ENTRIES, encode_segment(&write_indexed(&entry_rows)?)));
    drop(entry_rows);
    let identity_rows: Vec<Vec<u8>> = derived
        .resolution
        .entries
        .iter()
        .map(|entry| {
            let mut row = Writer::new();
            write_identity_entry(&mut row, entry)?;
            Ok(row.payload())
        })
        .collect::<Result<_, IndexFormatError>>()?;
    out.push((
        SEG_IDENTITIES,
        encode_segment(&write_indexed(&identity_rows)?),
    ));
    drop(identity_rows);
    out.push((SEG_SECTIONS, encode_segment(&write_indexed(&section_rows)?)));
    drop(section_rows);
    out.push((SEG_TOKENS, encode_segment(&write_indexed(&token_rows)?)));
    drop(token_rows);

    let postings_rows: Vec<Vec<u8>> = postings_lists
        .iter()
        .map(|docids| {
            let mut w = Writer::new();
            w.u32_list(docids)?;
            Ok(w.payload())
        })
        .collect::<Result<_, IndexFormatError>>()?;
    drop(postings_lists);
    out.push((SEG_POSTINGS, encode_segment(&write_indexed(&postings_rows)?)));
    drop(postings_rows);

    let termdict_rows: Vec<Vec<u8>> = termdict
        .iter()
        .map(|term| {
            let mut w = Writer::new();
            w.text(term)?;
            Ok(w.payload())
        })
        .collect::<Result<_, IndexFormatError>>()?;
    out.push((SEG_TERMDICT, encode_segment(&write_indexed(&termdict_rows)?)));
    drop(termdict_rows);

    // The central identity projection, not the searchable effective rows,
    // owns exact point resolution. In federation this retains qualified
    // parent aliases and canonical override redirects.
    let mut alias_docids: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (docid, entry) in derived.resolution.entries.iter().enumerate() {
        let docid = docid as u32;
        for alias in &entry.aliases {
            let docids = alias_docids.entry(py_casefold(alias)).or_default();
            if docids.last() != Some(&docid) {
                docids.push(docid);
            }
        }
    }
    let aliasmap_rows: Vec<Vec<u8>> = alias_docids
        .iter()
        .map(|(key, docids)| {
            let mut w = Writer::new();
            w.text(key)?;
            w.u32_list(docids)?;
            Ok(w.payload())
        })
        .collect::<Result<_, IndexFormatError>>()?;
    drop(alias_docids);
    out.push((SEG_ALIASMAP, encode_segment(&write_indexed(&aliasmap_rows)?)));
    drop(aliasmap_rows);

    // Path map: rows sorted by path STRING (docids index walk order).
    let mut path_pairs: Vec<(&str, u32)> = entries
        .iter()
        .enumerate()
        .map(|(docid, entry)| (entry.path.as_str(), docid as u32))
        .collect();
    path_pairs.sort();
    let pathmap_rows: Vec<Vec<u8>> = path_pairs
        .iter()
        .map(|(path, docid)| {
            let mut w = Writer::new();
            w.text(path)?;
            w.u32(u64::from(*docid))?;
            Ok(w.payload())
        })
        .collect::<Result<_, IndexFormatError>>()?;
    out.push((SEG_PATHMAP, encode_segment(&write_indexed(&pathmap_rows)?)));
    drop(pathmap_rows);

    let mut key_docids: BTreeMap<&ArtifactKey, Vec<u32>> = BTreeMap::new();
    for (docid, entry) in derived.resolution.entries.iter().enumerate() {
        key_docids
            .entry(entry.key.as_ref().expect("identity key checked"))
            .or_default()
            .push(docid as u32);
    }
    let keymap_rows: Vec<Vec<u8>> = key_docids
        .iter()
        .map(|(key, docids)| {
            let mut w = Writer::new();
            w.text(&key.source)?;
            w.text(&key.canonical_id)?;
            w.u32_list(docids)?;
            Ok(w.payload())
        })
        .collect::<Result<_, IndexFormatError>>()?;
    out.push((SEG_KEYMAP, encode_segment(&write_indexed(&keymap_rows)?)));

    let mut artifact_path_pairs: Vec<(&ArtifactPath, u32)> = derived
        .resolution
        .entries
        .iter()
        .enumerate()
        .map(|(docid, entry)| {
            (
                entry.artifact_path.as_ref().expect("identity path checked"),
                docid as u32,
            )
        })
        .collect();
    artifact_path_pairs.sort_by(|(left, _), (right, _)| left.cmp(right));
    let artifactpathmap_rows: Vec<Vec<u8>> = artifact_path_pairs
        .iter()
        .map(|(path, docid)| {
            let mut w = Writer::new();
            w.text(&path.source)?;
            w.text(&path.relative_path)?;
            w.u32(u64::from(*docid))?;
            Ok(w.payload())
        })
        .collect::<Result<_, IndexFormatError>>()?;
    out.push((
        SEG_ARTIFACTPATHMAP,
        encode_segment(&write_indexed(&artifactpathmap_rows)?),
    ));

    let mut layers = Writer::new();
    layers.u32(derived.layers.len() as u64)?;
    for layer in &derived.layers {
        write_layer(&mut layers, layer)?;
    }
    out.push((SEG_LAYERS, encode_segment(&layers.payload())));

    let mut redirects = Writer::new();
    redirects.u32(derived.resolution.canonical_redirects.len() as u64)?;
    for redirect in &derived.resolution.canonical_redirects {
        write_artifact_key(&mut redirects, Some(&redirect.parent))?;
        write_artifact_key(&mut redirects, Some(&redirect.replacement))?;
        write_artifact_key(&mut redirects, Some(&redirect.rationale))?;
    }
    out.push((SEG_REDIRECTS, encode_segment(&redirects.payload())));

    let mut relationships = Writer::new();
    relationships.u32(derived.relationships.len() as u64)?;
    for rel in &derived.relationships {
        relationships.text(&rel.source_path)?;
        relationships.text(&rel.relationship)?;
        relationships.text(&rel.target)?;
        relationships.opt_text(rel.resolved_path.as_deref())?;
        relationships.opt_text(rel.issue.as_deref())?;
        write_artifact_path(&mut relationships, rel.source_artifact.as_ref())?;
        write_artifact_path(&mut relationships, rel.resolved_artifact.as_ref())?;
    }
    out.push((SEG_RELATIONSHIPS, encode_segment(&relationships.payload())));

    let mut live = Writer::new();
    live.u32(derived.live_decision_keys.len() as u64)?;
    for (key, path) in derived
        .live_decision_keys
        .iter()
        .zip(&derived.live_decision_paths)
    {
        write_artifact_key(&mut live, Some(key))?;
        live.text(path)?;
    }
    out.push((SEG_LIVE, encode_segment(&live.payload())));

    let mut scope = Writer::new();
    scope.u32(derived.scope_rows.len() as u64)?;
    for row in &derived.scope_rows {
        write_artifact_key(&mut scope, row.key.as_ref())?;
        write_artifact_path(&mut scope, row.artifact_path.as_ref())?;
        write_origin(&mut scope, row.origin.as_ref())?;
        scope.text(&row.id)?;
        scope.text(&row.title)?;
        scope.text(&row.status)?;
        scope.text(&row.path)?;
        scope.text_list(&row.scope_entries)?;
    }
    out.push((SEG_SCOPE, encode_segment(&scope.payload())));

    // The one JSON-in-binary blob: `json.dumps(summary, ensure_ascii=False)`.
    let mut portfolio = Writer::new();
    portfolio.text(&crate::pyjson::dumps_compact(&derived.portfolio_summary))?;
    out.push((SEG_PORTFOLIO, encode_segment(&portfolio.payload())));

    let mut header = Writer::new();
    header.text(corpus_hash)?;
    header.text(bundle_version)?;
    header.text(scoring_fingerprint())?;
    header.u32(n_entries)?;
    for value in length_sums {
        header.u32(value)?;
    }
    header.u32(n_terms)?;
    out.push((SEG_HEADER, encode_segment(&header.payload())));

    Ok(out)
}

fn write_file_synced_measured(
    path: &Path,
    payload: &[u8],
    timing: bool,
    write_duration: &mut std::time::Duration,
    sync_duration: &mut std::time::Duration,
) -> std::io::Result<()> {
    let write_started = timing.then(std::time::Instant::now);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(payload)?;
    if let Some(started) = write_started {
        *write_duration += started.elapsed();
    }
    let sync_started = timing.then(std::time::Instant::now);
    let result = file.sync_all();
    if let Some(started) = sync_started {
        *sync_duration += started.elapsed();
    }
    result
}

fn write_file_synced(path: &Path, payload: &[u8]) -> std::io::Result<()> {
    let mut write_duration = std::time::Duration::ZERO;
    let mut sync_duration = std::time::Duration::ZERO;
    write_file_synced_measured(
        path,
        payload,
        false,
        &mut write_duration,
        &mut sync_duration,
    )
}

fn fsync_dir(path: &Path) {
    if let Ok(dir) = fs::File::open(path) {
        let _ = dir.sync_all();
    }
}

fn remove_tree(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

/// Temp-dir suffix entropy: pid plus a few clock-derived bytes (never mapped
/// into any payload — mirrors the oracle's pid+urandom temp names).
fn temp_suffix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{}-{:08x}", std::process::id(), nanos)
}

/// Write the store for `corpus_hash` atomically; return whether it landed.
pub fn write_store(
    cache_dir: &Path,
    corpus_hash: &str,
    bundle_version: &str,
    derived: &DerivedIndex,
) -> bool {
    write_store_in_layout(
        cache_dir,
        STORE_LAYOUT_VERSION,
        corpus_hash,
        corpus_hash,
        bundle_version,
        derived,
        None,
    )
}

/// Stable graph-only state persisted beside the derived read model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphStoreMetadata {
    pub generation: String,
    pub layers: Vec<CorpusLayer>,
    pub mappings: Vec<GenerationMapping>,
    pub terminal_redirects: Vec<GenerationRedirect>,
}

impl GraphStoreMetadata {
    pub fn from_composition(
        generation: impl Into<String>,
        layers: Vec<CorpusLayer>,
        composition: &crate::graph_composition::GraphComposition,
    ) -> Self {
        Self {
            generation: generation.into(),
            layers,
            mappings: composition
                .ordered_overrides()
                .iter()
                .map(|mapping| GenerationMapping {
                    owner_rank: mapping.owner_rank,
                    owner_source: mapping.owner_source.clone(),
                    target: mapping.target.clone(),
                    replacement: mapping.replacement.clone(),
                    rationale: mapping.rationale.clone(),
                })
                .collect(),
            terminal_redirects: composition
                .terminal_redirects()
                .iter()
                .map(|(target, terminal)| GenerationRedirect {
                    target: target.clone(),
                    terminal: terminal.clone(),
                })
                .collect(),
        }
    }

    /// Confirm that a fresh graph projection is the exact model described by
    /// this metadata before it can become resident. Store opens perform the
    /// same check after decoding; exposing the predicate keeps cache-disabled
    /// graph reads on the identical fail-closed boundary.
    pub fn matches_derived(&self, derived: &DerivedIndex) -> bool {
        graph_metadata_matches_derived(self, derived)
    }
}

/// Write one graph-federation generation under the isolated `store/v3`
/// namespace. `generation` is the full canonical `sha256-v3:` text.
pub fn write_graph_store(
    cache_dir: &Path,
    generation: &str,
    bundle_version: &str,
    derived: &DerivedIndex,
    metadata: &GraphStoreMetadata,
) -> bool {
    let Some(directory_key) = graph_store_directory_key(generation) else {
        return false;
    };
    if metadata.generation != generation || !graph_metadata_matches_derived(metadata, derived) {
        return false;
    }
    write_store_in_layout(
        cache_dir,
        GRAPH_STORE_LAYOUT_VERSION,
        directory_key,
        generation,
        bundle_version,
        derived,
        Some(metadata),
    )
}

fn write_store_in_layout(
    cache_dir: &Path,
    layout: &str,
    directory_key: &str,
    corpus_hash: &str,
    bundle_version: &str,
    derived: &DerivedIndex,
    graph_metadata: Option<&GraphStoreMetadata>,
) -> bool {
    let root = store_root_for_layout(cache_dir, layout);
    let final_dir = root.join(directory_key);
    if final_dir.is_dir() {
        // Content addressing: a same-hash store is byte-equivalent within one
        // format. Probe readability with the full open; replace when bad.
        let opened = MmapIndexReader::open(&final_dir, corpus_hash, bundle_version).ok();
        let model_valid = opened.is_some()
            && graph_metadata
                .is_none_or(|expected| graph_metadata_matches_mapped(expected, opened.as_ref().unwrap()));
        let graph_valid = graph_metadata.is_none_or(|expected| {
            encode_graph_metadata(expected)
                .ok()
                .zip(fs::read(final_dir.join(SEG_GRAPH)).ok())
                .is_some_and(|(expected_bytes, actual_bytes)| expected_bytes == actual_bytes)
        });
        if model_valid && graph_valid {
            return true;
        }
        remove_tree(&final_dir);
    }
    let encode_started = crate::timing::start();
    let Ok(mut segments) = encode_segments(corpus_hash, bundle_version, derived) else {
        crate::timing::emit_since("store.encode", encode_started, &[("success", 0)]);
        return false;
    };
    if let Some(metadata) = graph_metadata {
        let Ok(payload) = encode_graph_metadata(metadata) else {
            crate::timing::emit_since("store.encode", encode_started, &[("success", 0)]);
            return false;
        };
        segments.push((SEG_GRAPH, payload));
    }
    crate::timing::emit_since(
        "store.encode",
        encode_started,
        &[("success", 1), ("segments", segments.len() as u64)],
    );
    let tmp = root.join(format!(".{directory_key}.tmp-{}", temp_suffix()));
    let timing = crate::timing::enabled();
    let mut write_duration = std::time::Duration::ZERO;
    let mut sync_duration = std::time::Duration::ZERO;
    let mut write_all = || -> std::io::Result<()> {
        fs::create_dir_all(&tmp)?;
        for (name, payload) in &segments {
            write_file_synced_measured(
                &tmp.join(name),
                payload,
                timing,
                &mut write_duration,
                &mut sync_duration,
            )?;
        }
        let sync_started = timing.then(std::time::Instant::now);
        fsync_dir(&tmp);
        if let Some(started) = sync_started {
            sync_duration += started.elapsed();
        }
        Ok(())
    };
    let write_result = write_all();
    crate::timing::emit(
        "store.segment_write",
        write_duration,
        &[("segments", segments.len() as u64)],
    );
    crate::timing::emit(
        "store.segment_sync",
        sync_duration,
        &[("segments", segments.len() as u64)],
    );
    if write_result.is_err() {
        remove_tree(&tmp);
        return false;
    }
    match fs::rename(&tmp, &final_dir) {
        Ok(()) => true,
        Err(_) => {
            // Populated by a concurrent writer (identical content), or the
            // rename failed: discard and report the store's presence honestly.
            remove_tree(&tmp);
            final_dir.is_dir()
        }
    }
}

/// Best-effort removal of a store directory (used to clear a corrupt one).
pub fn remove_store(cache_dir: &Path, corpus_hash: &str) {
    remove_tree(&store_dir(cache_dir, corpus_hash));
}

/// Best-effort removal of one graph-generation store.
pub fn remove_graph_store(cache_dir: &Path, generation: &str) {
    if let Some(directory) = graph_store_dir(cache_dir, generation) {
        remove_tree(&directory);
    }
}

// ---------------------------------------------------------------------------
// Reader — mmap the segments, validate on open, point-access the rows.
// ---------------------------------------------------------------------------

/// Memory-mapped reader over one corpus-hash store directory (the base).
pub struct MmapIndexReader {
    maps: Vec<Mmap>, // ALL_SEGMENTS order; payload = &map[18..] after gates
    pub doc_count: u32,
    pub field_length_sums: [u64; 6],
    pub term_count: u32,
}

fn seg_index(name: &str) -> usize {
    ALL_SEGMENTS.iter().position(|s| *s == name).expect("known segment")
}

impl MmapIndexReader {
    pub fn open(
        directory: &Path,
        corpus_hash: &str,
        bundle_version: &str,
    ) -> Result<Self, IndexFormatError> {
        let mut maps = Vec::with_capacity(ALL_SEGMENTS.len());
        for name in ALL_SEGMENTS {
            let path = directory.join(name);
            let file = fs::File::open(&path)
                .map_err(|e| IndexFormatError(format!("cannot open {name}: {e}")))?;
            let len = file
                .metadata()
                .map_err(|e| IndexFormatError(format!("cannot stat {name}: {e}")))?
                .len();
            if len == 0 {
                return Err(IndexFormatError(format!("empty segment: {name}")));
            }
            let map = unsafe { Mmap::map(&file) }
                .map_err(|e| IndexFormatError(format!("cannot map {name}: {e}")))?;
            segment_payload(&map)?; // framing gates: magic, version, length
            maps.push(map);
        }
        let mut reader = Self {
            maps,
            doc_count: 0,
            field_length_sums: [0; 6],
            term_count: 0,
        };
        reader.read_header(corpus_hash, bundle_version)?;
        Ok(reader)
    }

    fn payload(&self, name: &str) -> &[u8] {
        segment_payload(&self.maps[seg_index(name)]).expect("validated on open")
    }

    fn read_header(
        &mut self,
        corpus_hash: &str,
        bundle_version: &str,
    ) -> Result<(), IndexFormatError> {
        let payload = segment_payload(&self.maps[seg_index(SEG_HEADER)])?;
        let mut reader = Reader::new(payload);
        let stored_hash = reader.text()?;
        let stored_bundle = reader.text()?;
        let stored_fingerprint = reader.text()?;
        let doc_count = reader.u32()?;
        let mut sums = [0u64; 6];
        for slot in &mut sums {
            *slot = u64::from(reader.u32()?);
        }
        let term_count = reader.u32()?;
        if stored_hash != corpus_hash {
            return Err(IndexFormatError("store corpus-hash mismatch".into()));
        }
        if stored_bundle != bundle_version {
            return Err(IndexFormatError("store bundle-version mismatch".into()));
        }
        if stored_fingerprint != scoring_fingerprint() {
            return Err(IndexFormatError("store scoring-constant mismatch".into()));
        }
        self.doc_count = doc_count;
        self.field_length_sums = sums;
        self.term_count = term_count;
        Ok(())
    }

    fn indexed(&self, name: &str) -> Result<IndexedSegment<'_>, IndexFormatError> {
        IndexedSegment::new(self.payload(name))
    }

    /// The lightweight identity row (no sections, no inbound).
    pub fn identity_entry(&self, docid: u32) -> Result<IndexEntry, IndexFormatError> {
        read_identity_entry(&mut self.indexed(SEG_IDENTITIES)?.row(docid)?)
    }

    pub fn identity_count(&self) -> Result<u32, IndexFormatError> {
        Ok(self.indexed(SEG_IDENTITIES)?.count())
    }

    /// The full index row: identity plus searchable sections and inbound.
    pub fn full_entry(&self, docid: u32) -> Result<IndexEntry, IndexFormatError> {
        let mut reader = self.indexed(SEG_ENTRIES)?.row(docid)?;
        let id = reader.text()?;
        let artifact_type = reader.text()?;
        let title = reader.opt_text()?;
        let path = reader.text()?;
        let aliases = reader.text_list()?;
        let tags = reader.text_list()?;
        let inbound = reader.u32()?;
        for _ in 0..6 {
            reader.u32()?; // field lengths
        }
        let key = read_artifact_key(&mut reader)?;
        let artifact_path = read_artifact_path(&mut reader)?;
        let origin = read_origin(&mut reader)?;
        let sections = self.read_sections(docid)?;
        Ok(IndexEntry {
            key,
            artifact_path,
            origin,
            id,
            artifact_type,
            title,
            path,
            aliases,
            search_sections: sections,
            inbound_count: i64::from(inbound),
            tags,
        })
    }

    /// Stable source-aware identity parallel to one persisted index row.
    pub fn source_artifact(&self, docid: u32) -> Result<SourceAwareArtifact, IndexFormatError> {
        let entry = self.identity_entry(docid)?;
        let key = entry
            .key
            .ok_or_else(|| IndexFormatError("v2 entry is missing its artifact key".into()))?;
        let path = entry
            .artifact_path
            .ok_or_else(|| IndexFormatError("v2 entry is missing its artifact path".into()))?;
        let origin = entry
            .origin
            .ok_or_else(|| IndexFormatError("v2 entry is missing its origin".into()))?;
        Ok(SourceAwareArtifact {
            key,
            path,
            origin,
            display_path: entry.path,
        })
    }

    fn read_sections(
        &self,
        docid: u32,
    ) -> Result<Vec<crate::markdown::SearchSection>, IndexFormatError> {
        let mut reader = self.indexed(SEG_SECTIONS)?.row(docid)?;
        let count = reader.u32()?;
        let mut sections = Vec::with_capacity(count.min(1 << 16) as usize);
        for _ in 0..count {
            sections.push(crate::markdown::SearchSection {
                heading: reader.text()?,
                lines: reader.text_list()?,
            });
        }
        Ok(sections)
    }

    pub fn entry_path(&self, docid: u32) -> Result<String, IndexFormatError> {
        let mut reader = self.indexed(SEG_ENTRIES)?.row(docid)?;
        reader.text()?; // id
        reader.text()?; // type
        reader.opt_text()?; // title
        reader.text() // path
    }

    /// The first non-empty line of this document's `## Status` section.
    /// Status is already present in the searchable-section segment, so
    /// grounding can determine lifecycle without reopening and reparsing the
    /// Markdown file.
    pub fn entry_status(&self, docid: u32) -> Result<String, IndexFormatError> {
        let mut reader = self.indexed(SEG_SECTIONS)?.row(docid)?;
        let count = reader.u32()?;
        for _ in 0..count {
            let is_status = {
                let heading = reader.text_ref()?;
                crate::pycompat::py_casefold(crate::pycompat::py_strip(heading)) == "status"
            };
            let line_count = reader.u32()?;
            if is_status {
                let mut status = String::new();
                for _ in 0..line_count {
                    let line = crate::pycompat::py_strip(reader.text_ref()?);
                    if status.is_empty() && !line.is_empty() {
                        status = line.to_string();
                    }
                }
                return Ok(status);
            }
            for _ in 0..line_count {
                reader.text_ref()?;
            }
        }
        Ok(String::new())
    }

    /// Per-field token counts for one doc, FIELDS order.
    pub fn field_lengths(&self, docid: u32) -> Result<[u64; 6], IndexFormatError> {
        let mut reader = self.indexed(SEG_ENTRIES)?.row(docid)?;
        reader.text()?;
        reader.text()?;
        reader.opt_text()?;
        reader.text()?;
        reader.text_list()?; // aliases
        reader.text_list()?; // tags
        reader.u32()?; // inbound
        let mut lengths = [0u64; 6];
        for slot in &mut lengths {
            *slot = u64::from(reader.u32()?);
        }
        Ok(lengths)
    }

    /// The six forward token-id sequences of one doc, FIELDS order.
    pub fn forward_token_ids(&self, docid: u32) -> Result<[Vec<u32>; 6], IndexFormatError> {
        let mut reader = self.indexed(SEG_TOKENS)?.row(docid)?;
        Ok([
            reader.u32_list()?,
            reader.u32_list()?,
            reader.u32_list()?,
            reader.u32_list()?,
            reader.u32_list()?,
            reader.u32_list()?,
        ])
    }

    pub fn term_at(&self, term_id: u32) -> Result<String, IndexFormatError> {
        self.indexed(SEG_TERMDICT)?.row(term_id)?.text()
    }

    /// Reconstruct one doc's per-field token vectors in document order.
    pub fn field_tokens(&self, docid: u32) -> Result<FieldTokens, IndexFormatError> {
        let ids = self.forward_token_ids(docid)?;
        let terms = self.indexed(SEG_TERMDICT)?;
        let resolve = |ids: &[u32]| -> Result<Vec<String>, IndexFormatError> {
            ids.iter().map(|&i| terms.row(i)?.text()).collect()
        };
        Ok(FieldTokens {
            id: resolve(&ids[0])?,
            title: resolve(&ids[1])?,
            path: resolve(&ids[2])?,
            heading: resolve(&ids[3])?,
            body: resolve(&ids[4])?,
            tags: resolve(&ids[5])?,
        })
    }

    fn bisect_left(&self, target: &str) -> Result<u32, IndexFormatError> {
        let segment = self.indexed(SEG_TERMDICT)?;
        let (mut lo, mut hi) = (0u32, segment.count());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if segment.row(mid)?.text()?.as_str() < target {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Ok(lo)
    }

    /// The `[lo, hi)` term-id range of every indexed term `term` prefixes.
    pub fn prefix_range(&self, term: &str) -> Result<(u32, u32), IndexFormatError> {
        if term.is_empty() {
            return Ok((0, 0));
        }
        let lo = self.bisect_left(term)?;
        // Successor: last char's code point incremented (Python chr(ord+1)).
        let mut chars: Vec<char> = term.chars().collect();
        let last = chars.pop().expect("non-empty");
        let successor_last = char::from_u32(last as u32 + 1);
        let hi = match successor_last {
            Some(c) => {
                chars.push(c);
                let successor: String = chars.into_iter().collect();
                self.bisect_left(&successor)?
            }
            None => self.indexed(SEG_TERMDICT)?.count(),
        };
        Ok((lo, hi))
    }

    /// The ascending docids that hold `term_id` in any field.
    pub fn postings(&self, term_id: u32) -> Result<Vec<u32>, IndexFormatError> {
        self.indexed(SEG_POSTINGS)?.row(term_id)?.u32_list()
    }

    /// Distinct docids matching `term` under the prefix predicate (ADR-037).
    pub fn prefix_docids(
        &self,
        term: &str,
    ) -> Result<std::collections::BTreeSet<u32>, IndexFormatError> {
        let (lo, hi) = self.prefix_range(term)?;
        let mut result = std::collections::BTreeSet::new();
        for term_id in lo..hi {
            result.extend(self.postings(term_id)?);
        }
        Ok(result)
    }

    /// The ascending docids whose identity set holds `wanted` (already
    /// casefolded by the caller) — binary search over the alias map.
    pub fn alias_docids(&self, wanted: &str) -> Result<Vec<u32>, IndexFormatError> {
        let segment = self.indexed(SEG_ALIASMAP)?;
        let (mut lo, mut hi) = (0u32, segment.count());
        while lo < hi {
            let mid = (lo + hi) / 2;
            let mut reader = segment.row(mid)?;
            let key = reader.text()?;
            match key.as_str().cmp(wanted) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return reader.u32_list(),
            }
        }
        Ok(Vec::new())
    }

    /// The docid whose stored path equals `path`, or None — binary search.
    pub fn docid_for_path(&self, path: &str) -> Result<Option<u32>, IndexFormatError> {
        let segment = self.indexed(SEG_PATHMAP)?;
        let (mut lo, mut hi) = (0u32, segment.count());
        while lo < hi {
            let mid = (lo + hi) / 2;
            let mut reader = segment.row(mid)?;
            let key = reader.text()?;
            match key.as_str().cmp(path) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Ok(Some(reader.u32()?)),
            }
        }
        Ok(None)
    }

    /// Resolve the stable `(source, canonical_id)` identity to every matching
    /// docid. More than one result remains observable as an ambiguity rather
    /// than acquiring iteration-order precedence.
    pub fn docids_for_key(&self, key: &ArtifactKey) -> Result<Vec<u32>, IndexFormatError> {
        let segment = self.indexed(SEG_KEYMAP)?;
        let (mut lo, mut hi) = (0u32, segment.count());
        while lo < hi {
            let mid = (lo + hi) / 2;
            let mut reader = segment.row(mid)?;
            let source = reader.text()?;
            let canonical_id = reader.text()?;
            match (source.as_str(), canonical_id.as_str())
                .cmp(&(key.source.as_str(), key.canonical_id.as_str()))
            {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return reader.u32_list(),
            }
        }
        Ok(Vec::new())
    }

    /// Return one stable-key match only when it is unambiguous.
    pub fn docid_for_key(&self, key: &ArtifactKey) -> Result<Option<u32>, IndexFormatError> {
        let docids = self.docids_for_key(key)?;
        Ok((docids.len() == 1).then(|| docids[0]))
    }

    /// Resolve the stable `(source, corpus-relative path)` identity to a
    /// docid without using a checkout or display path.
    pub fn docid_for_artifact_path(
        &self,
        path: &ArtifactPath,
    ) -> Result<Option<u32>, IndexFormatError> {
        let segment = self.indexed(SEG_ARTIFACTPATHMAP)?;
        let (mut lo, mut hi) = (0u32, segment.count());
        while lo < hi {
            let mid = (lo + hi) / 2;
            let mut reader = segment.row(mid)?;
            let source = reader.text()?;
            let relative_path = reader.text()?;
            match (source.as_str(), relative_path.as_str())
                .cmp(&(path.source.as_str(), path.relative_path.as_str()))
            {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Ok(Some(reader.u32()?)),
            }
        }
        Ok(None)
    }

    /// Stable layer identities represented by this persisted generation.
    pub fn layers(&self) -> Result<Vec<CorpusLayer>, IndexFormatError> {
        let mut reader = Reader::new(self.payload(SEG_LAYERS));
        let count = reader.u32()?;
        let mut layers = Vec::with_capacity(count.min(1 << 20) as usize);
        for _ in 0..count {
            layers.push(read_layer(&mut reader)?);
        }
        Ok(layers)
    }

    /// Validated canonical redirects retained by the composed generation.
    pub fn canonical_redirects(&self) -> Result<Vec<CanonicalRedirect>, IndexFormatError> {
        let mut reader = Reader::new(self.payload(SEG_REDIRECTS));
        let count = reader.u32()?;
        let mut redirects = Vec::with_capacity(count.min(1 << 20) as usize);
        for _ in 0..count {
            let parent = read_artifact_key(&mut reader)?
                .ok_or_else(|| IndexFormatError("redirect parent is absent".into()))?;
            let replacement = read_artifact_key(&mut reader)?
                .ok_or_else(|| IndexFormatError("redirect replacement is absent".into()))?;
            let rationale = read_artifact_key(&mut reader)?
                .ok_or_else(|| IndexFormatError("redirect rationale is absent".into()))?;
            redirects.push(CanonicalRedirect {
                parent,
                replacement,
                rationale,
            });
        }
        Ok(redirects)
    }

    pub fn canonical_redirect(
        &self,
        parent: &ArtifactKey,
    ) -> Result<Option<CanonicalRedirect>, IndexFormatError> {
        Ok(self
            .canonical_redirects()?
            .into_iter()
            .find(|redirect| &redirect.parent == parent))
    }

    pub fn relationships(&self) -> Result<Vec<Relationship>, IndexFormatError> {
        let mut reader = Reader::new(self.payload(SEG_RELATIONSHIPS));
        let count = reader.u32()?;
        let mut result = Vec::with_capacity(count.min(1 << 20) as usize);
        for _ in 0..count {
            let source_path = reader.text()?;
            let relationship = reader.text()?;
            let target = reader.text()?;
            let resolved_path = reader.opt_text()?;
            let issue = reader.opt_text()?;
            let source_artifact = read_artifact_path(&mut reader)?;
            let resolved_artifact = read_artifact_path(&mut reader)?;
            result.push(Relationship {
                source_artifact,
                source_path,
                relationship,
                target,
                resolved_artifact,
                resolved_path,
                issue,
            });
        }
        Ok(result)
    }

    fn live_decisions(&self) -> Result<Vec<(ArtifactKey, String)>, IndexFormatError> {
        let mut reader = Reader::new(self.payload(SEG_LIVE));
        let count = reader.u32()?;
        let mut rows = Vec::with_capacity(count.min(1 << 20) as usize);
        for _ in 0..count {
            let key = read_artifact_key(&mut reader)?
                .ok_or_else(|| IndexFormatError("live decision key is absent".into()))?;
            rows.push((key, reader.text()?));
        }
        Ok(rows)
    }

    pub fn live_decision_keys(&self) -> Result<Vec<ArtifactKey>, IndexFormatError> {
        Ok(self
            .live_decisions()?
            .into_iter()
            .map(|(key, _)| key)
            .collect())
    }

    pub fn live_decision_paths(&self) -> Result<Vec<String>, IndexFormatError> {
        Ok(self
            .live_decisions()?
            .into_iter()
            .map(|(_, path)| path)
            .collect())
    }

    pub fn scope_rows(&self) -> Result<Vec<crate::retrieve::ScopeRow>, IndexFormatError> {
        let mut reader = Reader::new(self.payload(SEG_SCOPE));
        let count = reader.u32()?;
        let mut rows = Vec::with_capacity(count.min(1 << 20) as usize);
        for _ in 0..count {
            rows.push(crate::retrieve::ScopeRow {
                key: read_artifact_key(&mut reader)?,
                artifact_path: read_artifact_path(&mut reader)?,
                origin: read_origin(&mut reader)?,
                id: reader.text()?,
                title: reader.text()?,
                status: reader.text()?,
                path: reader.text()?,
                scope_entries: reader.text_list()?,
            });
        }
        Ok(rows)
    }

    /// The portfolio summary parsed back from its stored JSON text.
    pub fn portfolio_summary(&self) -> Result<Value, IndexFormatError> {
        let text = Reader::new(self.payload(SEG_PORTFOLIO)).text()?;
        serde_json::from_str(&text)
            .map_err(|e| IndexFormatError(format!("portfolio segment is not valid JSON: {e}")))
    }
}

/// Open the store for `corpus_hash`, or `None` on any miss (never fatal).
pub fn open_store(
    cache_dir: &Path,
    corpus_hash: &str,
    bundle_version: &str,
) -> Option<MmapIndexReader> {
    let directory = store_dir(cache_dir, corpus_hash);
    if !directory.is_dir() {
        return None;
    }
    MmapIndexReader::open(&directory, corpus_hash, bundle_version).ok()
}

/// Open one graph generation from `store/v3`, or return a cache miss. V1 and
/// V2 directories are never considered by this path.
pub fn open_graph_store(
    cache_dir: &Path,
    generation: &str,
    bundle_version: &str,
    expected: &GraphStoreMetadata,
) -> Option<GraphMmapIndexReader> {
    if expected.generation != generation {
        return None;
    }
    let directory = graph_store_dir(cache_dir, generation)?;
    if !directory.is_dir() {
        return None;
    }
    let model = MmapIndexReader::open(&directory, generation, bundle_version).ok()?;
    let metadata = decode_graph_metadata(&fs::read(directory.join(SEG_GRAPH)).ok()?).ok()?;
    if metadata != canonical_graph_metadata(expected)
        || !graph_metadata_matches_mapped(&metadata, &model)
    {
        return None;
    }
    Some(GraphMmapIndexReader { model, metadata })
}

/// One validated graph store. The derived segments and graph-only mapping
/// state are opened atomically from the same versioned directory.
pub struct GraphMmapIndexReader {
    pub model: MmapIndexReader,
    pub metadata: GraphStoreMetadata,
}

const MAX_GRAPH_LAYERS: usize = 257;
const MAX_GRAPH_MAPPINGS: usize = 4_096;
const MAX_GRAPH_REDIRECTS: usize = 4_096;

fn canonical_graph_metadata(metadata: &GraphStoreMetadata) -> GraphStoreMetadata {
    let mut canonical = metadata.clone();
    canonical.layers.sort();
    canonical.mappings.sort_by(graph_mapping_order);
    canonical
        .terminal_redirects
        .sort_by(|left, right| left.target.cmp(&right.target));
    canonical
}

fn graph_mapping_order(
    left: &GenerationMapping,
    right: &GenerationMapping,
) -> std::cmp::Ordering {
    (
        left.owner_rank,
        &left.owner_source,
        &left.target,
        &left.replacement,
        &left.rationale,
    )
        .cmp(&(
            right.owner_rank,
            &right.owner_source,
            &right.target,
            &right.replacement,
            &right.rationale,
        ))
}

fn validate_graph_metadata_shape(metadata: &GraphStoreMetadata) -> Result<(), IndexFormatError> {
    if graph_store_directory_key(&metadata.generation).is_none() {
        return Err(IndexFormatError("invalid graph generation".into()));
    }
    if metadata.layers.is_empty() || metadata.layers.len() > MAX_GRAPH_LAYERS {
        return Err(IndexFormatError("invalid graph layer count".into()));
    }
    if metadata.mappings.len() > MAX_GRAPH_MAPPINGS {
        return Err(IndexFormatError("graph mapping limit exceeded".into()));
    }
    if metadata.terminal_redirects.len() > MAX_GRAPH_REDIRECTS {
        return Err(IndexFormatError("graph redirect limit exceeded".into()));
    }

    let mut sources = std::collections::BTreeSet::new();
    let mut local_count = 0usize;
    for layer in &metadata.layers {
        if !crate::scaffold::valid_corpus_source(&layer.source) || !sources.insert(&layer.source) {
            return Err(IndexFormatError("invalid or duplicate graph source".into()));
        }
        match layer.layer {
            Layer::Local => {
                local_count += 1;
                if layer.pin.is_some() || layer.alias.is_some() {
                    return Err(IndexFormatError(
                        "root graph layer must omit pin and alias".into(),
                    ));
                }
            }
            Layer::Inherited => {
                if layer
                    .pin
                    .as_deref()
                    .and_then(|pin| valid_versioned_digest(pin, "sha256-v2:"))
                    .is_none()
                    || layer.alias.is_some()
                {
                    return Err(IndexFormatError(
                        "inherited graph layer requires a canonical v2 pin and no global alias"
                            .into(),
                    ));
                }
            }
        }
    }
    if local_count != 1 {
        return Err(IndexFormatError(
            "graph metadata must contain exactly one root layer".into(),
        ));
    }

    let valid_key = |key: &ArtifactKey| {
        sources.contains(&key.source)
            && !key.canonical_id.is_empty()
            && key.canonical_id.len() <= 4096
            && !key.canonical_id.contains("::")
    };
    let mut mapping_targets = std::collections::BTreeSet::new();
    for mapping in &metadata.mappings {
        if !sources.contains(&mapping.owner_source)
            || !valid_key(&mapping.target)
            || !valid_key(&mapping.replacement)
            || !valid_key(&mapping.rationale)
            || mapping.replacement.source != mapping.owner_source
            || mapping.rationale.source != mapping.owner_source
            || !mapping_targets.insert(&mapping.target)
        {
            return Err(IndexFormatError("invalid graph mapping row".into()));
        }
    }
    let mut redirect_targets = std::collections::BTreeSet::new();
    for redirect in &metadata.terminal_redirects {
        if !valid_key(&redirect.target)
            || !valid_key(&redirect.terminal)
            || redirect.target == redirect.terminal
            || !redirect_targets.insert(&redirect.target)
        {
            return Err(IndexFormatError("invalid graph redirect row".into()));
        }
    }
    Ok(())
}

fn graph_metadata_matches_derived(metadata: &GraphStoreMetadata, derived: &DerivedIndex) -> bool {
    if validate_graph_metadata_shape(metadata).is_err() {
        return false;
    }
    let mut layers = derived.layers.clone();
    layers.sort();
    layers.dedup();
    if layers != canonical_graph_metadata(metadata).layers {
        return false;
    }
    let identity_keys: std::collections::BTreeSet<ArtifactKey> = derived
        .resolution
        .entries
        .iter()
        .filter_map(|entry| entry.key.clone())
        .collect();
    graph_rows_match_model(metadata, &identity_keys, &derived.resolution.canonical_redirects)
}

fn graph_metadata_matches_mapped(
    metadata: &GraphStoreMetadata,
    model: &MmapIndexReader,
) -> bool {
    if validate_graph_metadata_shape(metadata).is_err() {
        return false;
    }
    let Ok(mut layers) = model.layers() else {
        return false;
    };
    layers.sort();
    layers.dedup();
    if layers != canonical_graph_metadata(metadata).layers {
        return false;
    }
    let Ok(identity_count) = model.identity_count() else {
        return false;
    };
    let mut identity_keys = std::collections::BTreeSet::new();
    for docid in 0..identity_count {
        let Ok(entry) = model.identity_entry(docid) else {
            return false;
        };
        let Some(key) = entry.key else {
            return false;
        };
        identity_keys.insert(key);
    }
    let Ok(redirects) = model.canonical_redirects() else {
        return false;
    };
    graph_rows_match_model(metadata, &identity_keys, &redirects)
}

fn graph_rows_match_model(
    metadata: &GraphStoreMetadata,
    identity_keys: &std::collections::BTreeSet<ArtifactKey>,
    redirects: &[CanonicalRedirect],
) -> bool {
    let metadata_mappings: std::collections::BTreeSet<_> = metadata
        .mappings
        .iter()
        .map(|mapping| {
            (
                mapping.target.clone(),
                mapping.replacement.clone(),
                mapping.rationale.clone(),
            )
        })
        .collect();
    let model_mappings: std::collections::BTreeSet<_> = redirects
        .iter()
        .map(|mapping| {
            (
                mapping.parent.clone(),
                mapping.replacement.clone(),
                mapping.rationale.clone(),
            )
        })
        .collect();
    metadata_mappings == model_mappings
        && metadata.mappings.iter().all(|mapping| {
            identity_keys.contains(&mapping.target)
                && identity_keys.contains(&mapping.replacement)
                && identity_keys.contains(&mapping.rationale)
        })
        && metadata.terminal_redirects.iter().all(|redirect| {
            identity_keys.contains(&redirect.target) && identity_keys.contains(&redirect.terminal)
        })
}

fn encode_graph_metadata(metadata: &GraphStoreMetadata) -> Result<Vec<u8>, IndexFormatError> {
    validate_graph_metadata_shape(metadata)?;
    let metadata = canonical_graph_metadata(metadata);
    let mut writer = Writer::new();
    writer.text(std::str::from_utf8(GRAPH_CONTRACT).expect("ASCII graph contract"))?;
    writer.text(&metadata.generation)?;

    writer.u32(metadata.layers.len() as u64)?;
    for layer in &metadata.layers {
        write_layer(&mut writer, layer)?;
    }

    writer.u32(metadata.mappings.len() as u64)?;
    for mapping in &metadata.mappings {
        writer.u32(mapping.owner_rank as u64)?;
        writer.text(&mapping.owner_source)?;
        write_required_artifact_key(&mut writer, &mapping.target)?;
        write_required_artifact_key(&mut writer, &mapping.replacement)?;
        write_required_artifact_key(&mut writer, &mapping.rationale)?;
    }

    writer.u32(metadata.terminal_redirects.len() as u64)?;
    for redirect in &metadata.terminal_redirects {
        write_required_artifact_key(&mut writer, &redirect.target)?;
        write_required_artifact_key(&mut writer, &redirect.terminal)?;
    }
    Ok(encode_segment(&writer.payload()))
}

fn decode_graph_metadata(bytes: &[u8]) -> Result<GraphStoreMetadata, IndexFormatError> {
    let payload = segment_payload(bytes)?;
    let mut reader = Reader::new(payload);
    if reader.text()?.as_bytes() != GRAPH_CONTRACT {
        return Err(IndexFormatError("graph contract mismatch".into()));
    }
    let generation = reader.text()?;
    let layer_count = reader.u32()?;
    if layer_count as usize > MAX_GRAPH_LAYERS {
        return Err(IndexFormatError("invalid graph layer count".into()));
    }
    let mut layers = Vec::with_capacity(layer_count.min(1 << 20) as usize);
    for _ in 0..layer_count {
        layers.push(read_layer(&mut reader)?);
    }
    let mapping_count = reader.u32()?;
    if mapping_count as usize > MAX_GRAPH_MAPPINGS {
        return Err(IndexFormatError("graph mapping limit exceeded".into()));
    }
    let mut mappings = Vec::with_capacity(mapping_count.min(1 << 20) as usize);
    for _ in 0..mapping_count {
        mappings.push(GenerationMapping {
            owner_rank: reader.u32()? as usize,
            owner_source: reader.text()?,
            target: read_required_artifact_key(&mut reader)?,
            replacement: read_required_artifact_key(&mut reader)?,
            rationale: read_required_artifact_key(&mut reader)?,
        });
    }
    let redirect_count = reader.u32()?;
    if redirect_count as usize > MAX_GRAPH_REDIRECTS {
        return Err(IndexFormatError("graph redirect limit exceeded".into()));
    }
    let mut terminal_redirects = Vec::with_capacity(redirect_count.min(1 << 20) as usize);
    for _ in 0..redirect_count {
        terminal_redirects.push(GenerationRedirect {
            target: read_required_artifact_key(&mut reader)?,
            terminal: read_required_artifact_key(&mut reader)?,
        });
    }
    reader.finish()?;
    let metadata = GraphStoreMetadata {
        generation,
        layers,
        mappings,
        terminal_redirects,
    };
    validate_graph_metadata_shape(&metadata)?;
    if encode_graph_metadata(&metadata)? != bytes {
        return Err(IndexFormatError(
            "graph metadata is not in canonical order".into(),
        ));
    }
    Ok(metadata)
}

fn write_required_artifact_key(
    writer: &mut Writer,
    key: &ArtifactKey,
) -> Result<(), IndexFormatError> {
    writer.text(&key.source)?;
    writer.text(&key.canonical_id)?;
    Ok(())
}

fn read_required_artifact_key(reader: &mut Reader<'_>) -> Result<ArtifactKey, IndexFormatError> {
    Ok(ArtifactKey::new(reader.text()?, reader.text()?))
}

// ---------------------------------------------------------------------------
// Per-file validation-result store (`.vseg`, ADR-106) — codec only here;
// the incremental-validate seam consumes it (INDEX-PLAN B4).
// ---------------------------------------------------------------------------

pub const VALIDATE_STORE_DIRNAME: &str = "validate";
pub const VALIDATE_LAYOUT_VERSION: &str = "v1";

/// One file's cached validation result plus its freshness stat proxy.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationCacheRow {
    pub size: u64,
    pub mtime_ns: u64,
    pub content_hash: String,
    pub artifact_type: String,
    pub status: String,
    pub issues: Vec<CachedIssue>,
}

/// A path-free cached issue (`Issue` without location context).
#[derive(Debug, Clone, PartialEq)]
pub struct CachedIssue {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub line: Option<u32>,
}

pub fn validate_store_root(cache_dir: &Path) -> PathBuf {
    cache_dir
        .join(VALIDATE_STORE_DIRNAME)
        .join(VALIDATE_LAYOUT_VERSION)
}

fn validate_store_path(cache_dir: &Path, root_key: &str) -> PathBuf {
    validate_store_root(cache_dir).join(format!("{root_key}.vseg"))
}

/// Encode the `.vseg` payload — rows in insertion order.
pub fn encode_validation_store(
    config_hash: &str,
    rows: &[(String, ValidationCacheRow)],
) -> Result<Vec<u8>, IndexFormatError> {
    let mut writer = Writer::new();
    writer.text(config_hash)?;
    writer.u32(rows.len() as u64)?;
    for (rel, row) in rows {
        writer.text(rel)?;
        writer.u64(row.size);
        writer.u64(row.mtime_ns);
        writer.text(&row.content_hash)?;
        writer.text(&row.artifact_type)?;
        writer.text(&row.status)?;
        writer.u32(row.issues.len() as u64)?;
        for issue in &row.issues {
            writer.text(&issue.severity)?;
            writer.text(&issue.code)?;
            writer.text(&issue.message)?;
            match issue.line {
                None => {
                    writer.u32(0)?;
                    writer.u32(0)?;
                }
                Some(line) => {
                    writer.u32(1)?;
                    writer.u32(u64::from(line))?;
                }
            }
        }
    }
    Ok(encode_segment(&writer.payload()))
}

/// Decode a `.vseg` payload; `None` on config mismatch (a miss).
pub fn decode_validation_store(
    payload: &[u8],
    config_hash: &str,
) -> Result<Option<Vec<(String, ValidationCacheRow)>>, IndexFormatError> {
    let mut reader = Reader::new(payload);
    if reader.text()? != config_hash {
        return Ok(None);
    }
    let count = reader.u32()?;
    let mut rows = Vec::with_capacity(count.min(1 << 20) as usize);
    for _ in 0..count {
        let rel = reader.text()?;
        let size = reader.u64()?;
        let mtime_ns = reader.u64()?;
        let content_hash = reader.text()?;
        let artifact_type = reader.text()?;
        let status = reader.text()?;
        let issue_count = reader.u32()?;
        let mut issues = Vec::with_capacity(issue_count.min(1 << 16) as usize);
        for _ in 0..issue_count {
            let severity = reader.text()?;
            let code = reader.text()?;
            let message = reader.text()?;
            let has_line = reader.u32()?;
            let line_value = reader.u32()?;
            issues.push(CachedIssue {
                severity,
                code,
                message,
                line: if has_line != 0 { Some(line_value) } else { None },
            });
        }
        rows.push((
            rel,
            ValidationCacheRow {
                size,
                mtime_ns,
                content_hash,
                artifact_type,
                status,
                issues,
            },
        ));
    }
    Ok(Some(rows))
}

/// Load the per-file validation rows for a corpus root, or `None` on a miss.
pub fn open_validation_store(
    cache_dir: &Path,
    root_key: &str,
    config_hash: &str,
) -> Option<Vec<(String, ValidationCacheRow)>> {
    let data = fs::read(validate_store_path(cache_dir, root_key)).ok()?;
    let payload = segment_payload(&data).ok()?;
    decode_validation_store(payload, config_hash).ok()?
}

/// Write the per-file validation rows atomically; return whether it landed.
pub fn write_validation_store(
    cache_dir: &Path,
    root_key: &str,
    config_hash: &str,
    rows: &[(String, ValidationCacheRow)],
) -> bool {
    let Ok(payload) = encode_validation_store(config_hash, rows) else {
        return false;
    };
    atomic_write(
        &validate_store_root(cache_dir),
        root_key,
        &validate_store_path(cache_dir, root_key),
        &payload,
    )
}

// ---------------------------------------------------------------------------
// Per-root freshness-manifest store (`.fseg`, ADR-112)
// ---------------------------------------------------------------------------

pub const MANIFEST_DIRNAME: &str = "manifest";
pub const MANIFEST_LAYOUT_VERSION: &str = "v1";
const MANIFEST_FORMAT_VERSION: u32 = 1;

/// The freshness proxy for one file: content hash plus the stat pair.
#[derive(Debug, Clone, PartialEq)]
pub struct FileState {
    pub content_hash: String,
    pub size: u64,
    pub mtime_ns: u64,
}

pub fn manifest_store_root(cache_dir: &Path) -> PathBuf {
    cache_dir.join(MANIFEST_DIRNAME).join(MANIFEST_LAYOUT_VERSION)
}

/// `Path(directory).resolve()` — absolutise against the cwd and normalise,
/// canonicalising the longest existing prefix (Python resolves symlinks for
/// the part of the path that exists and keeps the nonexistent tail).
pub fn py_resolve(directory: &str) -> PathBuf {
    let path = Path::new(directory);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    // Lexically normalise `.` and `..`, then canonicalise the longest
    // existing prefix so symlinked ancestors resolve as Python's do.
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for comp in absolute.components() {
        use std::path::Component;
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::RootDir | Component::Prefix(_) => {}
            Component::Normal(p) => parts.push(p.to_os_string()),
        }
    }
    let mut resolved = PathBuf::from("/");
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut existing = PathBuf::from("/");
    for (i, part) in parts.iter().enumerate() {
        existing.push(part);
        if tail.is_empty() && existing.exists() {
            continue;
        }
        if tail.is_empty() {
            // first nonexistent component: canonicalise what exists so far
            let prefix = {
                let mut p = PathBuf::from("/");
                for q in &parts[..i] {
                    p.push(q);
                }
                p
            };
            resolved = fs::canonicalize(&prefix).unwrap_or(prefix);
        }
        tail.push(part.clone());
    }
    if tail.is_empty() {
        fs::canonicalize(&existing).unwrap_or(existing)
    } else {
        for part in tail {
            resolved.push(part);
        }
        resolved
    }
}

/// A stable key for one corpus root in one recursion mode.
pub fn manifest_root_key(directory: &str, recursive: bool) -> String {
    let mode = if recursive { "recursive" } else { "top-level" };
    let seed = format!("{}\0{mode}", py_resolve(directory).display());
    crate::sha256::hexdigest(seed.as_bytes())
}

fn manifest_store_path(cache_dir: &Path, root_key: &str) -> PathBuf {
    manifest_store_root(cache_dir).join(format!("{root_key}.fseg"))
}

/// Encode the `.fseg` manifest — rows in insertion (scan) order.
pub fn encode_freshness_manifest(
    manifest: &[(String, FileState)],
) -> Result<Vec<u8>, IndexFormatError> {
    let mut writer = Writer::new();
    writer.u32(u64::from(MANIFEST_FORMAT_VERSION))?;
    writer.u32(manifest.len() as u64)?;
    for (rel, state) in manifest {
        writer.text(rel)?;
        writer.u64(state.size);
        writer.u64(state.mtime_ns);
        writer.text(&state.content_hash)?;
    }
    Ok(encode_segment(&writer.payload()))
}

/// Load the persisted stat manifest for a corpus root, or `None` on a miss.
pub fn open_freshness_manifest(
    cache_dir: &Path,
    root_key: &str,
) -> Option<Vec<(String, FileState)>> {
    let data = fs::read(manifest_store_path(cache_dir, root_key)).ok()?;
    let payload = segment_payload(&data).ok()?;
    let mut reader = Reader::new(payload);
    if reader.u32().ok()? != MANIFEST_FORMAT_VERSION {
        return None;
    }
    let count = reader.u32().ok()?;
    let mut manifest = Vec::with_capacity(count.min(1 << 20) as usize);
    for _ in 0..count {
        let rel = reader.text().ok()?;
        let size = reader.u64().ok()?;
        let mtime_ns = reader.u64().ok()?;
        let content_hash = reader.text().ok()?;
        manifest.push((
            rel,
            FileState {
                content_hash,
                size,
                mtime_ns,
            },
        ));
    }
    Some(manifest)
}

/// Write the stat manifest atomically; return whether it landed.
pub fn write_freshness_manifest(
    cache_dir: &Path,
    root_key: &str,
    manifest: &[(String, FileState)],
) -> bool {
    let Ok(payload) = encode_freshness_manifest(manifest) else {
        return false;
    };
    atomic_write(
        &manifest_store_root(cache_dir),
        root_key,
        &manifest_store_path(cache_dir, root_key),
        &payload,
    )
}

/// Shared temp-file + rename atomic write for the single-file stores.
fn atomic_write(root: &Path, key: &str, target: &Path, payload: &[u8]) -> bool {
    if fs::create_dir_all(root).is_err() {
        return false;
    }
    let tmp = root.join(format!(".{key}.tmp-{}", temp_suffix()));
    if write_file_synced(&tmp, payload).is_err() {
        let _ = fs::remove_file(&tmp);
        return false;
    }
    if fs::rename(&tmp, target).is_err() {
        let _ = fs::remove_file(&tmp);
        return false;
    }
    true
}

#[cfg(test)]
mod graph_layout_tests {
    use super::*;

    fn graph_metadata_fixture() -> GraphStoreMetadata {
        GraphStoreMetadata {
            generation: format!("sha256-v3:{}", "b".repeat(64)),
            layers: vec![
                CorpusLayer {
                    source: "acme/standards".into(),
                    layer: Layer::Inherited,
                    pin: Some(format!("sha256-v2:{}", "a".repeat(64))),
                    alias: None,
                },
                CorpusLayer::local("acme/app"),
            ],
            mappings: vec![GenerationMapping {
                owner_rank: 1,
                owner_source: "acme/app".into(),
                target: ArtifactKey::new("acme/standards", "STD-0123456789AB"),
                replacement: ArtifactKey::new("acme/app", "APP-0123456789AB"),
                rationale: ArtifactKey::new("acme/app", "APP-ABCDEFGHJKMN"),
            }],
            terminal_redirects: vec![GenerationRedirect {
                target: ArtifactKey::new("acme/standards", "STD-0123456789AB"),
                terminal: ArtifactKey::new("acme/app", "APP-0123456789AB"),
            }],
        }
    }

    fn identity_entry(key: &ArtifactKey, layer: &CorpusLayer) -> IndexEntry {
        IndexEntry {
            key: Some(key.clone()),
            artifact_path: Some(ArtifactPath::new(
                key.source.clone(),
                format!("{}.md", key.canonical_id),
            )),
            origin: Some(layer.origin()),
            id: key.canonical_id.clone(),
            artifact_type: "decision".into(),
            title: None,
            path: format!("{}.md", key.canonical_id),
            aliases: vec![key.canonical_id.clone()],
            search_sections: Vec::new(),
            inbound_count: 0,
            tags: Vec::new(),
        }
    }

    fn graph_derived_fixture(metadata: &GraphStoreMetadata) -> DerivedIndex {
        let canonical = canonical_graph_metadata(metadata);
        let layer_by_source: std::collections::BTreeMap<_, _> = canonical
            .layers
            .iter()
            .map(|layer| (layer.source.clone(), layer))
            .collect();
        let mut keys = std::collections::BTreeSet::new();
        for mapping in &canonical.mappings {
            keys.extend([
                mapping.target.clone(),
                mapping.replacement.clone(),
                mapping.rationale.clone(),
            ]);
        }
        for redirect in &canonical.terminal_redirects {
            keys.extend([redirect.target.clone(), redirect.terminal.clone()]);
        }
        let entries = keys
            .iter()
            .map(|key| identity_entry(key, layer_by_source[&key.source]))
            .collect();
        DerivedIndex {
            layers: canonical.layers,
            source_artifacts: Vec::new(),
            resolution: Box::new(crate::derived::ResolutionProjection {
                entries,
                canonical_redirects: canonical
                    .mappings
                    .iter()
                    .map(|mapping| CanonicalRedirect {
                        parent: mapping.target.clone(),
                        replacement: mapping.replacement.clone(),
                        rationale: mapping.rationale.clone(),
                    })
                    .collect(),
            }),
            index_entries: Vec::new(),
            field_tokens: Vec::new(),
            relationships: Vec::new(),
            live_decision_keys: Vec::new(),
            live_decision_paths: Vec::new(),
            portfolio_summary: serde_json::json!({}),
            scope_rows: Vec::new(),
        }
    }

    #[test]
    fn graph_store_isolated_from_v2_with_portable_directory_key() {
        let cache = Path::new("cache");
        let digest = "a".repeat(64);
        let generation = format!("sha256-v3:{digest}");
        assert_eq!(store_root(cache), cache.join("store/v2"));
        assert_eq!(graph_store_root(cache), cache.join("store/v3"));
        assert_eq!(
            graph_store_dir(cache, &generation),
            Some(cache.join("store/v3").join(digest))
        );
        assert_ne!(
            graph_store_dir(cache, &generation),
            Some(store_dir(cache, &generation))
        );
    }

    #[test]
    fn malformed_generation_cannot_escape_store_root() {
        let cache = Path::new("cache");
        assert!(graph_store_dir(cache, "sha256-v3:../../outside").is_none());
        assert!(graph_store_dir(cache, &format!("sha256-v3:{}", "A".repeat(64))).is_none());
    }

    #[test]
    fn graph_metadata_round_trips_in_canonical_order() {
        let metadata = graph_metadata_fixture();
        let encoded = encode_graph_metadata(&metadata).expect("encode graph metadata");
        let decoded = decode_graph_metadata(&encoded).expect("decode graph metadata");
        let mut expected = metadata;
        expected.layers.sort();
        assert_eq!(decoded, expected);

        let mut trailing = segment_payload(&encoded).unwrap().to_vec();
        trailing.push(0);
        assert!(decode_graph_metadata(&encode_segment(&trailing)).is_err());
    }

    #[test]
    fn graph_metadata_rejects_noncanonical_and_incomplete_rows() {
        let mut metadata = GraphStoreMetadata {
            generation: format!("sha256-v3:{}", "b".repeat(64)),
            layers: vec![CorpusLayer::local("acme/app")],
            mappings: Vec::new(),
            terminal_redirects: Vec::new(),
        };
        assert!(encode_graph_metadata(&metadata).is_ok());

        metadata.generation = "not-a-generation".into();
        assert!(encode_graph_metadata(&metadata).is_err());
        metadata.generation = format!("sha256-v3:{}", "b".repeat(64));
        metadata.layers.push(CorpusLayer::local("acme/app"));
        assert!(encode_graph_metadata(&metadata).is_err());
    }

    #[test]
    fn graph_store_rejects_corrupt_or_request_mismatched_metadata() {
        let cache = std::env::temp_dir().join(format!(
            "asdecided-graph-store-{}-{}",
            std::process::id(),
            temp_suffix()
        ));
        let metadata = graph_metadata_fixture();
        let derived = graph_derived_fixture(&metadata);
        assert!(write_graph_store(
            &cache,
            &metadata.generation,
            crate::derived::SCHEMA_VERSION,
            &derived,
            &metadata,
        ));
        assert!(open_store(
            &cache,
            &metadata.generation,
            crate::derived::SCHEMA_VERSION
        )
        .is_none());
        assert!(open_graph_store(
            &cache,
            &metadata.generation,
            crate::derived::SCHEMA_VERSION,
            &metadata,
        )
        .is_some());

        let mut request_mismatch = metadata.clone();
        request_mismatch.mappings.clear();
        request_mismatch.terminal_redirects.clear();
        assert!(open_graph_store(
            &cache,
            &metadata.generation,
            crate::derived::SCHEMA_VERSION,
            &request_mismatch,
        )
        .is_none());

        let directory = graph_store_dir(&cache, &metadata.generation).unwrap();
        let corrupt = encode_graph_metadata(&request_mismatch).unwrap();
        std::fs::write(directory.join(SEG_GRAPH), corrupt).unwrap();
        assert!(open_graph_store(
            &cache,
            &metadata.generation,
            crate::derived::SCHEMA_VERSION,
            &metadata,
        )
        .is_none());

        let mut wrong_derived = graph_derived_fixture(&metadata);
        wrong_derived.resolution.canonical_redirects.clear();
        assert!(!write_graph_store(
            &cache,
            &metadata.generation,
            crate::derived::SCHEMA_VERSION,
            &wrong_derived,
            &metadata,
        ));
        assert!(!write_graph_store(
            &cache,
            "not-a-generation",
            crate::derived::SCHEMA_VERSION,
            &derived,
            &metadata,
        ));
        let _ = std::fs::remove_dir_all(cache);
    }
}
