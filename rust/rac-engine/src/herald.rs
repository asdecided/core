//! Native deterministic renderer for Herald's governing-decisions PR comment.

use std::collections::{BTreeMap, BTreeSet};

use crate::corpus::{ArtifactKey, ArtifactOrigin, Layer};
use crate::retrieve::{decisions_for_path, decisions_for_path_with_rows, scope_rows_from_items};

pub const MARKER: &str = "<!-- lore-decisions-on-pr -->";
const PATHS_SHOWN: usize = 3;

#[derive(Debug)]
pub struct HeraldDecision {
    pub key: Option<ArtifactKey>,
    pub origin: Option<ArtifactOrigin>,
    pub id: String,
    pub title: String,
    pub status: String,
    pub path: String,
    pub matching_entries: BTreeSet<String>,
    pub changed_paths: BTreeSet<String>,
}

#[derive(Debug)]
pub struct HeraldReport {
    pub decisions: Vec<HeraldDecision>,
}

impl HeraldReport {
    pub fn has_decisions(&self) -> bool {
        !self.decisions.is_empty()
    }
}

pub fn collect(corpus: &str, paths: &[String], recursive: bool) -> HeraldReport {
    let mut merged: BTreeMap<String, HeraldDecision> = BTreeMap::new();
    let changed: BTreeSet<&String> = paths
        .iter()
        .filter(|path| !path.trim().is_empty())
        .collect();
    for path in changed {
        for decision in decisions_for_path(corpus, path, recursive).decisions {
            let entry = merged
                .entry(decision.id.clone())
                .or_insert_with(|| HeraldDecision {
                    key: decision.key,
                    origin: None,
                    id: decision.id,
                    title: decision.title,
                    status: decision.status,
                    path: decision.path,
                    matching_entries: BTreeSet::new(),
                    changed_paths: BTreeSet::new(),
                });
            entry.matching_entries.insert(decision.matching_entry);
            entry.changed_paths.insert(path.clone());
        }
    }
    HeraldReport {
        decisions: merged.into_values().collect(),
    }
}

/// Herald collection over one effective composed scope projection. Stable
/// ArtifactKey deduplication prevents equal canonical ids or relative paths
/// in different sources from collapsing.
pub fn collect_from_composed(
    corpus_directory: &str,
    paths: &[String],
    corpus: &crate::composition::ComposedCorpus,
) -> HeraldReport {
    let items: Vec<_> = corpus.effective().cloned().collect();
    let rows = scope_rows_from_items(&items);
    let mut merged: BTreeMap<ArtifactKey, HeraldDecision> = BTreeMap::new();
    let changed: BTreeSet<&String> = paths
        .iter()
        .filter(|path| !path.trim().is_empty())
        .collect();
    for path in changed {
        for decision in decisions_for_path_with_rows(&rows, corpus_directory, path).decisions {
            let Some(key) = decision.key.clone() else {
                continue;
            };
            let entry = merged.entry(key.clone()).or_insert_with(|| HeraldDecision {
                key: Some(key),
                origin: decision.origin,
                id: decision.id,
                title: decision.title,
                status: decision.status,
                path: decision.path,
                matching_entries: BTreeSet::new(),
                changed_paths: BTreeSet::new(),
            });
            entry.matching_entries.insert(decision.matching_entry);
            entry.changed_paths.insert(path.clone());
        }
    }
    HeraldReport {
        decisions: merged.into_values().collect(),
    }
}

