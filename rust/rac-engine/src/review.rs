//! Repository review (`decided.services.review`) — the prioritized, actionable
//! report `decided review` renders. Composes the portfolio summary, git-native
//! drift advisories, and an optional write-cadence nudge.

use std::path::{Path, PathBuf};

use crate::gitinfo;
use crate::portfolio::{
    portfolio_from_corpus, AttentionItem, PortfolioSummary, ATTENTION_BROKEN_RELATIONSHIP,
    ATTENTION_INVALID, ATTENTION_MISSING_RECOMMENDED,
};
use crate::relationships::{corpus_items, relationships_from_corpus, CorpusItem};

pub const PRIORITY_INVALID_ARTIFACT: i64 = 1;
pub const PRIORITY_BROKEN_RELATIONSHIP: i64 = 2;
pub const PRIORITY_UNKNOWN_ARTIFACT: i64 = 3;
pub const PRIORITY_MISSING_RECOMMENDED: i64 = 4;
pub const PRIORITY_STALE_CORPUS: i64 = 5;
pub const PRIORITY_SUSPECT_DRIFT: i64 = 6;

pub const REVIEW_UNKNOWN_ARTIFACT: &str = "unknown-artifact";
pub const REVIEW_STALE_CORPUS: &str = "stale-corpus";
pub const REVIEW_SUSPECT_ARTIFACT: &str = "suspect-artifact";

const GENERIC_IMPACT: &str = "This finding affects repository quality.";

fn impact_for(code: &str) -> &'static str {
    match code {
        ATTENTION_INVALID => {
            "The artifact fails its schema, so tooling and validation cannot trust it."
        }
        ATTENTION_BROKEN_RELATIONSHIP => {
            "A declared reference does not resolve, leaving traceability incomplete."
        }
        ATTENTION_MISSING_RECOMMENDED => {
            "Recommended sections are empty, weakening the artifact's completeness."
        }
        REVIEW_UNKNOWN_ARTIFACT => {
            "No schema matched, so required structure cannot be checked."
        }
        REVIEW_STALE_CORPUS => {
            "The write habit has stalled; product knowledge stops reflecting the work."
        }
        REVIEW_SUSPECT_ARTIFACT => {
            "A referenced artifact changed after this one did, so the reference may be stale."
        }
        _ => GENERIC_IMPACT,
    }
}

#[derive(Debug, Clone)]
pub struct ReviewIssue {
    pub priority: i64,
    pub severity: String,
    pub path: String,
    pub identifier: String,
    pub code: String,
    pub message: String,
    pub action: String,
    pub impact: String,
}

pub struct ReviewReport {
    pub directory: String,
    pub recursive: bool,
    pub portfolio: PortfolioSummary,
    pub issues: Vec<ReviewIssue>,
}

impl ReviewReport {
    pub fn ok(&self) -> bool {
        !self
            .issues
            .iter()
            .any(|i| i.priority <= PRIORITY_BROKEN_RELATIONSHIP)
    }

    /// Deduplicated suggested actions in issue (priority) order.
    pub fn actions(&self) -> Vec<String> {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut ordered = Vec::new();
        for issue in &self.issues {
            if seen.insert(issue.action.clone()) {
                ordered.push(issue.action.clone());
            }
        }
        ordered
    }
}

fn attention_priority(code: &str) -> i64 {
    match code {
        ATTENTION_INVALID => PRIORITY_INVALID_ARTIFACT,
        ATTENTION_BROKEN_RELATIONSHIP => PRIORITY_BROKEN_RELATIONSHIP,
        ATTENTION_MISSING_RECOMMENDED => PRIORITY_MISSING_RECOMMENDED,
        _ => PRIORITY_MISSING_RECOMMENDED,
    }
}

fn sort_issues(issues: &mut [ReviewIssue]) {
    issues.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then(a.path.cmp(&b.path))
            .then(a.code.cmp(&b.code))
    });
}

