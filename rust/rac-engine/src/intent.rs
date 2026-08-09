//! Deterministic intent analysis (`decided.services.intent`): pure, explainable
//! checks over a `RepositoryComparison` — measurable requirements becoming
//! vague, mandatory language weakening, ambiguous wording arriving,
//! acceptance criteria / success measures disappearing, relationship
//! impact, and new scope without supporting context. Token-boundary text
//! matching and parsed-section comparison only; no semantic scoring.

use std::collections::HashMap;

use crate::compare::{
    ArtifactChange, RepoState, RepositoryComparison, CHANGE_ADDED, CHANGE_MODIFIED,
    CHANGE_REMOVED,
};
use crate::diff::diff as diff_products;
use crate::parse::Artifact;
use crate::pycompat::{is_re_digit, is_re_word, py_strip};

// Stable finding codes (part of the watchkeeper JSON contract, ADR-007).
pub const SPECIFICITY_REGRESSION: &str = "specificity_regression";
pub const AMBIGUITY_INTRODUCED: &str = "ambiguity_introduced";
pub const CONSTRAINT_WEAKENED: &str = "constraint_weakened";
pub const CONSTRAINT_REMOVED: &str = "constraint_removed";
pub const ACCEPTANCE_CRITERIA_REMOVED: &str = "acceptance_criteria_removed";
pub const SUCCESS_MEASURES_REMOVED: &str = "success_measures_removed";
pub const RELATIONSHIP_IMPACT: &str = "relationship_impact";
pub const UNLINKED_SCOPE: &str = "unlinked_scope";

pub const SEVERITY_WARNING: &str = "warning";
pub const SEVERITY_INFO: &str = "info";

/// Pinned by the v0.12.1 implementation contract. Kept in the SORTED order
/// `_ambiguous_terms` reports (the oracle sorts the matching subset of its
/// frozenset).
const AMBIGUITY_TERMS: [&str; 10] = [
    "easy",
    "fast",
    "flexible",
    "intuitive",
    "quickly",
    "robust",
    "scalable",
    "seamless",
    "simple",
    "user-friendly",
];
const MANDATORY_TERMS: [&str; 2] = ["must", "shall"];
const HEDGE_TERMS: [&str; 3] = ["should", "may", "could"];

// Normalized section headings (Product.sections keys are casefolded).
const ACCEPTANCE_SECTIONS: [&str; 1] = ["acceptance criteria"];
const SUCCESS_SECTIONS: [&str; 2] = ["success measures", "success metrics"];

/// One deterministic intent finding about a compared change.
#[derive(Debug, Clone)]
pub struct IntentFinding {
    pub code: &'static str,
    pub severity: &'static str, // SEVERITY_WARNING | SEVERITY_INFO
    pub path: String,           // corpus-relative (head side; base side for removals)
    pub identifier: Option<String>,
    pub detail: String, // one deterministic human sentence
    pub evidence: Vec<String>,
}

/// `re.search(rf"\b{re.escape(token)}\b", text, re.IGNORECASE)` for the
/// pinned ASCII vocabulary: word boundaries via the Python `\w` table,
/// ASCII-case-insensitive character match (the vocabulary has no non-ASCII
/// case pairs to worry about).
fn has_token(text: &str, token: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let tok: Vec<char> = token.chars().collect();
    let (n, m) = (chars.len(), tok.len());
    if m == 0 || m > n {
        return false;
    }
    for i in 0..=(n - m) {
        if i > 0 && is_re_word(chars[i - 1]) {
            continue; // no leading word boundary here
        }
        let matched = (0..m).all(|k| {
            let tc = chars[i + k];
            let wc = tok[k];
            tc == wc || (tc.is_ascii_alphabetic() && tc.to_ascii_lowercase() == wc)
        });
        if matched && (i + m == n || !is_re_word(chars[i + m])) {
            return true;
        }
    }
    false
}

/// Matching ambiguity terms, sorted (the const table is pre-sorted).
fn ambiguous_terms(text: &str) -> Vec<&'static str> {
    AMBIGUITY_TERMS
        .iter()
        .copied()
        .filter(|term| has_token(text, term))
        .collect()
}

fn has_digit(text: &str) -> bool {
    text.chars().any(is_re_digit)
}

fn has_mandatory(text: &str) -> bool {
    MANDATORY_TERMS.iter().any(|t| has_token(text, t))
}

fn has_hedge(text: &str) -> bool {
    HEDGE_TERMS.iter().any(|t| has_token(text, t))
}

