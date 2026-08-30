//! Compound deterministic grounding retrieval (`decided retrieve`, ADR-113) — a
//! port of `src/asdecided/services/retrieve.py` and `src/asdecided/services/scope.py` /
//! `scope_paths.py` (the scope-binding channel) from the
//! `grounding-retrieval-surface` branch (oracle `0.1.dev55+gf2091befd`).
//! The ADR-033 response budget (serialization + truncation) lives in
//! `crate::budget`.
//!
//! Landmines reproduced here:
//! - Excerpts are Python character slices (`content[:share]`), over the file's
//!   text read with universal newlines (`\r\n`/`\r` → `\n`); an unreadable or
//!   non-UTF-8 file contributes an empty excerpt.
//! - Payload/provenance key ORDER is Python dict insertion order: items are
//!   `id, type, title, status, path, excerpt, provenance`; provenance keys in
//!   first-set order (`channels` first, then whichever of `matching_entry`,
//!   `superseded`, `evidence` was set first).
//! - Scope binding matches `scope._entry_covers`: segment-aware globs compiled
//!   exactly like `_glob_to_regex` (`*`/`?` within a segment, `**` across,
//!   `**/` zero-or-more whole segments, `[...]` classes, `.`-collapse and
//!   `..`-rejection in path normalisation).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::budget::py_slice_to;
use crate::corpus::{ArtifactKey, ArtifactOrigin, ArtifactPath};
use crate::identity::artifact_identifier;
use crate::pycompat::{py_casefold, py_strip, read_text_universal};
use crate::relationships::{
    classify_scope_entry, corpus_items, extract_relationships_full, normalized_scope_path,
    relationships_from_corpus, CorpusItem, Relationship,
};
use crate::resolve::{
    artifact_status, index_from_items, is_live_decision, is_retired_status, search_index,
    IndexEntry, SearchResult,
};

// Defaults pinned by the grounding-retrieval-surface design.
pub const DEFAULT_TOP_K: i64 = 5;

const SUPERSEDES: &str = "supersedes";
const DECISION_TYPE: &str = "decision";

// Discovery channel names on the wire (pinned by the design).
const CHANNEL_KEYWORD: &str = "keyword";
const CHANNEL_SCOPE: &str = "scope";
const CHANNEL_SUPERSEDES: &str = "supersedes";

// ---------------------------------------------------------------------------
// scope_paths.py — path normalisation, repository root (entry classification
// is shared with relationships.rs: `classify_scope_entry` /
// `normalized_scope_path`)
// ---------------------------------------------------------------------------

/// `PurePosixPath(text).parts` minus any root marker: empty and `.` segments
/// collapse; the root marker (when the text is absolute) is returned apart.
fn pure_posix_parts(text: &str) -> (Option<&'static str>, Vec<String>) {
    let root = if text.starts_with('/') {
        // POSIX: exactly two leading slashes are the special `//` root;
        // one or three-plus collapse to `/`.
        if text.starts_with("//") && !text.starts_with("///") {
            Some("//")
        } else {
            Some("/")
        }
    } else {
        None
    };
    let parts = text
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .map(str::to_string)
        .collect();
    (root, parts)
}

/// `repository_root(directory)` — nearest ancestor holding `.decided/config.yaml`,
/// else the resolved directory itself.
fn repository_root(directory: &str) -> PathBuf {
    let resolved = Path::new(directory).canonicalize().unwrap_or_else(|_| {
        // Python resolve() is non-strict; absolutize against the cwd.
        std::env::current_dir()
            .map(|c| c.join(directory))
            .unwrap_or_else(|_| PathBuf::from(directory))
    });
    for candidate in resolved.ancestors() {
        if candidate.join(".decided").join("config.yaml").is_file() {
            return candidate.to_path_buf();
        }
    }
    resolved
}

// ---------------------------------------------------------------------------
// scope.py — the `_glob_to_regex` glob matcher (compiled, not regex-backed)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum ClassItem {
    Ch(char),
    Range(char, char),
    Digit,
    NonDigit,
    Word,
    NonWord,
    Space,
    NonSpace,
}

#[derive(Debug, Clone)]
enum GlobTok {
    Lit(char),
    /// `[^/]*`
    Star,
    /// `[^/]`
    Q,
    /// `(?:[^/]+/)*`
    SegStar,
    /// `.*` (any char except `\n`)
    DotStar,
    Class {
        negated: bool,
        items: Vec<ClassItem>,
    },
}

/// Compile the pattern exactly as `_glob_to_regex` builds its regex.
fn compile_glob(pattern: &str) -> Vec<GlobTok> {
    let chars: Vec<char> = pattern.chars().collect();
    let n = chars.len();
    let mut out: Vec<GlobTok> = Vec::new();
    let mut i = 0usize;
    while i < n {
        let c = chars[i];
        if c == '*' {
            if i + 1 < n && chars[i + 1] == '*' {
                i += 2;
                if i < n && chars[i] == '/' {
                    i += 1;
                    out.push(GlobTok::SegStar);
                } else {
                    out.push(GlobTok::DotStar);
                }
                continue;
            }
            out.push(GlobTok::Star);
        } else if c == '?' {
            out.push(GlobTok::Q);
        } else if c == '[' {
            let mut j = i + 1;
            if j < n && (chars[j] == '!' || chars[j] == '^') {
                j += 1;
            }
            if j < n && chars[j] == ']' {
                j += 1;
            }
            while j < n && chars[j] != ']' {
                j += 1;
            }
            if j >= n {
                out.push(GlobTok::Lit('[')); // unterminated class → literal '['
            } else {
                let inner: Vec<char> = chars[i + 1..j].to_vec();
                let (negated, body) = match inner.first() {
                    Some('!') | Some('^') => (true, &inner[1..]),
                    _ => (false, &inner[..]),
                };
                out.push(GlobTok::Class {
                    negated,
                    items: parse_class_items(body),
                });
                i = j + 1;
                continue;
            }
        } else {
            out.push(GlobTok::Lit(c));
        }
        i += 1;
    }
    out
}

