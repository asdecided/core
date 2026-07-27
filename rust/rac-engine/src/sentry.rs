//! Deterministic decision-to-code enforcement.
//!
//! Constraints live in a decision artifact's `## Code Constraints` fenced
//! YAML block. This module deliberately has no network or model integration:
//! repository bytes, corpus bytes, and an optional git diff are the complete
//! input.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use globset::Glob;
use regex::Regex;
use serde::Deserialize;

use crate::parse::{Artifact, Issue};
use crate::relationships::{corpus_items, CorpusItem};
use crate::resolve::artifact_status;
use crate::spec::spec_for;

pub const CODE_VIOLATION: &str = "code-constraint-violation";
pub const CODE_EMPTY_MATCH: &str = "code-constraint-empty-match";
pub const CODE_UNSUPPORTED_LANGUAGE: &str = "code-constraint-unsupported-language";
pub const MALFORMED_CONSTRAINTS: &str = "malformed-code-constraints";
pub const UNSUPPORTED_VERSION: &str = "unsupported-code-constraints-version";
pub const INVALID_CONSTRAINT: &str = "invalid-code-constraint";
pub const DUPLICATE_RULE_ID: &str = "duplicate-code-constraint-id";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConstraintDocument {
    version: u64,
    rules: Vec<ConstraintRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConstraintRule {
    id: String,
    kind: RuleKind,
    path_glob: String,
    pattern: String,
    message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RuleKind {
    ForbidPattern,
    RequirePattern,
    ForbidImport,
}

#[derive(Debug, Clone)]
pub struct SentryFinding {
    pub code: &'static str,
    pub decision_path: String,
    pub rule_id: Option<String>,
    pub path: String,
    pub line: Option<i64>,
    pub message: String,
}

#[derive(Debug)]
pub struct SentryReport {
    pub corpus: String,
    pub repository: String,
    pub base: Option<String>,
    pub full_tree: bool,
    pub live_decisions: usize,
    pub constrained_decisions: usize,
    pub findings: Vec<SentryFinding>,
}

impl SentryReport {
    pub fn ok(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn coverage_percent(&self) -> f64 {
        if self.live_decisions == 0 {
            0.0
        } else {
            self.constrained_decisions as f64 * 100.0 / self.live_decisions as f64
        }
    }
}

fn is_live_decision(item: &CorpusItem) -> bool {
    if item.spec.map(|s| s.name.as_str()) != Some("decision") {
        return false;
    }
    !matches!(
        artifact_status(&item.artifact)
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "superseded" | "deprecated"
    )
}

fn fenced_yaml(section: &str) -> Result<&str, &'static str> {
    let trimmed = section.trim();
    let body = trimmed
        .strip_prefix("```yaml")
        .or_else(|| trimmed.strip_prefix("```yml"))
        .ok_or("expected exactly one fenced yaml block")?;
    let body = body
        .strip_suffix("```")
        .ok_or("unterminated fenced yaml block")?;
    if body.contains("\n```") {
        return Err("expected exactly one fenced yaml block");
    }
    Ok(body.trim_matches('\n'))
}

fn valid_rule_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some('a'..='z'))
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.ends_with('-')
        && !id.contains("--")
}

fn safe_glob(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\\')
        && !Path::new(value).is_absolute()
        && !Path::new(value)
            .components()
            .any(|part| part == Component::ParentDir)
}

fn raw_constraint_section(artifact: &Artifact) -> Result<Option<String>, &'static str> {
    let text = match std::fs::read_to_string(&artifact.product.source_path) {
        Ok(text) => text,
        Err(_) => return Ok(artifact.section("code constraints").map(str::to_string)),
    };
    let heading = Regex::new(r"(?i)^##[ \t]+code[ \t]+constraints[ \t]*#*[ \t]*$").unwrap();
    let any_h2 = Regex::new(r"^##(?:[ \t]|$)").unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let starts: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| heading.is_match(line).then_some(index))
        .collect();
    match starts.as_slice() {
        [] => Ok(None),
        [_] => {
            let start = starts[0] + 1;
            let end = (start..lines.len())
                .find(|index| any_h2.is_match(lines[*index]))
                .unwrap_or(lines.len());
            Ok(Some(lines[start..end].join("\n")))
        }
        _ => Err("more than one Code Constraints section"),
    }
}

