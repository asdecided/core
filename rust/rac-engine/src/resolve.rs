//! Resolve & search (`decided resolve`, `decided find`) — a port of
//! `src/asdecided/services/resolve.py` (+ the index construction in
//! `src/asdecided/services/index.py`), per PORT-CONTRACT.d/06.
//!
//! Landmines reproduced here (contract §15):
//! - ASCII-only tokenizer (§1) vs full-Unicode casefold/strip in exact
//!   resolution (§3) and the `--tag` facet (§5.2).
//! - Corpus statistics are corpus-global (all types, unknowns included) even
//!   under `--type`/`--tag`; ranks are over the matched set only (§6).
//! - Duplicate query tokens are NOT deduped: `df` increments per occurrence
//!   and the per-term score adds per occurrence (§7.1).
//! - BM25F float operation ORDER is normative (§7): weighted_tf accumulates
//!   in `id, title, path, heading, body, tags` field order with zero-tf
//!   fields skipped; score accumulates in query-token order;
//!   `idf = ln(1 + (n - d + 0.5)/(d + 0.5))` via plain f64 ops (not ln_1p).
//! - Competition ranks share on EXACT f64 equality (§8).
//! - Sort key is `(-py_round(fused, 12), path)`; the stored fused value stays
//!   unrounded; evidence carries `py_round(., 6)` (§9–10).

use std::collections::{HashMap, HashSet};

use crate::identity::{artifact_identifier, artifact_identifiers};
use crate::markdown::SearchSection;
use crate::parse::Artifact;
use crate::pycompat::{first_nonempty_line, py_casefold, py_round, py_strip};
use crate::relationships::{
    corpus_items, edge_spec, resolution_index_from_rows, validation_row, CorpusItem,
};
use crate::spec::spec_for;

pub const OUTCOME_RESOLVED: &str = "resolved";
pub const OUTCOME_NOT_FOUND: &str = "not-found";
pub const OUTCOME_DUPLICATE: &str = "duplicate";

// Match-field tier ladder (ADR-037/038/109): id, title, tags, path, heading,
// body — lower rank wins.
const RANK_ID: i64 = 0;
const RANK_TITLE: i64 = 1;
const RANK_TAGS: i64 = 2;
const RANK_PATH: i64 = 3;
const RANK_HEADING: i64 = 4;
const RANK_BODY: i64 = 5;

fn rank_name(rank: i64) -> &'static str {
    match rank {
        RANK_ID => "id",
        RANK_TITLE => "title",
        RANK_TAGS => "tags",
        RANK_PATH => "path",
        RANK_HEADING => "heading",
        _ => "body",
    }
}

// BM25F constants (ADR-078).
const RRF_K: i64 = 60;
const GRAPH_WEIGHT: f64 = 0.5;
/// A graph signal may only contribute for lexical near-ties. Candidates below
/// 85% of the strongest BM25 score keep their lexical score but receive no
/// graph contribution (retrieval-diagnostics, Initiative 2).
pub const GRAPH_FLOOR_RATIO: f64 = 0.85;
const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

/// `_FIELD_BOOSTS` in insertion order — `tags` is LAST, not at its tier
/// position (deliberate: preserves the pre-ADR-109 float summation order).
const FIELD_BOOSTS: [(&str, f64); 6] = [
    ("id", 4.0),
    ("title", 3.0),
    ("path", 2.0),
    ("heading", 1.5),
    ("body", 1.0),
    ("tags", 2.5),
];

// ---------------------------------------------------------------------------
// Tokenization (ADR-037) — ASCII-only splitter + ASCII camel seams
// ---------------------------------------------------------------------------

/// `tokenize(text)`: split on runs of non-`[0-9A-Za-z]` (every non-ASCII char
/// is a separator), split each piece at ASCII lowercase→uppercase seams, then
/// casefold (pure-ASCII pieces: exactly `A-Z -> a-z`).
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for piece in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if piece.is_empty() {
            continue;
        }
        let bytes = piece.as_bytes();
        let mut start = 0usize;
        for i in 1..bytes.len() {
            if bytes[i - 1].is_ascii_lowercase() && bytes[i].is_ascii_uppercase() {
                tokens.push(piece[start..i].to_ascii_lowercase());
                start = i;
            }
        }
        tokens.push(piece[start..].to_ascii_lowercase());
    }
    tokens
}

/// `_term_hits_tokens`: term equals or is a prefix of any token.
fn term_hits_tokens(term: &str, tokens: &[String]) -> bool {
    tokens.iter().any(|t| t.starts_with(term))
}

/// `_tf(term, tokens)`: count of tokens the term equals or prefixes.
fn tf(term: &str, tokens: &[String]) -> i64 {
    tokens.iter().filter(|t| t.starts_with(term)).count() as i64
}

// ---------------------------------------------------------------------------
// Index entries (decided.services.index.IndexEntry)
// ---------------------------------------------------------------------------

/// One searchable row of the repository index.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub id: String,
    pub artifact_type: String,
    pub title: Option<String>,
    pub path: String,
    /// Canonical ID first, then legacy aliases (case-insensitively deduped).
    pub aliases: Vec<String>,
    pub search_sections: Vec<SearchSection>,
    /// Count of resolved inbound relationship edges (the graph signal).
    pub inbound_count: i64,
    /// Frontmatter tags, in frontmatter order.
    pub tags: Vec<String>,
}

/// The identity-only projection of an entry (the oracle's `_identity_index`):
/// `_identity_index` never reads tags/sections/graph, so those stay at their
/// empty defaults — the resolved artifact matches the oracle's shape exactly
/// and the discarded clones never happen.
pub(crate) fn identity_entry_from_item(item: &CorpusItem) -> IndexEntry {
    let artifact_type = item
        .spec
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "unknown".to_string());
    IndexEntry {
        id: artifact_identifier(&item.artifact, item.spec, &item.path),
        artifact_type,
        title: item.artifact.product.title.clone(),
        path: item.path.clone(),
        aliases: artifact_identifiers(&item.artifact, item.spec, &item.path),
        search_sections: Vec::new(),
        inbound_count: 0,
        tags: Vec::new(),
    }
}

