//! Repository intelligence summary (`decided.services.portfolio`), the byte-derived
//! core `decided review` composes. Walk -> per-artifact validation/completeness ->
//! relationship summary + gate -> attention items + health score.

use crate::classify::missing_sections;
use crate::pycompat::py_round;
use crate::relationships::{
    summary_from_rows, validation_from_rows, CorpusItem, RelationshipSummary,
    ValidationRow, ISSUE_SELF_REFERENCE, ISSUE_TARGET_AMBIGUOUS, ISSUE_TARGET_NOT_FOUND,
};
use crate::validate::{apply_overrides, has_errors, load_overrides, py_title, validate, SeverityOverrides};

// Stable attention codes (JSON contract, ADR-007).
pub const ATTENTION_INVALID: &str = "invalid-artifact";
pub const ATTENTION_MISSING_RECOMMENDED: &str = "missing-recommended-sections";
pub const ATTENTION_BROKEN_RELATIONSHIP: &str = "broken-relationship";

/// `by_type` insertion order: the five specs then unknown.
const BY_TYPE_ORDER: [&str; 6] = [
    "requirement",
    "decision",
    "roadmap",
    "prompt",
    "design",
    "unknown",
];

#[derive(Debug, Clone)]
pub struct AttentionItem {
    pub path: String,
    pub identifier: String,
    pub severity: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug)]
pub struct PortfolioSummary {
    pub directory: String,
    pub recursive: bool,
    /// Ordered `{type: count}` including unknown.
    pub by_type: Vec<(String, usize)>,
    pub valid_artifacts: usize,
    pub invalid_artifacts: usize,
    pub recommended_slots: usize,
    pub filled_slots: usize,
    pub relationships: RelationshipSummary,
    pub attention: Vec<AttentionItem>,
    pub unknown_paths: Vec<String>,
    pub relationships_ok: bool,
}

impl PortfolioSummary {
    pub fn total_artifacts(&self) -> usize {
        self.by_type.iter().map(|(_, c)| c).sum()
    }

    pub fn completeness(&self) -> f64 {
        if self.recommended_slots == 0 {
            return 1.0;
        }
        py_round(self.filled_slots as f64 / self.recommended_slots as f64, 4)
    }

    pub fn health_score(&self) -> i64 {
        let total = self.total_artifacts();
        let validity = if total != 0 {
            self.valid_artifacts as f64 / total as f64
        } else {
            1.0
        };
        let completeness = self.completeness();
        let checked = self.relationships.total;
        let rel_integrity = if checked != 0 {
            (checked - self.relationships.broken) as f64 / checked as f64
        } else {
            1.0
        };
        let raw = 0.5 * validity + 0.25 * completeness + 0.25 * rel_integrity;
        py_round(100.0 * raw, 0) as i64
    }
}

/// Per-artifact projection matching `PortfolioRow`.
#[derive(Clone)]
pub struct PortfolioRow {
    path: String,
    artifact_type: String,
    identifier: String,
    validation: ValidationRow,
    validate_issues: Vec<crate::parse::Issue>,
    recommended_slots: usize,
    missing_recommended: Vec<String>,
}

pub fn portfolio_row(item: &CorpusItem) -> PortfolioRow {
    let path = item.path.clone();
    let artifact_type = item
        .spec
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let vrow = crate::relationships::validation_row_from_item(item);
    match item.spec {
        None => PortfolioRow {
            path,
            artifact_type,
            identifier: vrow.canonical_id.clone(),
            validation: vrow,
            validate_issues: Vec::new(),
            recommended_slots: 0,
            missing_recommended: Vec::new(),
        },
        Some(spec) => {
            let (_, missing_rec) = missing_sections(&item.artifact, spec);
            let identifier =
                crate::identity::artifact_identifier(&item.artifact, item.spec, &path);
            PortfolioRow {
                path,
                artifact_type: artifact_type.clone(),
                identifier,
                validation: vrow,
                validate_issues: validate(&item.artifact, None, Some(&artifact_type)),
                recommended_slots: spec.recommended.len(),
                missing_recommended: missing_rec,
            }
        }
    }
}

fn rel_issue_phrase(code: &str) -> &'static str {
    match code {
        ISSUE_TARGET_NOT_FOUND => "references missing artifact",
        ISSUE_TARGET_AMBIGUOUS => "has an ambiguous reference to",
        ISSUE_SELF_REFERENCE => "references itself via",
        _ => "has an unresolved reference",
    }
}

/// `portfolio_from_corpus(directory, entries, recursive)`.
pub fn portfolio_from_corpus(
    directory: &str,
    items: &[CorpusItem],
    recursive: bool,
) -> PortfolioSummary {
    let rows: Vec<PortfolioRow> = items
        .iter()
        .map(|item| {
            let mut row = portfolio_row(item);
            if item.origin.layer == crate::corpus::Layer::Inherited {
                // Parent structural warnings and completeness advisories are
                // emitted by the parent corpus, not duplicated into every
                // child summary. Parent errors already block composition.
                row.validate_issues.clear();
                row.missing_recommended.clear();
            }
            row
        })
        .collect();
    portfolio_from_rows(directory, &rows, recursive)
}

