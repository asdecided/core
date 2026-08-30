//! Preview-only P6 base-plus-delta document generation.
//!
//! The immutable base is shared by `Arc`; staging a change clones only the
//! current overlay.  A live document is selected as:
//!
//! `(base - tombstones) + upserts`
//!
//! P6.2 adds an independently staged identity/status projection and exact
//! resolution over the overlay. P6.3 adds token rows, postings, filters, and
//! exact global search statistics. P6.4 adds relationship rows, reverse target
//! buckets, resolved edges, and inbound counts. P6.5 adds scope/live-decision
//! rows and validated portfolio projections. P6.6 materializes durable store
//! segments from those projections; fresh derivation remains test-only.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde_json::Value;

use crate::derived::DerivedIndex;
use crate::portfolio::{portfolio_from_rows, portfolio_row, PortfolioRow};
use crate::pycompat::{py_casefold, py_strip};
use crate::relationships::{
    edge_spec, rows_from_corpus_items, CorpusItem, Relationship, ValidationRow,
    ISSUE_SELF_REFERENCE, ISSUE_TARGET_AMBIGUOUS, ISSUE_TARGET_NOT_FOUND,
};
use crate::resolve::{
    artifact_status, entry_from_item, entry_has_tags, field_tokens_of, identity_entry_from_item,
    is_retired_status, match_entry_with_fields, rank_and_build, resolved_from_entry, tokenize,
    CorpusStats, FieldTokens, IndexEntry, ResolutionResult, SearchResult, OUTCOME_DUPLICATE,
    OUTCOME_NOT_FOUND, OUTCOME_RESOLVED,
};
use crate::retrieve::{scope_rows_from_items, ScopeRow};

/// The identity/status projection for one parsed artifact. Rows are shared by
/// `Arc` so compaction promotes unchanged identities without rebuilding them.
#[derive(Clone)]
pub struct IdentityRow {
    pub entry: IndexEntry,
    pub artifact_path: crate::corpus::ArtifactPath,
    pub origin: crate::corpus::ArtifactOrigin,
    pub status: String,
}

impl IdentityRow {
    fn from_item(item: &CorpusItem) -> Self {
        Self {
            entry: identity_entry_from_item(item),
            artifact_path: item.artifact_path.clone(),
            origin: item.origin.clone(),
            status: artifact_status(&item.artifact),
        }
    }
}

/// Immutable base identity rows plus a cumulative changeset-bound overlay.
///
/// The compacted alias map stays shared. Point resolution consults that map,
/// masks base rows changed in the overlay, and scans only the (bounded) delta
/// aliases. Staging therefore never clones or walks the full base.
#[derive(Clone, Default)]
pub struct IdentityGeneration {
    base: Arc<BTreeMap<String, Arc<IdentityRow>>>,
    base_aliases: Arc<BTreeMap<String, Vec<String>>>,
    upserts: BTreeMap<String, Arc<IdentityRow>>,
    tombstones: BTreeSet<String>,
}

impl IdentityGeneration {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn stage(&self, changed: &BTreeSet<String>, parsed: &BTreeMap<String, CorpusItem>) -> Self {
        let mut next = self.clone();
        for path in changed {
            if let Some(item) = parsed.get(path) {
                next.upserts
                    .insert(path.clone(), Arc::new(IdentityRow::from_item(item)));
                next.tombstones.remove(path);
            } else {
                next.upserts.remove(path);
                if next.base.contains_key(path) {
                    next.tombstones.insert(path.clone());
                } else {
                    next.tombstones.remove(path);
                }
            }
        }
        next
    }

    pub fn from_items<'a>(items: impl IntoIterator<Item = (&'a str, &'a CorpusItem)>) -> Self {
        let base: BTreeMap<String, Arc<IdentityRow>> = items
            .into_iter()
            .map(|(path, item)| (path.to_string(), Arc::new(IdentityRow::from_item(item))))
            .collect();
        Self {
            base_aliases: Arc::new(alias_map(&base)),
            base: Arc::new(base),
            upserts: BTreeMap::new(),
            tombstones: BTreeSet::new(),
        }
    }

    pub fn promote(&mut self) {
        let mut base = self.base.as_ref().clone();
        for path in &self.tombstones {
            base.remove(path);
        }
        for (path, row) in &self.upserts {
            base.insert(path.clone(), Arc::clone(row));
        }
        self.base_aliases = Arc::new(alias_map(&base));
        self.base = Arc::new(base);
        self.upserts.clear();
        self.tombstones.clear();
    }

    pub fn resolve(&self, artifact_id: &str) -> ResolutionResult {
        let wanted = py_casefold(py_strip(artifact_id));
        let mut matches: Vec<&IndexEntry> = self
            .base_aliases
            .get(&wanted)
            .into_iter()
            .flatten()
            .filter(|path| !self.tombstones.contains(*path) && !self.upserts.contains_key(*path))
            .filter_map(|path| self.base.get(path).map(|row| &row.entry))
            .collect();
        matches.extend(
            self.upserts
                .values()
                .filter(|row| {
                    row.entry
                        .aliases
                        .iter()
                        .any(|alias| py_casefold(alias) == wanted)
                })
                .map(|row| &row.entry),
        );
        if matches.is_empty() {
            return ResolutionResult {
                artifact_id: artifact_id.to_string(),
                outcome: OUTCOME_NOT_FOUND,
                artifact: None,
                duplicate_paths: Vec::new(),
            };
        }
        if matches.len() > 1 {
            let mut duplicate_paths: Vec<String> =
                matches.iter().map(|entry| entry.path.clone()).collect();
            duplicate_paths.sort();
            return ResolutionResult {
                artifact_id: artifact_id.to_string(),
                outcome: OUTCOME_DUPLICATE,
                artifact: None,
                duplicate_paths,
            };
        }
        ResolutionResult {
            artifact_id: artifact_id.to_string(),
            outcome: OUTCOME_RESOLVED,
            artifact: Some(resolved_from_entry(matches[0])),
            duplicate_paths: Vec::new(),
        }
    }

    pub fn status_for_path(&self, path: &str) -> Option<&str> {
        if let Some(row) = self.upserts.get(path) {
            return Some(&row.status);
        }
        if self.tombstones.contains(path) {
            return None;
        }
        self.base.get(path).map(|row| row.status.as_str())
    }

    pub fn artifact_path_for_path(&self, path: &str) -> Option<&crate::corpus::ArtifactPath> {
        if let Some(row) = self.upserts.get(path) {
            return Some(&row.artifact_path);
        }
        if self.tombstones.contains(path) {
            return None;
        }
        self.base.get(path).map(|row| &row.artifact_path)
    }

    pub fn origin_for_path(&self, path: &str) -> Option<&crate::corpus::ArtifactOrigin> {
        if let Some(row) = self.upserts.get(path) {
            return Some(&row.origin);
        }
        if self.tombstones.contains(path) {
            return None;
        }
        self.base.get(path).map(|row| &row.origin)
    }

    pub fn entries(&self) -> Vec<IndexEntry> {
        self.base
            .keys()
            .chain(self.upserts.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|path| {
                self.upserts.get(path).or_else(|| {
                    (!self.tombstones.contains(path))
                        .then(|| self.base.get(path))
                        .flatten()
                })
            })
            .map(|row| row.entry.clone())
            .collect()
    }

    pub fn base_len(&self) -> usize {
        self.base.len()
    }

    pub fn upsert_len(&self) -> usize {
        self.upserts.len()
    }

    pub fn tombstone_len(&self) -> usize {
        self.tombstones.len()
    }
}