pub(crate) fn entry_from_item(item: &CorpusItem, inbound: i64) -> IndexEntry {
    IndexEntry {
        search_sections: item.artifact.product.search_sections.clone(),
        inbound_count: inbound,
        tags: item
            .artifact
            .metadata
            .as_ref()
            .map(|m| m.tags.clone())
            .unwrap_or_default(),
        ..identity_entry_from_item(item)
    }
}

/// `inbound_counts_from_corpus`: `{path -> count of resolved edges pointing
/// at it}` — resolved, unique, non-self edges only; external edges (ADR-087)
/// never resolve.
fn inbound_counts(items: &[CorpusItem]) -> HashMap<String, i64> {
    let rows: Vec<_> = items
        .iter()
        .map(|item| validation_row(&item.path, &item.artifact, item.spec))
        .collect();
    let index = resolution_index_from_rows(&rows);
    let mut counts: HashMap<String, i64> = HashMap::new();
    for row in &rows {
        for (section, refs) in &row.edges {
            let external = edge_spec(section).map(|e| e.external).unwrap_or(false);
            if external {
                continue;
            }
            for r in refs {
                let targets = index.get(&py_casefold(r));
                if targets.len() == 1 && targets[0].0 != row.path {
                    *counts.entry(targets[0].0.clone()).or_insert(0) += 1;
                }
            }
        }
    }
    counts
}

/// `build_repository_index(directory, recursive).artifacts` — the searchable
/// index in corpus-walk (sorted-path) order, inbound counts included.
pub fn build_index(directory: &str, recursive: bool) -> Vec<IndexEntry> {
    index_from_items(&corpus_items(directory, recursive))
}

pub fn index_from_items(items: &[CorpusItem]) -> Vec<IndexEntry> {
    let inbound = inbound_counts(items);
    items
        .iter()
        .map(|item| entry_from_item(item, *inbound.get(&item.path).unwrap_or(&0)))
        .collect()
}

// ---------------------------------------------------------------------------
// Exact resolution (contract §3)
// ---------------------------------------------------------------------------

/// One resolved artifact / search match (`ResolvedArtifact`).
#[derive(Debug, Clone)]
pub struct ResolvedArtifact {
    pub id: String,
    pub artifact_type: String,
    pub title: Option<String>,
    pub path: String,
    pub section: Option<String>,
    pub snippet: Option<String>,
    pub evidence: Option<Evidence>,
    pub recency: Option<Recency>,
    pub tags: Vec<String>,
}

/// The `--explain` evidence object plus the unrounded score components
/// (`bm25_raw`/`fused_raw` are not serialized; they exist so conformance
/// tests can assert exact f64 bit equality against the oracle).
#[derive(Debug, Clone)]
pub struct Evidence {
    pub field: &'static str,
    /// Distinct casefolded query tokens, in query order.
    pub terms: Vec<String>,
    pub tier: i64,
    /// `py_round(fused, 6)`.
    pub score: f64,
    /// `py_round(bm25, 6)`.
    pub bm25: f64,
    pub lexical_rank: i64,
    pub graph_rank: i64,
    pub inbound: i64,
    /// The fixed lexical floor used to decide whether graph boost may apply.
    pub graph_floor_ratio: f64,
    /// `"applied"` when the candidate clears the lexical floor, otherwise
    /// `"clamped"` (its graph contribution is zero).
    pub graph_gate: &'static str,
    /// The unrounded BM25F score (test-only surface).
    pub bm25_raw: f64,
    /// The unrounded fused RRF score (test-only surface).
    pub fused_raw: f64,
}

/// The git-derived recency join (`Staleness.to_dict()` shape).
#[derive(Debug, Clone)]
pub struct Recency {
    pub last_committed: Option<String>,
    pub age_days: Option<i64>,
    pub stale: Option<bool>,
}

/// Outcome of one exact-ID lookup (`ResolutionResult`).
#[derive(Debug, Clone)]
pub struct ResolutionResult {
    /// The query as given (unstripped).
    pub artifact_id: String,
    pub outcome: &'static str,
    pub artifact: Option<ResolvedArtifact>,
    pub duplicate_paths: Vec<String>,
}

pub(crate) fn resolved_from_entry(entry: &IndexEntry) -> ResolvedArtifact {
    ResolvedArtifact {
        id: entry.id.clone(),
        artifact_type: entry.artifact_type.clone(),
        title: entry.title.clone(),
        path: entry.path.clone(),
        section: None,
        snippet: None,
        evidence: None,
        recency: None,
        tags: entry.tags.clone(),
    }
}

/// `resolve_in_index(entries, artifact_id)`: full-Unicode strip + casefold on
/// the query, casefolded exact equality against every alias.
pub fn resolve_in_index(entries: &[IndexEntry], artifact_id: &str) -> ResolutionResult {
    let wanted = py_casefold(py_strip(artifact_id));
    let matches: Vec<&IndexEntry> = entries
        .iter()
        .filter(|e| e.aliases.iter().any(|a| py_casefold(a) == wanted))
        .collect();
    if matches.is_empty() {
        return ResolutionResult {
            artifact_id: artifact_id.to_string(),
            outcome: OUTCOME_NOT_FOUND,
            artifact: None,
            duplicate_paths: Vec::new(),
        };
    }
    if matches.len() > 1 {
        let mut paths: Vec<String> = matches.iter().map(|e| e.path.clone()).collect();
        paths.sort(); // Python str sort = code-point order = UTF-8 byte order
        return ResolutionResult {
            artifact_id: artifact_id.to_string(),
            outcome: OUTCOME_DUPLICATE,
            artifact: None,
            duplicate_paths: paths,
        };
    }
    ResolutionResult {
        artifact_id: artifact_id.to_string(),
        outcome: OUTCOME_RESOLVED,
        artifact: Some(resolved_from_entry(matches[0])),
        duplicate_paths: Vec::new(),
    }
}

