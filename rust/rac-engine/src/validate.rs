//! Structural validation (`decided.core.validation`), severity overrides
//! (`decided.core.overrides`), OKF conformance (`decided.services.okf_conformance`),
//! and the `.decided/config.yaml` loaders (`decided.services.init`) — per
//! PORT-CONTRACT.d/04 §4-6.
//!
//! Emission order is the contract: [metadata issues][ticketing issues]
//! [per-type issues], with each per-type validator's internal append order
//! replicated verbatim. Messages are byte-exact, `{x!r}` via
//! `pycompat::py_repr_str`.

use std::path::{Path, PathBuf};

use crate::classify::classify;
use crate::identity::identity_conflict;
use crate::parse::{Artifact, Issue};
use crate::pycompat::{is_re_digit, py_casefold, py_is_space, py_repr_str, py_splitlines, py_strip};
use crate::spec::{spec_for, ArtifactSpec};

pub const MAX_REQUIREMENTS: usize = 50;

// ---------------------------------------------------------------------------
// Small Python-semantics helpers
// ---------------------------------------------------------------------------

/// Python `str.title()` — titlecase the first char of each run of cased
/// chars, lowercase the rest. (Reached only with ASCII section/type names.)
pub fn py_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_cased = false;
    for c in s.chars() {
        let cased = c.is_alphabetic();
        if cased && !prev_cased {
            out.extend(c.to_uppercase());
        } else if cased {
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
        prev_cased = cased;
    }
    out
}

/// validation's `_first_value(body)`: first non-blank stripped line — NO
/// list-marker stripping (distinct from identity's helper).
fn first_value(body: &str) -> String {
    crate::pycompat::first_nonempty_line(body).to_string()
}

fn is_word_char(c: char) -> bool {
    crate::pycompat::is_re_word(c)
}

/// `re.findall` over `\b(word1|word2|...)\b` IGNORECASE (ASCII words):
/// returns the matched substrings (original casing) in scan order.
/// Alternation is tried in `words` order at each position, mirroring the
/// regex engine.
fn findall_words<'a>(text: &'a str, words: &[&str]) -> Vec<&'a str> {
    let mut found = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let at_boundary = i == 0 || !is_word_char(chars[i - 1].1);
        if at_boundary {
            let mut matched_len = 0usize; // in chars
            for w in words {
                let wlen = w.chars().count();
                if i + wlen > n {
                    continue;
                }
                let candidate: bool = w
                    .chars()
                    .zip(chars[i..i + wlen].iter().map(|(_, c)| *c))
                    .all(|(wc, tc)| {
                        tc == wc || (tc.is_ascii_alphabetic() && tc.to_ascii_lowercase() == wc)
                    });
                if candidate {
                    // Trailing word boundary.
                    if i + wlen == n || !is_word_char(chars[i + wlen].1) {
                        matched_len = wlen;
                        break;
                    }
                }
            }
            if matched_len > 0 {
                let start = chars[i].0;
                let end = if i + matched_len < n {
                    chars[i + matched_len].0
                } else {
                    text.len()
                };
                found.push(&text[start..end]);
                i += matched_len;
                continue;
            }
        }
        i += 1;
    }
    found
}

/// `_EARS_IF_RE = ^\s*if\b` (IGNORECASE, `re.search` — `^` anchors at the
/// string start only, no MULTILINE).
fn ears_if(text: &str) -> bool {
    let rest = text.trim_start_matches(py_is_space);
    let mut it = rest.chars();
    match (it.next(), it.next()) {
        (Some(a), Some(b))
            if a.eq_ignore_ascii_case(&'i') && b.eq_ignore_ascii_case(&'f') =>
        {
            match it.next() {
                Some(c) => !is_word_char(c),
                None => true,
            }
        }
        (Some(a), None) if a.eq_ignore_ascii_case(&'i') => false,
        _ => false,
    }
}

/// `\bthen\b` IGNORECASE search.
fn has_then(text: &str) -> bool {
    !findall_words(text, &["then"]).is_empty()
}