fn parse_document(item: &CorpusItem) -> Result<Option<ConstraintDocument>, Box<SentryFinding>> {
    let section = raw_constraint_section(&item.artifact).map_err(|problem| {
        Box::new(SentryFinding {
            code: MALFORMED_CONSTRAINTS,
            decision_path: item.path.clone(),
            rule_id: None,
            path: item.path.clone(),
            line: None,
            message: problem.to_string(),
        })
    })?;
    let Some(section) = section else {
        return Ok(None);
    };
    let yaml = fenced_yaml(&section).map_err(|problem| {
        Box::new(SentryFinding {
            code: MALFORMED_CONSTRAINTS,
            decision_path: item.path.clone(),
            rule_id: None,
            path: item.path.clone(),
            line: None,
            message: problem.to_string(),
        })
    })?;
    let document: ConstraintDocument = serde_yaml::from_str(yaml).map_err(|error| {
        Box::new(SentryFinding {
            code: MALFORMED_CONSTRAINTS,
            decision_path: item.path.clone(),
            rule_id: None,
            path: item.path.clone(),
            line: None,
            message: format!("invalid code-constraint YAML: {error}"),
        })
    })?;
    if document.version != 1 {
        return Err(Box::new(SentryFinding {
            code: UNSUPPORTED_VERSION,
            decision_path: item.path.clone(),
            rule_id: None,
            path: item.path.clone(),
            line: None,
            message: format!(
                "unsupported code-constraint version {}; supported version is 1",
                document.version
            ),
        }));
    }
    if document.rules.is_empty() {
        return Err(invalid(item, None, "rules must not be empty"));
    }
    let mut ids = BTreeSet::new();
    for rule in &document.rules {
        if !valid_rule_id(&rule.id) {
            return Err(invalid(item, Some(&rule.id), "invalid rule id"));
        }
        if !ids.insert(rule.id.clone()) {
            return Err(Box::new(SentryFinding {
                code: DUPLICATE_RULE_ID,
                decision_path: item.path.clone(),
                rule_id: Some(rule.id.clone()),
                path: item.path.clone(),
                line: None,
                message: format!("duplicate code-constraint id '{}'", rule.id),
            }));
        }
        if !safe_glob(&rule.path_glob) {
            return Err(invalid(
                item,
                Some(&rule.id),
                "path_glob must be repository-relative and contain no '..' component",
            ));
        }
        if Glob::new(&rule.path_glob).is_err() {
            return Err(invalid(item, Some(&rule.id), "invalid path_glob"));
        }
        if rule.pattern.is_empty() || Regex::new(&rule.pattern).is_err() {
            return Err(invalid(item, Some(&rule.id), "invalid regular expression"));
        }
        if rule
            .message
            .as_ref()
            .is_some_and(|message| message.is_empty())
        {
            return Err(invalid(item, Some(&rule.id), "message must not be empty"));
        }
    }
    Ok(Some(document))
}

/// Structural validation for `decided validate`: syntax and rule contracts do
/// not require a repository tree and therefore fail the ordinary corpus gate
/// even when code enforcement was not requested.
pub fn validate_artifact(artifact: &Artifact) -> Vec<Issue> {
    let item = CorpusItem {
        path: String::new(),
        artifact: artifact.clone(),
        spec: spec_for("decision"),
    };
    match parse_document(&item) {
        Err(finding) => vec![Issue::new("error", finding.code, finding.message, None)],
        _ => Vec::new(),
    }
}

fn invalid(item: &CorpusItem, rule_id: Option<&str>, message: &str) -> Box<SentryFinding> {
    Box::new(SentryFinding {
        code: INVALID_CONSTRAINT,
        decision_path: item.path.clone(),
        rule_id: rule_id.map(str::to_string),
        path: item.path.clone(),
        line: None,
        message: message.to_string(),
    })
}