/// `resolve_artifact(directory, artifact_id, recursive)`. The oracle's
/// identity-only walk (`_identity_index`) leaves sections/graph/tags at their
/// empty defaults; resolve output reads only id/type/title/path, so those
/// fields never surface (resolve JSON never gains a "tags" key).
pub fn resolve_artifact(directory: &str, artifact_id: &str, recursive: bool) -> ResolutionResult {
    let items = corpus_items(directory, recursive);
    let entries: Vec<IndexEntry> = items.iter().map(identity_entry_from_item).collect();
    resolve_in_index(&entries, artifact_id)
}

// ---------------------------------------------------------------------------
// Tokenised entries (contract §4)
// ---------------------------------------------------------------------------

/// Flat per-field token vectors, one per scorable field.
#[derive(Debug, Clone, Default)]
pub struct FieldTokens {
    pub id: Vec<String>,
    pub title: Vec<String>,
    pub tags: Vec<String>,
    pub path: Vec<String>,
    pub heading: Vec<String>,
    pub body: Vec<String>,
}

impl FieldTokens {
    pub(crate) fn get(&self, name: &str) -> &Vec<String> {
        match name {
            "id" => &self.id,
            "title" => &self.title,
            "tags" => &self.tags,
            "path" => &self.path,
            "heading" => &self.heading,
            _ => &self.body,
        }
    }
}

struct SectionTokens {
    heading: String,
    heading_tokens: Vec<String>,
    lines: Vec<(String, Vec<String>)>,
}

pub(crate) struct EntryTokens {
    pub(crate) fields: FieldTokens,
    sections: Vec<SectionTokens>,
}

pub(crate) fn tokenize_entry(entry: &IndexEntry) -> EntryTokens {
    let mut sections: Vec<SectionTokens> = Vec::new();
    let mut heading_tokens: Vec<String> = Vec::new();
    let mut body_tokens: Vec<String> = Vec::new();
    for sec in &entry.search_sections {
        let sec_heading_tokens = tokenize(&sec.heading);
        heading_tokens.extend(sec_heading_tokens.iter().cloned());
        let mut sec_lines: Vec<(String, Vec<String>)> = Vec::new();
        for line in &sec.lines {
            let line_tokens = tokenize(line);
            body_tokens.extend(line_tokens.iter().cloned());
            sec_lines.push((line.clone(), line_tokens));
        }
        sections.push(SectionTokens {
            heading: sec.heading.clone(),
            heading_tokens: sec_heading_tokens,
            lines: sec_lines,
        });
    }
    let mut id_tokens: Vec<String> = Vec::new();
    for alias in &entry.aliases {
        id_tokens.extend(tokenize(alias));
    }
    let mut tag_tokens: Vec<String> = Vec::new();
    for tag in &entry.tags {
        tag_tokens.extend(tokenize(tag));
    }
    EntryTokens {
        fields: FieldTokens {
            id: id_tokens,
            title: tokenize(entry.title.as_deref().unwrap_or("")),
            tags: tag_tokens,
            path: tokenize(&entry.path),
            heading: heading_tokens,
            body: body_tokens,
        },
        sections,
    }
}

/// The six flat per-field token vectors of one entry — the projection the
/// derived read-model persists (`field_tokens_for_entries`, INDEX-PLAN B2).
pub(crate) fn field_tokens_of(entry: &IndexEntry) -> FieldTokens {
    tokenize_entry(entry).fields
}

// ---------------------------------------------------------------------------
// Tier matching (contract §4)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct TierMatch {
    rank: i64,
    section: Option<String>,
    snippet: Option<String>,
    /// Distinct matched terms, in query-token order.
    terms: Vec<String>,
}

fn trace_field_matches<'a>(
    fields: &FieldTokens,
    terms: &'a [String],
) -> (Option<i64>, HashSet<&'a str>) {
    let mut matched_terms: HashSet<&'a str> = HashSet::new();
    let mut best_rank: Option<i64> = None;
    for (rank, field) in [
        (RANK_ID, "id"),
        (RANK_TITLE, "title"),
        (RANK_TAGS, "tags"),
        (RANK_PATH, "path"),
        (RANK_HEADING, "heading"),
        (RANK_BODY, "body"),
    ] {
        let tokens = fields.get(field);
        let mut any = false;
        for term in terms {
            if term_hits_tokens(term, tokens) {
                matched_terms.insert(term.as_str());
                any = true;
            }
        }
        if any && best_rank.is_none() {
            best_rank = Some(rank);
        }
    }
    (best_rank, matched_terms)
}

fn match_fields(fields: &FieldTokens, terms: &[String]) -> Option<(i64, Vec<String>)> {
    let (best_rank, matched_terms) = trace_field_matches(fields, terms);

    // AND semantics: every distinct term must have matched somewhere.
    let distinct: HashSet<&str> = terms.iter().map(|t| t.as_str()).collect();
    if !distinct.is_subset(&matched_terms) {
        return None;
    }
    let best_rank = best_rank?;

    // Matched terms in query order, deduped (dict.fromkeys semantics).
    let mut seen: HashSet<&str> = HashSet::new();
    let mut ordered: Vec<String> = Vec::new();
    for term in terms {
        if seen.insert(term.as_str()) && matched_terms.contains(term.as_str()) {
            ordered.push(term.clone());
        }
    }
    Some((best_rank, ordered))
}

fn any_term_hits(terms: &[String], tokens: &[String]) -> bool {
    terms.iter().any(|term| term_hits_tokens(term, tokens))
}