/// `_QUARTER_RE = ^Q[1-4]\s+\d{4}$` (`re.match`; `$` = end or before one
/// trailing `\n`).
fn quarter_match(text: &str) -> bool {
    let t = text.strip_suffix('\n').unwrap_or(text);
    let mut it = t.char_indices();
    match it.next() {
        Some((_, 'Q')) => {}
        _ => return false,
    }
    match it.next() {
        Some((_, c)) if ('1'..='4').contains(&c) => {}
        _ => return false,
    }
    // \s+
    let rest_start = match it.next() {
        Some((i, c)) if py_is_space(c) => i,
        _ => return false,
    };
    let rest = &t[rest_start..];
    let after_ws = rest.trim_start_matches(py_is_space);
    // \d{4}$
    let digits: Vec<char> = after_ws.chars().collect();
    digits.len() == 4 && digits.iter().all(|&c| is_re_digit(c))
}

// ---------------------------------------------------------------------------
// Ticketing format-lint (ADR-087)
// ---------------------------------------------------------------------------

pub const TICKETING_SECTION: &str = "related tickets";

/// `^https?://\S+$` — `\S` = not Python-whitespace.
fn url_match(entry: &str) -> bool {
    let rest = entry
        .strip_prefix("https://")
        .or_else(|| entry.strip_prefix("http://"));
    match rest {
        Some(rest) => !rest.is_empty() && rest.chars().all(|c| !py_is_space(c)),
        None => false,
    }
}

fn jira_key(e: &str) -> bool {
    // ^[A-Z][A-Z0-9]+-\d+$
    key_dash_digits(e, 2)
}

fn linear_key(e: &str) -> bool {
    // ^[A-Z][A-Z0-9]*-\d+$
    key_dash_digits(e, 1)
}

/// `^[A-Z][A-Z0-9]{min_key-1,}-\d+$` (both Jira/Linear shapes).
fn key_dash_digits(e: &str, min_key: usize) -> bool {
    let Some(dash) = e.find('-') else {
        return false;
    };
    let (key, digits) = (&e[..dash], &e[dash + 1..]);
    let kchars: Vec<char> = key.chars().collect();
    if kchars.len() < min_key {
        return false;
    }
    if !kchars[0].is_ascii_uppercase() {
        return false;
    }
    if !kchars[1..]
        .iter()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return false;
    }
    // NOTE: regex `-\d+$` — the FIRST dash found may not be the regex's:
    // `[A-Z0-9]+` cannot contain '-', so the first '-' is the split point.
    !digits.is_empty() && digits.chars().all(is_re_digit)
}

fn github_ref(e: &str) -> bool {
    // ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+#\d+$
    let Some(slash) = e.find('/') else {
        return false;
    };
    let owner = &e[..slash];
    let rest = &e[slash + 1..];
    let Some(hash) = rest.find('#') else {
        return false;
    };
    let repo = &rest[..hash];
    let digits = &rest[hash + 1..];
    let seg_ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    };
    seg_ok(owner) && seg_ok(repo) && !digits.is_empty() && digits.chars().all(is_re_digit)
}

fn ado_ref(e: &str) -> bool {
    // ^(?:AB#)?\d+$
    let rest = e.strip_prefix("AB#").unwrap_or(e);
    !rest.is_empty() && rest.chars().all(is_re_digit)
}

fn servicenow_ref(e: &str) -> bool {
    // ^[A-Z]{2,}\d{5,}$
    let letters: usize = e.chars().take_while(|c| c.is_ascii_uppercase()).count();
    if letters < 2 {
        return false;
    }
    let rest: Vec<char> = e.chars().skip(letters).collect();
    rest.len() >= 5 && rest.iter().all(|&c| is_re_digit(c))
}

type TicketValidator = fn(&str) -> bool;

/// `(validator, label)` per recognised provider.
fn ticketing_provider(name: &str) -> Option<(TicketValidator, &'static str)> {
    match name {
        "jira" => Some((jira_key, "Jira key (e.g. PROJ-1234) or URL")),
        "github" => Some((github_ref, "GitHub issue (e.g. owner/repo#123) or URL")),
        "linear" => Some((linear_key, "Linear key (e.g. ENG-123) or URL")),
        "azure-devops" => Some((ado_ref, "Azure DevOps work item (e.g. 1234 or AB#1234) or URL")),
        "servicenow" => Some((servicenow_ref, "ServiceNow record (e.g. INC0010023) or URL")),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// validate() — the finding catalog
// ---------------------------------------------------------------------------

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.severity == "error")
}

