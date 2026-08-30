//! Store-served search (ADR-104) — `decided find` answered from the memory-
//! mapped base, byte-identical to the fresh walk (INDEX-PLAN B3).
//!
//! The candidate set comes from the term-major postings and only candidates
//! are reconstructed; the corpus-global statistics the scorer needs — n and
//! the per-field Σ from the header, document frequency from the prefix
//! ranges — carry the non-matching corpus's contribution without touching
//! its rows. Matching, snippets, and ranking run through the one shared
//! `rank_and_build` tail, so warm bytes equal cold bytes by construction.
//!
//! Deliberate divergence from the oracle's warm path (PORT-CONTRACT.d/10
//! §0a): a query term listed twice contributes its document frequency once
//! per OCCURRENCE here, exactly as the fresh walk counts it. The oracle's
//! store path dedups (an ADR-112 violation recorded as an oracle defect);
//! the native engine keeps warm == cold instead.

use crate::index_store::MmapIndexReader;
use crate::resolve::{
    entry_has_tags, entry_is_retired, match_entry_with_fields, rank_and_build, tokenize,
    CorpusStats, SearchResult,
};

/// `decided find` served from the store — reproduces `search_index_filtered`.
pub fn store_search(
    reader: &MmapIndexReader,
    query: &str,
    artifact_type: Option<&str>,
    tags: &[String],
    live_only: bool,
) -> SearchResult {
    let timing = crate::timing::enabled();
    let tokenize_started = timing.then(std::time::Instant::now);
    let terms = tokenize(query);
    if let Some(started) = tokenize_started {
        crate::timing::emit(
            "search.query_tokenize",
            started.elapsed(),
            &[("terms", terms.len() as u64)],
        );
    }
    let empty = || SearchResult {
        query: query.to_string(),
        artifact_type: artifact_type.map(str::to_string),
        matches: Vec::new(),
    };
    if terms.is_empty() {
        return empty();
    }
    let tag_filter: Vec<String> = tags.iter().map(|t| crate::pycompat::py_casefold(t)).collect();

    // AND matching requires every distinct term somewhere in the document,
    // so only the intersection of their cross-field postings can match. Keep
    // the set ascending so matched rows retain walk (docid) order.
    let mut postings = Vec::new();
    let mut distinct = std::collections::HashSet::new();
    let mut postings_duration = std::time::Duration::ZERO;
    let mut merge_duration = std::time::Duration::ZERO;
    for term in &terms {
        if !distinct.insert(term.as_str()) {
            continue;
        }
        let started = timing.then(std::time::Instant::now);
        let decoded = reader.prefix_docids(term);
        if let Some(started) = started {
            postings_duration += started.elapsed();
        }
        match decoded {
            Ok(docids) => postings.push(docids),
            Err(_) => return empty(), // corrupt row mid-read: valid empty, never a crash
        }
    }
    postings.sort_by_key(std::collections::BTreeSet::len);
    let started = timing.then(std::time::Instant::now);
    let mut postings = postings.into_iter();
    let mut candidates = postings.next().unwrap_or_default();
    for docids in postings {
        candidates.retain(|docid| docids.contains(docid));
        if candidates.is_empty() {
            break;
        }
    }
    if let Some(started) = started {
        merge_duration += started.elapsed();
    }
    crate::timing::emit(
        "search.postings_decode",
        postings_duration,
        &[("terms", terms.len() as u64)],
    );
    crate::timing::emit(
        "search.candidate_merge",
        merge_duration,
        &[("candidates", candidates.len() as u64)],
    );

    let mut matched = Vec::new();
    let candidate_count = candidates.len() as u64;
    let mut row_decode_duration = std::time::Duration::ZERO;
    let mut row_tokenize_duration = std::time::Duration::ZERO;
    let mut matching_duration = std::time::Duration::ZERO;
    for docid in candidates {
        let started = timing.then(std::time::Instant::now);
        let decoded = reader.full_entry(docid);
        if let Some(started) = started {
            row_decode_duration += started.elapsed();
        }
        let Ok(entry) = decoded else {
            continue;
        };
        if let Some(t) = artifact_type {
            if entry.artifact_type != t {
                continue;
            }
        }
        if !tag_filter.is_empty() && !entry_has_tags(&entry, &tag_filter) {
            continue;
        }
        let started = timing.then(std::time::Instant::now);
        let decoded_fields = reader.field_tokens(docid);
        if let Some(started) = started {
            row_tokenize_duration += started.elapsed();
        }
        let Ok(fields) = decoded_fields else {
            continue;
        };
        let started = timing.then(std::time::Instant::now);
        let matched_entry = match_entry_with_fields(&entry, &fields, &terms);
        if let Some(started) = started {
            matching_duration += started.elapsed();
        }
        if let Some(m) = matched_entry {
            matched.push((entry, fields, m));
        }
    }
    crate::timing::emit(
        "search.row_decode",
        row_decode_duration,
        &[("candidates", candidate_count)],
    );
    crate::timing::emit(
        "search.row_tokenize",
        row_tokenize_duration,
        &[("candidates", candidate_count)],
    );
    crate::timing::emit(
        "search.matching",
        matching_duration,
        &[("matched", matched.len() as u64)],
    );
    if live_only && !matched.is_empty() {
        matched.retain(|(entry, _, _)| !entry_is_retired(entry));
    }
    if matched.is_empty() {
        return empty();
    }

    // Corpus-global statistics from the store's integer accumulators: the
    // same values the walk derives, without materialising unmatched rows.
    let n = i64::from(reader.doc_count);
    let mut avglen = [0.0f64; 6];
    for (i, sum) in reader.field_length_sums.iter().enumerate() {
        avglen[i] = if n != 0 { *sum as f64 / n as f64 } else { 0.0 };
    }
    let mut df: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut occurrences: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for term in &terms {
        *occurrences.entry(term.as_str()).or_insert(0) += 1;
    }
    for (term, occ) in occurrences {
        let doc_count = reader
            .prefix_docids(term)
            .map(|d| d.len() as i64)
            .unwrap_or(0);
        // Per-OCCURRENCE df, the fresh walk's accounting (see module doc).
        df.insert(term.to_string(), occ * doc_count);
    }
    let stats = CorpusStats { n, df, avglen };

    let scored: Vec<_> = matched
        .iter()
        .map(|(entry, fields, m)| (entry, fields, m.clone()))
        .collect();
    rank_and_build(query, artifact_type, scored, &terms, &stats)
}