pub(crate) fn match_entry(entry_tokens: &EntryTokens, terms: &[String]) -> Option<TierMatch> {
    let (best_rank, ordered) = match_fields(&entry_tokens.fields, terms)?;

    // Only the winning tier's snippet is surfaced; metadata wins carry none.
    let snippet = match best_rank {
        RANK_HEADING => entry_tokens
            .sections
            .iter()
            .find(|section| any_term_hits(terms, &section.heading_tokens))
            .map(|section| (section.heading.clone(), section.heading.clone())),
        RANK_BODY => entry_tokens.sections.iter().find_map(|section| {
            section
                .lines
                .iter()
                .find(|(_, tokens)| any_term_hits(terms, tokens))
                .map(|(line, _)| (section.heading.clone(), line.clone()))
        }),
        _ => None,
    };
    let (section, snippet) = match snippet {
        Some((section, line)) => (Some(section), Some(line)),
        None => (None, None),
    };
    Some(TierMatch {
        rank: best_rank,
        section,
        snippet,
        terms: ordered,
    })
}

/// Match a store row using its persisted flat tokens. Raw section text is
/// tokenized only until the winning heading/body snippet is found; metadata
/// winners do not rebuild section tokens at all.
pub(crate) fn match_entry_with_fields(
    entry: &IndexEntry,
    fields: &FieldTokens,
    terms: &[String],
) -> Option<TierMatch> {
    let (best_rank, ordered) = match_fields(fields, terms)?;
    let snippet = match best_rank {
        RANK_HEADING => entry.search_sections.iter().find_map(|section| {
            let tokens = tokenize(&section.heading);
            any_term_hits(terms, &tokens)
                .then(|| (section.heading.clone(), section.heading.clone()))
        }),
        RANK_BODY => entry.search_sections.iter().find_map(|section| {
            section.lines.iter().find_map(|line| {
                let tokens = tokenize(line);
                any_term_hits(terms, &tokens).then(|| (section.heading.clone(), line.clone()))
            })
        }),
        _ => None,
    };
    let (section, snippet) = match snippet {
        Some((section, line)) => (Some(section), Some(line)),
        None => (None, None),
    };
    Some(TierMatch {
        rank: best_rank,
        section,
        snippet,
        terms: ordered,
    })
}

// ---------------------------------------------------------------------------
// Corpus statistics + BM25F (contract §6–7)
// ---------------------------------------------------------------------------

/// Corpus-global BM25 statistics (all entries, unknowns included).
pub struct CorpusStats {
    pub n: i64,
    /// Per-term document frequency; duplicate query terms double-count.
    pub df: HashMap<String, i64>,
    /// Mean field length in `FIELD_BOOSTS` order.
    pub avglen: [f64; 6],
}

fn corpus_stats(field_tokens: &[FieldTokens], terms: &[String]) -> CorpusStats {
    let n = field_tokens.len() as i64;
    let mut length_sums = [0i64; 6];
    let mut df: HashMap<String, i64> = HashMap::new();
    for term in terms {
        df.entry(term.clone()).or_insert(0);
    }
    for fields in field_tokens {
        for (i, (name, _)) in FIELD_BOOSTS.iter().enumerate() {
            length_sums[i] += fields.get(name).len() as i64;
        }
        // Duplicates iterate: a term appearing twice increments its df twice.
        for term in terms {
            if FIELD_BOOSTS
                .iter()
                .any(|(name, _)| tf(term, fields.get(name)) != 0)
            {
                *df.get_mut(term.as_str()).expect("df pre-seeded") += 1;
            }
        }
    }
    let mut avglen = [0.0f64; 6];
    for (i, sum) in length_sums.iter().enumerate() {
        avglen[i] = if n != 0 { *sum as f64 / n as f64 } else { 0.0 };
    }
    CorpusStats { n, df, avglen }
}

/// Corpus-global statistics for one query over `entries` — the conformance
/// vector surface (`gen_vectors_resolve.py` pins `n`/`df`/`avglen`).
pub fn stats_for(entries: &[IndexEntry], query: &str) -> CorpusStats {
    let terms = tokenize(query);
    let field_tokens: Vec<FieldTokens> = entries
        .iter()
        .map(|e| tokenize_entry(e).fields)
        .collect();
    corpus_stats(&field_tokens, &terms)
}

/// `_bm25f` — the EXACT f64 operation sequence (contract §7).
fn bm25f(fields: &FieldTokens, terms: &[String], stats: &CorpusStats) -> f64 {
    let mut score = 0.0f64;
    for term in terms {
        // QUERY-TOKEN ORDER, DUPLICATES INCLUDED
        let d = *stats.df.get(term.as_str()).unwrap_or(&0);
        if d == 0 {
            continue;
        }
        // arg = 1 + (n - d + 0.5)/(d + 0.5) — plain add, then ln (not ln_1p).
        let num = (stats.n - d) as f64 + 0.5;
        let den = d as f64 + 0.5;
        let idf = (1.0 + num / den).ln();
        let mut weighted_tf = 0.0f64;
        for (i, (name, boost)) in FIELD_BOOSTS.iter().enumerate() {
            let tokens = fields.get(name);
            let tfv = tf(term, tokens);
            if tfv == 0 {
                continue; // zero-tf fields are SKIPPED (no +0.0 term)
            }
            let length = tokens.len() as i64;
            let mean = stats.avglen[i];
            let denom = if mean > 0.0 {
                1.0 - BM25_B + BM25_B * (length as f64 / mean)
            } else {
                1.0
            };
            weighted_tf += boost * (tfv as f64 / denom);
        }
        if weighted_tf > 0.0 {
            score += idf * (weighted_tf / (BM25_K1 + weighted_tf));
        }
    }
    score
}

/// `_competition_ranks`: 1-based ranks aligned with `scores`; ties (EXACT
/// f64 equality) share a rank, ordered by `(-score, path)`.
fn competition_ranks(scores: &[f64], paths: &[&str]) -> Vec<i64> {
    let mut ordered: Vec<usize> = (0..scores.len()).collect();
    ordered.sort_by(|&a, &b| {
        scores[b]
            .partial_cmp(&scores[a])
            .expect("finite score")
            .then_with(|| paths[a].cmp(paths[b]))
    });
    let mut ranks = vec![0; scores.len()];
    let mut previous: Option<f64> = None;
    let mut rank = 0i64;
    for (position, &index) in ordered.iter().enumerate() {
        let position = position as i64 + 1;
        if Some(scores[index]) != previous {
            rank = position;
            previous = Some(scores[index]);
        }
        ranks[index] = rank;
    }
    ranks
}