/// `decided.core.validation.validate(product, ticketing_provider, artifact_type)`.
pub fn validate(
    artifact: &Artifact,
    ticketing_provider_name: Option<&str>,
    artifact_type: Option<&str>,
) -> Vec<Issue> {
    let artifact_type: String = match artifact_type {
        Some(t) => t.to_string(),
        None => classify(artifact).artifact_type,
    };
    let mut issues = validate_metadata(artifact, &artifact_type);
    issues.extend(validate_ticketing_references(
        artifact,
        ticketing_provider_name,
        &artifact_type,
    ));
    match artifact_type.as_str() {
        "decision" => {
            issues.extend(validate_decision(artifact));
            issues.extend(crate::sentry::validate_artifact(artifact));
        }
        "roadmap" => issues.extend(validate_roadmap(artifact)),
        "prompt" => issues.extend(validate_prompt(artifact)),
        "design" => issues.extend(validate_design(artifact)),
        "requirement" => {
            let spec = spec_for("requirement").expect("requirement spec exists");
            issues.extend(validate_requirement(artifact));
            issues.extend(validate_status_metadata(artifact, spec));
            issues.extend(validate_requirement_standards(artifact));
        }
        _ => issues.extend(validate_requirement(artifact)),
    }
    issues
}

fn validate_metadata(artifact: &Artifact, artifact_type: &str) -> Vec<Issue> {
    let mut issues: Vec<Issue> = artifact.metadata_issues.clone();
    issues.extend(artifact.parse_issues.iter().cloned());
    let spec = spec_for(artifact_type);
    if let Some((fm_id, legacy_id)) = identity_conflict(artifact, spec) {
        issues.push(Issue::new(
            "error",
            "conflicting-identity",
            format!(
                "frontmatter id {} conflicts with declared legacy identity {}; \
                 align them — RAC will not choose one",
                py_repr_str(&fm_id),
                py_repr_str(&legacy_id)
            ),
            None,
        ));
    }
    issues
}

fn validate_ticketing_references(
    artifact: &Artifact,
    provider: Option<&str>,
    artifact_type: &str,
) -> Vec<Issue> {
    let Some(provider) = provider else {
        return Vec::new();
    };
    if provider.is_empty() || provider == "none" {
        return Vec::new();
    }
    let Some((is_valid, label)) = ticketing_provider(provider) else {
        return Vec::new();
    };
    let Some(spec) = spec_for(artifact_type) else {
        return Vec::new();
    };
    if !spec.optional.iter().any(|s| s == TICKETING_SECTION) {
        return Vec::new();
    }
    let body = artifact.section(TICKETING_SECTION).unwrap_or("");
    let mut issues = Vec::new();
    for line in py_splitlines(body) {
        let entry =
            py_strip(crate::identity::strip_list_marker(py_strip(line))).to_string();
        if !entry.is_empty() && !url_match(&entry) && !is_valid(&entry) {
            issues.push(Issue::new(
                "error",
                "malformed-ticket-reference",
                format!(
                    "## Related Tickets entry {} is not a valid {}.",
                    py_repr_str(&entry),
                    label
                ),
                None,
            ));
        }
    }
    issues
}

fn validate_status_metadata(artifact: &Artifact, spec: &ArtifactSpec) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (field_name, allowed) in &spec.metadata {
        let body = artifact.section(field_name).unwrap_or("");
        let value = first_value(body);
        if value.is_empty() {
            continue;
        }
        let vf = py_casefold(&value);
        if !allowed.iter().any(|a| py_casefold(a) == vf) {
            issues.push(Issue::new(
                "error",
                &format!("invalid-{}-{}", spec.name, field_name),
                format!(
                    "## {} value {} is not one of: {}.",
                    py_title(field_name),
                    py_repr_str(&value),
                    allowed.join(", ")
                ),
                None,
            ));
        }
    }
    issues
}

fn validate_title(artifact: &Artifact) -> Vec<Issue> {
    let mut issues = Vec::new();
    let missing = match &artifact.product.title {
        None => true,
        Some(t) => t.is_empty(),
    };
    if missing {
        issues.push(Issue::new(
            "error",
            "missing-title",
            "File has no top-level # title.".to_string(),
            None,
        ));
    }
    if !artifact.product.extra_title_lines.is_empty() {
        issues.push(Issue::new(
            "error",
            "multiple-titles",
            "File has more than one top-level # title; expected exactly one.".to_string(),
            Some(artifact.product.extra_title_lines[0]),
        ));
    }
    issues
}

fn validate_required_sections(artifact: &Artifact, spec: &ArtifactSpec) -> Vec<Issue> {
    let mut issues = Vec::new();
    for section in &spec.required {
        if !artifact.has_section(section) {
            issues.push(Issue::new(
                "error",
                &format!("missing-{}", section.replace(' ', "-")),
                format!(
                    "{} is missing a ## {} section.",
                    py_title(&spec.name),
                    py_title(section)
                ),
                None,
            ));
        }
    }
    issues
}