pub fn portfolio_from_rows(
    directory: &str,
    rows: &[PortfolioRow],
    recursive: bool,
) -> PortfolioSummary {
    let validation_rows: Vec<ValidationRow> = rows.iter().map(|r| r.validation.clone()).collect();
    let overrides: SeverityOverrides = load_overrides(directory);
    let rel_summary = summary_from_rows(&validation_rows);
    let relationships_ok =
        validation_from_rows(directory, &validation_rows, recursive).ok();
    portfolio_from_rows_with_analysis(
        directory,
        rows,
        recursive,
        &overrides,
        rel_summary,
        relationships_ok,
    )
}

/// Build a portfolio from the central composed relationship projection and
/// the exact child-config snapshot belonging to the generation.
pub(crate) fn portfolio_from_corpus_with_analysis(
    directory: &str,
    items: &[CorpusItem],
    recursive: bool,
    overrides: &SeverityOverrides,
    relationship_summary: RelationshipSummary,
    relationships_ok: bool,
) -> PortfolioSummary {
    let rows: Vec<PortfolioRow> = items.iter().map(portfolio_row).collect();
    portfolio_from_rows_with_analysis(
        directory,
        &rows,
        recursive,
        overrides,
        relationship_summary,
        relationships_ok,
    )
}

fn portfolio_from_rows_with_analysis(
    directory: &str,
    rows: &[PortfolioRow],
    recursive: bool,
    overrides: &SeverityOverrides,
    rel_summary: RelationshipSummary,
    relationships_ok: bool,
) -> PortfolioSummary {

    let mut by_type: Vec<(String, usize)> =
        BY_TYPE_ORDER.iter().map(|t| (t.to_string(), 0)).collect();
    let bump = |by_type: &mut Vec<(String, usize)>, t: &str| {
        if let Some(entry) = by_type.iter_mut().find(|(k, _)| k == t) {
            entry.1 += 1;
        } else {
            by_type.push((t.to_string(), 1));
        }
    };

    let mut valid_count = 0usize;
    let mut invalid_count = 0usize;
    let mut recommended_slots = 0usize;
    let mut filled_slots = 0usize;
    let mut attention: Vec<AttentionItem> = Vec::new();
    let mut unknown_paths: Vec<String> = Vec::new();
    let mut path_to_identifier: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for row in rows {
        bump(&mut by_type, &row.artifact_type);
        if row.validation.spec_name.is_none() {
            unknown_paths.push(row.path.clone());
            continue;
        }
        path_to_identifier.insert(row.path.clone(), row.identifier.clone());

        let issues = apply_overrides(row.validate_issues.clone(), &row.artifact_type, overrides);
        if has_errors(&issues) {
            invalid_count += 1;
            let error_codes: Vec<String> = issues
                .iter()
                .filter(|i| i.severity == "error")
                .map(|i| i.code.clone())
                .collect();
            attention.push(AttentionItem {
                path: row.path.clone(),
                identifier: row.identifier.clone(),
                severity: "error".to_string(),
                code: ATTENTION_INVALID.to_string(),
                message: format!("Validation errors: {}", error_codes.join(", ")),
            });
        } else {
            valid_count += 1;
        }

        let slots = row.recommended_slots;
        recommended_slots += slots;
        let missing_rec = &row.missing_recommended;
        filled_slots += slots - missing_rec.len();
        if !missing_rec.is_empty() {
            let names: Vec<String> = missing_rec.iter().map(|s| py_title(s)).collect();
            attention.push(AttentionItem {
                path: row.path.clone(),
                identifier: row.identifier.clone(),
                severity: "warning".to_string(),
                code: ATTENTION_MISSING_RECOMMENDED.to_string(),
                message: format!("Missing recommended sections: {}", names.join(", ")),
            });
        }
    }

    for issue in &rel_summary.issues {
        let source = issue.source_path.clone().unwrap_or_default();
        let label = py_title(&issue.relationship.clone().unwrap_or_default().replace('_', " "));
        let phrase = rel_issue_phrase(&issue.code);
        let identifier = path_to_identifier
            .get(&source)
            .cloned()
            .unwrap_or_else(|| source.clone());
        attention.push(AttentionItem {
            path: source,
            identifier,
            severity: "warning".to_string(),
            code: ATTENTION_BROKEN_RELATIONSHIP.to_string(),
            message: format!(
                "{label} {phrase}: {}",
                issue.target.clone().unwrap_or_default()
            ),
        });
    }

    // Sort: errors before warnings, then path, then code.
    let sev_order = |s: &str| match s {
        "error" => 0,
        "warning" => 1,
        _ => 2,
    };
    attention.sort_by(|a, b| {
        sev_order(&a.severity)
            .cmp(&sev_order(&b.severity))
            .then(a.path.cmp(&b.path))
            .then(a.code.cmp(&b.code))
    });

    PortfolioSummary {
        directory: directory.to_string(),
        recursive,
        by_type,
        valid_artifacts: valid_count,
        invalid_artifacts: invalid_count,
        recommended_slots,
        filled_slots,
        relationships: rel_summary,
        attention,
        unknown_paths,
        relationships_ok,
    }
}