/// Whether a candidate is lexically close enough to the strongest match for
/// the graph signal to act as a near-tie breaker.
fn graph_gate_allows(bm25: f64, strongest_bm25: f64) -> bool {
    bm25 >= strongest_bm25 * GRAPH_FLOOR_RATIO
}

// ---------------------------------------------------------------------------
// Search (contract §4–10)
// ---------------------------------------------------------------------------

/// Outcome of one repository search (`SearchResult`).
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub query: String,
    /// The `--type` value; `"decision"` under `--decisions`; else None.
    pub artifact_type: Option<String>,
    pub matches: Vec<ResolvedArtifact>,
}

pub const DIAGNOSIS_COMPLETE: &str = "diagnosed";
pub const DIAGNOSIS_TARGET_NOT_FOUND: &str = "target-not-found";
pub const DIAGNOSIS_TARGET_DUPLICATE: &str = "target-duplicate";

/// Named-target trace for `decided diagnose`. The diagnostic consumes the
/// same tokenisation, matching, filters, and ranked result as `find`; it only
/// explains that result and never mutates membership or order.
#[derive(Debug, Clone)]
pub struct SearchDiagnosis {
    pub query: String,
    pub target: String,
    pub outcome: &'static str,
    pub reason: &'static str,
    pub surface_limit: usize,
    pub match_count: usize,
    pub query_terms: Vec<String>,
    pub matched_terms: Vec<String>,
    pub missing_terms: Vec<String>,
    pub rank: Option<usize>,
    pub outranked_by: Vec<String>,
    pub artifact: Option<ResolvedArtifact>,
    pub duplicate_paths: Vec<String>,
}

fn ordered_distinct_terms(terms: &[String]) -> Vec<String> {
    let mut seen: HashSet<&str> = HashSet::new();
    terms
        .iter()
        .filter(|term| seen.insert(term.as_str()))
        .cloned()
        .collect()
}

/// Diagnose one named target against the exact ranked retrieval path.
pub fn diagnose_index(
    entries: &[IndexEntry],
    query: &str,
    target: &str,
    artifact_type: Option<&str>,
    tags: &[String],
    live_only: bool,
    surface_limit: usize,
) -> SearchDiagnosis {
    let terms = tokenize(query);
    let query_terms = ordered_distinct_terms(&terms);
    let resolution = resolve_in_index(entries, target);
    if resolution.outcome != OUTCOME_RESOLVED {
        return SearchDiagnosis {
            query: query.to_string(),
            target: target.to_string(),
            outcome: if resolution.outcome == OUTCOME_DUPLICATE {
                DIAGNOSIS_TARGET_DUPLICATE
            } else {
                DIAGNOSIS_TARGET_NOT_FOUND
            },
            reason: if resolution.outcome == OUTCOME_DUPLICATE {
                "duplicate_target"
            } else {
                "unknown_target"
            },
            surface_limit,
            match_count: 0,
            query_terms,
            matched_terms: Vec::new(),
            missing_terms: Vec::new(),
            rank: None,
            outranked_by: Vec::new(),
            artifact: None,
            duplicate_paths: resolution.duplicate_paths,
        };
    }

    let resolved = resolution.artifact.expect("resolved target has artifact");
    let entry = entries
        .iter()
        .find(|entry| entry.path == resolved.path)
        .expect("resolved target came from entries");
    let tokenized = tokenize_entry(entry);
    let (_, matched_set) = trace_field_matches(&tokenized.fields, &terms);
    let matched_terms: Vec<String> = query_terms
        .iter()
        .filter(|term| matched_set.contains(term.as_str()))
        .cloned()
        .collect();
    let missing_terms: Vec<String> = query_terms
        .iter()
        .filter(|term| !matched_set.contains(term.as_str()))
        .cloned()
        .collect();

    let tag_filter: Vec<String> = tags.iter().map(|tag| py_casefold(tag)).collect();
    let filtered_reason = if artifact_type
        .is_some_and(|wanted| wanted != entry.artifact_type.as_str())
    {
        Some("filtered_by_type")
    } else if !tag_filter.is_empty() && !entry_has_tags(entry, &tag_filter) {
        Some("filtered_by_tags")
    } else if live_only && entry_is_retired(entry) {
        Some("filtered_by_liveness")
    } else {
        None
    };

    let ranked = search_index_filtered(entries, query, artifact_type, tags, live_only);
    let rank_index = ranked
        .matches
        .iter()
        .position(|artifact| artifact.path == resolved.path);
    let rank = rank_index.map(|index| index + 1);
    let reason = if let Some(reason) = filtered_reason {
        reason
    } else if matched_terms.is_empty() {
        "no_term_match"
    } else if !missing_terms.is_empty() {
        "partial_term_match"
    } else if rank.is_some_and(|value| value > surface_limit) {
        "budget_truncated"
    } else if rank.is_some_and(|value| value > 1) {
        "ranked_below"
    } else if rank == Some(1) {
        "surfaced"
    } else {
        "not_returned"
    };
    let outranked_by = rank_index
        .map(|index| {
            ranked.matches[..index.min(surface_limit)]
                .iter()
                .map(|artifact| artifact.id.clone())
                .collect()
        })
        .unwrap_or_default();
    let artifact = rank_index
        .map(|index| ranked.matches[index].clone())
        .or(Some(resolved));

    SearchDiagnosis {
        query: query.to_string(),
        target: target.to_string(),
        outcome: DIAGNOSIS_COMPLETE,
        reason,
        surface_limit,
        match_count: ranked.matches.len(),
        query_terms,
        matched_terms,
        missing_terms,
        rank,
        outranked_by,
        artifact,
        duplicate_paths: Vec::new(),
    }
}

/// Directory-backed named-target diagnosis.
pub struct DiagnoseOptions<'a> {
    pub artifact_type: Option<&'a str>,
    pub recursive: bool,
    pub tags: &'a [String],
    pub live_only: bool,
    pub surface_limit: usize,
}