/// Immutable relationship rows plus a cumulative source overlay. Raw target
/// buckets let an identity/alias edit re-resolve only sources that mention an
/// affected identifier, including otherwise unchanged documents.
#[derive(Clone, Default)]
pub struct GraphGeneration {
    base_rows: Arc<BTreeMap<String, Arc<ValidationRow>>>,
    base_relationships: Arc<BTreeMap<String, Arc<Vec<Relationship>>>>,
    base_referrers: Arc<BTreeMap<String, Vec<String>>>,
    base_inbound: Arc<BTreeMap<String, i64>>,
    row_upserts: BTreeMap<String, Arc<ValidationRow>>,
    relationship_upserts: BTreeMap<String, Arc<Vec<Relationship>>>,
    tombstones: BTreeSet<String>,
    inbound_delta: BTreeMap<String, i64>,
}

impl GraphGeneration {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_items<'a>(
        items: impl IntoIterator<Item = (&'a str, &'a CorpusItem)>,
        identity: &IdentityGeneration,
    ) -> Self {
        let owned: Vec<(&str, &CorpusItem)> = items.into_iter().collect();
        let corpus: Vec<CorpusItem> = owned.iter().map(|(_, item)| (*item).clone()).collect();
        let rows = rows_from_corpus_items(&corpus);
        let base_rows: BTreeMap<String, Arc<ValidationRow>> = rows
            .into_iter()
            .zip(owned.iter())
            .map(|(row, (key, _))| ((*key).to_string(), Arc::new(row)))
            .collect();
        let base_relationships: BTreeMap<String, Arc<Vec<Relationship>>> = base_rows
            .iter()
            .map(|(path, row)| (path.clone(), Arc::new(resolve_graph_row(row, identity))))
            .collect();
        let base_inbound =
            inbound_for_relationships(base_relationships.values().flat_map(|v| v.iter()));
        Self {
            base_referrers: Arc::new(referrer_map(&base_rows)),
            base_rows: Arc::new(base_rows),
            base_relationships: Arc::new(base_relationships),
            base_inbound: Arc::new(base_inbound),
            row_upserts: BTreeMap::new(),
            relationship_upserts: BTreeMap::new(),
            tombstones: BTreeSet::new(),
            inbound_delta: BTreeMap::new(),
        }
    }

    pub fn stage(
        &self,
        changed: &BTreeSet<String>,
        parsed: &BTreeMap<String, CorpusItem>,
        identity: &IdentityGeneration,
    ) -> Self {
        let mut next = self.clone();
        let mut affected_aliases = BTreeSet::new();
        for path in changed {
            if let Some(old) = self.row(path) {
                affected_aliases.extend(old.identifiers.iter().map(|id| py_casefold(id)));
            }
            if let Some(item) = parsed.get(path) {
                let row = rows_from_corpus_items(std::slice::from_ref(item))
                    .into_iter()
                    .next()
                    .expect("one graph row per parsed item");
                affected_aliases.extend(row.identifiers.iter().map(|id| py_casefold(id)));
                next.row_upserts.insert(path.clone(), Arc::new(row));
                next.tombstones.remove(path);
            } else {
                next.row_upserts.remove(path);
                next.relationship_upserts.remove(path);
                if next.base_rows.contains_key(path) {
                    next.tombstones.insert(path.clone());
                } else {
                    next.tombstones.remove(path);
                }
            }
        }

        let mut affected_sources = changed.clone();
        for alias in affected_aliases {
            if let Some(paths) = self.base_referrers.get(&alias) {
                affected_sources.extend(paths.iter().cloned());
            }
            for (path, row) in &next.row_upserts {
                if row_references(row, &alias) {
                    affected_sources.insert(path.clone());
                }
            }
        }
        for path in affected_sources {
            if let Some(edges) = self.relationships_for_source(&path) {
                adjust_inbound(&mut next.inbound_delta, edges, -1);
            }
            if let Some(row) = next.row(&path) {
                let edges = Arc::new(resolve_graph_row(row, identity));
                adjust_inbound(&mut next.inbound_delta, &edges, 1);
                next.relationship_upserts.insert(path, edges);
            } else {
                next.relationship_upserts.remove(&path);
            }
        }
        next
    }

    pub fn promote(&mut self) {
        let mut rows = self.base_rows.as_ref().clone();
        let mut relationships = self.base_relationships.as_ref().clone();
        for path in &self.tombstones {
            rows.remove(path);
            relationships.remove(path);
        }
        for (path, row) in &self.row_upserts {
            rows.insert(path.clone(), Arc::clone(row));
        }
        for (path, edges) in &self.relationship_upserts {
            relationships.insert(path.clone(), Arc::clone(edges));
        }
        self.base_referrers = Arc::new(referrer_map(&rows));
        self.base_inbound = Arc::new(inbound_for_relationships(
            relationships.values().flat_map(|edges| edges.iter()),
        ));
        self.base_rows = Arc::new(rows);
        self.base_relationships = Arc::new(relationships);
        self.row_upserts.clear();
        self.relationship_upserts.clear();
        self.tombstones.clear();
        self.inbound_delta.clear();
    }

    pub fn relationships(&self) -> Vec<Relationship> {
        self.base_relationships
            .keys()
            .chain(self.relationship_upserts.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|path| self.relationships_for_source(path))
            .flat_map(|edges| edges.iter().cloned())
            .collect()
    }

    pub fn inbound_count(&self, path: &str) -> i64 {
        self.inbound_counts().get(path).copied().unwrap_or(0)
    }

    pub fn inbound_counts(&self) -> HashMap<String, i64> {
        let mut counts: HashMap<String, i64> = self
            .base_inbound
            .iter()
            .map(|(path, count)| (path.clone(), *count))
            .collect();
        for (path, delta) in &self.inbound_delta {
            let count = counts.entry(path.clone()).or_insert(0);
            *count += delta;
            if *count == 0 {
                counts.remove(path);
            }
        }
        counts
    }

    fn row(&self, path: &str) -> Option<&ValidationRow> {
        self.row_upserts.get(path).map(AsRef::as_ref).or_else(|| {
            (!self.tombstones.contains(path))
                .then(|| self.base_rows.get(path).map(AsRef::as_ref))
                .flatten()
        })
    }

    fn relationships_for_source(&self, path: &str) -> Option<&[Relationship]> {
        self.relationship_upserts
            .get(path)
            .map(|edges| edges.as_slice())
            .or_else(|| {
                (!self.tombstones.contains(path))
                    .then(|| {
                        self.base_relationships
                            .get(path)
                            .map(|edges| edges.as_slice())
                    })
                    .flatten()
            })
    }

    pub fn base_len(&self) -> usize {
        self.base_rows.len()
    }
    pub fn upsert_len(&self) -> usize {
        self.row_upserts.len()
    }
    pub fn tombstone_len(&self) -> usize {
        self.tombstones.len()
    }
}

fn adjust_inbound(deltas: &mut BTreeMap<String, i64>, edges: &[Relationship], direction: i64) {
    for edge in edges {
        if let Some(path) = &edge.resolved_path {
            *deltas.entry(path.clone()).or_insert(0) += direction;
        }
    }
    deltas.retain(|_, delta| *delta != 0);
}

fn inbound_for_relationships<'a>(
    edges: impl IntoIterator<Item = &'a Relationship>,
) -> BTreeMap<String, i64> {
    let mut inbound = BTreeMap::new();
    for edge in edges {
        if let Some(path) = &edge.resolved_path {
            *inbound.entry(path.clone()).or_insert(0) += 1;
        }
    }
    inbound
}