fn validate_decision(artifact: &Artifact) -> Vec<Issue> {
    let spec = spec_for("decision").expect("decision spec exists");
    let mut issues = validate_title(artifact);
    issues.extend(validate_required_sections(artifact, spec));
    issues.extend(validate_status_metadata(artifact, spec));
    issues
}

fn validate_roadmap(artifact: &Artifact) -> Vec<Issue> {
    let spec = spec_for("roadmap").expect("roadmap spec exists");
    let mut issues = validate_title(artifact);
    issues.extend(validate_required_sections(artifact, spec));

    let horizon = first_value(artifact.section("horizon").unwrap_or(""));
    if !horizon.is_empty() {
        let hf = py_casefold(&horizon);
        if hf != "now" && hf != "next" && hf != "later" && !quarter_match(&horizon) {
            issues.push(Issue::new(
                "error",
                "invalid-roadmap-horizon",
                format!(
                    "## Horizon value {} is not one of: now, next, later, \
                     or a quarter (e.g. Q3 2026).",
                    py_repr_str(&horizon)
                ),
                None,
            ));
        }
    }

    if !artifact.has_section("related requirements") && !artifact.has_section("related decisions") {
        issues.push(Issue::new(
            "warning",
            "roadmap-no-advancement-link",
            "Roadmap links no ## Related Requirements or ## Related Decisions it advances."
                .to_string(),
            None,
        ));
    }

    issues.extend(validate_status_metadata(artifact, spec));
    issues
}

fn validate_prompt(artifact: &Artifact) -> Vec<Issue> {
    let spec = spec_for("prompt").expect("prompt spec exists");
    let mut issues = validate_title(artifact);
    issues.extend(validate_required_sections(artifact, spec));
    issues.extend(validate_status_metadata(artifact, spec));
    issues
}

fn validate_design(artifact: &Artifact) -> Vec<Issue> {
    let spec = spec_for("design").expect("design spec exists");
    let mut issues = validate_title(artifact);
    issues.extend(validate_required_sections(artifact, spec));
    issues.extend(validate_status_metadata(artifact, spec));
    issues
}

/// `_report_duplicates`: one issue per duplicated key, at the first
/// occurrence, in document order.
fn report_duplicates<K: Fn(&crate::markdown::Requirement) -> String>(
    requirements: &[crate::markdown::Requirement],
    key: K,
    severity: &'static str,
    code: &str,
    message: impl Fn(&crate::markdown::Requirement, usize) -> String,
) -> Vec<Issue> {
    let keys: Vec<String> = requirements.iter().map(&key).collect();
    let mut issues = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (idx, r) in requirements.iter().enumerate() {
        let k = keys[idx].as_str();
        let count = keys.iter().filter(|x| x.as_str() == k).count();
        if count > 1 && !seen.contains(&k) {
            seen.push(k);
            issues.push(Issue::new(severity, code, message(r, count), Some(r.line)));
        }
    }
    issues
}

fn malformed_requirement_issues(artifact: &Artifact) -> Vec<Issue> {
    let mut issues = Vec::new();
    for m in &artifact.product.malformed_requirements {
        match &m.bad_id {
            None => issues.push(Issue::new(
                "error",
                "req-missing-id",
                format!(
                    "Requirement line has no [REQ-NNN] ID: {}",
                    py_repr_str(&m.raw)
                ),
                Some(m.line),
            )),
            Some(bad_id) if m.empty_text => issues.push(Issue::new(
                "error",
                "empty-req-text",
                format!("Requirement [{bad_id}] has no description text."),
                Some(m.line),
            )),
            Some(bad_id) => issues.push(Issue::new(
                "error",
                "malformed-req-id",
                format!("Malformed requirement ID [{bad_id}]; expected form [REQ-NNN]."),
                Some(m.line),
            )),
        }
    }
    issues
}