pub fn diagnose_artifact(
    directory: &str,
    query: &str,
    target: &str,
    options: DiagnoseOptions<'_>,
) -> SearchDiagnosis {
    let entries = build_index(directory, options.recursive);
    diagnose_index(
        &entries,
        query,
        target,
        options.artifact_type,
        options.tags,
        options.live_only,
        options.surface_limit,
    )
}

/// `_entry_has_tags`: exact whole-tag comparison, full Unicode casefold.
pub(crate) fn entry_has_tags(entry: &IndexEntry, wanted: &[String]) -> bool {
    let have: HashSet<String> = entry.tags.iter().map(|t| py_casefold(t)).collect();
    wanted.iter().all(|w| have.contains(w))
}

/// `search_index(entries, query, artifact_type, tags)` — matching, corpus
/// stats, BM25F + RRF ranking, and the `(-round(fused,12), path)` sort.
pub fn search_index(
    entries: &[IndexEntry],
    query: &str,
    artifact_type: Option<&str>,
    tags: &[String],
) -> SearchResult {
    search_index_filtered(entries, query, artifact_type, tags, false)
}

/// `entry_is_retired(entry)` — the `live_only` facet (ADR-113): re-read the
/// entry's `## Status` from its file and test it against the type's
/// `retired_status` set (`is_retired_status`). Unreadable/unknown ⇒ live.
pub fn entry_is_retired(entry: &IndexEntry) -> bool {
    let status = artifact_status(&crate::parse::parse_file(&entry.path));
    is_retired_status(&entry.artifact_type, &status)
}

/// `agent_rules.is_retired_status(artifact_type, status)` (ADR-113):
/// spec-driven retirement for every typed artifact. An unknown type retires
/// nothing; an empty status is never retired.
pub fn is_retired_status(artifact_type: &str, status: &str) -> bool {
    let Some(spec) = spec_for(artifact_type) else {
        return false;
    };
    let wanted = py_casefold(status);
    spec.retired_status.iter().any(|s| py_casefold(s) == wanted)
}

/// `search_index(..., live_only=...)` (ADR-113, additive): with `live_only`,
/// retired artifacts of every type are dropped from the matched set BEFORE
/// scoring, so competition ranks are computed among the live survivors. With
/// `live_only=false` the result is byte-identical to `search_index`.
pub fn search_index_filtered(
    entries: &[IndexEntry],
    query: &str,
    artifact_type: Option<&str>,
    tags: &[String],
    live_only: bool,
) -> SearchResult {
    let terms = tokenize(query);
    let tag_filter: Vec<String> = tags.iter().map(|t| py_casefold(t)).collect();
    let mut matched: Vec<(usize, TierMatch)> = Vec::new();
    let mut tokenized: Vec<Option<EntryTokens>> = Vec::with_capacity(entries.len());
    tokenized.resize_with(entries.len(), || None);
    if !terms.is_empty() {
        for (i, entry) in entries.iter().enumerate() {
            if let Some(t) = artifact_type {
                if entry.artifact_type != t {
                    continue;
                }
            }
            if !tag_filter.is_empty() && !entry_has_tags(entry, &tag_filter) {
                continue;
            }
            let entry_tokens = tokenize_entry(entry);
            let m = match_entry(&entry_tokens, &terms);
            tokenized[i] = Some(entry_tokens);
            if let Some(m) = m {
                matched.push((i, m));
            }
        }
    }
    // The live-only facet filters the matched set before scoring (ADR-113), so
    // competition ranks are computed among the live survivors — only matched
    // files are re-read, never the whole corpus.
    if live_only && !matched.is_empty() {
        matched.retain(|(i, _)| !entry_is_retired(&entries[*i]));
    }
    if matched.is_empty() {
        return SearchResult {
            query: query.to_string(),
            artifact_type: artifact_type.map(str::to_string),
            matches: Vec::new(),
        };
    }

    // Corpus-wide statistics over EVERY entry (type/tag-excluded included).
    let field_tokens: Vec<FieldTokens> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| match tokenized[i].take() {
            Some(t) => t.fields,
            None => tokenize_entry(entry).fields,
        })
        .collect();
    let stats = corpus_stats(&field_tokens, &terms);

    let scored: Vec<(&IndexEntry, &FieldTokens, TierMatch)> = matched
        .into_iter()
        .map(|(i, m)| (&entries[i], &field_tokens[i], m))
        .collect();
    rank_and_build(query, artifact_type, scored, &terms, &stats)
}