fn row_references(row: &ValidationRow, wanted: &str) -> bool {
    row.edges
        .iter()
        .any(|(_, refs)| refs.iter().any(|target| py_casefold(target) == wanted))
}

fn referrer_map(rows: &BTreeMap<String, Arc<ValidationRow>>) -> BTreeMap<String, Vec<String>> {
    let mut refs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, row) in rows {
        for (_, targets) in &row.edges {
            for target in targets {
                refs.entry(py_casefold(target))
                    .or_default()
                    .push(path.clone());
            }
        }
    }
    refs
}

fn resolve_graph_row(row: &ValidationRow, identity: &IdentityGeneration) -> Vec<Relationship> {
    let mut edges = Vec::new();
    for (section, targets) in &row.edges {
        let external = edge_spec(section).is_some_and(|spec| spec.external);
        for target in targets {
            let (resolved_path, resolved_artifact, issue) = if external {
                (None, None, None)
            } else {
                let result = identity.resolve(target);
                match result.outcome {
                    OUTCOME_NOT_FOUND => {
                        (None, None, Some(ISSUE_TARGET_NOT_FOUND.to_string()))
                    }
                    OUTCOME_DUPLICATE => {
                        (None, None, Some(ISSUE_TARGET_AMBIGUOUS.to_string()))
                    }
                    OUTCOME_RESOLVED => {
                        let path = result
                            .artifact
                            .expect("resolved identity has artifact")
                            .path;
                        if path == row.path {
                            (None, None, Some(ISSUE_SELF_REFERENCE.to_string()))
                        } else {
                            let artifact_path = identity.artifact_path_for_path(&path).cloned();
                            (Some(path), artifact_path, None)
                        }
                    }
                    _ => unreachable!("identity resolution outcome"),
                }
            };
            edges.push(Relationship {
                source_artifact: Some(row.artifact_path.clone()),
                source_path: row.path.clone(),
                relationship: section.clone(),
                target: target.clone(),
                resolved_artifact,
                resolved_path,
                issue,
            });
        }
    }
    edges
}

fn alias_map(rows: &BTreeMap<String, Arc<IdentityRow>>) -> BTreeMap<String, Vec<String>> {
    let mut aliases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, row) in rows {
        for alias in &row.entry.aliases {
            aliases
                .entry(py_casefold(alias))
                .or_default()
                .push(path.clone());
        }
    }
    aliases
}

/// Searchable row owned by the P6.3 generation. P6.4 supplies graph-derived
/// inbound counts at query time so the token row remains graph-independent.
#[derive(Clone)]
pub struct SearchRow {
    pub entry: IndexEntry,
    pub fields: FieldTokens,
    pub status: String,
}

impl SearchRow {
    fn from_item(item: &CorpusItem) -> Self {
        let entry = entry_from_item(item, 0);
        Self {
            fields: field_tokens_of(&entry),
            status: artifact_status(&item.artifact),
            entry,
        }
    }
}

/// Immutable compacted token/posting base plus a cumulative search overlay.
#[derive(Clone, Default)]
pub struct SearchGeneration {
    base: Arc<BTreeMap<String, Arc<SearchRow>>>,
    base_postings: Arc<BTreeMap<String, Vec<String>>>,
    base_length_sums: [i64; 6],
    upserts: BTreeMap<String, Arc<SearchRow>>,
    tombstones: BTreeSet<String>,
}

impl SearchGeneration {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_items<'a>(items: impl IntoIterator<Item = (&'a str, &'a CorpusItem)>) -> Self {
        let base: BTreeMap<String, Arc<SearchRow>> = items
            .into_iter()
            .map(|(path, item)| (path.to_string(), Arc::new(SearchRow::from_item(item))))
            .collect();
        Self {
            base_postings: Arc::new(postings_for(&base)),
            base_length_sums: length_sums(base.values().map(AsRef::as_ref)),
            base: Arc::new(base),
            upserts: BTreeMap::new(),
            tombstones: BTreeSet::new(),
        }
    }

    pub fn stage(
        &self,
        changed: &BTreeSet<String>,
        parsed: &BTreeMap<String, CorpusItem>,
    ) -> Self {
        let mut next = self.clone();
        for path in changed {
            if let Some(item) = parsed.get(path) {
                next
                    .upserts
                    .insert(path.clone(), Arc::new(SearchRow::from_item(item)));
                next.tombstones.remove(path);
            } else {
                next.upserts.remove(path);
                if next.base.contains_key(path) {
                    next.tombstones.insert(path.clone());
                } else {
                    next.tombstones.remove(path);
                }
            }
        }
        next
    }

    pub fn promote(&mut self) {
        let mut base = self.base.as_ref().clone();
        for path in &self.tombstones {
            base.remove(path);
        }
        for (path, row) in &self.upserts {
            base.insert(path.clone(), Arc::clone(row));
        }
        self.base_postings = Arc::new(postings_for(&base));
        self.base_length_sums = length_sums(base.values().map(AsRef::as_ref));
        self.base = Arc::new(base);
        self.upserts.clear();
        self.tombstones.clear();
    }

    pub fn search(
        &self,
        query: &str,
        artifact_type: Option<&str>,
        tags: &[String],
        live_only: bool,
        graph: &GraphGeneration,
    ) -> SearchResult {
        let terms = tokenize(query);
        if terms.is_empty() {
            return empty_search(query, artifact_type);
        }
        let distinct: BTreeSet<&str> = terms.iter().map(String::as_str).collect();
        let mut candidates: Option<BTreeSet<String>> = None;
        for term in distinct {
            let paths = self.paths_for_term(term);
            candidates = Some(match candidates {
                Some(current) => current.intersection(&paths).cloned().collect(),
                None => paths,
            });
            if candidates.as_ref().is_some_and(BTreeSet::is_empty) {
                return empty_search(query, artifact_type);
            }
        }

        let tag_filter: Vec<String> = tags.iter().map(|tag| py_casefold(tag)).collect();
        let inbound_by_path = graph.inbound_counts();
        let mut prepared = Vec::new();
        for path in candidates.unwrap_or_default() {
            let Some(row) = self.row(&path) else {
                continue;
            };
            if artifact_type.is_some_and(|wanted| row.entry.artifact_type != wanted) {
                continue;
            }
            if !tag_filter.is_empty() && !entry_has_tags(&row.entry, &tag_filter) {
                continue;
            }
            if live_only && is_retired_status(&row.entry.artifact_type, &row.status) {
                continue;
            }
            let Some(tier) = match_entry_with_fields(&row.entry, &row.fields, &terms) else {
                continue;
            };
            let mut entry = row.entry.clone();
            entry.inbound_count = inbound_by_path
                .get(row.entry.path.as_str())
                .copied()
                .unwrap_or(0);
            prepared.push((entry, &row.fields, tier));
        }
        if prepared.is_empty() {
            return empty_search(query, artifact_type);
        }
        let stats = self.stats(&terms);
        let matched = prepared
            .iter()
            .map(|(entry, fields, tier)| (entry, *fields, tier.clone()))
            .collect();
        rank_and_build(query, artifact_type, matched, &terms, &stats)
    }

    pub fn entries(&self, graph: &GraphGeneration) -> Vec<IndexEntry> {
        let inbound = graph.inbound_counts();
        self.base
            .keys()
            .chain(self.upserts.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|path| self.row(path))
            .map(|row| {
                let mut entry = row.entry.clone();
                entry.inbound_count = inbound.get(&entry.path).copied().unwrap_or(0);
                entry
            })
            .collect()
    }

    fn entries_and_fields(&self, graph: &GraphGeneration) -> (Vec<IndexEntry>, Vec<FieldTokens>) {
        let inbound = graph.inbound_counts();
        let mut entries = Vec::new();
        let mut fields = Vec::new();
        for path in self
            .base
            .keys()
            .chain(self.upserts.keys())
            .collect::<BTreeSet<_>>()
        {
            let Some(row) = self.row(path) else { continue };
            let mut entry = row.entry.clone();
            entry.inbound_count = inbound.get(&entry.path).copied().unwrap_or(0);
            entries.push(entry);
            fields.push(row.fields.clone());
        }
        (entries, fields)
    }

    fn row(&self, path: &str) -> Option<&SearchRow> {
        self.upserts.get(path).map(AsRef::as_ref).or_else(|| {
            (!self.tombstones.contains(path))
                .then(|| self.base.get(path).map(AsRef::as_ref))
                .flatten()
        })
    }

    fn paths_for_term(&self, term: &str) -> BTreeSet<String> {
        let mut paths = BTreeSet::new();
        for (token, posting) in self.base_postings.range(term.to_string()..) {
            if !token.starts_with(term) {
                break;
            }
            paths.extend(posting.iter().filter(|path| {
                !self.tombstones.contains(*path) && !self.upserts.contains_key(*path)
            }).cloned());
        }
        for (path, row) in &self.upserts {
            if row_has_term(row, term) {
                paths.insert(path.clone());
            }
        }
        paths
    }

    fn stats(&self, terms: &[String]) -> CorpusStats {
        let mut df = HashMap::new();
        for term in terms {
            let count = self.paths_for_term(term).len() as i64;
            *df.entry(term.clone()).or_insert(0) += count;
        }
        let mut sums = self.base_length_sums;
        for path in self.tombstones.iter().chain(self.upserts.keys()) {
            if let Some(row) = self.base.get(path) {
                subtract_lengths(&mut sums, &row.fields);
            }
        }
        for row in self.upserts.values() {
            add_lengths(&mut sums, &row.fields);
        }
        let replaced = self
            .upserts
            .keys()
            .filter(|path| self.base.contains_key(*path))
            .count();
        let n = self.base.len() - self.tombstones.len() - replaced + self.upserts.len();
        let mut avglen = [0.0; 6];
        if n != 0 {
            for (average, sum) in avglen.iter_mut().zip(sums) {
                *average = sum as f64 / n as f64;
            }
        }
        CorpusStats {
            n: n as i64,
            df,
            avglen,
        }
    }

    pub fn base_len(&self) -> usize {
        self.base.len()
    }

    pub fn upsert_len(&self) -> usize {
        self.upserts.len()
    }

    pub fn tombstone_len(&self) -> usize {
        self.tombstones.len()
    }
}

