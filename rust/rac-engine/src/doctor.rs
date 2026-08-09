//! Repository health diagnostic (`decided.services.doctor`, v0.23.0 WS3) plus
//! its two service dependencies with no prior Rust port:
//! mentioned-but-unlinked reference detection (`decided.services.links`,
//! ADR-082) and the six injection-style content heuristics (REQ-005).
//!
//! Composes: structural validation (invalid-artifact errors), relationship
//! integrity (upstream codes, upstream severities), the one-hop degree pass
//! (orphans + high-fan-out hubs), injection-style content flags, unlinked
//! body references, and git-native suspect-artifact drift (shared with
//! `decided review` via `review::suspect_drift` — one source of truth).
//! Exit is non-zero only on an error-severity finding (REQ-007).

use crate::commands::{validate_directory, STATUS_INVALID};
use crate::identity::path_stem;
use crate::pycompat::{py_casefold, py_repr_str, py_strip};
use crate::relationships::{
    corpus_items, relationship_severity, relationships_from_corpus, validate_relationships,
    CorpusItem, Relationship, RelationshipIssue, RelationshipValidation,
    ISSUE_DUPLICATE_IDENTIFIER, ISSUE_RELATIONSHIP_CYCLE,
};
use crate::resolve::{index_from_items, resolve_in_index, IndexEntry, OUTCOME_RESOLVED};
use crate::review::{drift_problem, suspect_drift};
use crate::validate::py_title;

pub const DEFAULT_HUB_THRESHOLD: i64 = 20;

pub const SEVERITY_ERROR: &str = "error";
pub const SEVERITY_WARNING: &str = "warning";

pub const CODE_INVALID_ARTIFACT: &str = "invalid-artifact";
pub const CODE_ORPHANED_ARTIFACT: &str = "orphaned-artifact";
pub const CODE_HIGH_FAN_OUT_HUB: &str = "high-fan-out-hub";
pub const CODE_INJECTION_CONTENT: &str = "injection-style-content";
pub const CODE_UNLINKED_REFERENCE: &str = "unlinked-reference";
pub const CODE_SUSPECT_ARTIFACT: &str = "suspect-artifact";

const FIX_ORPHAN: &str = "Reference it from a related artifact (a `## Related ...` section), \
                          or confirm it is intentionally standalone.";
const FIX_HUB: &str = "Consider splitting this artifact or narrowing its relationships so a \
                       single node is not a traversal bottleneck.";
const FIX_INJECTION: &str = "Review this content; artifact content is untrusted and the trust \
                             boundary is human PR review (ADR-065). Remove or quote the flagged \
                             phrasing if it was not intended as literal guidance.";
const FIX_SUSPECT: &str = "Review whether this artifact still reflects the newer target and \
                           update it if needed. Advisory only \u{2014} RAC changes nothing (ADR-034).";

#[derive(Debug)]
pub struct DoctorFinding {
    pub path: String,
    pub code: &'static str,
    pub severity: &'static str,
    pub problem: String,
    pub fix: String,
}

pub struct DoctorReport {
    pub directory: String,
    pub hub_threshold: i64,
    pub findings: Vec<DoctorFinding>,
}

impl DoctorReport {
    pub fn error_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == SEVERITY_ERROR)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == SEVERITY_WARNING)
            .count()
    }

    /// A run passes when no error-severity finding is present (REQ-007).
    pub fn ok(&self) -> bool {
        self.error_count() == 0
    }
}

fn severity_rank(severity: &str) -> i64 {
    if severity == SEVERITY_ERROR {
        0
    } else {
        1
    }
}

/// `diagnose(directory, recursive, hub_threshold)` — all phases in the
/// oracle's insertion order (validation, relationships, degree, injection,
/// unlinked, suspect), then one stable sort by
/// `(severity rank, path, code, problem)`.
pub fn diagnose(directory: &str, recursive: bool, hub_threshold: i64) -> DoctorReport {
    let items = corpus_items(directory, recursive);
    let mut findings: Vec<DoctorFinding> = Vec::new();
    findings.extend(validation_findings(directory, recursive));
    findings.extend(relationship_findings(directory, recursive));
    findings.extend(degree_findings(&items, hub_threshold));
    findings.extend(injection_findings(&items));
    findings.extend(unlinked_reference_findings(&items));
    findings.extend(suspect_artifact_findings(directory, &items));
    findings.sort_by(|a, b| {
        severity_rank(a.severity)
            .cmp(&severity_rank(b.severity))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.code.cmp(b.code))
            .then_with(|| a.problem.cmp(&b.problem))
    });
    DoctorReport {
        directory: directory.to_string(),
        hub_threshold,
        findings,
    }
}