/// The shared scoring/ranking/build tail of the tiered search — one code
/// path for the fresh walk and the store-served read-model (ADR-104), so
/// warm and cold emit identical bytes by construction. `matched` rows are
/// in entry (walk/docid) order; `stats` carries the corpus-global n/df/
/// avglen however the caller derived them.
pub(crate) fn rank_and_build(
    query: &str,
    artifact_type: Option<&str>,
    matched: Vec<(&IndexEntry, &FieldTokens, TierMatch)>,
    terms: &[String],
    stats: &CorpusStats,
) -> SearchResult {
    // Score the matched set only.
    let bm25_started = crate::timing::start();
    let bm25_scores: Vec<f64> = matched
        .iter()
        .map(|(_, fields, _)| bm25f(fields, terms, stats))
        .collect();
    crate::timing::emit_since(
        "search.bm25f",
        bm25_started,
        &[("matched", matched.len() as u64)],
    );
    let fusion_started = crate::timing::start();
    let inbound_scores: Vec<f64> = matched
        .iter()
        .map(|(entry, _, _)| entry.inbound_count as f64)
        .collect();
    let paths: Vec<&str> = matched.iter().map(|(entry, _, _)| entry.path.as_str()).collect();
    let lexical_rank = competition_ranks(&bm25_scores, &paths);
    let graph_rank = competition_ranks(&inbound_scores, &paths);
    let strongest_bm25 = bm25_scores.iter().copied().fold(0.0_f64, f64::max);
    let graph_gate_applied: Vec<bool> = bm25_scores
        .iter()
        .map(|score| graph_gate_allows(*score, strongest_bm25))
        .collect();
    let fused: Vec<f64> = bm25_scores
        .iter()
        .enumerate()
        .map(|(index, _)| {
            1.0 / ((RRF_K + lexical_rank[index]) as f64)
                + if graph_gate_applied[index] {
                    GRAPH_WEIGHT / ((RRF_K + graph_rank[index]) as f64)
                } else {
                    0.0
                }
        })
        .collect();
    crate::timing::emit_since(
        "search.rank_fusion",
        fusion_started,
        &[("matched", matched.len() as u64)],
    );

    // Fused score descending (rounded to 12 places inside the key only),
    // ties broken by path: total and byte-stable.
    let sort_started = crate::timing::start();
    let fused_sort_keys: Vec<f64> = fused.iter().map(|score| py_round(*score, 12)).collect();
    let mut order: Vec<usize> = (0..matched.len()).collect();
    order.sort_by(|&a, &b| {
        fused_sort_keys[b]
            .partial_cmp(&fused_sort_keys[a])
            .expect("finite fused")
            .then_with(|| paths[a].cmp(paths[b]))
    });
    crate::timing::emit_since(
        "search.final_sort",
        sort_started,
        &[("matched", matched.len() as u64)],
    );

    let projection_started = crate::timing::start();
    let matches: Vec<ResolvedArtifact> = order
        .into_iter()
        .map(|index| {
            let (entry, _, m) = &matched[index];
            let fused_raw = fused[index];
            let bm25_raw = bm25_scores[index];
            ResolvedArtifact {
                id: entry.id.clone(),
                artifact_type: entry.artifact_type.clone(),
                title: entry.title.clone(),
                path: entry.path.clone(),
                section: m.section.clone(),
                snippet: m.snippet.clone(),
                evidence: Some(Evidence {
                    field: rank_name(m.rank),
                    terms: m.terms.clone(),
                    tier: m.rank,
                    score: py_round(fused_raw, 6),
                    bm25: py_round(bm25_raw, 6),
                    lexical_rank: lexical_rank[index],
                    graph_rank: graph_rank[index],
                    inbound: entry.inbound_count,
                    graph_floor_ratio: GRAPH_FLOOR_RATIO,
                    graph_gate: if graph_gate_applied[index] {
                        "applied"
                    } else {
                        "clamped"
                    },
                    bm25_raw,
                    fused_raw,
                }),
                recency: None,
                tags: entry.tags.clone(),
            }
        })
        .collect();
    crate::timing::emit_since(
        "search.response_projection",
        projection_started,
        &[("matches", matches.len() as u64)],
    );

    SearchResult {
        query: query.to_string(),
        artifact_type: artifact_type.map(str::to_string),
        matches,
    }
}

/// `find_artifacts(directory, query, artifact_type, recursive, tags, live_only)`.
pub fn find_artifacts(
    directory: &str,
    query: &str,
    artifact_type: Option<&str>,
    recursive: bool,
    tags: &[String],
    live_only: bool,
) -> SearchResult {
    let entries = build_index(directory, recursive);
    search_index_filtered(&entries, query, artifact_type, tags, live_only)
}

// ---------------------------------------------------------------------------
// Live decision query (`--decisions`, ADR-067)
// ---------------------------------------------------------------------------

/// `agent_rules.artifact_status`: first non-empty stripped line of `## Status`.
pub fn artifact_status(artifact: &Artifact) -> String {
    artifact
        .section("status")
        .map(first_nonempty_line)
        .unwrap_or("")
        .to_string()
}

/// `agent_rules.is_live_decision`: Accepted and not retired.
pub(crate) fn is_live_decision(artifact: &Artifact) -> bool {
    let status = py_casefold(&artifact_status(artifact));
    if status != "accepted" {
        return false;
    }
    let retired: Vec<String> = spec_for("decision")
        .map(|s| s.retired_status.iter().map(|r| py_casefold(r)).collect())
        .unwrap_or_default();
    !retired.contains(&status)
}