fn empty_search(query: &str, artifact_type: Option<&str>) -> SearchResult {
    SearchResult {
        query: query.to_string(),
        artifact_type: artifact_type.map(str::to_string),
        matches: Vec::new(),
    }
}

fn field_lengths(fields: &FieldTokens) -> [i64; 6] {
    [
        fields.id.len() as i64,
        fields.title.len() as i64,
        fields.path.len() as i64,
        fields.heading.len() as i64,
        fields.body.len() as i64,
        fields.tags.len() as i64,
    ]
}

fn add_lengths(sums: &mut [i64; 6], fields: &FieldTokens) {
    for (sum, length) in sums.iter_mut().zip(field_lengths(fields)) {
        *sum += length;
    }
}

fn subtract_lengths(sums: &mut [i64; 6], fields: &FieldTokens) {
    for (sum, length) in sums.iter_mut().zip(field_lengths(fields)) {
        *sum -= length;
    }
}

fn length_sums<'a>(rows: impl IntoIterator<Item = &'a SearchRow>) -> [i64; 6] {
    let mut sums = [0; 6];
    for row in rows {
        add_lengths(&mut sums, &row.fields);
    }
    sums
}

fn row_has_term(row: &SearchRow, term: &str) -> bool {
    all_tokens(&row.fields).any(|token| token.starts_with(term))
}

fn all_tokens(fields: &FieldTokens) -> impl Iterator<Item = &str> {
    fields
        .id
        .iter()
        .chain(&fields.title)
        .chain(&fields.path)
        .chain(&fields.heading)
        .chain(&fields.body)
        .chain(&fields.tags)
        .map(String::as_str)
}

fn postings_for(rows: &BTreeMap<String, Arc<SearchRow>>) -> BTreeMap<String, Vec<String>> {
    let mut postings: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (path, row) in rows {
        for token in all_tokens(&row.fields) {
            postings
                .entry(token.to_string())
                .or_default()
                .insert(path.clone());
        }
    }
    postings
        .into_iter()
        .map(|(token, paths)| (token, paths.into_iter().collect()))
        .collect()
}

/// Per-document scope and live-decision projections. An entry is present in
/// `scope` only when the document is a live decision with declared scope;
/// `live` independently tracks every live decision for topic-mode filtering.
#[derive(Clone, Default)]
pub struct ScopeGeneration {
    base_scope: Arc<BTreeMap<String, Arc<ScopeRow>>>,
    base_live: Arc<BTreeMap<String, String>>,
    scope_upserts: BTreeMap<String, Arc<ScopeRow>>,
    live_upserts: BTreeMap<String, String>,
    tombstones: BTreeSet<String>,
}

impl ScopeGeneration {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_items<'a>(items: impl IntoIterator<Item = (&'a str, &'a CorpusItem)>) -> Self {
        let mut scope = BTreeMap::new();
        let mut live = BTreeMap::new();
        for (path, item) in items {
            if is_live_decision_item(item) {
                live.insert(path.to_string(), item.path.clone());
            }
            if let Some(row) = scope_row(item) {
                scope.insert(path.to_string(), Arc::new(row));
            }
        }
        Self {
            base_scope: Arc::new(scope),
            base_live: Arc::new(live),
            scope_upserts: BTreeMap::new(),
            live_upserts: BTreeMap::new(),
            tombstones: BTreeSet::new(),
        }
    }

    pub fn stage(
        &self,
        changed: &BTreeSet<String>,
        parsed: &BTreeMap<String, CorpusItem>,
    ) -> Self {
        let mut next = self.clone();
        for path in changed {
            next.scope_upserts.remove(path);
            next.live_upserts.remove(path);
            if let Some(item) = parsed.get(path) {
                if is_live_decision_item(item) {
                    next.live_upserts.insert(path.clone(), item.path.clone());
                }
                if let Some(row) = scope_row(item) {
                    next.scope_upserts.insert(path.clone(), Arc::new(row));
                }
                if next.base_scope.contains_key(path) || next.base_live.contains_key(path) {
                    next.tombstones.insert(path.clone());
                } else {
                    next.tombstones.remove(path);
                }
            } else if next.base_scope.contains_key(path) || next.base_live.contains_key(path) {
                next.tombstones.insert(path.clone());
            } else {
                next.tombstones.remove(path);
            }
        }
        next
    }

    pub fn promote(&mut self) {
        let mut scope = self.base_scope.as_ref().clone();
        let mut live = self.base_live.as_ref().clone();
        for path in &self.tombstones {
            scope.remove(path);
            live.remove(path);
        }
        for path in self.scope_upserts.keys() {
            scope.remove(path);
        }
        for path in self.live_upserts.keys() {
            live.remove(path);
        }
        for (path, row) in &self.scope_upserts {
            scope.insert(path.clone(), Arc::clone(row));
        }
        for (path, display_path) in &self.live_upserts {
            live.insert(path.clone(), display_path.clone());
        }
        self.base_scope = Arc::new(scope);
        self.base_live = Arc::new(live);
        self.scope_upserts.clear();
        self.live_upserts.clear();
        self.tombstones.clear();
    }

    pub fn rows(&self) -> Vec<ScopeRow> {
        self.base_scope
            .keys()
            .chain(self.scope_upserts.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|path| {
                self.scope_upserts.get(path).or_else(|| {
                    (!self.tombstones.contains(path))
                        .then(|| self.base_scope.get(path))
                        .flatten()
                })
            })
            .map(|row| row.as_ref().clone())
            .collect()
    }

    pub fn live_paths(&self) -> Vec<String> {
        self.base_live
            .keys()
            .chain(self.live_upserts.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|path| {
                self.live_upserts.get(path).or_else(|| {
                    (!self.tombstones.contains(path))
                        .then(|| self.base_live.get(path))
                        .flatten()
                })
            })
            .cloned()
            .collect()
    }

    pub fn base_scope_len(&self) -> usize {
        self.base_scope.len()
    }
    pub fn scope_upsert_len(&self) -> usize {
        self.scope_upserts.len()
    }
}