fn section_filled(artifact: &Artifact, headings: &[&str]) -> bool {
    headings
        .iter()
        .any(|h| !py_strip(artifact.section(h).unwrap_or("")).is_empty())
}

fn quoted_join(terms: &[&str]) -> String {
    terms
        .iter()
        .map(|t| format!("'{t}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn modified_findings(
    change: &ArtifactChange,
    base: &Artifact,
    head: &Artifact,
) -> Vec<IntentFinding> {
    let mut findings: Vec<IntentFinding> = Vec::new();
    let empty = crate::diff::Diff::default();
    let diff = change.diff.as_ref().unwrap_or(&empty);

    for req_change in &diff.modified_requirements {
        let evidence = vec![
            format!("- {}", req_change.old_text),
            format!("+ {}", req_change.new_text),
        ];
        if has_digit(&req_change.old_text) && !has_digit(&req_change.new_text) {
            findings.push(IntentFinding {
                code: SPECIFICITY_REGRESSION,
                severity: SEVERITY_WARNING,
                path: change.path.clone(),
                identifier: change.id.clone(),
                detail: format!("Measurable requirement {} became vague.", req_change.id),
                evidence: evidence.clone(),
            });
        }
        let new_terms: Vec<&str> = ambiguous_terms(&req_change.new_text)
            .into_iter()
            .filter(|term| !has_token(&req_change.old_text, term))
            .collect();
        if !new_terms.is_empty() {
            findings.push(IntentFinding {
                code: AMBIGUITY_INTRODUCED,
                severity: SEVERITY_WARNING,
                path: change.path.clone(),
                identifier: change.id.clone(),
                detail: format!(
                    "Ambiguous wording introduced in {}: {}.",
                    req_change.id,
                    quoted_join(&new_terms)
                ),
                evidence: evidence.clone(),
            });
        }
        if has_mandatory(&req_change.old_text)
            && !has_mandatory(&req_change.new_text)
            && has_hedge(&req_change.new_text)
        {
            findings.push(IntentFinding {
                code: CONSTRAINT_WEAKENED,
                severity: SEVERITY_WARNING,
                path: change.path.clone(),
                identifier: change.id.clone(),
                detail: format!(
                    "Mandatory requirement {} weakened to hedged wording.",
                    req_change.id
                ),
                evidence,
            });
        }
    }

    for removed in &diff.removed_requirements {
        if has_mandatory(&removed.text) {
            findings.push(IntentFinding {
                code: CONSTRAINT_REMOVED,
                severity: SEVERITY_WARNING,
                path: change.path.clone(),
                identifier: change.id.clone(),
                detail: format!("Requirement {} with mandatory wording removed.", removed.id),
                evidence: vec![format!("- {}", removed.text)],
            });
        }
    }

    if section_filled(base, &ACCEPTANCE_SECTIONS) && !section_filled(head, &ACCEPTANCE_SECTIONS) {
        findings.push(IntentFinding {
            code: ACCEPTANCE_CRITERIA_REMOVED,
            severity: SEVERITY_WARNING,
            path: change.path.clone(),
            identifier: change.id.clone(),
            detail: "Acceptance criteria section removed.".to_string(),
            evidence: Vec::new(),
        });
    }
    if section_filled(base, &SUCCESS_SECTIONS) && !section_filled(head, &SUCCESS_SECTIONS) {
        findings.push(IntentFinding {
            code: SUCCESS_MEASURES_REMOVED,
            severity: SEVERITY_WARNING,
            path: change.path.clone(),
            identifier: change.id.clone(),
            detail: "Success measures section removed.".to_string(),
            evidence: Vec::new(),
        });
    }

    findings
}

fn removed_findings(change: &ArtifactChange, base: &RepoState) -> Vec<IntentFinding> {
    let mut findings = Vec::new();
    if let Some(entry) = base.entry(&change.path) {
        for requirement in &entry.artifact.product.requirements {
            if has_mandatory(&requirement.text) {
                findings.push(IntentFinding {
                    code: CONSTRAINT_REMOVED,
                    severity: SEVERITY_WARNING,
                    path: change.path.clone(),
                    identifier: change.id.clone(),
                    detail: format!(
                        "Requirement {} with mandatory wording removed.",
                        requirement.id
                    ),
                    evidence: vec![format!("- {}", requirement.text)],
                });
            }
        }
    }
    findings
}

fn added_findings(change: &ArtifactChange, head: &RepoState) -> Vec<IntentFinding> {
    let mut findings = Vec::new();
    if let Some(entry) = head.entry(&change.path) {
        for requirement in &entry.artifact.product.requirements {
            let terms = ambiguous_terms(&requirement.text);
            if !terms.is_empty() {
                findings.push(IntentFinding {
                    code: AMBIGUITY_INTRODUCED,
                    severity: SEVERITY_WARNING,
                    path: change.path.clone(),
                    identifier: change.id.clone(),
                    detail: format!(
                        "Ambiguous wording introduced in {}: {}.",
                        requirement.id,
                        quoted_join(&terms)
                    ),
                    evidence: vec![format!("+ {}", requirement.text)],
                });
            }
        }
    }
    findings
}

/// Incoming references (target rel-path -> source ids, in relationship
/// order) and the set of rel-paths that declare any outgoing target.
fn reference_maps(state: &RepoState) -> (HashMap<String, Vec<String>>, Vec<String>) {
    let mut incoming: HashMap<String, Vec<String>> = HashMap::new();
    let mut outgoing: Vec<String> = Vec::new();
    for relationship in &state.relationships {
        let source_rel = state.rel_of(&relationship.source_path);
        if !outgoing.contains(&source_rel) {
            outgoing.push(source_rel.clone());
        }
        if let Some(resolved) = &relationship.resolved_path {
            let target_rel = state.rel_of(resolved);
            let source_id = state
                .entry(&source_rel)
                .map(|e| e.info.id.clone())
                .unwrap_or_else(|| source_rel.clone());
            incoming.entry(target_rel).or_default().push(source_id);
        }
    }
    (incoming, outgoing)
}

fn impact_finding(
    change: &ArtifactChange,
    incoming: &HashMap<String, Vec<String>>,
    verb: &str,
) -> Option<IntentFinding> {
    let mut sources: Vec<String> = incoming.get(&change.path).cloned().unwrap_or_default();
    sources.sort();
    sources.dedup();
    if sources.is_empty() {
        return None;
    }
    Some(IntentFinding {
        code: RELATIONSHIP_IMPACT,
        severity: SEVERITY_INFO,
        path: change.path.clone(),
        identifier: change.id.clone(),
        detail: format!(
            "{verb} artifact is referenced by {} artifact(s).",
            sources.len()
        ),
        evidence: sources,
    })
}

/// Return the unique add/remove counterpart with the same opaque artifact id.
///
/// The v1 comparison contract remains path-based, so a move still appears in
/// `changes` as one add plus one removal. Intent analysis must nevertheless
/// compare the two bodies as one artifact: otherwise a path-only move reports
/// every mandatory requirement as removed. Ambiguous duplicate ids retain the
/// path-based behaviour and are left to validation.
fn move_counterpart<'a>(
    change: &ArtifactChange,
    comparison: &'a RepositoryComparison,
    counterpart_kind: &str,
) -> Option<&'a ArtifactChange> {
    let id = change.id.as_deref()?;
    if comparison
        .base
        .entries
        .iter()
        .filter(|entry| entry.info.id == id)
        .count()
        != 1
        || comparison
            .head
            .entries
            .iter()
            .filter(|entry| entry.info.id == id)
            .count()
            != 1
    {
        return None;
    }
    let same_kind_count = comparison
        .changes
        .iter()
        .filter(|candidate| {
            candidate.change == change.change && candidate.id.as_deref() == Some(id)
        })
        .count();
    if same_kind_count != 1 {
        return None;
    }
    let mut counterparts = comparison.changes.iter().filter(|candidate| {
        candidate.change == counterpart_kind && candidate.id.as_deref() == Some(id)
    });
    let counterpart = counterparts.next()?;
    if counterparts.next().is_some() {
        return None;
    }
    Some(counterpart)
}