/// `find_decisions(directory, topic, recursive)`: the type-restricted tiered
/// search, post-filtered to live decisions (ranks keep their gaps — evidence
/// is computed over all matched decisions including non-live ones).
pub fn find_decisions(directory: &str, topic: &str, recursive: bool) -> SearchResult {
    let items = corpus_items(directory, recursive);
    let live: HashSet<String> = items
        .iter()
        .filter(|item| {
            item.spec.map(|s| s.name.as_str()) == Some("decision")
                && is_live_decision(&item.artifact)
        })
        .map(|item| item.path.clone())
        .collect();
    let entries = index_from_items(&items);
    let mut result = search_index(&entries, topic, Some("decision"), &[]);
    result.matches.retain(|m| live.contains(&m.path));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<String> {
        tokenize(s)
    }

    fn test_entry(id: &str, title: &str, path: &str, body: &str, inbound: i64) -> IndexEntry {
        IndexEntry {
            id: id.to_string(),
            artifact_type: "decision".to_string(),
            title: Some(title.to_string()),
            path: path.to_string(),
            aliases: vec![id.to_string()],
            search_sections: vec![SearchSection {
                heading: "Decision".to_string(),
                lines: vec![body.to_string()],
            }],
            inbound_count: inbound,
            tags: Vec::new(),
        }
    }

    #[test]
    fn tokenize_contract_examples() {
        assert_eq!(toks("soft-delete"), vec!["soft", "delete"]);
        assert_eq!(toks("camelCase"), vec!["camel", "case"]);
        assert_eq!(toks("HTTPServer"), vec!["httpserver"]);
        assert_eq!(
            toks("MiXeD-Case_fooBAR"),
            vec!["mi", "xe", "d", "case", "foo", "bar"]
        );
        assert_eq!(toks("v0.22.0"), vec!["v0", "22", "0"]);
        assert_eq!(toks("ADR-037"), vec!["adr", "037"]);
        assert_eq!(toks("foo_barBaz2Qux"), vec!["foo", "bar", "baz2qux"]);
        assert_eq!(toks("caf\u{e9}"), vec!["caf"]);
        assert_eq!(toks("e\u{301}clair"), vec!["e", "clair"]);
        assert_eq!(toks("\u{130}stanbul"), vec!["stanbul"]);
        assert_eq!(toks("Stra\u{df}e"), vec!["stra", "e"]);
        assert_eq!(toks("..."), Vec::<String>::new());
        assert_eq!(toks(""), Vec::<String>::new());
    }

    #[test]
    fn prefix_matching_is_one_directional() {
        let tokens = vec!["searching".to_string()];
        assert!(term_hits_tokens("sear", &tokens));
        assert!(term_hits_tokens("searching", &tokens));
        assert!(!term_hits_tokens("searchingx", &tokens));
        assert_eq!(tf("sear", &tokens), 1);
    }

    #[test]
    fn competition_ranks_share_on_exact_equality() {
        let scores = vec![2.0, 2.0, 1.0];
        let paths = vec!["b", "a", "c"];
        let ranks = competition_ranks(&scores, &paths);
        assert_eq!(ranks, vec![1, 1, 3]);
    }

    #[test]
    fn graph_gate_uses_inclusive_eighty_five_percent_floor() {
        assert!(!graph_gate_allows(8.499_999, 10.0));
        assert!(graph_gate_allows(8.5, 10.0));
        assert!(graph_gate_allows(10.0, 10.0));
        assert!(graph_gate_allows(0.0, 0.0));
    }

    #[test]
    fn graph_gate_clamps_a_weak_lexical_hub_in_real_ranking() {
        let entries = vec![
            test_entry(
                "RAC-STRONG000001",
                "Widget storage",
                "strong.md",
                "Widget storage keeps widget state durable.",
                0,
            ),
            test_entry(
                "RAC-WEAKHUB0001",
                "Popular reference",
                "weak-hub.md",
                "Widget.",
                20,
            ),
        ];
        let result = search_index(&entries, "widget", None, &[]);
        assert_eq!(
            result
                .matches
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["RAC-STRONG000001", "RAC-WEAKHUB0001"]
        );
        let hub = result.matches[1].evidence.as_ref().expect("search evidence");
        assert_eq!(hub.graph_gate, "clamped");
        assert!(
            hub.bm25_raw < result.matches[0].evidence.as_ref().unwrap().bm25_raw * 0.85
        );
    }

    #[test]
    fn named_target_diagnosis_distinguishes_rank_and_budget_misses() {
        let entries = vec![
            test_entry(
                "RAC-STRONG000001",
                "Widget storage",
                "strong.md",
                "Widget storage keeps widget state durable.",
                0,
            ),
            test_entry(
                "RAC-TARGET000001",
                "Secondary option",
                "target.md",
                "Widget storage remains available.",
                0,
            ),
        ];

        let ranked = diagnose_index(
            &entries,
            "widget storage",
            "RAC-TARGET000001",
            None,
            &[],
            false,
            5,
        );
        assert_eq!(ranked.reason, "ranked_below");
        assert_eq!(ranked.rank, Some(2));
        assert_eq!(ranked.outranked_by, vec!["RAC-STRONG000001"]);

        let truncated = diagnose_index(
            &entries,
            "widget storage",
            "RAC-TARGET000001",
            None,
            &[],
            false,
            1,
        );
        assert_eq!(truncated.reason, "budget_truncated");
        assert_eq!(truncated.rank, Some(2));
    }

    #[test]
    fn named_target_diagnosis_reports_no_and_partial_term_matches() {
        let entries = vec![test_entry(
            "RAC-TARGET000001",
            "Widget storage",
            "target.md",
            "Widget state remains durable.",
            0,
        )];

        let none = diagnose_index(
            &entries,
            "quorum lease",
            "RAC-TARGET000001",
            None,
            &[],
            false,
            5,
        );
        assert_eq!(none.reason, "no_term_match");
        assert_eq!(none.missing_terms, vec!["quorum", "lease"]);

        let partial = diagnose_index(
            &entries,
            "widget quorum",
            "RAC-TARGET000001",
            None,
            &[],
            false,
            5,
        );
        assert_eq!(partial.reason, "partial_term_match");
        assert_eq!(partial.matched_terms, vec!["widget"]);
        assert_eq!(partial.missing_terms, vec!["quorum"]);
    }

    #[test]
    fn persisted_field_matching_preserves_tiers_and_snippets() {
        let entry = IndexEntry {
            id: "RAC-EXAMPLE1234".to_string(),
            artifact_type: "requirement".to_string(),
            title: Some("Search latency".to_string()),
            path: "requirements/search-latency.md".to_string(),
            aliases: vec!["RAC-EXAMPLE1234".to_string(), "legacy-search".to_string()],
            search_sections: vec![
                SearchSection {
                    heading: "Acceptance Criteria".to_string(),
                    lines: vec!["Warm lookup stays below budget".to_string()],
                },
                SearchSection {
                    heading: "Risks".to_string(),
                    lines: vec!["Corpus growth may affect sorting".to_string()],
                },
            ],
            inbound_count: 2,
            tags: vec!["performance".to_string()],
        };
        let tokenized = tokenize_entry(&entry);
        for query in [
            "example1234",
            "search performance",
            "acceptance",
            "warm budget",
            "growth sorting",
            "missing",
            "search search",
        ] {
            let terms = tokenize(query);
            let fresh = match_entry(&tokenized, &terms);
            let stored = match_entry_with_fields(&entry, &tokenized.fields, &terms);
            assert_eq!(fresh.is_some(), stored.is_some(), "query {query:?}");
            if let (Some(fresh), Some(stored)) = (fresh, stored) {
                assert_eq!(fresh.rank, stored.rank, "query {query:?}: rank");
                assert_eq!(fresh.section, stored.section, "query {query:?}: section");
                assert_eq!(fresh.snippet, stored.snippet, "query {query:?}: snippet");
                assert_eq!(fresh.terms, stored.terms, "query {query:?}: terms");
            }
        }
    }
}