/// Diagnose only writable child artifacts while resolving their graph through
/// the authoritative composed catalog. Parent warnings and advisories are not
/// copied into each child; parent load/validation failures have already
/// stopped composition at the command boundary.
pub fn diagnose_composed(
    directory: &str,
    recursive: bool,
    hub_threshold: i64,
    corpus: &crate::composition::ComposedCorpus,
) -> DoctorReport {
    let items: Vec<CorpusItem> = corpus.local_items().cloned().collect();
    let relationships = corpus.relationships();
    let validation = crate::commands::validate_directory_from_items(directory, recursive, &items);
    let relationship_validation = corpus.validate_relationships(directory, recursive);
    let identity_index = corpus.identity_index();
    let mut findings = Vec::new();
    findings.extend(validation_findings_from_result(&validation));
    findings.extend(relationship_findings_from_result(
        directory,
        &relationship_validation,
    ));
    findings.extend(degree_findings_with_relationships(
        &items,
        &relationships,
        hub_threshold,
    ));
    findings.extend(injection_findings(&items));
    findings.extend(unlinked_reference_findings_with_index(
        &items,
        &identity_index,
        &relationships,
    ));
    findings.extend(suspect_artifact_findings(directory, &items));
    findings.sort_by(|a, b| {
        severity_rank(a.severity)
            .cmp(&severity_rank(b.severity))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.code.cmp(b.code))
            .then_with(|| a.problem.cmp(&b.problem))
    });
    DoctorReport {
        directory: directory.to_string(),
        hub_threshold,
        findings,
    }
}

/// One finding per structurally invalid artifact; the problem names the
/// sorted, deduplicated error codes.
fn validation_findings(directory: &str, recursive: bool) -> Vec<DoctorFinding> {
    let result = validate_directory(directory, recursive);
    validation_findings_from_result(&result)
}

fn validation_findings_from_result(
    result: &crate::commands::DirectoryValidation,
) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();
    for file in &result.files {
        if file.status != STATUS_INVALID {
            continue;
        }
        let mut codes: Vec<&str> = file
            .issues
            .iter()
            .filter(|i| i.severity == SEVERITY_ERROR)
            .map(|i| i.code.as_str())
            .collect();
        codes.sort_unstable();
        codes.dedup();
        findings.push(DoctorFinding {
            path: file.path.clone(),
            code: CODE_INVALID_ARTIFACT,
            severity: SEVERITY_ERROR,
            problem: format!("structural validation failed: {}", codes.join(", ")),
            fix: format!("Run: decided validate {}", file.path),
        });
    }
    findings
}

fn issue_path(issue: &RelationshipIssue) -> String {
    if let Some(source) = &issue.source_path {
        if !source.is_empty() {
            return source.clone();
        }
    }
    if let Some(paths) = &issue.paths {
        if let Some(first) = paths.first() {
            return first.clone();
        }
    }
    String::new()
}

fn issue_problem(issue: &RelationshipIssue) -> String {
    if issue.code == ISSUE_DUPLICATE_IDENTIFIER {
        return format!(
            "duplicate artifact identifier {} in: {}",
            py_repr_str(issue.identifier.as_deref().unwrap_or("")),
            issue.paths.clone().unwrap_or_default().join(", ")
        );
    }
    if issue.code == ISSUE_RELATIONSHIP_CYCLE {
        return format!(
            "relationship cycle in {}: {}",
            py_repr_str(issue.relationship.as_deref().unwrap_or("")),
            issue.paths.clone().unwrap_or_default().join(" -> ")
        );
    }
    format!(
        "{} via {} -> {}",
        issue.code,
        py_repr_str(issue.relationship.as_deref().unwrap_or("")),
        py_repr_str(issue.target.as_deref().unwrap_or(""))
    )
}

/// One finding per relationship-integrity issue; intrinsic severity is the
/// recorded map with doctor's own "error" fallback
/// (`RELATIONSHIP_SEVERITY.get(code, SEVERITY_ERROR)`).
fn relationship_findings(directory: &str, recursive: bool) -> Vec<DoctorFinding> {
    let result = validate_relationships(directory, recursive);
    relationship_findings_from_result(directory, &result)
}