fn requirement_warning_issues(artifact: &Artifact) -> Vec<Issue> {
    let p = &artifact.product;
    let mut issues = Vec::new();

    if !p.has_metrics_section {
        issues.push(Issue::new(
            "warning",
            "missing-success-metrics",
            "No ## Success Metrics section (optional, but recommended).".to_string(),
            None,
        ));
    }
    if !p.has_risks_section {
        issues.push(Issue::new(
            "warning",
            "missing-risks",
            "No ## Risks section (optional, but recommended).".to_string(),
            None,
        ));
    }

    if p.has_problem_section && py_strip(p.problem.as_deref().unwrap_or("")).is_empty() {
        issues.push(Issue::new(
            "warning",
            "empty-problem",
            "## Problem section is empty.".to_string(),
            None,
        ));
    }

    if p.requirements.len() > MAX_REQUIREMENTS {
        issues.push(Issue::new(
            "warning",
            "too-many-requirements",
            format!(
                "{} requirements (more than {MAX_REQUIREMENTS}); consider splitting the feature.",
                p.requirements.len()
            ),
            None,
        ));
    }

    issues.extend(report_duplicates(
        &p.requirements,
        |r| py_casefold(py_strip(&r.text)),
        "warning",
        "duplicate-req-text",
        |r, _n| format!("Duplicate requirement text: {}.", py_repr_str(&r.text)),
    ));

    issues.extend(ambiguous_verb_issues(artifact));
    issues
}

const AMBIGUOUS_VERBS: [&str; 4] = ["support", "handle", "allow", "enable"];

fn ambiguous_verb_issues(artifact: &Artifact) -> Vec<Issue> {
    let mut issues = Vec::new();
    for r in &artifact.product.requirements {
        let found = findall_words(&r.text, &AMBIGUOUS_VERBS);
        if !found.is_empty() {
            let mut unique: Vec<String> = Vec::new();
            for v in &found {
                let lower = v.to_lowercase();
                if !unique.contains(&lower) {
                    unique.push(lower);
                }
            }
            unique.sort();
            issues.push(Issue::new(
                "warning",
                "ambiguous-verb",
                format!(
                    "{} uses ambiguous verb(s) ({}); be more specific.",
                    r.id,
                    unique.join(", ")
                ),
                Some(r.line),
            ));
        }
    }
    issues
}

fn validate_requirement(artifact: &Artifact) -> Vec<Issue> {
    let p = &artifact.product;
    let mut issues = validate_title(artifact);

    if !p.has_problem_section {
        issues.push(Issue::new(
            "error",
            "missing-problem",
            "File is missing a ## Problem section.".to_string(),
            None,
        ));
    }
    if !p.has_requirements_section {
        issues.push(Issue::new(
            "error",
            "missing-requirements",
            "File is missing a ## Requirements section.".to_string(),
            None,
        ));
    }

    issues.extend(malformed_requirement_issues(artifact));
    issues.extend(report_duplicates(
        &p.requirements,
        |r| r.id.clone(),
        "error",
        "duplicate-req-id",
        |r, n| format!("Duplicate requirement ID {} (used {} times).", r.id, n),
    ));
    issues.extend(requirement_warning_issues(artifact));
    issues
}

const NORMATIVE_KEYWORDS: [&str; 3] = ["shall", "must", "should"];

fn validate_requirement_standards(artifact: &Artifact) -> Vec<Issue> {
    let mut issues = Vec::new();
    for r in &artifact.product.requirements {
        let keywords = findall_words(&r.text, &NORMATIVE_KEYWORDS);

        // BCP-14: `sorted({k for k in keywords if k != k.upper()})`.
        let mut ambiguous: Vec<&str> = Vec::new();
        for k in &keywords {
            if *k != k.to_uppercase() && !ambiguous.contains(k) {
                ambiguous.push(k);
            }
        }
        ambiguous.sort();
        if !ambiguous.is_empty() {
            issues.push(Issue::new(
                "error",
                "requirement-normative-keyword",
                format!(
                    "{} uses non-normative {}; only uppercase MUST/SHALL/SHOULD/MAY \
                     carry normative weight (BCP 14).",
                    r.id,
                    py_repr_str(&ambiguous.join(", "))
                ),
                Some(r.line),
            ));
        }

        if keywords.len() > 1 {
            issues.push(Issue::new(
                "warning",
                "requirement-not-singular",
                format!(
                    "{} has {} normative keywords; a requirement should be singular \
                     (ISO/IEC/IEEE 29148).",
                    r.id,
                    keywords.len()
                ),
                Some(r.line),
            ));
        }

        if keywords.is_empty() {
            issues.push(Issue::new(
                "warning",
                "requirement-non-ears",
                format!(
                    "{} has no normative keyword (SHALL/SHOULD/MAY); it does not \
                     state a testable requirement (EARS).",
                    r.id
                ),
                Some(r.line),
            ));
        } else if ears_if(&r.text) && !has_then(&r.text) {
            issues.push(Issue::new(
                "warning",
                "requirement-ears-clause",
                format!(
                    "{} opens with 'If' but has no 'then' response clause \
                     (EARS unwanted-behaviour pattern: If <condition> then <system> SHALL \u{2026}).",
                    r.id
                ),
                Some(r.line),
            ));
        }
    }
    issues
}