/// Live-decision topic search served from the store (ADR-067): the
/// decision-typed search, then the liveness filter over the precomputed
/// source-aware live-decision keys — `ReadModelView.find_decisions`.
pub fn store_find_decisions(reader: &MmapIndexReader, topic: &str) -> SearchResult {
    let mut result = store_search(reader, topic, Some("decision"), &[], false);
    let live_keys: std::collections::HashSet<_> =
        reader.live_decision_keys().unwrap_or_default().into_iter().collect();
    result
        .matches
        .retain(|m| m.key.as_ref().is_some_and(|key| live_keys.contains(key)));
    result
}

/// Point resolution over the persisted alias map — `Fold.resolve`,
/// byte-identical to `resolve_in_index` over a walk of the same corpus.
pub fn store_resolve(
    reader: &MmapIndexReader,
    artifact_id: &str,
) -> crate::resolve::ResolutionResult {
    use crate::resolve::{OUTCOME_DUPLICATE, OUTCOME_NOT_FOUND, OUTCOME_RESOLVED};
    let wanted =
        crate::pycompat::py_casefold(crate::pycompat::py_strip(artifact_id));
    let docids = reader.alias_docids(&wanted).unwrap_or_default();
    if docids.is_empty() {
        return crate::resolve::ResolutionResult {
            artifact_id: artifact_id.to_string(),
            outcome: OUTCOME_NOT_FOUND,
            artifact: None,
            duplicate_paths: Vec::new(),
        };
    }
    if docids.len() > 1 {
        let mut paths: Vec<String> = docids
            .iter()
            .filter_map(|&docid| reader.identity_entry(docid).ok().map(|entry| entry.path))
            .collect();
        paths.sort();
        return crate::resolve::ResolutionResult {
            artifact_id: artifact_id.to_string(),
            outcome: OUTCOME_DUPLICATE,
            artifact: None,
            duplicate_paths: paths,
        };
    }
    match reader.identity_entry(docids[0]) {
        // `from_entry` copies whatever tags the resolved projection carries:
        // the store's identity rows DO persist tags (ADR-109), so the mapped
        // base resolves WITH them — exactly as the oracle's Fold does. (The
        // delta snapshot resolves over the tag-free identity projection; that
        // asymmetry is the oracle's, mirrored at the caller.)
        Ok(entry) => crate::resolve::ResolutionResult {
            artifact_id: artifact_id.to_string(),
            outcome: OUTCOME_RESOLVED,
            artifact: Some(crate::resolve::resolved_from_entry(&entry)),
            duplicate_paths: Vec::new(),
        },
        Err(_) => crate::resolve::ResolutionResult {
            artifact_id: artifact_id.to_string(),
            outcome: OUTCOME_NOT_FOUND,
            artifact: None,
            duplicate_paths: Vec::new(),
        },
    }
}

/// Every identity row of the mapped base, in docid (walk) order — the
/// materialised projection `get_related`'s graph helpers read.
pub fn store_identity_entries(
    reader: &MmapIndexReader,
) -> Vec<crate::resolve::IndexEntry> {
    (0..reader.identity_count().unwrap_or(0))
        .filter_map(|docid| reader.identity_entry(docid).ok())
        .collect()
}

/// `find_decisions_in` over already-derived structures — the fresh-build
/// arm of the cache seam (`_find_from_store`'s `else` branch).
pub fn find_decisions_in(
    entries: &[crate::resolve::IndexEntry],
    live_paths: &[String],
    topic: &str,
) -> SearchResult {
    find_decisions_in_source_aware(entries, &[], live_paths, topic)
}

/// Source-aware live-decision filter for a composed cold model. Empty
/// `live_keys` retains the released display-path behaviour.
pub fn find_decisions_in_source_aware(
    entries: &[crate::resolve::IndexEntry],
    live_keys: &[crate::corpus::ArtifactKey],
    live_paths: &[String],
    topic: &str,
) -> SearchResult {
    let mut result = crate::resolve::search_index(entries, topic, Some("decision"), &[]);
    if !live_keys.is_empty() {
        let live: std::collections::HashSet<_> = live_keys.iter().collect();
        result
            .matches
            .retain(|m| m.key.as_ref().is_some_and(|key| live.contains(key)));
        return result;
    }
    let live: std::collections::HashSet<&str> = live_paths.iter().map(String::as_str).collect();
    result.matches.retain(|m| live.contains(m.path.as_str()));
    result
}