/// Parse a regex character-class body (`a-z`, escapes, shorthands).
fn parse_class_items(body: &[char]) -> Vec<ClassItem> {
    let mut items: Vec<ClassItem> = Vec::new();
    let mut k = 0usize;
    let n = body.len();
    while k < n {
        // Resolve one class atom (an escaped char/shorthand or a literal).
        let (atom, used, shorthand) = if body[k] == '\\' && k + 1 < n {
            let e = body[k + 1];
            let sh = match e {
                'd' => Some(ClassItem::Digit),
                'D' => Some(ClassItem::NonDigit),
                'w' => Some(ClassItem::Word),
                'W' => Some(ClassItem::NonWord),
                's' => Some(ClassItem::Space),
                'S' => Some(ClassItem::NonSpace),
                _ => None,
            };
            (e, 2usize, sh)
        } else {
            (body[k], 1usize, None)
        };
        if let Some(sh) = shorthand {
            items.push(sh);
            k += used;
            continue;
        }
        // Range: atom '-' atom (the '-' not last in the class body).
        if k + used < n && body[k + used] == '-' && k + used + 1 < n {
            let mut m = k + used + 1;
            let hi = if body[m] == '\\' && m + 1 < n {
                m += 1;
                body[m]
            } else {
                body[m]
            };
            items.push(ClassItem::Range(atom, hi));
            k = m + 1;
            continue;
        }
        items.push(ClassItem::Ch(atom));
        k += used;
    }
    items
}

fn class_matches(negated: bool, items: &[ClassItem], c: char) -> bool {
    let hit = items.iter().any(|item| match item {
        ClassItem::Ch(x) => c == *x,
        ClassItem::Range(lo, hi) => (*lo..=*hi).contains(&c),
        ClassItem::Digit => crate::pycompat::is_re_digit(c),
        ClassItem::NonDigit => !crate::pycompat::is_re_digit(c),
        ClassItem::Word => crate::pycompat::is_re_word(c),
        ClassItem::NonWord => !crate::pycompat::is_re_word(c),
        ClassItem::Space => py_re_space(c),
        ClassItem::NonSpace => !py_re_space(c),
    });
    hit != negated
}

/// Python `re` `\s` over str patterns.
fn py_re_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c' | '\u{1c}'..='\u{1f}' | '\u{85}')
        || crate::pycompat::py_is_space(c)
}

/// Backtracking matcher — boolean-equivalent to `re.match(regex + r"\Z", s)`.
fn glob_match_at(toks: &[GlobTok], s: &[char]) -> bool {
    let Some(tok) = toks.first() else {
        return s.is_empty();
    };
    let rest = &toks[1..];
    match tok {
        GlobTok::Lit(c) => s.first() == Some(c) && glob_match_at(rest, &s[1..]),
        GlobTok::Q => s.first().is_some_and(|&c| c != '/') && glob_match_at(rest, &s[1..]),
        GlobTok::Star => {
            let limit = s.iter().take_while(|&&c| c != '/').count();
            (0..=limit).any(|k| glob_match_at(rest, &s[k..]))
        }
        GlobTok::DotStar => {
            let limit = s.iter().take_while(|&&c| c != '\n').count();
            (0..=limit).any(|k| glob_match_at(rest, &s[k..]))
        }
        GlobTok::SegStar => {
            // zero segments:
            if glob_match_at(rest, s) {
                return true;
            }
            // one whole segment `[^/]+/`, then this token again:
            let mut i = 0usize;
            while i < s.len() && s[i] != '/' {
                i += 1;
            }
            i > 0 && i < s.len() && glob_match_at(toks, &s[i + 1..])
        }
        GlobTok::Class { negated, items } => s
            .first()
            .is_some_and(|&c| class_matches(*negated, items, c))
            && glob_match_at(rest, &s[1..]),
    }
}

/// `_entry_covers(entry, query)`.
fn entry_covers(entry: &str, query: &str) -> bool {
    match classify_scope_entry(entry) {
        "component" => false,
        "glob" => {
            let toks = compile_glob(py_strip(entry));
            let q: Vec<char> = query.chars().collect();
            glob_match_at(&toks, &q)
        }
        _ => match normalized_scope_path(entry) {
            None => false,
            Some(normalized) => {
                query == normalized || query.starts_with(&format!("{normalized}/"))
            }
        },
    }
}