// ---------------------------------------------------------------------------
// Severity overrides (ADR-053)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SeverityOverrides {
    /// rule code -> error | warning | off
    pub rules: Vec<(String, String)>,
    /// artifact type -> error | warning
    pub types: Vec<(String, String)>,
}

impl SeverityOverrides {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.types.is_empty()
    }

    fn rule(&self, code: &str) -> Option<&str> {
        self.rules
            .iter()
            .find(|(k, _)| k == code)
            .map(|(_, v)| v.as_str())
    }

    fn type_ceiling(&self, artifact_type: &str) -> Option<&str> {
        self.types
            .iter()
            .find(|(k, _)| k == artifact_type)
            .map(|(_, v)| v.as_str())
    }
}

/// `resolve_severity(base, code, type, overrides)`.
pub fn resolve_severity<'a>(
    base: &'a str,
    code: &str,
    artifact_type: &str,
    overrides: &'a SeverityOverrides,
) -> &'a str {
    let mut sev = base;
    if overrides.type_ceiling(artifact_type) == Some("warning") && sev == "error" {
        sev = "warning";
    }
    if let Some(rule) = overrides.rule(code) {
        sev = rule;
    }
    sev
}

/// `apply_overrides(issues, artifact_type, overrides)`.
pub fn apply_overrides(
    issues: Vec<Issue>,
    artifact_type: &str,
    overrides: &SeverityOverrides,
) -> Vec<Issue> {
    if overrides.is_empty() {
        return issues;
    }
    let mut out = Vec::with_capacity(issues.len());
    for mut issue in issues {
        let sev = resolve_severity(issue.severity, &issue.code, artifact_type, overrides);
        if sev == "off" {
            continue;
        }
        issue.severity = if sev == "error" { "error" } else { "warning" };
        out.push(issue);
    }
    out
}

// ---------------------------------------------------------------------------
// OKF conformance (ADR-048)
// ---------------------------------------------------------------------------

pub const OKF_TYPES: [&str; 5] = ["requirement", "decision", "design", "roadmap", "prompt"];
pub const RESERVED_FILENAMES: [&str; 2] = ["index.md", "log.md"];

#[derive(Debug, Clone)]
pub struct OkfFinding {
    pub code: String,
    pub path: String,
    pub message: String,
    pub severity: String,
}

#[derive(Debug, Clone)]
pub struct OkfConformanceReport {
    pub artifacts_checked: usize,
    pub findings: Vec<OkfFinding>,
}

impl OkfConformanceReport {
    pub fn ok(&self) -> bool {
        !self.findings.iter().any(|f| f.severity == "error")
    }
}

/// One walked corpus entry's OKF projection: `(display path, artifact type,
/// final filename)`.
pub struct OkfEntry<'a> {
    pub path: &'a str,
    pub artifact_type: &'a str,
    pub file_name: &'a str,
}