fn is_live_decision_item(item: &CorpusItem) -> bool {
    item.spec
        .is_some_and(|spec| spec.name == crate::derived::DECISION_TYPE)
        && crate::resolve::is_live_decision(&item.artifact)
}

fn scope_row(item: &CorpusItem) -> Option<ScopeRow> {
    scope_rows_from_items(std::slice::from_ref(item))
        .into_iter()
        .next()
}

/// Incrementally validated per-document portfolio projections. Global summary
/// fields reduce these compact rows at publication time; parsing, structural
/// validation, and completeness classification remain change-bound.
#[derive(Clone, Default)]
pub struct SummaryGeneration {
    base: Arc<BTreeMap<String, Arc<PortfolioRow>>>,
    upserts: BTreeMap<String, Arc<PortfolioRow>>,
    tombstones: BTreeSet<String>,
}

impl SummaryGeneration {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_items<'a>(items: impl IntoIterator<Item = (&'a str, &'a CorpusItem)>) -> Self {
        Self {
            base: Arc::new(
                items
                    .into_iter()
                    .map(|(path, item)| (path.to_string(), Arc::new(portfolio_row(item))))
                    .collect(),
            ),
            upserts: BTreeMap::new(),
            tombstones: BTreeSet::new(),
        }
    }

    pub fn stage(
        &self,
        changed: &BTreeSet<String>,
        parsed: &BTreeMap<String, CorpusItem>,
    ) -> Self {
        let mut next = self.clone();
        for path in changed {
            if let Some(item) = parsed.get(path) {
                next
                    .upserts
                    .insert(path.clone(), Arc::new(portfolio_row(item)));
                next.tombstones.remove(path);
            } else {
                next.upserts.remove(path);
                if next.base.contains_key(path) {
                    next.tombstones.insert(path.clone());
                } else {
                    next.tombstones.remove(path);
                }
            }
        }
        next
    }

    pub fn value(&self, directory: &str, recursive: bool) -> Value {
        let rows: Vec<PortfolioRow> = self
            .base
            .keys()
            .chain(self.upserts.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|path| {
                self.upserts.get(path).or_else(|| {
                    (!self.tombstones.contains(path))
                        .then(|| self.base.get(path))
                        .flatten()
                })
            })
            .map(|row| row.as_ref().clone())
            .collect();
        crate::output::portfolio_summary_value(&portfolio_from_rows(directory, &rows, recursive))
    }

    pub fn promote(&mut self) {
        let mut base = self.base.as_ref().clone();
        for path in &self.tombstones {
            base.remove(path);
        }
        for (path, row) in &self.upserts {
            base.insert(path.clone(), Arc::clone(row));
        }
        self.base = Arc::new(base);
        self.upserts.clear();
        self.tombstones.clear();
    }

    pub fn base_len(&self) -> usize {
        self.base.len()
    }
    pub fn upsert_len(&self) -> usize {
        self.upserts.len()
    }
}

/// Parsed documents for an immutable base plus one cumulative overlay.
#[derive(Clone, Default)]
pub struct DeltaDocuments {
    base: Arc<BTreeMap<String, CorpusItem>>,
    upserts: BTreeMap<String, CorpusItem>,
    tombstones: BTreeSet<String>,
}

impl DeltaDocuments {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Stage one detected change set without mutating the served generation.
    pub fn stage(
        &self,
        changed: &BTreeSet<String>,
        mut parsed: BTreeMap<String, CorpusItem>,
    ) -> Self {
        let mut next = self.clone();
        for path in changed {
            if let Some(item) = parsed.remove(path) {
                next.upserts.insert(path.clone(), item);
                next.tombstones.remove(path);
            } else {
                next.upserts.remove(path);
                if next.base.contains_key(path) {
                    next.tombstones.insert(path.clone());
                } else {
                    // Adding and deleting a path within one uncompacted
                    // window leaves no trace in the base-relative overlay.
                    next.tombstones.remove(path);
                }
            }
        }
        next
    }