/// `_normalize_query(path, root)` — POSIX repo-relative form, or None.
fn normalize_query(path: &str, root: &Path) -> Option<String> {
    let text = py_strip(path);
    if text.is_empty() {
        return None;
    }
    let (cand_root, mut cand_parts) = pure_posix_parts(text);
    if cand_root.is_some() {
        // PurePosixPath.relative_to(root.as_posix()) — parts-prefix check;
        // a ValueError (differing root marker, or not nested) → None.
        let root_posix = root.to_string_lossy().replace('\\', "/");
        let (root_marker, root_parts) = pure_posix_parts(&root_posix);
        if cand_root != root_marker
            || cand_parts.len() < root_parts.len()
            || cand_parts[..root_parts.len()] != root_parts[..]
        {
            return None; // outside the repository
        }
        cand_parts = cand_parts[root_parts.len()..].to_vec();
    }
    let mut parts: Vec<String> = Vec::new();
    for part in cand_parts {
        if part == ".." {
            return None;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

// ---------------------------------------------------------------------------
// derived_cache.py — scope rows + governing_decisions
// ---------------------------------------------------------------------------

/// One live decision's declared `## Applies To` scope (`ScopeRow`).
#[derive(Clone)]
pub struct ScopeRow {
    /// Absent only when reconstructed from the frozen v1 persistent store.
    pub key: Option<ArtifactKey>,
    /// Absent only when reconstructed from the frozen v1 persistent store.
    pub artifact_path: Option<ArtifactPath>,
    /// Absent only when reconstructed from the frozen v1 persistent store.
    pub origin: Option<ArtifactOrigin>,
    pub id: String,
    pub title: String,
    pub status: String,
    pub path: String,
    pub scope_entries: Vec<String>,
}

/// `_scope_rows_from_corpus(entries)` — live decisions with declared scope.
pub fn scope_rows_from_items(items: &[CorpusItem]) -> Vec<ScopeRow> {
    let mut rows = Vec::new();
    for item in items {
        let Some(spec) = item.spec else { continue };
        if spec.name != DECISION_TYPE || !is_live_decision(&item.artifact) {
            continue;
        }
        // SCOPE_SECTIONS = ("applies to",) → snake key "applies_to".
        let declared: Vec<String> = extract_relationships_full(&item.artifact, spec)
            .into_iter()
            .filter(|(section, _)| section == "applies_to")
            .flat_map(|(_, refs)| refs)
            .collect();
        if declared.is_empty() {
            continue;
        }
        rows.push(ScopeRow {
            key: Some(item.key.clone()),
            artifact_path: Some(item.artifact_path.clone()),
            origin: Some(item.origin.clone()),
            id: artifact_identifier(&item.artifact, Some(spec), &item.path),
            title: item.artifact.product.title.clone().unwrap_or_default(),
            status: artifact_status(&item.artifact),
            path: item.path.clone(),
            scope_entries: declared,
        });
    }
    rows
}

/// One governing decision (`GoverningDecision` — the fields retrieve and
/// `decided decisions-for` read).
pub struct GoverningDecision {
    pub key: Option<ArtifactKey>,
    pub artifact_path: Option<ArtifactPath>,
    pub origin: Option<ArtifactOrigin>,
    pub id: String,
    pub title: String,
    pub status: String,
    pub path: String,
    pub matching_entry: String,
}

/// `governing_decisions(scope_rows, directory, path).decisions`.
fn governing_decisions(rows: &[ScopeRow], directory: &str, path: &str) -> Vec<GoverningDecision> {
    let root = repository_root(directory);
    let Some(query) = normalize_query(path, &root) else {
        return Vec::new();
    };
    let mut matches: Vec<GoverningDecision> = Vec::new();
    for row in rows {
        for declared in &row.scope_entries {
            if entry_covers(declared, &query) {
                matches.push(GoverningDecision {
                    key: row.key.clone(),
                    artifact_path: row.artifact_path.clone(),
                    origin: row.origin.clone(),
                    id: row.id.clone(),
                    title: row.title.clone(),
                    status: row.status.clone(),
                    path: row.path.clone(),
                    matching_entry: declared.clone(),
                });
                break;
            }
        }
    }
    let mut sources: Vec<&str> = matches
        .iter()
        .filter_map(|decision| decision.origin.as_ref().map(|origin| origin.source.as_str()))
        .collect();
    sources.sort_unstable();
    sources.dedup();
    let federated = sources.len() > 1;
    matches.sort_by(|a, b| {
        py_casefold(&a.id)
            .cmp(&py_casefold(&b.id))
            .then_with(|| {
                if federated {
                    a.artifact_path.cmp(&b.artifact_path)
                } else {
                    a.path.cmp(&b.path)
                }
            })
            .then_with(|| a.path.cmp(&b.path))
    });
    matches
}

/// `ScopeLookupResult` — the decisions governing a queried path. `query` is
/// the POSIX repo-relative form when the path lies inside the repository,
/// else the raw stripped input; an outside-repository or ungoverned path is
/// a valid empty answer, never an error (REQ-004).
pub struct ScopeLookupResult {
    pub query: String,
    pub in_repository: bool,
    pub decisions: Vec<GoverningDecision>,
}

/// `decided.services.scope.decisions_for_path(directory, path, recursive)` — the
/// CLI face of the scope lookup. Byte-identical to the derived-cache path
/// (`governing_decisions`) for the same corpus and path; `recursive` threads
/// the CLI's `--top-level` through the corpus walk (the MCP `find_decisions`
/// path mode always walks recursively).
pub fn decisions_for_path(directory: &str, path: &str, recursive: bool) -> ScopeLookupResult {
    let root = repository_root(directory);
    match normalize_query(path, &root) {
        None => ScopeLookupResult {
            query: py_strip(path).to_string(),
            in_repository: false,
            decisions: Vec::new(),
        },
        Some(query) => {
            let items = corpus_items(directory, recursive);
            let rows = scope_rows_from_items(&items);
            ScopeLookupResult {
                query,
                in_repository: true,
                decisions: governing_decisions(&rows, directory, path),
            }
        }
    }
}

/// `ScopeLookupResult.to_dict()` — `{schema_version, query, in_repository,
/// decisions}` in Python dict insertion order.
pub fn scope_lookup_value(result: &ScopeLookupResult) -> Value {
    scope_lookup_value_with_origin(result, false)
}

/// Federated scope payload. Origin is opt-in so a repository without a
/// manifest retains the released JSON shape byte for byte.
pub fn scope_lookup_value_with_origin(
    result: &ScopeLookupResult,
    include_origin: bool,
) -> Value {
    let mut payload = Map::new();
    payload.insert("schema_version".to_string(), json!("1"));
    payload.insert("query".to_string(), json!(result.query));
    payload.insert("in_repository".to_string(), json!(result.in_repository));
    let decisions: Vec<Value> = result
        .decisions
        .iter()
        .map(|d| {
            let mut m = Map::new();
            m.insert("id".to_string(), json!(d.id));
            m.insert("title".to_string(), json!(d.title));
            m.insert("status".to_string(), json!(d.status));
            m.insert("path".to_string(), json!(d.path));
            m.insert("matching_entry".to_string(), json!(d.matching_entry));
            if include_origin {
                if let Some(origin) = &d.origin {
                    m.insert(
                        "provenance".to_string(),
                        crate::output::artifact_origin_value(origin),
                    );
                }
            }
            Value::Object(m)
        })
        .collect();
    payload.insert("decisions".to_string(), Value::Array(decisions));
    Value::Object(payload)
}

pub fn scope_lookup_value_with_composed(
    result: &ScopeLookupResult,
    corpus: &crate::composition::ComposedCorpus,
) -> Value {
    let mut payload = scope_lookup_value_with_origin(result, true);
    if let Some(decisions) = payload
        .get_mut("decisions")
        .and_then(Value::as_array_mut)
    {
        for (value, decision) in decisions.iter_mut().zip(&result.decisions) {
            let Some(provenance) = decision
                .key
                .as_ref()
                .and_then(|key| corpus.provenance_for(key))
            else {
                continue;
            };
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "provenance".to_string(),
                    crate::output::composed_provenance_value(&provenance),
                );
            }
        }
    }
    payload
}

/// `decisions_for_path` over ALREADY-DERIVED scope rows (ADR-103): the
/// read-model arm of the MCP `find_decisions` path mode, byte-identical to
/// the fresh walk for the same corpus state.
pub fn decisions_for_path_with_rows(
    rows: &[ScopeRow],
    directory: &str,
    path: &str,
) -> ScopeLookupResult {
    let root = repository_root(directory);
    match normalize_query(path, &root) {
        None => ScopeLookupResult {
            query: py_strip(path).to_string(),
            in_repository: false,
            decisions: Vec::new(),
        },
        Some(query) => ScopeLookupResult {
            query,
            in_repository: true,
            decisions: governing_decisions(rows, directory, path),
        },
    }
}

/// `find_decisions` path mode (MCP surface): the `ScopeLookupResult.to_dict()`
/// payload — `{schema_version, query, in_repository, decisions}` — for the live
/// decisions whose declared `## Applies To` scope governs `path`. Additive
/// wrapper over the same scope internals `retrieve_grounding` uses
/// (`scope_rows_from_items` / `normalize_query` / `entry_covers`), byte-identical
/// to `decided.services.derived_cache.governing_decisions(...).to_dict()`.
pub fn find_decisions_path_payload(directory: &str, path: &str) -> Value {
    scope_lookup_value(&decisions_for_path(directory, path, true))
}

// ---------------------------------------------------------------------------
// retrieve.py — the compound grounding payload
// ---------------------------------------------------------------------------

/// `_successor_map(relationships)` — retired target path → sorted superseding
/// source paths (resolved `supersedes` edges only).
fn successor_map(relationships: &[Relationship]) -> HashMap<String, Vec<String>> {
    let mut by_target: HashMap<String, Vec<String>> = HashMap::new();
    for rel in relationships {
        if rel.relationship == SUPERSEDES {
            if let Some(target) = &rel.resolved_path {
                by_target
                    .entry(target.clone())
                    .or_default()
                    .push(rel.source_path.clone());
            }
        }
    }
    for sources in by_target.values_mut() {
        sources.sort();
        sources.dedup();
    }
    by_target
}

/// `_live_successors(path, by_target, is_retired, visited)`.
fn live_successors(
    path: &str,
    by_target: &HashMap<String, Vec<String>>,
    is_retired: &dyn Fn(&str) -> bool,
    visited: &mut std::collections::HashSet<String>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(sources) = by_target.get(path) else {
        return out;
    };
    for source in sources {
        if visited.contains(source) {
            continue;
        }
        visited.insert(source.clone());
        if is_retired(source) {
            out.extend(live_successors(source, by_target, is_retired, visited));
        } else {
            out.push(source.clone());
        }
    }
    out
}

/// One in-progress item (Python's per-path dict + its provenance dict).
struct ItemBuilder {
    id: String,
    item_type: String,
    title: Option<String>,
    status: String,
    path: String,
    /// Provenance keys in insertion order.
    provenance: Map<String, Value>,
}

struct ComposedItemBuilder {
    key: ArtifactKey,
    id: String,
    item_type: String,
    title: Option<String>,
    status: String,
    path: String,
    provenance: Map<String, Value>,
}

#[allow(clippy::too_many_arguments)]
fn add_composed_item(
    items: &mut Vec<ComposedItemBuilder>,
    index_of: &mut HashMap<ArtifactKey, usize>,
    key: &ArtifactKey,
    path: &ArtifactPath,
    origin: &ArtifactOrigin,
    channel: &str,
    item_id: &str,
    item_type: &str,
    title: Option<&str>,
    status: &str,
    matching_entry: Option<&str>,
    superseded: Option<&str>,
    evidence: Option<Value>,
) {
    let index = match index_of.get(key) {
        Some(index) => *index,
        None => {
            let mut provenance = Map::new();
            provenance.insert("channels".to_string(), json!([]));
            provenance.insert("source".to_string(), json!(origin.source));
            provenance.insert("layer".to_string(), json!(origin.layer.as_str()));
            if let Some(pin) = &origin.pin {
                provenance.insert("pin".to_string(), json!(pin));
            }
            items.push(ComposedItemBuilder {
                key: key.clone(),
                id: item_id.to_string(),
                item_type: item_type.to_string(),
                title: title.map(str::to_string),
                status: status.to_string(),
                path: path.relative_path.clone(),
                provenance,
            });
            index_of.insert(key.clone(), items.len() - 1);
            items.len() - 1
        }
    };
    let provenance = &mut items[index].provenance;
    let channels = provenance
        .get_mut("channels")
        .and_then(Value::as_array_mut)
        .expect("channels array");
    if !channels.iter().any(|value| value.as_str() == Some(channel)) {
        channels.push(json!(channel));
    }
    if let Some(entry) = matching_entry {
        provenance
            .entry("matching_entry".to_string())
            .or_insert_with(|| json!(entry));
    }
    if let Some(replaced_id) = superseded {
        let replaced = provenance
            .entry("superseded".to_string())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("superseded array");
        if !replaced
            .iter()
            .any(|value| value.as_str() == Some(replaced_id))
        {
            replaced.push(json!(replaced_id));
        }
    }
    if let Some(evidence) = evidence {
        provenance
            .entry("evidence".to_string())
            .or_insert(evidence);
    }
}

fn composed_successor_map(
    relationships: &[Relationship],
) -> HashMap<ArtifactPath, Vec<ArtifactPath>> {
    let mut successors: HashMap<ArtifactPath, Vec<ArtifactPath>> = HashMap::new();
    for relationship in relationships {
        if relationship.relationship != SUPERSEDES {
            continue;
        }
        let (Some(source), Some(target)) = (
            relationship.source_artifact.as_ref(),
            relationship.resolved_artifact.as_ref(),
        ) else {
            continue;
        };
        successors
            .entry(target.clone())
            .or_default()
            .push(source.clone());
    }
    for paths in successors.values_mut() {
        paths.sort();
        paths.dedup();
    }
    successors
}

fn composed_live_successors(
    path: &ArtifactPath,
    successors: &HashMap<ArtifactPath, Vec<ArtifactPath>>,
    is_retired: &dyn Fn(&ArtifactPath) -> bool,
    visited: &mut HashSet<ArtifactPath>,
) -> Vec<ArtifactPath> {
    let mut result = Vec::new();
    let Some(paths) = successors.get(path) else {
        return result;
    };
    for successor in paths {
        if !visited.insert(successor.clone()) {
            continue;
        }
        if is_retired(successor) {
            result.extend(composed_live_successors(
                successor,
                successors,
                is_retired,
                visited,
            ));
        } else {
            result.push(successor.clone());
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn add_item(
    items: &mut Vec<ItemBuilder>,
    index_of: &mut HashMap<String, usize>,
    path: &str,
    channel: &str,
    item_id: &str,
    item_type: &str,
    title: Option<&str>,
    status: &str,
    matching_entry: Option<&str>,
    superseded: Option<&str>,
    evidence: Option<Value>,
) {
    let idx = match index_of.get(path) {
        Some(&i) => i,
        None => {
            let mut provenance = Map::new();
            provenance.insert("channels".to_string(), json!([]));
            items.push(ItemBuilder {
                id: item_id.to_string(),
                item_type: item_type.to_string(),
                title: title.map(str::to_string),
                status: status.to_string(),
                path: path.to_string(),
                provenance,
            });
            index_of.insert(path.to_string(), items.len() - 1);
            items.len() - 1
        }
    };
    let provenance = &mut items[idx].provenance;
    {
        let channels = provenance
            .get_mut("channels")
            .and_then(Value::as_array_mut)
            .expect("channels array");
        if !channels.iter().any(|c| c.as_str() == Some(channel)) {
            channels.push(json!(channel));
        }
    }
    if let Some(entry) = matching_entry {
        if !provenance.contains_key("matching_entry") {
            provenance.insert("matching_entry".to_string(), json!(entry));
        }
    }
    if let Some(replaced_id) = superseded {
        if !provenance.contains_key("superseded") {
            provenance.insert("superseded".to_string(), json!([]));
        }
        let replaced = provenance
            .get_mut("superseded")
            .and_then(Value::as_array_mut)
            .expect("superseded array");
        if !replaced.iter().any(|r| r.as_str() == Some(replaced_id)) {
            replaced.push(json!(replaced_id));
        }
    }
    if let Some(ev) = evidence {
        if !provenance.contains_key("evidence") {
            provenance.insert("evidence".to_string(), ev);
        }
    }
}

/// `retrieve_grounding(directory, task, scope, top_k, budget, live_only)` —
/// the contract-shaped payload, pre-serialization (`budget::serialize` caps
/// it).
pub fn retrieve_grounding(
    directory: &str,
    task: &str,
    scope: Option<&str>,
    top_k: i64,
    budget: i64,
    live_only: bool,
) -> Value {
    let top_k = top_k.max(1);
    let corpus = corpus_items(directory, true);
    let entries: Vec<IndexEntry> = index_from_items(&corpus);
    let entry_by_path: HashMap<&str, &IndexEntry> =
        entries.iter().map(|e| (e.path.as_str(), e)).collect();
    // Memoised per-call status reader: every queried path is a corpus path, so
    // re-parsing its bytes yields exactly the already-parsed artifact.
    let status_by_path: HashMap<&str, String> = corpus
        .iter()
        .map(|item| (item.path.as_str(), artifact_status(&item.artifact)))
        .collect();
    let status_of = |path: &str| -> String {
        match status_by_path.get(path) {
            Some(s) => s.clone(),
            // Not part of the walked corpus: parse fresh, "" when unreadable.
            None => {
                if Path::new(path).is_file() {
                    artifact_status(&crate::parse::parse_file(path))
                } else {
                    String::new()
                }
            }
        }
    };
    let keyword = search_index(&entries, task, None, &[]);
    let relationships = relationships_from_corpus(&corpus);
    let scope_rows = scope_rows_from_items(&corpus);
    retrieve_grounding_from_parts(
        directory,
        task,
        scope,
        top_k,
        budget,
        live_only,
        keyword,
        &scope_rows,
        &relationships,
        |path| entry_by_path.get(path).map(|entry| (*entry).clone()),
        status_of,
    )
}

/// Grounding over the one verified composed snapshot. Identity, deduplication,
/// successor traversal, excerpts, and provenance are all source-aware; no
/// inherited path is reopened after verification.
pub fn retrieve_grounding_from_composed(
    directory: &str,
    task: &str,
    scope: Option<&str>,
    top_k: i64,
    budget: i64,
    live_only: bool,
    corpus: &crate::composition::ComposedCorpus,
) -> Value {
    let top_k = top_k.max(1);
    let effective: Vec<_> = corpus.effective().cloned().collect();
    let entries = corpus.effective_index();
    let keyword = search_index(&entries, task, None, &[]);
    let scope_rows = scope_rows_from_items(&effective);
    let relationships = corpus.relationships();
    let by_path: HashMap<ArtifactPath, &CorpusItem> = effective
        .iter()
        .map(|item| (item.artifact_path.clone(), item))
        .collect();
    let is_retired = |path: &ArtifactPath| -> bool {
        by_path.get(path).is_some_and(|item| {
            let artifact_type = item
                .spec
                .map(|spec| spec.name.as_str())
                .unwrap_or("unknown");
            is_retired_status(artifact_type, &artifact_status(&item.artifact))
        })
    };

    let mut items = Vec::new();
    let mut index_of = HashMap::new();
    let scope = scope.filter(|value| !value.is_empty());
    if let Some(scope_path) = scope {
        for governing in governing_decisions(&scope_rows, directory, scope_path) {
            let (Some(key), Some(path), Some(origin)) = (
                governing.key.as_ref(),
                governing.artifact_path.as_ref(),
                governing.origin.as_ref(),
            ) else {
                continue;
            };
            add_composed_item(
                &mut items,
                &mut index_of,
                key,
                path,
                origin,
                CHANNEL_SCOPE,
                &governing.id,
                DECISION_TYPE,
                (!governing.title.is_empty()).then_some(governing.title.as_str()),
                &governing.status,
                Some(&governing.matching_entry),
                None,
                None,
            );
        }
    }

    let successors = if live_only {
        composed_successor_map(&relationships)
    } else {
        Default::default()
    };
    for matched in &keyword.matches {
        let (Some(key), Some(path), Some(origin)) = (
            matched.key.as_ref(),
            matched.artifact_path.as_ref(),
            matched.origin.as_ref(),
        ) else {
            continue;
        };
        if live_only && is_retired(path) {
            let mut visited = HashSet::from([path.clone()]);
            for successor_path in
                composed_live_successors(path, &successors, &is_retired, &mut visited)
            {
                let Some(successor) = by_path.get(&successor_path) else {
                    continue;
                };
                let artifact_type = successor
                    .spec
                    .map(|spec| spec.name.as_str())
                    .unwrap_or("unknown");
                add_composed_item(
                    &mut items,
                    &mut index_of,
                    &successor.key,
                    &successor.artifact_path,
                    &successor.origin,
                    CHANNEL_SUPERSEDES,
                    &successor.key.canonical_id,
                    artifact_type,
                    successor.artifact.product.title.as_deref(),
                    &artifact_status(&successor.artifact),
                    None,
                    Some(&matched.id),
                    None,
                );
            }
            continue;
        }
        add_composed_item(
            &mut items,
            &mut index_of,
            key,
            path,
            origin,
            CHANNEL_KEYWORD,
            &matched.id,
            &matched.artifact_type,
            matched.title.as_deref(),
            by_path
                .get(path)
                .map(|item| artifact_status(&item.artifact))
                .unwrap_or_default()
                .as_str(),
            None,
            None,
            matched.evidence.as_ref().map(crate::output::evidence_value),
        );
    }

    items.truncate((top_k as usize).min(items.len()));
    let share = if items.is_empty() {
        0
    } else {
        budget.div_euclid((top_k.min(items.len() as i64)).max(1))
    };
    let shaped: Vec<Value> = items
        .into_iter()
        .map(|mut item| {
            let content = corpus
                .content(&item.key)
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .map(|text| text.replace("\r\n", "\n").replace('\r', "\n"))
                .unwrap_or_default();
            if let Some(overrides) = corpus
                .provenance_for(&item.key)
                .and_then(|provenance| {
                    crate::output::composed_provenance_value(&provenance)
                        .get("overrides")
                        .cloned()
                })
            {
                item.provenance.insert("overrides".to_string(), overrides);
            }
            let mut value = Map::new();
            value.insert("id".to_string(), json!(item.id));
            value.insert("type".to_string(), json!(item.item_type));
            value.insert("title".to_string(), json!(item.title));
            value.insert("status".to_string(), json!(item.status));
            value.insert("path".to_string(), json!(item.path));
            value.insert("excerpt".to_string(), json!(py_slice_to(&content, share)));
            value.insert("provenance".to_string(), Value::Object(item.provenance));
            Value::Object(value)
        })
        .collect();
    let mut payload = Map::new();
    payload.insert("schema_version".to_string(), json!("1"));
    payload.insert("task".to_string(), json!(task));
    if let Some(scope_path) = scope {
        payload.insert("scope".to_string(), json!(scope_path));
    }
    payload.insert("live_only".to_string(), json!(live_only));
    payload.insert("items".to_string(), Value::Array(shaped));
    Value::Object(payload)
}

/// Grounding over an already-derived mutation-window snapshot. Only matched,
/// governing, and successor paths are read from disk for status/excerpts; the
/// corpus itself is never walked or parsed again.
pub fn retrieve_grounding_from_derived(
    directory: &str,
    task: &str,
    scope: Option<&str>,
    top_k: i64,
    budget: i64,
    live_only: bool,
    derived: &crate::derived::DerivedIndex,
) -> Value {
    let keyword = search_index(&derived.index_entries, task, None, &[]);
    let entry_by_path: HashMap<&str, &IndexEntry> = derived
        .index_entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let status_cache = std::cell::RefCell::new(HashMap::<String, String>::new());
    let status_of = |path: &str| {
        entry_by_path
            .get(path)
            .map(|entry| status_from_entry(entry))
            .unwrap_or_else(|| cached_status(&status_cache, path))
    };
    retrieve_grounding_from_parts(
        directory,
        task,
        scope,
        top_k,
        budget,
        live_only,
        keyword,
        &derived.scope_rows,
        &derived.relationships,
        |path| entry_by_path.get(path).map(|entry| (*entry).clone()),
        status_of,
    )
}

/// Grounding over the immutable mmap store. Search uses postings, path lookup
/// uses the persisted path map, and only the relationship/scope projections
/// required by grounding are decoded.
pub fn retrieve_grounding_from_store(
    directory: &str,
    task: &str,
    scope: Option<&str>,
    top_k: i64,
    budget: i64,
    live_only: bool,
    reader: &crate::index_store::MmapIndexReader,
) -> Value {
    let search_started = crate::timing::start();
    let keyword = crate::read_model::store_search(reader, task, None, &[], false);
    crate::timing::emit_since(
        "grounding.search",
        search_started,
        &[("matches", keyword.matches.len() as u64)],
    );
    let decode_started = crate::timing::start();
    let scope_rows = if scope.is_some_and(|value| !value.is_empty()) {
        reader.scope_rows().unwrap_or_default()
    } else {
        Vec::new()
    };
    let relationships = if live_only {
        reader.relationships().unwrap_or_default()
    } else {
        Vec::new()
    };
    crate::timing::emit_since(
        "grounding.projections",
        decode_started,
        &[
            ("scope_rows", scope_rows.len() as u64),
            ("relationships", relationships.len() as u64),
        ],
    );
    let status_cache = std::cell::RefCell::new(HashMap::<String, String>::new());
    let status_of = |path: &str| {
        reader
            .docid_for_path(path)
            .ok()
            .flatten()
            .and_then(|docid| reader.entry_status(docid).ok())
            .unwrap_or_else(|| cached_status(&status_cache, path))
    };
    retrieve_grounding_from_parts(
        directory,
        task,
        scope,
        top_k,
        budget,
        live_only,
        keyword,
        &scope_rows,
        &relationships,
        |path| {
            reader
                .docid_for_path(path)
                .ok()
                .flatten()
                .and_then(|docid| reader.full_entry(docid).ok())
        },
        status_of,
    )
}

fn cached_status(
    cache: &std::cell::RefCell<HashMap<String, String>>,
    path: &str,
) -> String {
    if let Some(status) = cache.borrow().get(path) {
        return status.clone();
    }
    let status = if Path::new(path).is_file() {
        artifact_status(&crate::parse::parse_file(path))
    } else {
        String::new()
    };
    cache.borrow_mut().insert(path.to_string(), status.clone());
    status
}

fn status_from_entry(entry: &IndexEntry) -> String {
    entry
        .search_sections
        .iter()
        .find(|section| py_casefold(py_strip(&section.heading)) == "status")
        .and_then(|section| {
            section
                .lines
                .iter()
                .map(|line| py_strip(line))
                .find(|line| !line.is_empty())
        })
        .unwrap_or("")
        .to_string()
}

#[allow(clippy::too_many_arguments)]
fn retrieve_grounding_from_parts<EntryForPath, StatusOf>(
    directory: &str,
    task: &str,
    scope: Option<&str>,
    top_k: i64,
    budget: i64,
    live_only: bool,
    keyword: SearchResult,
    scope_rows: &[ScopeRow],
    relationships: &[Relationship],
    entry_for_path: EntryForPath,
    status_of: StatusOf,
) -> Value
where
    EntryForPath: Fn(&str) -> Option<IndexEntry>,
    StatusOf: Fn(&str) -> String,
{
    let top_k = top_k.max(1);
    let is_retired = |path: &str| -> bool {
        let artifact_type = entry_for_path(path)
            .map(|entry| entry.artifact_type)
            .unwrap_or_else(|| DECISION_TYPE.to_string());
        is_retired_status(&artifact_type, &status_of(path))
    };

    let mut items: Vec<ItemBuilder> = Vec::new();
    let mut index_of: HashMap<String, usize> = HashMap::new();

    // Scope stratum: declared `## Applies To` coverage binds regardless of
    // keyword match; the rows are live by construction.
    let scope = scope.filter(|s| !s.is_empty()); // Python `if scope:` truthiness
    if let Some(scope_path) = scope {
        for governing in governing_decisions(scope_rows, directory, scope_path) {
            add_item(
                &mut items,
                &mut index_of,
                &governing.path,
                CHANNEL_SCOPE,
                &governing.id,
                DECISION_TYPE,
                if governing.title.is_empty() {
                    None
                } else {
                    Some(&governing.title)
                },
                &governing.status,
                Some(&governing.matching_entry),
                None,
                None,
            );
        }
    }

    // Keyword stratum.
    let by_target = if live_only {
        successor_map(relationships)
    } else {
        HashMap::new()
    };
    for m in &keyword.matches {
        if live_only && is_retired(&m.path) {
            let mut visited: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            visited.insert(m.path.clone());
            for successor_path in live_successors(&m.path, &by_target, &is_retired, &mut visited)
            {
                let Some(successor) = entry_for_path(&successor_path) else {
                    continue;
                };
                add_item(
                    &mut items,
                    &mut index_of,
                    &successor_path,
                    CHANNEL_SUPERSEDES,
                    &successor.id,
                    &successor.artifact_type,
                    successor.title.as_deref(),
                    &status_of(&successor_path),
                    None,
                    Some(&m.id),
                    None,
                );
            }
            continue;
        }
        add_item(
            &mut items,
            &mut index_of,
            &m.path,
            CHANNEL_KEYWORD,
            &m.id,
            &m.artifact_type,
            m.title.as_deref(),
            &status_of(&m.path),
            None,
            None,
            m.evidence.as_ref().map(crate::output::evidence_value),
        );
    }

    let selected: Vec<ItemBuilder> = {
        let keep = (top_k.max(0) as usize).min(items.len());
        items.truncate(keep);
        items
    };
    // Even excerpt shaping: each item's excerpt is the head of the artifact's
    // stored text capped at the budget's per-item share.
    let share = if selected.is_empty() {
        0
    } else {
        budget.div_euclid((top_k.min(selected.len() as i64)).max(1))
    };
    let mut shaped: Vec<Value> = Vec::new();
    for item in selected {
        let content = read_text_universal(&item.path).unwrap_or_default();
        let mut obj = Map::new();
        obj.insert("id".to_string(), json!(item.id));
        obj.insert("type".to_string(), json!(item.item_type));
        obj.insert(
            "title".to_string(),
            item.title.map(|t| json!(t)).unwrap_or(Value::Null),
        );
        obj.insert("status".to_string(), json!(item.status));
        obj.insert("path".to_string(), json!(item.path));
        obj.insert("excerpt".to_string(), json!(py_slice_to(&content, share)));
        obj.insert("provenance".to_string(), Value::Object(item.provenance));
        shaped.push(Value::Object(obj));
    }

    let mut payload = Map::new();
    payload.insert("schema_version".to_string(), json!("1"));
    payload.insert("task".to_string(), json!(task));
    if let Some(scope_path) = scope {
        payload.insert("scope".to_string(), json!(scope_path));
    }
    payload.insert("live_only".to_string(), json!(live_only));
    payload.insert("items".to_string(), Value::Array(shaped));
    Value::Object(payload)
}
