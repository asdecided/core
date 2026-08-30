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

use crate::derived::DerivedIndex;
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
pub const STORE_LAYOUT_VERSION: &str = "v1";

const SEG_HEADER: &str = "header.seg";
const SEG_ENTRIES: &str = "entries.seg";
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

const ALL_SEGMENTS: [&str; 12] = [
    SEG_HEADER,
    SEG_ENTRIES,
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
    cache_dir.join(STORE_DIRNAME).join(STORE_LAYOUT_VERSION)
}

pub fn store_dir(cache_dir: &Path, corpus_hash: &str) -> PathBuf {
    store_root(cache_dir).join(corpus_hash)
}

// ---------------------------------------------------------------------------
// Writer — one DerivedIndex -> a directory of segment files, atomically.
// ---------------------------------------------------------------------------

fn encode_segments(
    corpus_hash: &str,
    bundle_version: &str,
    derived: &DerivedIndex,
) -> Result<Vec<(&'static str, Vec<u8>)>, IndexFormatError> {
    let entries = &derived.index_entries;
    let field_tokens = &derived.field_tokens;

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
    // Casefolded identifier -> ascending docids (consecutive-dup guarded).
    let mut alias_docids: BTreeMap<String, Vec<u32>> = BTreeMap::new();

    for (docid, entry) in entries.iter().enumerate() {
        let docid = docid as u32;
        let fields = &field_tokens[docid as usize];
        let lengths: Vec<u64> = FIELDS
            .iter()
            .map(|name| fields.get(name).len() as u64)
            .collect();
        for (i, value) in lengths.iter().enumerate() {
            length_sums[i] += value;
        }

        for alias in &entry.aliases {
            let docids = alias_docids.entry(py_casefold(alias)).or_default();
            if docids.last() != Some(&docid) {
                docids.push(docid);
            }
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

    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::with_capacity(12);
    out.push((SEG_ENTRIES, encode_segment(&write_indexed(&entry_rows)?)));
    drop(entry_rows);
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

    let mut relationships = Writer::new();
    relationships.u32(derived.relationships.len() as u64)?;
    for rel in &derived.relationships {
        relationships.text(&rel.source_path)?;
        relationships.text(&rel.relationship)?;
        relationships.text(&rel.target)?;
        relationships.opt_text(rel.resolved_path.as_deref())?;
        relationships.opt_text(rel.issue.as_deref())?;
    }
    out.push((SEG_RELATIONSHIPS, encode_segment(&relationships.payload())));

    let mut live = Writer::new();
    live.text_list(&derived.live_decision_paths)?;
    out.push((SEG_LIVE, encode_segment(&live.payload())));

    let mut scope = Writer::new();
    scope.u32(derived.scope_rows.len() as u64)?;
    for row in &derived.scope_rows {
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
    let root = store_root(cache_dir);
    let final_dir = root.join(corpus_hash);
    if final_dir.is_dir() {
        // Content addressing: a same-hash store is byte-equivalent within one
        // format. Probe readability with the full open; replace when bad.
        if MmapIndexReader::open(&final_dir, corpus_hash, bundle_version).is_ok() {
            return true;
        }
        remove_tree(&final_dir);
    }
    let encode_started = crate::timing::start();
    let Ok(segments) = encode_segments(corpus_hash, bundle_version, derived) else {
        crate::timing::emit_since("store.encode", encode_started, &[("success", 0)]);
        return false;
    };
    crate::timing::emit_since(
        "store.encode",
        encode_started,
        &[("success", 1), ("segments", segments.len() as u64)],
    );
    let tmp = root.join(format!(".{corpus_hash}.tmp-{}", temp_suffix()));
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
        let mut reader = self.indexed(SEG_ENTRIES)?.row(docid)?;
        let id = reader.text()?;
        let artifact_type = reader.text()?;
        let title = reader.opt_text()?;
        let path = reader.text()?;
        let aliases = reader.text_list()?;
        let tags = reader.text_list()?;
        Ok(IndexEntry {
            key: None,
            artifact_path: None,
            origin: None,
            id,
            artifact_type,
            title,
            path,
            aliases,
            search_sections: Vec::new(),
            inbound_count: 0,
            tags,
        })
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
        let sections = self.read_sections(docid)?;
        Ok(IndexEntry {
            key: None,
            artifact_path: None,
            origin: None,
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

    pub fn relationships(&self) -> Result<Vec<Relationship>, IndexFormatError> {
        let mut reader = Reader::new(self.payload(SEG_RELATIONSHIPS));
        let count = reader.u32()?;
        let mut result = Vec::with_capacity(count.min(1 << 20) as usize);
        for _ in 0..count {
            result.push(Relationship {
                source_artifact: None,
                source_path: reader.text()?,
                relationship: reader.text()?,
                target: reader.text()?,
                resolved_artifact: None,
                resolved_path: reader.opt_text()?,
                issue: reader.opt_text()?,
            });
        }
        Ok(result)
    }

    pub fn live_decision_paths(&self) -> Result<Vec<String>, IndexFormatError> {
        Reader::new(self.payload(SEG_LIVE)).text_list()
    }

    pub fn scope_rows(&self) -> Result<Vec<crate::retrieve::ScopeRow>, IndexFormatError> {
        let mut reader = Reader::new(self.payload(SEG_SCOPE));
        let count = reader.u32()?;
        let mut rows = Vec::with_capacity(count.min(1 << 20) as usize);
        for _ in 0..count {
            rows.push(crate::retrieve::ScopeRow {
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