    /// Materialize in canonical manifest order for the still-full P6.1
    /// derivation referee.
    pub fn ordered_items<'a>(
        &self,
        ordered_paths: impl IntoIterator<Item = &'a str>,
    ) -> Vec<CorpusItem> {
        ordered_paths
            .into_iter()
            .filter_map(|path| {
                self.upserts
                    .get(path)
                    .or_else(|| {
                        if self.tombstones.contains(path) {
                            None
                        } else {
                            self.base.get(path)
                        }
                    })
                    .cloned()
            })
            .collect()
    }

    /// Fold the overlay into a new immutable base after durable compaction.
    pub fn promote<'a>(&mut self, ordered_paths: impl IntoIterator<Item = &'a str>) {
        let live = ordered_paths
            .into_iter()
            .filter_map(|path| {
                self.upserts
                    .get(path)
                    .or_else(|| {
                        if self.tombstones.contains(path) {
                            None
                        } else {
                            self.base.get(path)
                        }
                    })
                    .cloned()
                    .map(|item| (path.to_string(), item))
            })
            .collect();
        self.base = Arc::new(live);
        self.upserts.clear();
        self.tombstones.clear();
    }

    pub fn base_len(&self) -> usize {
        self.base.len()
    }

    pub fn upsert_len(&self) -> usize {
        self.upserts.len()
    }

    pub fn tombstone_len(&self) -> usize {
        self.tombstones.len()
    }

    pub fn delta_len(&self) -> usize {
        self.upserts.len() + self.tombstones.len()
    }

    pub fn live_len(&self) -> usize {
        let replaced = self
            .upserts
            .keys()
            .filter(|path| self.base.contains_key(*path))
            .count();
        self.base.len() - self.tombstones.len() - replaced + self.upserts.len()
    }

    pub fn changed_paths(&self) -> Vec<String> {
        self.upserts
            .keys()
            .chain(self.tombstones.iter())
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

/// One fully-derived, immutable logical generation published atomically.
pub struct DeltaGeneration {
    pub base_generation: u64,
    pub serving_generation: u64,
    pub changed_paths: Vec<String>,
    pub identity: IdentityGeneration,
    pub search: SearchGeneration,
    pub graph: GraphGeneration,
    pub scope: ScopeGeneration,
    pub summary: SummaryGeneration,
}

impl DeltaGeneration {
    /// Assemble the persisted/read-model bundle from the already-published
    /// incremental projections. This performs no parsing or artifact
    /// validation and is therefore suitable for durable compaction.
    pub fn materialize_derived(&self, directory: &str, recursive: bool) -> DerivedIndex {
        let (index_entries, field_tokens) = self.search.entries_and_fields(&self.graph);
        let compatibility_layer = crate::corpus::compatible_local_layer(directory);
        let normalized_root = crate::walk::normalize_root(directory);
        let display_prefix = format!("{normalized_root}/");
        let source_artifacts: Vec<crate::derived::SourceAwareArtifact> = index_entries
            .iter()
            .map(|entry| {
                let identity_path = entry
                    .path
                    .strip_prefix(&display_prefix)
                    .unwrap_or(&entry.path);
                let path = self
                    .identity
                    .artifact_path_for_path(identity_path)
                    .cloned()
                    .unwrap_or_else(|| {
                        crate::corpus::ArtifactPath::new(
                            compatibility_layer.source.clone(),
                            entry.path.clone(),
                        )
                    });
                let origin = self
                    .identity
                    .origin_for_path(identity_path)
                    .cloned()
                    .unwrap_or_else(|| compatibility_layer.origin());
                crate::derived::SourceAwareArtifact {
                    key: origin.key(entry.id.clone()),
                    path,
                    origin,
                    display_path: entry.path.clone(),
                }
            })
            .collect();
        let mut layers: Vec<crate::corpus::CorpusLayer> = source_artifacts
            .iter()
            .map(|artifact| crate::corpus::CorpusLayer::from(&artifact.origin))
            .collect();
        layers.sort();
        layers.dedup();
        if layers.is_empty() {
            layers.push(compatibility_layer);
        }
        let identity_entries = index_entries
            .iter()
            .cloned()
            .map(|mut entry| {
                entry.search_sections.clear();
                entry.inbound_count = 0;
                entry
            })
            .collect();
        let live_decision_paths = self.scope.live_paths();
        let live_path_set: std::collections::HashSet<&str> =
            live_decision_paths.iter().map(String::as_str).collect();
        let live_decision_keys = source_artifacts
            .iter()
            .filter(|artifact| live_path_set.contains(artifact.display_path.as_str()))
            .map(|artifact| artifact.key.clone())
            .collect();
        DerivedIndex {
            layers,
            source_artifacts,
            resolution: Box::new(crate::derived::ResolutionProjection {
                entries: identity_entries,
                canonical_redirects: Vec::new(),
            }),
            index_entries,
            field_tokens,
            relationships: self.graph.relationships(),
            live_decision_keys,
            live_decision_paths,
            portfolio_summary: self.summary.value(directory, recursive),
            scope_rows: self.scope.rows(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type EdgeSignature = (String, String, String, Option<String>, Option<String>);

    const DOC: &str = "---\nschema_version: 1\nid: ADR-1\ntype: decision\n---\n# ADR-1: Delta\n\n## Context\n\nTest.\n\n## Decision\n\nKeep.\n\n## Consequences\n\nNone.\n\n## Status\n\nAccepted\n";

    fn item(path: &str, id: &str, status: &str) -> CorpusItem {
        let text = DOC.replace("ADR-1", id).replace("Accepted", status);
        let artifact = crate::parse::parse_text(&text, path);
        let spec = crate::spec::spec_for(&crate::classify::classify(&artifact).artifact_type);
        CorpusItem::compatible_local_file(path, artifact, spec)
    }

    fn tagged_item(path: &str, id: &str, status: &str, tag: &str) -> CorpusItem {
        let text = DOC
            .replace("ADR-1", id)
            .replace("Accepted", status)
            .replacen(
                "type: decision\n---",
                &format!("type: decision\ntags: [{tag}]\n---"),
                1,
            );
        let artifact = crate::parse::parse_text(&text, path);
        let spec = crate::spec::spec_for(&crate::classify::classify(&artifact).artifact_type);
        CorpusItem::compatible_local_file(path, artifact, spec)
    }

    fn related_item(path: &str, id: &str, target: &str) -> CorpusItem {
        let text = format!(
            "---\nschema_version: 1\nid: {id}\ntype: requirement\n---\n# Requirement\n\n## Status\n\nAccepted\n\n## Problem\n\nTest.\n\n## Requirements\n\n- [REQ-001] Keep the graph exact.\n\n## Related Decisions\n\n- {target}\n"
        );
        let artifact = crate::parse::parse_text(&text, path);
        let spec = crate::spec::spec_for(&crate::classify::classify(&artifact).artifact_type);
        CorpusItem::compatible_local_file(path, artifact, spec)
    }

    fn scoped_item(path: &str, id: &str, status: &str, scope: Option<&str>) -> CorpusItem {
        let scope = scope
            .map(|value| format!("\n\n## Applies To\n\n- {value}"))
            .unwrap_or_default();
        let text = DOC
            .replace("ADR-1", id)
            .replace("Accepted", status)
            .replace("\n## Status", &format!("{scope}\n\n## Status"));
        let artifact = crate::parse::parse_text(&text, path);
        let spec = crate::spec::spec_for(&crate::classify::classify(&artifact).artifact_type);
        CorpusItem::compatible_local_file(path, artifact, spec)
    }

    fn scope_signature(rows: Vec<ScopeRow>) -> Vec<(String, String, String, String, Vec<String>)> {
        rows.into_iter()
            .map(|row| (row.id, row.title, row.status, row.path, row.scope_entries))
            .collect()
    }

    fn edge_signature(edges: Vec<Relationship>) -> Vec<EdgeSignature> {
        edges
            .into_iter()
            .map(|edge| {
                (
                    edge.source_path,
                    edge.relationship,
                    edge.target,
                    edge.resolved_path,
                    edge.issue,
                )
            })
            .collect()
    }

    fn assert_graph_matches_fresh(graph: &GraphGeneration, items: &BTreeMap<String, CorpusItem>) {
        let ordered: Vec<CorpusItem> = items.values().cloned().collect();
        assert_eq!(
            edge_signature(graph.relationships()),
            edge_signature(crate::relationships::relationships_from_corpus(&ordered)),
        );
        let mut expected = HashMap::new();
        for edge in crate::relationships::relationships_from_corpus(&ordered) {
            if let Some(path) = edge.resolved_path {
                *expected.entry(path).or_insert(0) += 1;
            }
        }
        assert_eq!(graph.inbound_counts(), expected);
    }

    fn assert_search_matches_fresh(
        search: &SearchGeneration,
        items: &BTreeMap<String, CorpusItem>,
        query: &str,
        artifact_type: Option<&str>,
        tags: &[String],
        live_only: bool,
    ) {
        let ordered: Vec<CorpusItem> = items.values().cloned().collect();
        let referee = crate::resolve::index_from_items(&ordered);
        let identity = IdentityGeneration::from_items(
            items.iter().map(|(path, item)| (path.as_str(), item)),
        );
        let graph = GraphGeneration::from_items(
            items.iter().map(|(path, item)| (path.as_str(), item)),
            &identity,
        );
        let expected = crate::resolve::search_index_filtered(
            &referee,
            query,
            artifact_type,
            tags,
            live_only,
        );
        let actual = search.search(query, artifact_type, tags, live_only, &graph);
        assert_eq!(
            crate::output::search_result_value(&actual, true),
            crate::output::search_result_value(&expected, true),
            "query={query} type={artifact_type:?} tags={tags:?} live={live_only}"
        );
    }

    #[test]
    fn overlay_stage_promote_and_changed_order_are_canonical() {
        let initial = BTreeMap::from([
            (
                "b.md".to_string(),
                item("b.md", "RAC-111111111111", "Accepted"),
            ),
            (
                "d.md".to_string(),
                item("d.md", "RAC-222222222222", "Accepted"),
            ),
        ]);
        let mut documents = DeltaDocuments::empty().stage(
            &BTreeSet::from(["b.md".to_string(), "d.md".to_string()]),
            initial,
        );
        documents.promote(["b.md", "d.md"]);

        documents = documents.stage(
            &BTreeSet::from(["a.md".to_string(), "b.md".to_string(), "d.md".to_string()]),
            BTreeMap::from([
                (
                    "a.md".to_string(),
                    item("a.md", "RAC-333333333333", "Proposed"),
                ),
                (
                    "d.md".to_string(),
                    item("d.md", "RAC-222222222222", "Accepted"),
                ),
            ]),
        );
        assert_eq!(documents.base_len(), 2);
        assert_eq!(documents.upsert_len(), 2);
        assert_eq!(documents.tombstone_len(), 1);
        assert_eq!(documents.live_len(), 2);
        assert_eq!(documents.changed_paths(), vec!["a.md", "b.md", "d.md"]);
        let live: Vec<String> = documents
            .ordered_items(["a.md", "d.md"])
            .into_iter()
            .map(|item| item.path)
            .collect();
        assert_eq!(live, vec!["a.md", "d.md"]);

        documents.promote(["a.md", "d.md"]);
        assert_eq!(documents.base_len(), 2);
        assert_eq!(documents.delta_len(), 0);
        assert_eq!(documents.live_len(), 2);
    }

    #[test]
    fn identity_overlay_resolves_masks_duplicates_and_promotes() {
        let base_items = BTreeMap::from([
            (
                "b.md".to_string(),
                item("b.md", "RAC-111111111111", "Accepted"),
            ),
            (
                "d.md".to_string(),
                item("d.md", "RAC-222222222222", "Accepted"),
            ),
        ]);
        let mut identity = IdentityGeneration::from_items(
            base_items.iter().map(|(path, item)| (path.as_str(), item)),
        );
        assert_eq!(
            identity.resolve(" RAC-111111111111 ").outcome,
            OUTCOME_RESOLVED
        );
        assert_eq!(identity.status_for_path("b.md"), Some("Accepted"));

        let changed = BTreeSet::from(["a.md".to_string(), "b.md".to_string(), "d.md".to_string()]);
        let parsed = BTreeMap::from([
            (
                "a.md".to_string(),
                item("a.md", "RAC-333333333333", "Proposed"),
            ),
            (
                "d.md".to_string(),
                item("d.md", "RAC-333333333333", "Accepted"),
            ),
        ]);
        let shared_base = Arc::clone(&identity.base);
        let shared_aliases = Arc::clone(&identity.base_aliases);
        identity = identity.stage(&changed, &parsed);
        assert!(Arc::ptr_eq(&shared_base, &identity.base));
        assert!(Arc::ptr_eq(&shared_aliases, &identity.base_aliases));
        assert_eq!(
            identity.resolve("RAC-111111111111").outcome,
            OUTCOME_NOT_FOUND
        );
        let duplicate = identity.resolve("RAC-333333333333");
        assert_eq!(duplicate.outcome, OUTCOME_DUPLICATE);
        assert_eq!(duplicate.duplicate_paths, vec!["a.md", "d.md"]);
        assert_eq!(identity.status_for_path("a.md"), Some("Proposed"));
        assert_eq!(identity.status_for_path("b.md"), None);
        assert_eq!(identity.base_len(), 2);
        assert_eq!(identity.upsert_len(), 2);
        assert_eq!(identity.tombstone_len(), 1);

        identity.promote();
        assert_eq!(identity.base_len(), 2);
        assert_eq!(identity.upsert_len(), 0);
        assert_eq!(identity.tombstone_len(), 0);
        assert_eq!(
            identity.resolve("RAC-333333333333").duplicate_paths,
            vec!["a.md", "d.md"]
        );
    }

    #[test]
    fn graph_overlay_re_resolves_unchanged_referrers_for_identity_changes() {
        let mut items = BTreeMap::from([
            ("a.md".to_string(), related_item("a.md", "REQ-001", "RAC-222222222222")),
            ("b.md".to_string(), item("b.md", "RAC-222222222222", "Accepted")),
        ]);
        let mut identity = IdentityGeneration::from_items(
            items.iter().map(|(path, item)| (path.as_str(), item)),
        );
        let mut graph = GraphGeneration::from_items(
            items.iter().map(|(path, item)| (path.as_str(), item)),
            &identity,
        );
        assert_graph_matches_fresh(&graph, &items);
        assert_eq!(graph.inbound_count("b.md"), 1);

        let shared_rows = Arc::clone(&graph.base_rows);
        let shared_edges = Arc::clone(&graph.base_relationships);
        let changed = BTreeSet::from(["b.md".to_string()]);
        let parsed = BTreeMap::from([(
            "b.md".to_string(),
            item("b.md", "RAC-333333333333", "Accepted"),
        )]);
        identity = identity.stage(&changed, &parsed);
        graph = graph.stage(&changed, &parsed, &identity);
        items.insert("b.md".to_string(), parsed["b.md"].clone());
        assert!(Arc::ptr_eq(&shared_rows, &graph.base_rows));
        assert!(Arc::ptr_eq(&shared_edges, &graph.base_relationships));
        assert_graph_matches_fresh(&graph, &items);
        assert_eq!(
            graph.relationships()[0].issue.as_deref(),
            Some(ISSUE_TARGET_NOT_FOUND)
        );

        let changed = BTreeSet::from(["d.md".to_string()]);
        let parsed = BTreeMap::from([(
            "d.md".to_string(),
            item("d.md", "RAC-222222222222", "Accepted"),
        )]);
        identity = identity.stage(&changed, &parsed);
        graph = graph.stage(&changed, &parsed, &identity);
        items.insert("d.md".to_string(), parsed["d.md"].clone());
        assert_graph_matches_fresh(&graph, &items);
        assert_eq!(
            graph.relationships()[0].resolved_path.as_deref(),
            Some("d.md")
        );

        graph.promote();
        identity.promote();
        assert_eq!(graph.base_len(), 3);
        assert_eq!(graph.upsert_len(), 0);
        assert_eq!(graph.tombstone_len(), 0);
        assert_graph_matches_fresh(&graph, &items);
    }

    #[test]
    fn graph_overlay_handles_source_edit_delete_and_ambiguity() {
        let mut items = BTreeMap::from([
            ("a.md".to_string(), related_item("a.md", "REQ-001", "RAC-222222222222")),
            ("b.md".to_string(), item("b.md", "RAC-222222222222", "Accepted")),
            ("c.md".to_string(), item("c.md", "RAC-333333333333", "Accepted")),
        ]);
        let mut identity = IdentityGeneration::from_items(
            items.iter().map(|(path, item)| (path.as_str(), item)),
        );
        let mut graph = GraphGeneration::from_items(
            items.iter().map(|(path, item)| (path.as_str(), item)),
            &identity,
        );

        let changed = BTreeSet::from(["a.md".to_string()]);
        let parsed = BTreeMap::from([(
            "a.md".to_string(),
            related_item("a.md", "REQ-001", "RAC-333333333333"),
        )]);
        identity = identity.stage(&changed, &parsed);
        graph = graph.stage(&changed, &parsed, &identity);
        items.insert("a.md".to_string(), parsed["a.md"].clone());
        assert_graph_matches_fresh(&graph, &items);
        assert_eq!(graph.inbound_count("c.md"), 1);

        let changed = BTreeSet::from(["b.md".to_string()]);
        let parsed = BTreeMap::from([(
            "b.md".to_string(),
            item("b.md", "RAC-333333333333", "Accepted"),
        )]);
        identity = identity.stage(&changed, &parsed);
        graph = graph.stage(&changed, &parsed, &identity);
        items.insert("b.md".to_string(), parsed["b.md"].clone());
        assert_graph_matches_fresh(&graph, &items);
        assert_eq!(
            graph.relationships()[0].issue.as_deref(),
            Some(ISSUE_TARGET_AMBIGUOUS)
        );
        assert!(graph.inbound_counts().is_empty());

        let changed = BTreeSet::from(["a.md".to_string()]);
        let parsed = BTreeMap::new();
        identity = identity.stage(&changed, &parsed);
        graph = graph.stage(&changed, &parsed, &identity);
        items.remove("a.md");
        assert_graph_matches_fresh(&graph, &items);
        assert!(graph.relationships().is_empty());
    }

    #[test]
    fn scope_overlay_tracks_live_status_scope_delete_and_promote() {
        let mut items = BTreeMap::from([
            (
                "b.md".to_string(),
                scoped_item(
                    "b.md",
                    "RAC-111111111111",
                    "Accepted",
                    Some("src/**"),
                ),
            ),
            (
                "d.md".to_string(),
                scoped_item("d.md", "RAC-222222222222", "Accepted", None),
            ),
        ]);
        let mut scope = ScopeGeneration::from_items(
            items.iter().map(|(path, item)| (path.as_str(), item)),
        );
        assert_eq!(scope.live_paths(), vec!["b.md", "d.md"]);
        assert_eq!(scope.rows().len(), 1);

        let shared_scope = Arc::clone(&scope.base_scope);
        let shared_live = Arc::clone(&scope.base_live);
        let changed = BTreeSet::from(["a.md".to_string(), "b.md".to_string()]);
        let parsed = BTreeMap::from([
            (
                "a.md".to_string(),
                scoped_item(
                    "a.md",
                    "RAC-333333333333",
                    "Accepted",
                    Some("docs/**"),
                ),
            ),
            (
                "b.md".to_string(),
                scoped_item(
                    "b.md",
                    "RAC-111111111111",
                    "Superseded",
                    Some("src/**"),
                ),
            ),
        ]);
        scope = scope.stage(&changed, &parsed);
        items.extend(parsed.clone());
        assert!(Arc::ptr_eq(&shared_scope, &scope.base_scope));
        assert!(Arc::ptr_eq(&shared_live, &scope.base_live));
        assert_eq!(scope.live_paths(), vec!["a.md", "d.md"]);
        assert_eq!(
            scope_signature(scope.rows()),
            scope_signature(scope_rows_from_items(
                &items.values().cloned().collect::<Vec<_>>()
            ))
        );

        let changed = BTreeSet::from(["d.md".to_string()]);
        scope = scope.stage(&changed, &BTreeMap::new());
        items.remove("d.md");
        assert_eq!(scope.live_paths(), vec!["a.md"]);
        assert_eq!(
            scope_signature(scope.rows()),
            scope_signature(scope_rows_from_items(
                &items.values().cloned().collect::<Vec<_>>()
            ))
        );

        scope.promote();
        assert_eq!(scope.base_scope_len(), 1);
        assert_eq!(scope.scope_upsert_len(), 0);
        assert_eq!(scope.live_paths(), vec!["a.md"]);
    }

    #[test]
    fn summary_overlay_reuses_validated_base_rows_and_matches_fresh() {
        let directory = ".";
        let mut items = BTreeMap::from([
            (
                "b.md".to_string(),
                item("b.md", "RAC-111111111111", "Accepted"),
            ),
            (
                "d.md".to_string(),
                tagged_item("d.md", "RAC-222222222222", "Accepted", "alpha"),
            ),
        ]);
        let mut summary = SummaryGeneration::from_items(
            items.iter().map(|(path, item)| (path.as_str(), item)),
        );
        let fresh = |items: &BTreeMap<String, CorpusItem>| {
            crate::output::portfolio_summary_value(&crate::portfolio::portfolio_from_corpus(
                directory,
                &items.values().cloned().collect::<Vec<_>>(),
                true,
            ))
        };
        assert_eq!(summary.value(directory, true), fresh(&items));

        let shared = Arc::clone(&summary.base);
        let changed = BTreeSet::from(["a.md".to_string(), "b.md".to_string()]);
        let parsed = BTreeMap::from([
            (
                "a.md".to_string(),
                item("a.md", "RAC-333333333333", "Proposed"),
            ),
            (
                "b.md".to_string(),
                item("b.md", "RAC-111111111111", "Superseded"),
            ),
        ]);
        summary = summary.stage(&changed, &parsed);
        items.extend(parsed);
        assert!(Arc::ptr_eq(&shared, &summary.base));
        assert_eq!(summary.value(directory, true), fresh(&items));
        assert_eq!(summary.base_len(), 2);
        assert_eq!(summary.upsert_len(), 2);

        summary.promote();
        assert_eq!(summary.base_len(), 3);
        assert_eq!(summary.upsert_len(), 0);
        assert_eq!(summary.value(directory, true), fresh(&items));
    }

    #[test]
    fn search_overlay_matches_fresh_stats_filters_and_ranking() {
        let mut items = BTreeMap::from([
            (
                "a.md".to_string(),
                tagged_item("a.md", "RAC-111111111111", "Accepted", "alpha"),
            ),
            (
                "b.md".to_string(),
                tagged_item("b.md", "RAC-222222222222", "Superseded", "beta"),
            ),
        ]);
        let mut search = SearchGeneration::from_items(
            items.iter().map(|(path, item)| (path.as_str(), item)),
        );
        for (query, artifact_type, tags, live_only) in [
            ("delta", None, vec![], false),
            ("RAC 111111111111", Some("decision"), vec![], false),
            ("delta delta", None, vec![], false),
            ("delta", None, vec!["alpha".to_string()], false),
            ("zzzz-no-match", None, vec![], false),
        ] {
            assert_search_matches_fresh(
                &search,
                &items,
                query,
                artifact_type,
                &tags,
                live_only,
            );
        }

        let shared_base = Arc::clone(&search.base);
        let shared_postings = Arc::clone(&search.base_postings);
        let changed = BTreeSet::from([
            "a.md".to_string(),
            "b.md".to_string(),
            "c.md".to_string(),
        ]);
        let parsed = BTreeMap::from([
            (
                "b.md".to_string(),
                tagged_item("b.md", "RAC-333333333333", "Accepted", "alpha"),
            ),
            (
                "c.md".to_string(),
                tagged_item("c.md", "RAC-444444444444", "Accepted", "gamma"),
            ),
        ]);
        search = search.stage(&changed, &parsed);
        assert!(Arc::ptr_eq(&shared_base, &search.base));
        assert!(Arc::ptr_eq(&shared_postings, &search.base_postings));
        items.remove("a.md");
        items.extend(parsed);
        for query in ["delta", "RAC 333333333333", "delta delta"] {
            assert_search_matches_fresh(&search, &items, query, None, &[], false);
        }
        assert_search_matches_fresh(
            &search,
            &items,
            "delta",
            None,
            &["alpha".to_string()],
            true,
        );
        assert_eq!(search.base_len(), 2);
        assert_eq!(search.upsert_len(), 2);
        assert_eq!(search.tombstone_len(), 1);

        search.promote();
        assert_eq!(search.base_len(), 2);
        assert_eq!(search.upsert_len(), 0);
        assert_eq!(search.tombstone_len(), 0);
        assert_search_matches_fresh(&search, &items, "delta", None, &[], false);
    }
}