pub fn check_okf_conformance(
    entries: &[OkfEntry<'_>],
    overrides: &SeverityOverrides,
) -> OkfConformanceReport {
    let mut findings = Vec::new();
    let mut checked = 0usize;
    for entry in entries {
        if spec_for(entry.artifact_type).is_none() {
            continue;
        }
        checked += 1;
        if !OKF_TYPES.contains(&entry.artifact_type) {
            add_okf(
                &mut findings,
                "okf-unmapped-type",
                entry.path,
                format!(
                    "artifact type {} has no OKF type mapping; add it to \
                     decided.core.okf.OKF_TYPE so the artifact is carried in the \
                     OKF bundle (ADR-048)",
                    py_repr_str(entry.artifact_type)
                ),
                entry.artifact_type,
                overrides,
            );
        }
        if RESERVED_FILENAMES.contains(&entry.file_name) {
            add_okf(
                &mut findings,
                "okf-reserved-filename-collision",
                entry.path,
                format!(
                    "a typed artifact named {} collides with the generated OKF \
                     bundle entry point; rename the file — OKF reserves index.md \
                     and log.md (ADR-048)",
                    py_repr_str(entry.file_name)
                ),
                entry.artifact_type,
                overrides,
            );
        }
    }
    OkfConformanceReport {
        artifacts_checked: checked,
        findings,
    }
}

fn add_okf(
    findings: &mut Vec<OkfFinding>,
    code: &str,
    path: &str,
    message: String,
    artifact_type: &str,
    overrides: &SeverityOverrides,
) {
    let severity = resolve_severity("error", code, artifact_type, overrides);
    if severity == "off" {
        return;
    }
    findings.push(OkfFinding {
        code: code.to_string(),
        path: path.to_string(),
        message,
        severity: severity.to_string(),
    });
}

// ---------------------------------------------------------------------------
// .decided/config.yaml loaders (decided.services.init)
// ---------------------------------------------------------------------------

/// `find_config_file(start_dir)`: the nearest `.decided/config.yaml` at or above
/// the resolved `start_dir`.
pub fn find_config_file(start_dir: &str) -> Option<PathBuf> {
    find_config_file_with_boundary(start_dir, None)
}

/// Nearest config without walking above an optional trusted filesystem root.
pub fn find_config_file_with_boundary(
    start_dir: &str,
    boundary: Option<&Path>,
) -> Option<PathBuf> {
    let resolved = resolve_path(start_dir);
    let mut current: Option<&Path> = Some(resolved.as_path());
    while let Some(dir) = current {
        if boundary.is_some_and(|root| !dir.starts_with(root)) {
            return None;
        }
        let candidate = dir.join(".decided").join("config.yaml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if boundary.is_some_and(|root| dir == root) {
            return None;
        }
        current = dir.parent();
    }
    None
}

/// Python `Path(p).resolve()` approximation: canonicalize when possible,
/// else absolutize against the CWD (non-strict resolve of a missing path).
fn resolve_path(p: &str) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(p) {
        return c;
    }
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    }
}

use crate::frontmatter::{load_frontmatter_mapping, Yaml};

fn yaml_map_get<'a>(pairs: &'a [(Yaml, Yaml)], name: &str) -> Option<&'a Yaml> {
    pairs.iter().find_map(|(k, v)| match k {
        Yaml::Str(s) if s == name => Some(v),
        _ => None,
    })
}

fn load_config_mapping_with_boundary(
    start_dir: &str,
    boundary: Option<&Path>,
) -> Option<Vec<(Yaml, Yaml)>> {
    let config_path = find_config_file_with_boundary(start_dir, boundary)?;
    let text = std::fs::read_to_string(&config_path).ok()?;
    // The oracle uses full `yaml.safe_load`; the bounded frontmatter loader
    // covers the well-formed configs the parity corpus contains. (A config
    // exercising PyYAML beyond the bounded subset would be a divergence to
    // fix here, not silently accept.)
    let (pairs, _issues) = load_frontmatter_mapping(&text);
    pairs
}

fn load_config_mapping(start_dir: &str) -> Option<Vec<(Yaml, Yaml)>> {
    load_config_mapping_with_boundary(start_dir, None)
}