/// `analyze_intent(comparison)` — deterministic, stably ordered findings.
pub fn analyze_intent(comparison: &RepositoryComparison) -> Vec<IntentFinding> {
    let mut findings: Vec<IntentFinding> = Vec::new();
    let (base_incoming, _) = reference_maps(&comparison.base);
    let (head_incoming, head_outgoing) = reference_maps(&comparison.head);

    for change in &comparison.changes {
        if change.change == CHANGE_MODIFIED {
            let base_product = comparison.base.entry(&change.path).map(|e| &e.artifact);
            let head_product = comparison.head.entry(&change.path).map(|e| &e.artifact);
            if let (Some(base_product), Some(head_product)) = (base_product, head_product) {
                findings.extend(modified_findings(change, base_product, head_product));
            }
            if let Some(impact) = impact_finding(change, &head_incoming, "Modified") {
                findings.push(impact);
            }
        } else if change.change == CHANGE_REMOVED {
            if let Some(added) = move_counterpart(change, comparison, CHANGE_ADDED) {
                let base_product = comparison.base.entry(&change.path).map(|e| &e.artifact);
                let head_product = comparison.head.entry(&added.path).map(|e| &e.artifact);
                if let (Some(base_product), Some(head_product)) = (base_product, head_product) {
                    let product_diff = diff_products(base_product, head_product);
                    let moved = ArtifactChange {
                        change: CHANGE_MODIFIED,
                        type_name: added.type_name.clone(),
                        id: added.id.clone(),
                        title: added.title.clone(),
                        path: added.path.clone(),
                        base_status: change.base_status,
                        head_status: added.head_status,
                        diff: if product_diff.is_empty() {
                            None
                        } else {
                            Some(product_diff)
                        },
                    };
                    findings.extend(modified_findings(&moved, base_product, head_product));
                    if let Some(impact) = impact_finding(&moved, &head_incoming, "Modified") {
                        findings.push(impact);
                    }
                }
            } else {
                findings.extend(removed_findings(change, &comparison.base));
                if let Some(impact) = impact_finding(change, &base_incoming, "Removed") {
                    findings.push(impact);
                }
            }
        } else if change.change == CHANGE_ADDED {
            if move_counterpart(change, comparison, CHANGE_REMOVED).is_some() {
                continue;
            }
            findings.extend(added_findings(change, &comparison.head));
            if change.type_name != "unknown"
                && !head_outgoing.contains(&change.path)
                && !head_incoming.contains_key(&change.path)
            {
                findings.push(IntentFinding {
                    code: UNLINKED_SCOPE,
                    severity: SEVERITY_WARNING,
                    path: change.path.clone(),
                    identifier: change.id.clone(),
                    detail: "New artifact declares no relationships and nothing references it."
                        .to_string(),
                    evidence: Vec::new(),
                });
            }
        }
    }

    findings.sort_by(|a, b| {
        (a.severity != SEVERITY_WARNING, a.code, &a.path, &a.detail).cmp(&(
            b.severity != SEVERITY_WARNING,
            b.code,
            &b.path,
            &b.detail,
        ))
    });
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::{compare_states, load_state};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "asdecided-intent-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn requirement(requirements: &str) -> String {
        format!(
            "---\nschema_version: 1\nid: RAC-KWJ4VMKVSS65\ntype: requirement\n---\n\
             # Requirement: Stable move\n\n## Status\n\nProposed\n\n## Requirements\n\n\
             {requirements}\n"
        )
    }

    fn moved_comparison(base_requirements: &str, head_requirements: &str) -> RepositoryComparison {
        let root = temp_root("move");
        let base = root.join("base");
        let head = root.join("head");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&head).unwrap();
        fs::write(base.join("old-name.md"), requirement(base_requirements)).unwrap();
        fs::write(head.join("new-name.md"), requirement(head_requirements)).unwrap();

        let base_text = base.to_string_lossy().into_owned();
        let head_text = head.to_string_lossy().into_owned();
        let comparison = compare_states(
            load_state(&base_text, "base"),
            load_state(&head_text, "head"),
        );
        fs::remove_dir_all(root).unwrap();
        comparison
    }

    #[test]
    fn identity_preserving_move_does_not_remove_constraints() {
        let requirement = "- [REQ-001] The engine MUST preserve the requirement.";
        let comparison = moved_comparison(requirement, requirement);

        assert!(comparison.changes.iter().any(|c| c.change == CHANGE_ADDED));
        assert!(comparison
            .changes
            .iter()
            .any(|c| c.change == CHANGE_REMOVED));
        assert!(!analyze_intent(&comparison)
            .iter()
            .any(|finding| finding.code == CONSTRAINT_REMOVED));
    }

    #[test]
    fn identity_preserving_move_still_reports_a_removed_constraint() {
        let comparison = moved_comparison(
            "- [REQ-001] The engine MUST preserve the requirement.\n\
             - [REQ-002] The engine MUST retain this constraint.",
            "- [REQ-001] The engine MUST preserve the requirement.",
        );
        let findings = analyze_intent(&comparison);
        let removed: Vec<&IntentFinding> = findings
            .iter()
            .filter(|finding| finding.code == CONSTRAINT_REMOVED)
            .collect();

        assert_eq!(removed.len(), 1);
        assert!(removed[0].detail.contains("REQ-002"));
        assert_eq!(removed[0].path, "new-name.md");
    }
}