fn collect_files(root: &Path, dir: &Path, output: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() && !file_type.is_symlink() {
            collect_files(root, &path, output);
        } else if file_type.is_file() {
            if let Ok(relative) = path.strip_prefix(root) {
                output.push((relative.to_string_lossy().replace('\\', "/"), path));
            }
        }
    }
}

fn changed_lines(
    repository: &Path,
    base: &str,
) -> Result<BTreeMap<String, BTreeSet<usize>>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["diff", "--unified=0", "--no-ext-diff", base, "--"])
        .output()
        .map_err(|error| format!("could not run git diff: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut result: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let mut path: Option<String> = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("+++ b/") {
            path = Some(value.to_string());
        } else if let Some(hunk) = line.strip_prefix("@@ -").and_then(|s| s.split(" +").nth(1)) {
            let range = hunk.split(" @@").next().unwrap_or(hunk);
            let mut fields = range.split(',');
            let start = fields
                .next()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            let count = fields
                .next()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1);
            if let Some(path) = &path {
                result
                    .entry(path.clone())
                    .or_default()
                    .extend(start..start.saturating_add(count));
            }
        }
    }
    Ok(result)
}

fn import_targets(extension: &str, text: &str) -> Option<Vec<(usize, String)>> {
    let mut targets = Vec::new();
    match extension {
        "py" => {
            let import = Regex::new(r"^\s*import\s+([A-Za-z0-9_., ]+)").unwrap();
            let from = Regex::new(r"^\s*from\s+([A-Za-z0-9_.]+)\s+import\b").unwrap();
            for (index, line) in text.lines().enumerate() {
                if let Some(captures) = from.captures(line) {
                    targets.push((index + 1, captures[1].to_string()));
                } else if let Some(captures) = import.captures(line) {
                    for target in captures[1].split(',') {
                        targets.push((
                            index + 1,
                            target.split_whitespace().next().unwrap_or("").to_string(),
                        ));
                    }
                }
            }
        }
        "rs" => {
            let use_re = Regex::new(r"^\s*use\s+([^;]+)").unwrap();
            for (index, line) in text.lines().enumerate() {
                if let Some(captures) = use_re.captures(line) {
                    targets.push((index + 1, captures[1].trim().to_string()));
                }
            }
        }
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => {
            let from = Regex::new(r#"\bfrom\s+['"]([^'"]+)['"]"#).unwrap();
            let side_effect = Regex::new(r#"^\s*import\s+['"]([^'"]+)['"]"#).unwrap();
            let require = Regex::new(r#"\brequire\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap();
            for (index, line) in text.lines().enumerate() {
                for captures in from
                    .captures_iter(line)
                    .chain(side_effect.captures_iter(line))
                    .chain(require.captures_iter(line))
                {
                    targets.push((index + 1, captures[1].to_string()));
                }
            }
        }
        _ => return None,
    }
    Some(targets)
}

pub fn analyze(
    corpus: &str,
    repository: &str,
    recursive: bool,
    base: Option<&str>,
    full_tree: bool,
) -> Result<SentryReport, String> {
    let repository_path = Path::new(repository);
    if !repository_path.is_dir() {
        return Err(format!("not a directory: {repository}"));
    }
    if !full_tree && base.is_none() {
        return Err("a diff base is required unless --full is supplied".to_string());
    }
    let changed = if full_tree {
        None
    } else {
        Some(changed_lines(repository_path, base.unwrap())?)
    };
    let items = corpus_items(corpus, recursive);
    let live: Vec<&CorpusItem> = items.iter().filter(|item| is_live_decision(item)).collect();
    let mut documents = Vec::new();
    let mut findings = Vec::new();
    for item in &live {
        match parse_document(item) {
            Ok(Some(document)) => documents.push((*item, document)),
            Ok(None) => {}
            Err(finding) => findings.push(*finding),
        }
    }

    let mut files = Vec::new();
    collect_files(repository_path, repository_path, &mut files);
    for (item, document) in &documents {
        for rule in &document.rules {
            let matcher = Glob::new(&rule.path_glob).unwrap().compile_matcher();
            let selected: Vec<_> = files
                .iter()
                .filter(|(relative, _)| matcher.is_match(relative))
                .filter(|(relative, _)| {
                    full_tree
                        || changed
                            .as_ref()
                            .is_some_and(|set| set.contains_key(relative))
                })
                .collect();
            if matches!(rule.kind, RuleKind::RequirePattern) && selected.is_empty() {
                findings.push(rule_finding(
                    item,
                    rule,
                    CODE_EMPTY_MATCH,
                    &item.path,
                    None,
                    format!("rule '{}' selected no files", rule.id),
                ));
                continue;
            }
            let pattern = Regex::new(&rule.pattern).unwrap();
            for (relative, absolute) in selected {
                let text = match std::fs::read_to_string(absolute) {
                    Ok(text) => text,
                    Err(_) => {
                        findings.push(rule_finding(
                            item,
                            rule,
                            CODE_UNSUPPORTED_LANGUAGE,
                            relative,
                            None,
                            "selected source file is not readable UTF-8".to_string(),
                        ));
                        continue;
                    }
                };
                match rule.kind {
                    RuleKind::ForbidPattern => {
                        let mut line_starts = vec![0usize];
                        line_starts.extend(
                            text.bytes()
                                .enumerate()
                                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
                        );
                        for matched in pattern.find_iter(&text) {
                            let start_line =
                                line_starts.partition_point(|start| *start <= matched.start());
                            let end_offset = matched.end().saturating_sub(1);
                            let end_line =
                                line_starts.partition_point(|start| *start <= end_offset);
                            let touches_change = full_tree
                                || changed
                                    .as_ref()
                                    .and_then(|set| set.get(relative))
                                    .is_some_and(|lines| {
                                        (start_line..=end_line).any(|line| lines.contains(&line))
                                    });
                            if touches_change {
                                findings.push(rule_finding(
                                    item,
                                    rule,
                                    CODE_VIOLATION,
                                    relative,
                                    Some(start_line as i64),
                                    rule.message.clone().unwrap_or_else(|| {
                                        format!("forbidden pattern matched rule '{}'", rule.id)
                                    }),
                                ));
                            }
                        }
                    }
                    RuleKind::RequirePattern => {
                        if !pattern.is_match(&text) {
                            findings.push(rule_finding(
                                item,
                                rule,
                                CODE_VIOLATION,
                                relative,
                                None,
                                rule.message.clone().unwrap_or_else(|| {
                                    format!("required pattern missing for rule '{}'", rule.id)
                                }),
                            ));
                        }
                    }
                    RuleKind::ForbidImport => {
                        let extension = absolute
                            .extension()
                            .and_then(|value| value.to_str())
                            .unwrap_or("");
                        let Some(imports) = import_targets(extension, &text) else {
                            findings.push(rule_finding(
                                item,
                                rule,
                                CODE_UNSUPPORTED_LANGUAGE,
                                relative,
                                None,
                                format!("no deterministic import adapter for '.{extension}'"),
                            ));
                            continue;
                        };
                        for (line, target) in imports {
                            if pattern.is_match(&target)
                                && (full_tree
                                    || changed
                                        .as_ref()
                                        .and_then(|set| set.get(relative))
                                        .is_some_and(|lines| lines.contains(&line)))
                            {
                                findings.push(rule_finding(
                                    item,
                                    rule,
                                    CODE_VIOLATION,
                                    relative,
                                    Some(line as i64),
                                    rule.message.clone().unwrap_or_else(|| {
                                        format!(
                                            "forbidden import '{target}' matched rule '{}'",
                                            rule.id
                                        )
                                    }),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.unwrap_or(0).cmp(&b.line.unwrap_or(0)))
            .then(a.decision_path.cmp(&b.decision_path))
            .then(a.rule_id.cmp(&b.rule_id))
    });
    Ok(SentryReport {
        corpus: corpus.to_string(),
        repository: repository.to_string(),
        base: base.map(str::to_string),
        full_tree,
        live_decisions: live.len(),
        constrained_decisions: documents.len(),
        findings,
    })
}

fn rule_finding(
    item: &CorpusItem,
    rule: &ConstraintRule,
    code: &'static str,
    path: &str,
    line: Option<i64>,
    message: String,
) -> SentryFinding {
    SentryFinding {
        code,
        decision_path: item.path.clone(),
        rule_id: Some(rule.id.clone()),
        path: path.to_string(),
        line,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rule_ids_are_kebab_case() {
        assert!(valid_rule_id("no-hard-delete"));
        assert!(!valid_rule_id("NoHardDelete"));
        assert!(!valid_rule_id("no--delete"));
    }

    #[test]
    fn import_adapters_extract_targets() {
        assert_eq!(
            import_targets("py", "from sqlalchemy.orm import Session\nimport httpx\n").unwrap(),
            vec![(1, "sqlalchemy.orm".to_string()), (2, "httpx".to_string())]
        );
        assert_eq!(
            import_targets("rs", "use crate::domain::User;\n").unwrap(),
            vec![(1, "crate::domain::User".to_string())]
        );
    }

    #[test]
    fn full_tree_enforces_constraint_and_reports_coverage() {
        let root = std::env::temp_dir().join(format!(
            "decided-sentry-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let corpus = root.join("decisions");
        let source = root.join("src");
        fs::create_dir_all(&corpus).unwrap();
        fs::create_dir_all(&source).unwrap();
        fs::write(
            corpus.join("adr-001.md"),
            "# ADR-001: Retain users\n\n## Status\n\nAccepted\n\n## Context\n\nDeletion loses audit history.\n\n## Decision\n\nUse soft deletion.\n\n## Consequences\n\nRows remain recoverable.\n\n## Code Constraints\n\n```yaml\nversion: 1\nrules:\n  - id: no-hard-delete\n    kind: forbid_pattern\n    path_glob: \"src/**/*.sql\"\n    pattern: \"DELETE\\\\s+FROM\\\\s+users\"\n```\n",
        )
        .unwrap();
        fs::write(source.join("users.sql"), "DELETE FROM users;\n").unwrap();

        let report = analyze(
            corpus.to_str().unwrap(),
            root.to_str().unwrap(),
            true,
            None,
            true,
        )
        .unwrap();
        assert_eq!(report.live_decisions, 1);
        assert_eq!(report.constrained_decisions, 1, "{:#?}", report.findings);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].code, CODE_VIOLATION);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn diff_mode_reports_only_added_violation_lines() {
        let root = std::env::temp_dir().join(format!("decided-sentry-diff-{}", std::process::id()));
        let corpus = root.join("decisions");
        let source = root.join("src");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&corpus).unwrap();
        fs::create_dir_all(&source).unwrap();
        fs::write(
            corpus.join("adr-001.md"),
            "# ADR-001: Retain users\n\n## Status\n\nAccepted\n\n## Context\n\nDeletion loses audit history.\n\n## Decision\n\nUse soft deletion.\n\n## Consequences\n\nRows remain recoverable.\n\n## Code Constraints\n\n```yaml\nversion: 1\nrules:\n  - id: no-hard-delete\n    kind: forbid_pattern\n    path_glob: \"src/**/*.sql\"\n    pattern: \"DELETE\\\\s+FROM\\\\s+users\"\n```\n",
        )
        .unwrap();
        fs::write(source.join("users.sql"), "SELECT * FROM users;\n").unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=As Decided",
            "-c",
            "user.email=tests@asdecided.com",
            "commit",
            "-qm",
            "base",
        ]);
        fs::write(
            source.join("users.sql"),
            "SELECT * FROM users;\nDELETE FROM users;\n",
        )
        .unwrap();

        let report = analyze(
            corpus.to_str().unwrap(),
            root.to_str().unwrap(),
            true,
            Some("HEAD"),
            false,
        )
        .unwrap();
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].line, Some(2));

        fs::remove_dir_all(root).unwrap();
    }
}