fn bullet(decision: &HeraldDecision, link_base: &str) -> String {
    let scopes = decision
        .matching_entries
        .iter()
        .map(|scope| format!("`{scope}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let paths: Vec<&String> = decision.changed_paths.iter().collect();
    let mut changed = paths
        .iter()
        .take(PATHS_SHOWN)
        .map(|path| format!("`{path}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let more = paths.len().saturating_sub(PATHS_SHOWN);
    if more > 0 {
        changed.push_str(&format!(" +{more} more"));
    }
    let link = if link_base.is_empty() {
        decision.path.clone()
    } else {
        format!("{}/{}", link_base.trim_end_matches('/'), decision.path)
    };
    let label = if decision
        .origin
        .as_ref()
        .is_some_and(|origin| origin.layer == Layer::Inherited)
    {
        format!("**{} — {}**", decision.id, decision.title)
    } else {
        format!("**[{} — {}]({link})**", decision.id, decision.title)
    };
    format!(
        "- {label} ({}) — applies to {} — changed: {}",
        decision.status, scopes, changed
    )
}

pub fn render(report: &HeraldReport, link_base: &str, max_inline: i64) -> String {
    if report.decisions.is_empty() {
        return format!(
            "{MARKER}\n### Decisions governing this change\n\n\
             No recorded decisions govern the paths changed by this pull request.\n"
        );
    }
    let count = report.decisions.len();
    let plural = if count == 1 { "" } else { "s" };
    let mut lines = vec![
        MARKER.to_string(),
        "### Decisions governing this change".to_string(),
        String::new(),
        format!(
            "This pull request touches paths governed by {count} recorded decision{plural} — review recommended."
        ),
        String::new(),
    ];
    let inline_count = if max_inline > 0 {
        (max_inline as usize).min(count)
    } else {
        count
    };
    lines.extend(
        report.decisions[..inline_count]
            .iter()
            .map(|decision| bullet(decision, link_base)),
    );
    let rest = &report.decisions[inline_count..];
    if !rest.is_empty() {
        let rest_plural = if rest.len() == 1 { "" } else { "s" };
        lines.push(String::new());
        lines.push(format!(
            "<details><summary>{} more governing decision{rest_plural}</summary>",
            rest.len()
        ));
        lines.push(String::new());
        lines.extend(rest.iter().map(|decision| bullet(decision, link_base)));
        lines.push(String::new());
        lines.push("</details>".to_string());
    }
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn decision(id: &str) -> HeraldDecision {
        HeraldDecision {
            key: None,
            origin: None,
            id: id.to_string(),
            title: format!("{id} title"),
            status: "Accepted".to_string(),
            path: format!("decisions/{id}.md"),
            matching_entries: BTreeSet::from(["src/**".to_string()]),
            changed_paths: BTreeSet::from([
                "src/a.rs".to_string(),
                "src/b.rs".to_string(),
                "src/c.rs".to_string(),
                "src/d.rs".to_string(),
            ]),
        }
    }

    #[test]
    fn empty_state_is_stable() {
        let body = render(&HeraldReport { decisions: vec![] }, "", 5);
        assert!(body.starts_with(MARKER));
        assert!(body.contains("No recorded decisions govern"));
        assert!(body.ends_with('\n'));
    }

    #[test]
    fn overflow_and_path_collapse_match_contract() {
        let body = render(
            &HeraldReport {
                decisions: vec![decision("ADR-001"), decision("ADR-002")],
            },
            "https://example.com/blob/HEAD/",
            1,
        );
        assert!(body.contains("<details><summary>1 more governing decision</summary>"));
        assert!(body.contains("`src/a.rs`, `src/b.rs`, `src/c.rs` +1 more"));
        assert!(body.contains("https://example.com/blob/HEAD/decisions/ADR-001.md"));
    }

    #[test]
    fn collection_is_sorted_and_deduplicated_across_paths() {
        let root = std::env::temp_dir().join(format!("decided-herald-{}", std::process::id()));
        let corpus = root.join("decisions");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".decided")).unwrap();
        fs::create_dir_all(&corpus).unwrap();
        fs::write(root.join(".decided/config.yaml"), "repository_key: TEST\n").unwrap();
        fs::write(
            corpus.join("adr-001.md"),
            "# Alpha rule\n\n## Status\n\nAccepted\n\n## Context\n\nContext.\n\n## Decision\n\nRule.\n\n## Consequences\n\nConsequence.\n\n## Applies To\n\n- src/**/*.rs\n",
        )
        .unwrap();
        let report = collect(
            corpus.to_str().unwrap(),
            &[
                "src/a.rs".to_string(),
                "src/b.rs".to_string(),
                "src/a.rs".to_string(),
            ],
            true,
        );
        assert_eq!(report.decisions.len(), 1);
        assert_eq!(report.decisions[0].changed_paths.len(), 2);
        assert_eq!(
            report.decisions[0].matching_entries,
            BTreeSet::from(["src/**/*.rs".to_string()])
        );
        fs::remove_dir_all(root).unwrap();
    }
}