/// Coerce a YAML 1.1 severity value: bare `off` parses as Bool(false).
fn severity_value(v: &Yaml) -> Option<String> {
    match v {
        Yaml::Bool(false) => Some("off".to_string()),
        Yaml::Bool(true) => Some("on".to_string()),
        Yaml::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn parse_severity_map(section: Option<&Yaml>, allowed: &[&str]) -> Vec<(String, String)> {
    let Some(Yaml::Map(pairs)) = section else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (k, v) in pairs {
        let Yaml::Str(name) = k else { continue };
        let Some(sev) = severity_value(v) else {
            continue;
        };
        if allowed.contains(&sev.as_str()) {
            out.push((name.clone(), sev));
        }
    }
    out
}

fn overrides_from_mapping(pairs: &[(Yaml, Yaml)]) -> SeverityOverrides {
    let Some(Yaml::Map(section)) = yaml_map_get(pairs, "validation") else {
        return SeverityOverrides::default();
    };
    SeverityOverrides {
        rules: parse_severity_map(
            yaml_map_get(section, "rules"),
            &["error", "warning", "off"],
        ),
        types: parse_severity_map(yaml_map_get(section, "types"), &["error", "warning"]),
    }
}

/// `load_overrides(start_dir)` (ADR-053).
pub fn load_overrides(start_dir: &str) -> SeverityOverrides {
    let Some(pairs) = load_config_mapping(start_dir) else {
        return SeverityOverrides::default();
    };
    overrides_from_mapping(&pairs)
}

/// Parse validation overrides from the exact governing config snapshot.
/// Federated cache builds use this seam so portfolio validation cannot observe
/// config bytes newer than the logical generation it describes.
pub fn overrides_from_config_bytes(bytes: &[u8]) -> SeverityOverrides {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return SeverityOverrides::default();
    };
    let (Some(pairs), _issues) = load_frontmatter_mapping(text) else {
        return SeverityOverrides::default();
    };
    overrides_from_mapping(&pairs)
}

/// `load_freshness_threshold(start_dir)` (ADR-045): the
/// `freshness.stale_after_days` from the nearest `.decided/config.yaml`.
/// Defaults to 180 when there is no config, no `freshness` mapping, or the
/// value is not a positive int — YAML 1.1 bools are explicitly rejected
/// (`true`/`false` are not day counts even though `bool` is an `int`
/// subclass in Python).
pub fn load_freshness_threshold(start_dir: &str) -> i64 {
    const DEFAULT: i64 = 180;
    let Some(pairs) = load_config_mapping(start_dir) else {
        return DEFAULT;
    };
    let Some(Yaml::Map(section)) = yaml_map_get(&pairs, "freshness") else {
        return DEFAULT;
    };
    match yaml_map_get(section, "stale_after_days") {
        Some(Yaml::Int(v)) if *v > 0 => *v,
        _ => DEFAULT,
    }
}

/// `load_ticketing_provider(start_dir)` (ADR-088).
pub fn load_ticketing_provider(start_dir: &str) -> Option<String> {
    load_ticketing_provider_with_boundary(start_dir, None)
}

pub fn load_ticketing_provider_with_boundary(
    start_dir: &str,
    boundary: Option<&Path>,
) -> Option<String> {
    let pairs = load_config_mapping_with_boundary(start_dir, boundary)?;
    let Some(Yaml::Map(section)) = yaml_map_get(&pairs, "ticketing") else {
        return None;
    };
    match yaml_map_get(section, "provider") {
        Some(Yaml::Str(provider)) => Some(provider.clone()),
        _ => None,
    }
}

/// `repository_root(directory)` (scope_paths): nearest dir at or above the
/// resolved directory holding `.decided/config.yaml`, else the resolved dir.
pub fn repository_root(directory: &str) -> PathBuf {
    let resolved = resolve_path(directory);
    let mut current: Option<&Path> = Some(resolved.as_path());
    while let Some(dir) = current {
        if dir.join(".decided").join("config.yaml").is_file() {
            return dir.to_path_buf();
        }
        current = dir.parent();
    }
    resolved
}

/// `validate_product(product, start)` — classification-dispatched rules with
/// the repository's severity overrides applied.
pub fn validate_product(artifact: &Artifact, start: &str) -> Vec<Issue> {
    let artifact_type = classify(artifact).artifact_type;
    let provider = load_ticketing_provider(start);
    let issues = validate(artifact, provider.as_deref(), Some(&artifact_type));
    apply_overrides(issues, &artifact_type, &load_overrides(start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_boundaries() {
        assert_eq!(
            findall_words("MUST support export", &AMBIGUOUS_VERBS),
            vec!["support"]
        );
        assert!(findall_words("supports and handles stuff", &AMBIGUOUS_VERBS).is_empty());
        assert_eq!(
            findall_words("Shall we MUST?", &NORMATIVE_KEYWORDS),
            vec!["Shall", "MUST"]
        );
    }

    #[test]
    fn ears_and_quarter() {
        assert!(ears_if("  If the input is bad"));
        assert!(ears_if("if x"));
        assert!(!ears_if("iffy"));
        assert!(quarter_match("Q3 2026"));
        assert!(!quarter_match("Q5 2026"));
        assert!(!quarter_match("Q3 26"));
    }

    #[test]
    fn ticket_shapes() {
        assert!(jira_key("PROJ-1234"));
        assert!(!jira_key("P-1"));
        assert!(linear_key("P-1"));
        assert!(github_ref("owner/repo#123"));
        assert!(!github_ref("owner/repo/123"));
        assert!(ado_ref("AB#1234") && ado_ref("1234"));
        assert!(servicenow_ref("INC0010023"));
        assert!(url_match("https://x.example/y"));
        assert!(!url_match("https://"));
    }

    #[test]
    fn title_case() {
        assert_eq!(py_title("user need"), "User Need");
        assert_eq!(py_title("status"), "Status");
        assert_eq!(py_title("it's"), "It'S");
    }
}
