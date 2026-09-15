//! Watchkeeper report assembly (`decided.services.watchkeeper`): resolve the
//! base and head of a comparison — each an existing directory or a git
//! revision materialized through `revisions` — load both states, compare,
//! run intent analysis, and derive the deterministic review verdict.
//! `to_dict` (rendered in `output.rs`) is the stable JSON contract
//! (ADR-007, schema_version "1").

use std::path::Path;

use crate::compare::{compare_states, load_state, RepositoryComparison};
use crate::intent::{
    analyze_intent, IntentFinding, ACCEPTANCE_CRITERIA_REMOVED, CONSTRAINT_REMOVED,
    CONSTRAINT_WEAKENED, SEVERITY_WARNING, SPECIFICITY_REGRESSION, SUCCESS_MEASURES_REMOVED,
};
use crate::pycompat::{py_abspath, py_relpath};
use crate::revisions::{
    materialize_revision, repository_root, MaterializedRevision, RevisionError,
};

// Recommendation reason codes (part of the JSON contract, ADR-007).
pub const REASON_VALIDATION_REGRESSION: &str = "validation_regression";
pub const REASON_BROKEN_RELATIONSHIP: &str = "broken_relationship";

/// Findings that recommend review on their own. Ambiguity, unlinked scope,
/// and relationship impact inform but never recommend (v0.12.2 contract).
pub const RECOMMENDING_FINDINGS: [&str; 5] = [
    SPECIFICITY_REGRESSION,
    CONSTRAINT_WEAKENED,
    CONSTRAINT_REMOVED,
    ACCEPTANCE_CRITERIA_REMOVED,
    SUCCESS_MEASURES_REMOVED,
];

pub fn is_recommending(code: &str) -> bool {
    RECOMMENDING_FINDINGS.contains(&code)
}

/// Core-owned reason sentences, one per code: consumers render these, they
/// do not compose their own.
fn reason_text(code: &str) -> &'static str {
    match code {
        REASON_VALIDATION_REGRESSION => "One or more artifacts became invalid.",
        REASON_BROKEN_RELATIONSHIP => "One or more relationship references broke.",
        SPECIFICITY_REGRESSION => "A measurable requirement became vague.",
        CONSTRAINT_WEAKENED => "A mandatory requirement was weakened.",
        CONSTRAINT_REMOVED => "A requirement with mandatory wording was removed.",
        ACCEPTANCE_CRITERIA_REMOVED => "An acceptance criteria section was removed.",
        SUCCESS_MEASURES_REMOVED => "A success measures section was removed.",
        _ => "",
    }
}

/// One deterministic reason human review is recommended.
#[derive(Debug, Clone)]
pub struct ReviewRecommendation {
    pub code: &'static str,
    pub reason: &'static str,
}

/// One product knowledge review: base state, head state, what changed.
pub struct WatchkeeperReport {
    pub directory: String,
    pub base: String, // base label: revision name or directory path
    pub head: String, // head label: revision name or directory path (working tree)
    pub comparison: RepositoryComparison,
    pub findings: Vec<IntentFinding>,
    pub recommendations: Vec<ReviewRecommendation>,
}

impl WatchkeeperReport {
    pub fn review_recommended(&self) -> bool {
        !self.recommendations.is_empty()
    }

    pub fn has_warnings(&self) -> bool {
        self.findings.iter().any(|f| f.severity == SEVERITY_WARNING)
    }
}

/// The deterministic finding/delta -> reason mapping (v0.12.2): validation
/// regressions, broken relationships, then finding-driven reasons in
/// finding order, deduplicated by code.
pub fn derive_recommendations(
    comparison: &RepositoryComparison,
    findings: &[IntentFinding],
) -> Vec<ReviewRecommendation> {
    let mut codes: Vec<&'static str> = Vec::new();
    if !comparison.validation.newly_invalid.is_empty() {
        codes.push(REASON_VALIDATION_REGRESSION);
    }
    if !comparison.relationships.new_issues.is_empty() {
        codes.push(REASON_BROKEN_RELATIONSHIP);
    }
    for finding in findings {
        if is_recommending(finding.code) && !codes.contains(&finding.code) {
            codes.push(finding.code);
        }
    }
    codes
        .into_iter()
        .map(|code| ReviewRecommendation {
            code,
            reason: reason_text(code),
        })
        .collect()
}

/// A directory for one comparison side: `reference` itself when it names an
/// existing directory, or a materialization of it as a git revision.
fn resolve_side(
    guards: &mut Vec<MaterializedRevision>,
    directory: &str,
    reference: &str,
) -> Result<String, RevisionError> {
    if Path::new(reference).is_dir() {
        return Ok(reference.to_string());
    }
    let root = repository_root(directory)?;
    let subpath = py_relpath(&py_abspath(directory), &root);
    let materialized = materialize_revision(&root, reference, &subpath)?;
    let corpus = materialized.corpus.to_string_lossy().into_owned();
    guards.push(materialized);
    Ok(corpus)
}

/// `build_watchkeeper_report(directory, base=..., head=...)` — compare the
/// corpus at `directory` between `base` and `head` (`None` head = the
/// working tree at `directory`).
pub fn build_watchkeeper_report(
    directory: &str,
    base: &str,
    head: Option<&str>,
) -> Result<WatchkeeperReport, RevisionError> {
    let head_label = head.unwrap_or(directory);
    let mut guards: Vec<MaterializedRevision> = Vec::new();
    let base_dir = resolve_side(&mut guards, directory, base)?;
    let head_dir = match head {
        Some(reference) => resolve_side(&mut guards, directory, reference)?,
        None => directory.to_string(),
    };
    let base_state = load_state(&base_dir, base);
    let head_state = load_state(&head_dir, head_label);
    let comparison = compare_states(base_state, head_state);
    let findings = analyze_intent(&comparison);
    drop(guards); // materialized revisions are removed here, like ExitStack
    let recommendations = derive_recommendations(&comparison, &findings);
    Ok(WatchkeeperReport {
        directory: directory.to_string(),
        base: base.to_string(),
        head: head_label.to_string(),
        comparison,
        findings,
        recommendations,
    })
}