pub fn review_from_portfolio(
    directory: &str,
    portfolio: PortfolioSummary,
    recursive: bool,
) -> ReviewReport {
    let mut issues: Vec<ReviewIssue> = Vec::new();

    for item in &portfolio.attention {
        let AttentionItem {
            path,
            identifier,
            severity,
            code,
            message,
            origin: _,
        } = item;
        let priority = attention_priority(code);
        let action = if code == ATTENTION_INVALID {
            format!("Run: decided validate {path}")
        } else if code == ATTENTION_BROKEN_RELATIONSHIP {
            format!("Run: decided relationships {directory} --validate")
        } else {
            format!("Run: decided improve {path} --template")
        };
        issues.push(ReviewIssue {
            priority,
            severity: severity.clone(),
            path: path.clone(),
            identifier: identifier.clone(),
            code: code.clone(),
            message: message.clone(),
            action,
            impact: impact_for(code).to_string(),
        });
    }

    for path in &portfolio.unknown_paths {
        issues.push(ReviewIssue {
            priority: PRIORITY_UNKNOWN_ARTIFACT,
            severity: "info".to_string(),
            path: path.clone(),
            identifier: crate::identity::path_stem(path),
            code: REVIEW_UNKNOWN_ARTIFACT.to_string(),
            message: "No artifact schema matched this document.".to_string(),
            action: format!("Run: decided inspect {path} (see decided schema --list)"),
            impact: impact_for(REVIEW_UNKNOWN_ARTIFACT).to_string(),
        });
    }

    sort_issues(&mut issues);

    ReviewReport {
        directory: directory.to_string(),
        recursive,
        portfolio,
        issues,
    }
}

/// `build_review(directory, recursive, stale_after_days)`.
pub fn build_review(
    directory: &str,
    recursive: bool,
    stale_after_days: Option<i64>,
) -> ReviewReport {
    let items = corpus_items(directory, recursive);
    let portfolio = portfolio_from_corpus(directory, &items, recursive);
    let mut report = review_from_portfolio(directory, portfolio, recursive);

    let mut advisories: Vec<ReviewIssue> = drift_findings(directory, &items);
    if let Some(window) = stale_after_days {
        if let Some(finding) = cadence_finding(directory, &items, window) {
            advisories.push(finding);
        }
    }
    if !advisories.is_empty() {
        report.issues.extend(advisories);
        sort_issues(&mut report.issues);
    }
    report
}

pub fn build_review_composed(
    directory: &str,
    recursive: bool,
    stale_after_days: Option<i64>,
    corpus: &crate::composition::ComposedCorpus,
) -> ReviewReport {
    let items: Vec<CorpusItem> = corpus.local_items().cloned().collect();
    let portfolio = crate::portfolio::local_portfolio_from_composed(directory, corpus, recursive);
    let mut report = review_from_portfolio(directory, portfolio, recursive);
    let child_source = corpus.child_source();
    let local_relationships: Vec<crate::relationships::Relationship> = corpus
        .local_relationships()
        .into_iter()
        .filter(|relationship| {
            relationship
                .resolved_artifact
                .as_ref()
                .is_some_and(|path| Some(path.source.as_str()) == child_source)
        })
        .collect();
    let mut advisories = drift_findings_from_relationships(directory, &local_relationships);
    if let Some(window) = stale_after_days {
        if let Some(finding) = cadence_finding(directory, &items, window) {
            advisories.push(finding);
        }
    }
    if !advisories.is_empty() {
        report.issues.extend(advisories);
        sort_issues(&mut report.issues);
    }
    report
}

// --- git-native drift --------------------------------------------------------

pub(crate) struct DriftRecord {
    pub(crate) source_path: String,
    pub(crate) target_path: String,
    pub(crate) target_ref: String,
    pub(crate) source_committed: String,
    pub(crate) target_committed: String,
}

/// `suspect_drift(directory, entries)` — resolved edges whose target was
/// committed strictly after the referrer. Deduped per `(source, target)`,
/// sorted by `(source_path, target_path)`.
pub(crate) fn suspect_drift(directory: &str, items: &[CorpusItem]) -> Vec<DriftRecord> {
    let resolved: Vec<crate::relationships::Relationship> = relationships_from_corpus(items)
        .into_iter()
        .filter(|r| r.resolved_path.is_some())
        .collect();
    suspect_drift_from_relationships(directory, &resolved)
}

fn suspect_drift_from_relationships(
    directory: &str,
    resolved: &[crate::relationships::Relationship],
) -> Vec<DriftRecord> {
    if resolved.is_empty() {
        return Vec::new();
    }

    let mut involved: Vec<PathBuf> = Vec::new();
    let mut seen_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
    for rel in resolved {
        for p in [&rel.source_path, rel.resolved_path.as_ref().unwrap()] {
            if seen_paths.insert(p.clone()) {
                involved.push(PathBuf::from(p));
            }
        }
    }
    let committed_pairs = gitinfo::last_committed_for_paths(Path::new(directory), &involved);
    let committed: std::collections::HashMap<String, Option<String>> = committed_pairs
        .into_iter()
        .map(|(p, v)| (p.to_string_lossy().into_owned(), v))
        .collect();

    let mut records: Vec<DriftRecord> = Vec::new();
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    for rel in resolved {
        let target_path = rel.resolved_path.clone().unwrap();
        let source_when = committed.get(&rel.source_path).and_then(|v| v.clone());
        let target_when = committed.get(&target_path).and_then(|v| v.clone());
        let (source_when, target_when) = match (source_when, target_when) {
            (Some(s), Some(t)) => (s, t),
            _ => continue,
        };
        let source_epoch = gitinfo::parse_iso8601_epoch(&source_when);
        let target_epoch = gitinfo::parse_iso8601_epoch(&target_when);
        // target newer than source (strictly) => suspect.
        match (source_epoch, target_epoch) {
            (Some(se), Some(te)) if te > se => {}
            _ => continue,
        }
        let key = (rel.source_path.clone(), target_path.clone());
        if !seen.insert(key) {
            continue;
        }
        records.push(DriftRecord {
            source_path: rel.source_path.clone(),
            target_path: target_path.clone(),
            target_ref: rel.target.clone(),
            source_committed: source_when,
            target_committed: target_when,
        });
    }
    records.sort_by(|a, b| {
        a.source_path
            .cmp(&b.source_path)
            .then(a.target_path.cmp(&b.target_path))
    });
    records
}

