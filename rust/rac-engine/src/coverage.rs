//! Traceability coverage report (`decided.services.coverage`) — typed
//! completeness gaps derived from the resolved relationship graph.
//! Advisory, never a build failure: `decided coverage` always exits 0 on a
//! real directory. Three gap classes, one type and one expected edge
//! direction each:
//!
//! - **unscheduled** — a requirement with no resolved INCOMING edge from a
//!   roadmap,
//! - **unapplied** — a decision with no resolved incoming edge from a
//!   requirement or roadmap,
//! - **unscoped** — a roadmap with no resolved OUTGOING edge to a
//!   requirement.
//!
//! Self-edges (`resolved_path == source_path`) are skipped; external and
//! unresolved references contribute nothing (`resolved_path` is None).
//! Order is deterministic: gap class (unscheduled, unapplied, unscoped),
//! then ascending path.

use std::collections::{HashMap, HashSet};

use crate::corpus::{ArtifactOrigin, ArtifactPath};
use crate::identity::artifact_identifier;
use crate::relationships::{corpus_items, relationships_from_corpus, CorpusItem, Relationship};

pub const GAP_UNSCHEDULED: &str = "unscheduled";
pub const GAP_UNAPPLIED: &str = "unapplied";
pub const GAP_UNSCOPED: &str = "unscoped";

/// The per-class missing-coverage description (`_MISSING`).
fn missing_text(gap: &str) -> &'static str {
    match gap {
        GAP_UNSCHEDULED => "no roadmap schedules this requirement",
        GAP_UNAPPLIED => "no requirement or roadmap applies this decision",
        _ => "this roadmap references no requirement",
    }
}

/// One typed traceability gap (`CoverageGap`).
#[derive(Debug)]
pub struct CoverageGap {
    pub path: String,
    pub artifact_path: Option<ArtifactPath>,
    pub origin: Option<ArtifactOrigin>,
    pub id: String,
    pub artifact_type: String,
    pub gap: &'static str,
    pub missing: &'static str,
}

/// The coverage report for a directory (`CoverageReport`).
#[derive(Debug)]
pub struct CoverageReport {
    pub directory: String,
    pub gaps: Vec<CoverageGap>,
}

impl CoverageReport {
    /// `counts` — `(unscheduled, unapplied, unscoped)`.
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut out = (0usize, 0usize, 0usize);
        for gap in &self.gaps {
            match gap.gap {
                GAP_UNSCHEDULED => out.0 += 1,
                GAP_UNAPPLIED => out.1 += 1,
                _ => out.2 += 1,
            }
        }
        out
    }
}

fn class_order(gap: &str) -> usize {
    match gap {
        GAP_UNSCHEDULED => 0,
        GAP_UNAPPLIED => 1,
        _ => 2,
    }
}

/// `analyze_coverage(directory)` — always recursive, no writes, no git.
pub fn analyze_coverage(directory: &str) -> CoverageReport {
    let items = corpus_items(directory, true);
    let relationships = relationships_from_corpus(&items);
    analyze_coverage_from_items(directory, &items, &relationships, false)
}

/// Coverage over one already-composed effective graph. Source-aware artifact
/// paths prevent equal local/parent relative paths from collapsing into one
/// node; public provenance is additive only on this federated seam.
pub fn analyze_coverage_from_composed(
    directory: &str,
    corpus: &crate::composition::ComposedCorpus,
) -> CoverageReport {
    let items: Vec<CorpusItem> = corpus.effective().cloned().collect();
    let relationships = corpus.relationships();
    analyze_coverage_from_items(directory, &items, &relationships, true)
}

fn analyze_coverage_from_items(
    directory: &str,
    items: &[CorpusItem],
    relationships: &[Relationship],
    include_provenance: bool,
) -> CoverageReport {
    // The identity index rows coverage reads: (path, id, type) per artifact,
    // unknown documents included with type "unknown" (they never gap).
    let index: Vec<(String, ArtifactPath, ArtifactOrigin, String, String)> = items
        .iter()
        .map(|item| {
            let artifact_type = item
                .spec
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let id = artifact_identifier(&item.artifact, item.spec, &item.path);
            (
                item.path.clone(),
                item.artifact_path.clone(),
                item.origin.clone(),
                id,
                artifact_type,
            )
        })
        .collect();
    let type_by_path: HashMap<&ArtifactPath, &str> = index
        .iter()
        .map(|(_, artifact_path, _, _, artifact_type)| (artifact_path, artifact_type.as_str()))
        .collect();

    // Resolved incoming source types and resolved outgoing target types.
    let mut incoming_types: HashMap<&ArtifactPath, HashSet<&str>> = index
        .iter()
        .map(|(_, artifact_path, _, _, _)| (artifact_path, HashSet::new()))
        .collect();
    let mut outgoing_types: HashMap<&ArtifactPath, HashSet<&str>> = index
        .iter()
        .map(|(_, artifact_path, _, _, _)| (artifact_path, HashSet::new()))
        .collect();
    for rel in relationships {
        let (Some(source), Some(resolved)) = (
            rel.source_artifact.as_ref(),
            rel.resolved_artifact.as_ref(),
        ) else {
            continue;
        };
        if resolved == source {
            continue;
        }
        let source_type = type_by_path.get(source).copied();
        let target_type = type_by_path.get(resolved).copied();
        if let (Some(types), Some(source_type)) = (incoming_types.get_mut(resolved), source_type) {
            types.insert(source_type);
        }
        if let (Some(types), Some(target_type)) = (outgoing_types.get_mut(source), target_type) {
            types.insert(target_type);
        }
    }

    let mut gaps: Vec<CoverageGap> = Vec::new();
    for (path, artifact_path, origin, id, artifact_type) in &index {
        let incoming = &incoming_types[artifact_path];
        let gap = match artifact_type.as_str() {
            "requirement" if !incoming.contains("roadmap") => GAP_UNSCHEDULED,
            "decision" if !incoming.contains("requirement") && !incoming.contains("roadmap") => {
                GAP_UNAPPLIED
            }
            "roadmap" if !outgoing_types[artifact_path].contains("requirement") => GAP_UNSCOPED,
            _ => continue,
        };
        gaps.push(CoverageGap {
            path: path.clone(),
            artifact_path: include_provenance.then(|| artifact_path.clone()),
            origin: include_provenance.then(|| origin.clone()),
            id: id.clone(),
            artifact_type: artifact_type.clone(),
            gap,
            missing: missing_text(gap),
        });
    }

    // Deterministic order: gap class, then ascending path (REQ-003).
    gaps.sort_by(|a, b| {
        class_order(a.gap)
            .cmp(&class_order(b.gap))
            .then_with(|| a.artifact_path.cmp(&b.artifact_path))
            .then_with(|| a.path.cmp(&b.path))
    });
    CoverageReport {
        directory: directory.to_string(),
        gaps,
    }
}