fn relationship_findings_from_result(
    directory: &str,
    result: &RelationshipValidation,
) -> Vec<DoctorFinding> {
    result
        .issues
        .iter()
        .map(|issue| {
            let known = matches!(
                relationship_severity(&issue.code),
                "error" | "warning"
            );
            let severity = if known {
                if relationship_severity(&issue.code) == "error" {
                    SEVERITY_ERROR
                } else {
                    SEVERITY_WARNING
                }
            } else {
                SEVERITY_ERROR
            };
            DoctorFinding {
                path: issue_path(issue),
                code: issue_code_static(&issue.code),
                severity,
                problem: issue_problem(issue),
                fix: format!("Run: decided relationships {directory} --validate"),
            }
        })
        .collect()
}

/// Relationship finding codes are the upstream constants; map back to
/// 'static so `DoctorFinding.code` stays a static str across all sources.
fn issue_code_static(code: &str) -> &'static str {
    use crate::relationships as r;
    for known in [
        r::ISSUE_DUPLICATE_IDENTIFIER,
        r::ISSUE_TARGET_NOT_FOUND,
        r::ISSUE_TARGET_AMBIGUOUS,
        r::ISSUE_SELF_REFERENCE,
        r::ISSUE_EDGE_UNSUPPORTED,
        r::ISSUE_TARGET_SUPERSEDED,
        r::ISSUE_TARGET_TYPE_MISMATCH,
        r::ISSUE_RELATIONSHIP_CYCLE,
        r::ISSUE_SCOPE_TARGET_NOT_FOUND,
    ] {
        if code == known {
            return known;
        }
    }
    "unknown-relationship-issue"
}

/// Orphans (inbound degree 0) and high-fan-out hubs from one degree pass
/// over the resolved edges; the orphan definition matches the portfolio's
/// "never a resolved target" count exactly.
fn degree_findings(items: &[CorpusItem], hub_threshold: i64) -> Vec<DoctorFinding> {
    let relationships = relationships_from_corpus(items);
    degree_findings_with_relationships(items, &relationships, hub_threshold)
}

fn degree_findings_with_relationships(
    items: &[CorpusItem],
    relationships: &[Relationship],
    hub_threshold: i64,
) -> Vec<DoctorFinding> {
    let known: Vec<&CorpusItem> = items.iter().filter(|i| i.spec.is_some()).collect();
    let mut inbound: std::collections::HashMap<&crate::corpus::ArtifactPath, i64> = known
        .iter()
        .map(|item| (&item.artifact_path, 0))
        .collect();
    let mut outbound: std::collections::HashMap<&crate::corpus::ArtifactPath, i64> = known
        .iter()
        .map(|item| (&item.artifact_path, 0))
        .collect();
    for rel in relationships {
        let (Some(source), Some(resolved)) = (
            rel.source_artifact.as_ref(),
            rel.resolved_artifact.as_ref(),
        ) else {
            continue; // only resolved (unique, non-self) edges
        };
        if let Some(count) = inbound.get_mut(resolved) {
            *count += 1;
        }
        if let Some(count) = outbound.get_mut(source) {
            *count += 1;
        }
    }
    let mut findings = Vec::new();
    for item in &known {
        let path = item.path.as_str();
        let in_degree = *inbound.get(&item.artifact_path).unwrap_or(&0);
        let degree = in_degree + *outbound.get(&item.artifact_path).unwrap_or(&0);
        if in_degree == 0 {
            findings.push(DoctorFinding {
                path: path.to_string(),
                code: CODE_ORPHANED_ARTIFACT,
                severity: SEVERITY_WARNING,
                problem: "no other artifact references this one (orphaned)".to_string(),
                fix: FIX_ORPHAN.to_string(),
            });
        }
        if degree > hub_threshold {
            findings.push(DoctorFinding {
                path: path.to_string(),
                code: CODE_HIGH_FAN_OUT_HUB,
                severity: SEVERITY_WARNING,
                problem: format!(
                    "high-fan-out hub: {degree} resolved relationship edges (threshold {hub_threshold})"
                ),
                fix: FIX_HUB.to_string(),
            });
        }
    }
    findings
}