fn drift_findings(directory: &str, items: &[CorpusItem]) -> Vec<ReviewIssue> {
    suspect_drift(directory, items)
        .into_iter()
        .map(|record| ReviewIssue {
            priority: PRIORITY_SUSPECT_DRIFT,
            severity: "warning".to_string(),
            path: record.source_path.clone(),
            identifier: crate::identity::path_stem(&record.source_path),
            code: REVIEW_SUSPECT_ARTIFACT.to_string(),
            message: drift_problem(&record),
            action: format!("Run: decided doctor {directory}"),
            impact: impact_for(REVIEW_SUSPECT_ARTIFACT).to_string(),
        })
        .collect()
}

fn drift_findings_from_relationships(
    directory: &str,
    relationships: &[crate::relationships::Relationship],
) -> Vec<ReviewIssue> {
    suspect_drift_from_relationships(directory, relationships)
        .into_iter()
        .map(|record| ReviewIssue {
            priority: PRIORITY_SUSPECT_DRIFT,
            severity: "warning".to_string(),
            path: record.source_path.clone(),
            identifier: crate::identity::path_stem(&record.source_path),
            code: REVIEW_SUSPECT_ARTIFACT.to_string(),
            message: drift_problem(&record),
            action: format!("Run: decided doctor {directory}"),
            impact: impact_for(REVIEW_SUSPECT_ARTIFACT).to_string(),
        })
        .collect()
}

pub(crate) fn drift_problem(record: &DriftRecord) -> String {
    format!(
        "references {} which changed more recently (target last committed {}, this artifact {}) — review recommended",
        record.target_ref,
        gitinfo::isoformat_roundtrip(&record.target_committed),
        gitinfo::isoformat_roundtrip(&record.source_committed),
    )
}

// --- write-cadence nudge -----------------------------------------------------

fn cadence_finding(
    directory: &str,
    items: &[CorpusItem],
    window_days: i64,
) -> Option<ReviewIssue> {
    // most_recent = newest last-committed across recognised (non-unknown) artifacts.
    let recognised: Vec<PathBuf> = items
        .iter()
        .filter(|i| i.spec.is_some())
        .map(|i| PathBuf::from(&i.path))
        .collect();
    if recognised.is_empty() {
        return None;
    }
    let committed = gitinfo::last_committed_for_paths(Path::new(directory), &recognised);
    let mut most_recent_epoch: Option<i64> = None;
    let mut most_recent_stamp: Option<String> = None;
    for (_, stamp) in committed {
        if let Some(stamp) = stamp {
            if let Some(epoch) = gitinfo::parse_iso8601_epoch(&stamp) {
                if most_recent_epoch.map(|e| epoch > e).unwrap_or(true) {
                    most_recent_epoch = Some(epoch);
                    most_recent_stamp = Some(stamp);
                }
            }
        }
    }
    let (most_recent_epoch, _) = (most_recent_epoch?, most_recent_stamp);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // age.days = floor((now - most_recent)/86400); age <= window => suppress.
    let delta = now - most_recent_epoch;
    let age_days = delta.div_euclid(86_400);
    if age_days <= window_days {
        return None;
    }
    Some(ReviewIssue {
        priority: PRIORITY_STALE_CORPUS,
        severity: "info".to_string(),
        path: directory.to_string(),
        identifier: "corpus".to_string(),
        code: REVIEW_STALE_CORPUS.to_string(),
        message: format!(
            "No product knowledge recorded in the last {window_days} days (newest artifact is {age_days} days old)."
        ),
        action: "Run: decided new decision decisions/decisions/<name>.md".to_string(),
        impact: impact_for(REVIEW_STALE_CORPUS).to_string(),
    })
}