/// Suspect-link drift: shared with `decided review` via `review::suspect_drift`
/// (git recency over the validated relationship graph). Empty outside git.
fn suspect_artifact_findings(directory: &str, items: &[CorpusItem]) -> Vec<DoctorFinding> {
    suspect_drift(directory, items)
        .into_iter()
        .map(|record| DoctorFinding {
            path: record.source_path.clone(),
            code: CODE_SUSPECT_ARTIFACT,
            severity: SEVERITY_WARNING,
            problem: drift_problem(&record),
            fix: FIX_SUSPECT.to_string(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Injection-style content heuristics (decided.services.doctor._INJECTION_PATTERNS)
//
// Six deterministic Python-`re` patterns, hand-compiled (no regex crate in
// the workspace): IGNORECASE throughout; `.` never crosses a newline;
// `\s` includes the CPython extras \x1c-\x1f; `\b` is the Unicode word
// boundary. Each matcher answers existence only (`pattern.search`).
// ---------------------------------------------------------------------------

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Python `\s` (str patterns): Unicode whitespace plus the \x1c-\x1f
/// information separators CPython counts as space.
fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Case-insensitive char equality against a lowercase ASCII pattern char.
fn ci_eq(c: char, lower: char) -> bool {
    if c == lower {
        return true;
    }
    let mut it = c.to_lowercase();
    it.next() == Some(lower) && it.next().is_none()
}

/// Match a literal (lowercase ASCII, spaces literal) case-insensitively at
/// `i`; returns the end index.
fn lit(chars: &[char], i: usize, text: &str) -> Option<usize> {
    let mut k = i;
    for lc in text.chars() {
        if !ci_eq(*chars.get(k)?, lc) {
            return None;
        }
        k += 1;
    }
    Some(k)
}

/// `\b` before a word-char literal starting at `i`.
fn lead_boundary(chars: &[char], i: usize) -> bool {
    i == 0 || !is_word(chars[i - 1])
}

/// `\b` after a word-char literal ending at `end` (exclusive).
fn trail_boundary(chars: &[char], end: usize) -> bool {
    end >= chars.len() || !is_word(chars[end])
}

/// All `end` positions where one of `words` matches at `i` under the given
/// boundary requirements. Alternation for existence: every alternative that
/// fits is a candidate continuation point.
fn word_alt_ends(
    chars: &[char],
    i: usize,
    words: &[&str],
    lead: bool,
    trail: bool,
) -> Vec<usize> {
    if lead && !lead_boundary(chars, i) {
        return Vec::new();
    }
    let mut ends = Vec::new();
    for w in words {
        if let Some(end) = lit(chars, i, w) {
            if !trail || trail_boundary(chars, end) {
                ends.push(end);
            }
        }
    }
    ends
}

/// `.{0,max}` gap starts: every j in [from, from+max] reachable without
/// crossing a newline (`.` has no DOTALL here).
fn gap_positions(chars: &[char], from: usize, max: usize) -> Vec<usize> {
    let mut out = vec![from];
    let mut j = from;
    while j < chars.len() && j - from < max && chars[j] != '\n' {
        j += 1;
        out.push(j);
    }
    out
}

/// `\s+` from `i`: Some(first index past at least one whitespace run
/// position) — all lengths ≥ 1 are candidates, returned as a range.
fn ws_run(chars: &[char], i: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut j = i;
    while j < chars.len() && is_space(chars[j]) {
        j += 1;
        out.push(j);
    }
    out
}

fn p_instruction_override(chars: &[char]) -> bool {
    const G1: [&str; 5] = ["ignore", "disregard", "forget", "override", "bypass"];
    const G2: [&str; 8] = [
        "previous", "prior", "above", "earlier", "preceding", "all", "the system", "your",
    ];
    const G3: [&str; 8] = [
        "instruction",
        "instructions",
        "prompt",
        "directive",
        "directives",
        "rule",
        "rules",
        "context",
    ];
    for i in 0..chars.len() {
        for e1 in word_alt_ends(chars, i, &G1, true, true) {
            for j in gap_positions(chars, e1, 40) {
                for e2 in word_alt_ends(chars, j, &G2, true, true) {
                    for k in gap_positions(chars, e2, 20) {
                        if G3.iter().any(|w| lit(chars, k, w).is_some()) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn p_role_reassignment(chars: &[char]) -> bool {
    for i in 0..chars.len() {
        // \byou are now\b
        if lead_boundary(chars, i) {
            if let Some(end) = lit(chars, i, "you are now") {
                if trail_boundary(chars, end) {
                    return true;
                }
            }
            // \bpretend to be\b
            if let Some(end) = lit(chars, i, "pretend to be") {
                if trail_boundary(chars, end) {
                    return true;
                }
            }
            // \bfrom now on,?\s+you\s+(are|will|must|should|shall)\b
            if let Some(mut e) = lit(chars, i, "from now on") {
                if chars.get(e) == Some(&',') {
                    e += 1;
                }
                for a in ws_run(chars, e) {
                    if let Some(b) = lit(chars, a, "you") {
                        for c in ws_run(chars, b) {
                            if !word_alt_ends(
                                chars,
                                c,
                                &["are", "will", "must", "should", "shall"],
                                false,
                                true,
                            )
                            .is_empty()
                            {
                                return true;
                            }
                        }
                    }
                }
            }
            // \bact as if you\s+(are|were)\b
            if let Some(e) = lit(chars, i, "act as if you") {
                for a in ws_run(chars, e) {
                    if !word_alt_ends(chars, a, &["are", "were"], false, true).is_empty() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn p_ai_impersonation(chars: &[char]) -> bool {
    // \bas an ai(\s+language)?\s+model\b
    for i in 0..chars.len() {
        if !lead_boundary(chars, i) {
            continue;
        }
        let Some(e) = lit(chars, i, "as an ai") else {
            continue;
        };
        for a in ws_run(chars, e) {
            // Without the optional group: \s+model\b
            if let Some(end) = lit(chars, a, "model") {
                if trail_boundary(chars, end) {
                    return true;
                }
            }
            // With it: \s+language\s+model\b
            if let Some(l) = lit(chars, a, "language") {
                for b in ws_run(chars, l) {
                    if let Some(end) = lit(chars, b, "model") {
                        if trail_boundary(chars, end) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn p_chat_role_injection(chars: &[char]) -> bool {
    // ^\s*(system|assistant|developer|tool)\s*: with MULTILINE anchors.
    for anchor in std::iter::once(0).chain(
        chars
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == '\n')
            .map(|(i, _)| i + 1),
    ) {
        // \s* is greedy but the role word must start at the first
        // non-whitespace position after the anchor.
        let mut p = anchor;
        while p < chars.len() && is_space(chars[p]) {
            p += 1;
        }
        for role in ["system", "assistant", "developer", "tool"] {
            if let Some(mut e) = lit(chars, p, role) {
                while e < chars.len() && is_space(chars[e]) {
                    e += 1;
                }
                if chars.get(e) == Some(&':') {
                    return true;
                }
            }
        }
    }
    false
}

fn p_conceal_from_user(chars: &[char]) -> bool {
    const G1: [&str; 4] = ["do not", "don't", "never", "without"];
    const G2: [&str; 9] = [
        "tell",
        "telling",
        "inform",
        "informing",
        "mention",
        "mentioning",
        "reveal",
        "revealing",
        "notify",
    ];
    const G3: [&str; 3] = ["the user", "them", "anyone"];
    for i in 0..chars.len() {
        for e1 in word_alt_ends(chars, i, &G1, true, true) {
            for j in gap_positions(chars, e1, 30) {
                // G2 carries NO leading \b in the pattern — only trailing.
                for e2 in word_alt_ends(chars, j, &G2, false, true) {
                    for k in gap_positions(chars, e2, 20) {
                        if !word_alt_ends(chars, k, &G3, true, true).is_empty() {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn p_decision_steering(chars: &[char]) -> bool {
    const G1: [&str; 5] = ["ignore", "disregard", "override", "bypass", "violate"];
    const G3: [&str; 5] = ["decision", "decisions", "adr", "requirement", "policy"];
    for i in 0..chars.len() {
        for e1 in word_alt_ends(chars, i, &G1, true, true) {
            for j in gap_positions(chars, e1, 40) {
                // \b(recorded\s+)?(...)\b — with or without the prefix.
                if !word_alt_ends(chars, j, &G3, true, true).is_empty() {
                    return true;
                }
                if lead_boundary(chars, j) {
                    if let Some(e) = lit(chars, j, "recorded") {
                        for a in ws_run(chars, e) {
                            if !word_alt_ends(chars, a, &G3, false, true).is_empty() {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

/// One hand-compiled injection matcher (existence over the char sequence).
type InjectionMatcher = fn(&[char]) -> bool;

/// Pattern label -> matcher, in the oracle's declaration order; the finding
/// text joins the SORTED matching labels.
const INJECTION_PATTERNS: [(&str, InjectionMatcher); 6] = [
    ("instruction-override", p_instruction_override),
    ("role-reassignment", p_role_reassignment),
    ("ai-impersonation", p_ai_impersonation),
    ("chat-role-injection", p_chat_role_injection),
    ("conceal-from-user", p_conceal_from_user),
    ("decision-steering", p_decision_steering),
];

fn injection_findings(items: &[CorpusItem]) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();
    for item in items {
        // The oracle re-reads the stored text (strict UTF-8; unreadable
        // files are skipped).
        let Ok(bytes) = std::fs::read(&item.path) else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let chars: Vec<char> = text.chars().collect();
        let mut matched: Vec<&str> = INJECTION_PATTERNS
            .iter()
            .filter(|(_, matcher)| matcher(&chars))
            .map(|(label, _)| *label)
            .collect();
        if matched.is_empty() {
            continue;
        }
        matched.sort_unstable();
        findings.push(DoctorFinding {
            path: item.path.clone(),
            code: CODE_INJECTION_CONTENT,
            severity: SEVERITY_WARNING,
            problem: format!(
                "instruction-like / injection-style content for review ({})",
                matched.join(", ")
            ),
            fix: FIX_INJECTION.to_string(),
        });
    }
    findings
}

// ---------------------------------------------------------------------------
// Mentioned-but-unlinked reference detection (decided.services.links, ADR-082)
// ---------------------------------------------------------------------------

/// Normalized relationship-section headings whose lines are declared edges,
/// not body mentions (`RELATIONSHIP_SECTIONS`).
const RELATIONSHIP_HEADINGS: [&str; 9] = [
    "applies to",
    "related decisions",
    "related designs",
    "related prompts",
    "related requirements",
    "related roadmaps",
    "related tickets",
    "supersedes",
    "verified by",
];

/// `_CANDIDATE_RE = [0-9A-Za-z]+(?:-[0-9A-Za-z]+)*` — maximal ASCII
/// alphanumeric runs with single interior hyphens kept.
fn candidate_tokens(line: &str) -> Vec<String> {
    fn alnum(c: char) -> bool {
        c.is_ascii_alphanumeric()
    }
    let chars: Vec<char> = line.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !alnum(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && alnum(chars[i]) {
            i += 1;
        }
        // (?:-[0-9A-Za-z]+)* — a hyphen joins only when alnum follows.
        while i + 1 < chars.len() && chars[i] == '-' && alnum(chars[i + 1]) {
            i += 1;
            while i < chars.len() && alnum(chars[i]) {
                i += 1;
            }
        }
        tokens.push(chars[start..i].iter().collect());
    }
    tokens
}

/// `^[A-Za-z]+-\d+$` — the numbered short-alias shape (`adr-074`).
fn is_numbered_ref(alias: &str) -> bool {
    let Some(dash) = alias.find('-') else {
        return false;
    };
    let (letters, rest) = alias.split_at(dash);
    let digits = &rest[1..];
    !letters.is_empty()
        && letters.chars().all(|c| c.is_ascii_alphabetic())
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
        && !digits.contains('-')
}

/// `_preferred_ref(aliases, path)`: shortest numbered alias (stable on
/// ties, like `sorted(key=len)`), else the filename stem.
fn preferred_ref(aliases: &[String], path: &str) -> String {
    let mut numbered: Vec<&String> = aliases.iter().filter(|a| is_numbered_ref(a)).collect();
    numbered.sort_by_key(|a| a.chars().count());
    match numbered.first() {
        Some(alias) => (*alias).clone(),
        None => path_stem(path),
    }
}

/// `_related_section_for(target_type)` — the display heading.
fn related_section_for(target_type: &str) -> String {
    py_title(&format!("related {target_type}s"))
}

struct UnlinkedReference {
    source_path: String,
    target_id: String,
    matched_token: String,
    related_section: String,
    suggested_line: String,
}

/// `detect_unlinked_references` over the shared corpus snapshot: body
/// mentions resolving to another artifact with no declared resolved edge,
/// one finding per (source, target), sorted by `(source_path, target_id)`.
fn detect_unlinked_references(items: &[CorpusItem]) -> Vec<UnlinkedReference> {
    let index: Vec<IndexEntry> = index_from_items(items);
    let relationships = relationships_from_corpus(items);
    detect_unlinked_references_with_index(items, &index, &relationships)
}

fn detect_unlinked_references_with_index(
    items: &[CorpusItem],
    identity_index: &[IndexEntry],
    relationships: &[Relationship],
) -> Vec<UnlinkedReference> {
    let sources: Vec<IndexEntry> = index_from_items(items);
    let by_key: std::collections::HashMap<&crate::corpus::ArtifactKey, &IndexEntry> = identity_index
        .iter()
        .filter_map(|entry| entry.key.as_ref().map(|key| (key, entry)))
        .collect();

    let mut declared: std::collections::HashMap<
        crate::corpus::ArtifactPath,
        std::collections::HashSet<crate::corpus::ArtifactPath>,
    > = std::collections::HashMap::new();
    for rel in relationships {
        if let (Some(source), Some(resolved)) =
            (&rel.source_artifact, &rel.resolved_artifact)
        {
            declared
                .entry(source.clone())
                .or_default()
                .insert(resolved.clone());
        }
    }

    let mut findings: Vec<UnlinkedReference> = Vec::new();
    for source in &sources {
        let Some(source_path) = source.artifact_path.as_ref() else {
            continue;
        };
        let self_aliases: std::collections::HashSet<String> =
            source.aliases.iter().map(|a| py_casefold(a)).collect();
        let empty = std::collections::HashSet::new();
        let already = declared.get(source_path).unwrap_or(&empty);
        let mut seen_targets = std::collections::HashSet::new();
        for section in &source.search_sections {
            let heading = py_casefold(py_strip(&section.heading));
            if RELATIONSHIP_HEADINGS.contains(&heading.as_str()) {
                continue; // declared edges, not body mentions
            }
            for line in &section.lines {
                for token in candidate_tokens(line) {
                    if self_aliases.contains(&py_casefold(&token)) {
                        continue; // self-reference
                    }
                    let result = resolve_in_index(identity_index, &token);
                    if result.outcome != OUTCOME_RESOLVED {
                        continue; // not a unique corpus artifact
                    }
                    let target = result.artifact.expect("resolved implies artifact");
                    let (Some(target_key), Some(target_path)) =
                        (target.key.as_ref(), target.artifact_path.as_ref())
                    else {
                        continue;
                    };
                    if source.key.as_ref() == Some(target_key) || already.contains(target_path) {
                        continue;
                    }
                    if !seen_targets.insert(target_key.clone()) {
                        continue; // one finding per (source, target) pair
                    }
                    let target_entry = by_key[target_key];
                    findings.push(UnlinkedReference {
                        source_path: source.path.clone(),
                        target_id: target.id.clone(),
                        matched_token: token,
                        related_section: related_section_for(&target.artifact_type),
                        suggested_line: format!(
                            "- {}",
                            preferred_ref(&target_entry.aliases, &target.path)
                        ),
                    });
                }
            }
        }
    }
    findings.sort_by(|a, b| {
        a.source_path
            .cmp(&b.source_path)
            .then_with(|| a.target_id.cmp(&b.target_id))
    });
    findings
}

fn unlinked_reference_findings(items: &[CorpusItem]) -> Vec<DoctorFinding> {
    detect_unlinked_references(items)
        .into_iter()
        .map(|r| DoctorFinding {
            path: r.source_path,
            code: CODE_UNLINKED_REFERENCE,
            severity: SEVERITY_WARNING,
            problem: format!(
                "body references {} but declares no {} link to it",
                r.matched_token, r.related_section
            ),
            fix: format!(
                "Add `{}` under `## {}` if the link is intended \u{2014} a suggestion to \
                 review; RAC writes no edge (ADR-082).",
                r.suggested_line, r.related_section
            ),
        })
        .collect()
}

fn unlinked_reference_findings_with_index(
    items: &[CorpusItem],
    identity_index: &[IndexEntry],
    relationships: &[Relationship],
) -> Vec<DoctorFinding> {
    detect_unlinked_references_with_index(items, identity_index, relationships)
        .into_iter()
        .map(|record| DoctorFinding {
            path: record.source_path,
            code: CODE_UNLINKED_REFERENCE,
            severity: SEVERITY_WARNING,
            problem: format!(
                "body references {} but declares no {} link to it",
                record.matched_token, record.related_section
            ),
            fix: format!(
                "Add `{}` under `## {}` if the link is intended — a suggestion to \
                 review; RAC writes no edge (ADR-082).",
                record.suggested_line, record.related_section
            ),
        })
        .collect()
}
