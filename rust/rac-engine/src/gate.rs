//! Policy-aware unified enforcement (`decided.services.gate`, v0.21.14 /
//! ADR-049) plus the STRICT `.decided/config.yaml` loaders it alone consumes
//! (`decided.services.init.load_enforcement_policy` / `load_overrides`, the
//! raising paths).
//!
//! `decided gate` composes validation, relationship integrity, and review over
//! one corpus, then classifies every finding as blocking or advisory under
//! the corpus enforcement policy. The other commands keep the engine's
//! lenient config readers (`validate::load_overrides` skips malformed
//! entries); the gate is the one surface where a malformed config is an
//! operational error — `decided: malformed repository config <path>: <reason>`
//! on stderr, exit 1 (`MalformedRepositoryConfig` in `cmd_gate`).

use crate::commands::{validate_directory, DirectoryValidation};
use crate::frontmatter::{yaml_load_config, Yaml};
use crate::output::relationship_sarif_parts;
use crate::portfolio::portfolio_from_corpus;
use crate::relationships::{
    corpus_items, relationship_severity, validate_relationships, RelationshipValidation,
};
use crate::review::{review_from_portfolio, ReviewReport, PRIORITY_BROKEN_RELATIONSHIP};
use crate::validate::find_config_file;

pub const ENFORCEMENT_BLOCKING: &str = "blocking";
pub const ENFORCEMENT_ADVISORY: &str = "advisory";

pub const SOURCE_VALIDATE: &str = "validate";
pub const SOURCE_RELATIONSHIPS: &str = "relationships";
pub const SOURCE_REVIEW: &str = "review";
pub const SOURCE_SENTRY: &str = "sentry";

// ---------------------------------------------------------------------------
// MalformedRepositoryConfig (decided.services.init)
// ---------------------------------------------------------------------------

/// The gate's operational config error. `message()` is the oracle's
/// `str(MalformedRepositoryConfig)` shape; `cmd_gate` prefixes `decided: `.
#[derive(Debug)]
pub struct MalformedConfig {
    pub config_path: String,
    pub reason: String,
}

impl MalformedConfig {
    pub fn message(&self) -> String {
        format!(
            "malformed repository config {}: {}",
            self.config_path, self.reason
        )
    }
}

// ---------------------------------------------------------------------------
// EnforcementPolicy (decided.services.gate.EnforcementPolicy)
// ---------------------------------------------------------------------------

/// Finding codes mapped to an enforcement class (ADR-049). Precedence:
/// `off` (suppress, None) -> `blocking` -> `advisory` -> the caller default.
#[derive(Debug, Default)]
pub struct EnforcementPolicy {
    pub blocking: Vec<String>,
    pub advisory: Vec<String>,
    pub off: Vec<String>,
}

impl EnforcementPolicy {
    fn classify(&self, code: &str, default: &'static str) -> Option<&'static str> {
        if self.off.iter().any(|c| c == code) {
            return None;
        }
        if self.blocking.iter().any(|c| c == code) {
            return Some(ENFORCEMENT_BLOCKING);
        }
        if self.advisory.iter().any(|c| c == code) {
            return Some(ENFORCEMENT_ADVISORY);
        }
        Some(default)
    }
}

// ---------------------------------------------------------------------------
// Strict config loading (the raising `decided.services.init` paths)
// ---------------------------------------------------------------------------

/// `_parse_config_yaml`: read + YAML-parse the nearest config; a parse
/// failure is `invalid YAML: <problem>` (the oracle embeds PyYAML's exact
/// exception prose here — stderr is out of parity scope, so the engine's
/// own problem text stands in). Returns the top-level mapping pairs, or
/// None when the root is not a mapping (the loaders treat a non-mapping
/// root as "no section", matching `data.get(...) if isinstance(data, dict)`).
fn parse_config_pairs(
    config_path: &std::path::Path,
) -> Result<Option<Vec<(Yaml, Yaml)>>, MalformedConfig> {
    let display = config_path.to_string_lossy().into_owned();
    let text = std::fs::read_to_string(config_path).map_err(|e| MalformedConfig {
        config_path: display.clone(),
        reason: format!("invalid YAML: {e}"),
    })?;
    match yaml_load_config(&text) {
        Ok(Yaml::Map(pairs)) => Ok(Some(pairs)),
        Ok(_) => Ok(None),
        Err(problem) => Err(MalformedConfig {
            config_path: display,
            reason: format!("invalid YAML: {problem}"),
        }),
    }
}

fn yaml_get<'a>(pairs: &'a [(Yaml, Yaml)], name: &str) -> Option<&'a Yaml> {
    pairs.iter().find_map(|(k, v)| match k {
        Yaml::Str(s) if s == name => Some(v),
        _ => None,
    })
}

/// `_parse_code_list`: absent/null -> empty; anything but a list of strings
/// is a malformed config.
fn parse_code_list(
    config_path: &std::path::Path,
    value: Option<&Yaml>,
    where_: &str,
) -> Result<Vec<String>, MalformedConfig> {
    let malformed = || MalformedConfig {
        config_path: config_path.to_string_lossy().into_owned(),
        reason: format!("'{where_}' must be a list of finding-code strings"),
    };
    match value {
        None | Some(Yaml::Null) => Ok(Vec::new()),
        Some(Yaml::List(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Yaml::Str(s) => out.push(s.clone()),
                    _ => return Err(malformed()),
                }
            }
            Ok(out)
        }
        Some(_) => Err(malformed()),
    }
}

/// `load_enforcement_policy(start_dir)` — the STRICT reader `build_gate`
/// consumes. YAML 1.1 resolves a bare `off` key to Bool(false); both key
/// spellings are accepted, `off` winning (`section["off"] if "off" in
/// section else section.get(False)`).
pub fn load_enforcement_policy(start_dir: &str) -> Result<EnforcementPolicy, MalformedConfig> {
    let Some(config_path) = find_config_file(start_dir) else {
        return Ok(EnforcementPolicy::default());
    };
    let Some(pairs) = parse_config_pairs(&config_path)? else {
        return Ok(EnforcementPolicy::default());
    };
    let section = match yaml_get(&pairs, "enforcement") {
        None | Some(Yaml::Null) => return Ok(EnforcementPolicy::default()),
        Some(Yaml::Map(section)) => section,
        Some(_) => {
            return Err(MalformedConfig {
                config_path: config_path.to_string_lossy().into_owned(),
                reason: "'enforcement' must be a mapping".to_string(),
            })
        }
    };
    let blocking = parse_code_list(&config_path, yaml_get(section, "blocking"), "enforcement.blocking")?;
    let advisory = parse_code_list(&config_path, yaml_get(section, "advisory"), "enforcement.advisory")?;
    let off_value = yaml_get(section, "off").or_else(|| {
        section
            .iter()
            .find_map(|(k, v)| matches!(k, Yaml::Bool(false)).then_some(v))
    });
    let off = parse_code_list(&config_path, off_value, "enforcement.off")?;
    Ok(EnforcementPolicy {
        blocking,
        advisory,
        off,
    })
}

/// A Yaml key rendered the way Python's f-string would render the parsed
/// value inside `'validation.rules.<name>'` — best effort, stderr-only.
fn yaml_key_display(key: &Yaml) -> String {
    match key {
        Yaml::Str(s) => s.clone(),
        Yaml::Bool(true) => "True".to_string(),
        Yaml::Bool(false) => "False".to_string(),
        Yaml::Int(i) => i.to_string(),
        Yaml::Null => "None".to_string(),
        other => format!("{other:?}"),
    }
}

/// `_parse_severity_map` (strict): every entry must map a string name to an
/// allowed severity; YAML 1.1 bools coerce to `off`/`on` first.
fn check_severity_map(
    config_path: &std::path::Path,
    value: Option<&Yaml>,
    where_: &str,
    allowed: &[&str],
) -> Result<(), MalformedConfig> {
    let section = match value {
        None | Some(Yaml::Null) => return Ok(()),
        Some(Yaml::Map(section)) => section,
        Some(_) => {
            return Err(MalformedConfig {
                config_path: config_path.to_string_lossy().into_owned(),
                reason: format!("'{where_}' must be a mapping"),
            })
        }
    };
    for (name, sev) in section {
        let sev_text = match sev {
            Yaml::Bool(false) => Some("off".to_string()),
            Yaml::Bool(true) => Some("on".to_string()),
            Yaml::Str(s) => Some(s.clone()),
            _ => None,
        };
        let ok = matches!(name, Yaml::Str(_))
            && sev_text
                .as_deref()
                .map(|s| allowed.contains(&s))
                .unwrap_or(false);
        if !ok {
            return Err(MalformedConfig {
                config_path: config_path.to_string_lossy().into_owned(),
                reason: format!(
                    "'{where_}.{}' must map a name to one of {}",
                    yaml_key_display(name),
                    allowed.join(", ")
                ),
            });
        }
    }
    Ok(())
}

/// The STRICT face of `load_overrides` (ADR-053): `build_gate` validates the
/// `validation` section shape and raises where the oracle raises. The
/// override VALUES applied to the pipeline come from the engine's lenient
/// loader inside `validate_directory` — identical whenever this check
/// passes (the lenient reader only ever drops entries this one rejects).
pub fn check_overrides(start_dir: &str) -> Result<(), MalformedConfig> {
    let Some(config_path) = find_config_file(start_dir) else {
        return Ok(());
    };
    let Some(pairs) = parse_config_pairs(&config_path)? else {
        return Ok(());
    };
    let section = match yaml_get(&pairs, "validation") {
        None | Some(Yaml::Null) => return Ok(()),
        Some(Yaml::Map(section)) => section,
        Some(_) => {
            return Err(MalformedConfig {
                config_path: config_path.to_string_lossy().into_owned(),
                reason: "'validation' must be a mapping".to_string(),
            })
        }
    };
    check_severity_map(
        &config_path,
        yaml_get(section, "rules"),
        "validation.rules",
        &["error", "warning", "off"],
    )?;
    check_severity_map(
        &config_path,
        yaml_get(section, "types"),
        "validation.types",
        &["error", "warning"],
    )
}

// ---------------------------------------------------------------------------
// GateFinding / GateReport
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct GateFinding {
    pub source: &'static str,
    pub code: String,
    pub severity: String,
    pub enforcement: &'static str,
    pub path: String,
    pub line: Option<i64>,
    pub message: String,
    pub decision_id: Option<String>,
    pub origin: Option<crate::corpus::ArtifactOrigin>,
}

pub struct GateReport {
    pub directory: String,
    pub recursive: bool,
    pub findings: Vec<GateFinding>,
    pub code_coverage: Option<CodeCoverage>,
}

pub struct CodeCoverage {
    pub live_decisions: usize,
    pub classified_decisions: usize,
    pub unclassified_decisions: usize,
    pub eligible_decisions: usize,
    pub constrained_decisions: usize,
    pub active_rules: usize,
    pub corpus_adoption_percent: f64,
    pub eligible_coverage_percent: f64,
}

impl GateReport {
    pub fn blocking(&self) -> Vec<&GateFinding> {
        self.findings
            .iter()
            .filter(|f| f.enforcement == ENFORCEMENT_BLOCKING)
            .collect()
    }

    pub fn advisory(&self) -> Vec<&GateFinding> {
        self.findings
            .iter()
            .filter(|f| f.enforcement == ENFORCEMENT_ADVISORY)
            .collect()
    }

    /// True when nothing is blocking — advisory findings never fail the gate.
    pub fn ok(&self) -> bool {
        self.blocking().is_empty()
    }
}

// ---------------------------------------------------------------------------
// build_gate
// ---------------------------------------------------------------------------

/// One normalized validate finding: `(code, severity, path, line, message)`.
type RawFinding = (String, String, String, Option<i64>, String);

/// One normalized `(code, severity, path, line, message)` per validate
/// finding: per-file issues (line-anchored) then OKF findings (file-level).
fn validate_findings(result: &DirectoryValidation) -> Vec<RawFinding> {
    let mut out = Vec::new();
    for file in &result.files {
        for issue in &file.issues {
            out.push((
                issue.code.clone(),
                issue.severity.to_string(),
                file.path.clone(),
                issue.line,
                issue.message.clone(),
            ));
        }
    }
    if let Some(okf) = &result.okf {
        for finding in &okf.findings {
            out.push((
                finding.code.clone(),
                finding.severity.clone(),
                finding.path.clone(),
                None,
                finding.message.clone(),
            ));
        }
    }
    out
}

/// `build_gate(directory, recursive)` — run validation, relationships, and
/// review, then enforce the corpus policy. Findings the policy turns `off`
/// are dropped; the rest sort by `(path, line or 0, source, code, message)`.
pub fn build_gate(directory: &str, recursive: bool) -> Result<GateReport, MalformedConfig> {
    build_gate_with_code(directory, recursive, None)
}

pub struct CodeGateOptions<'a> {
    pub repository: &'a str,
    pub base: Option<&'a str>,
    pub full_tree: bool,
}

pub fn build_gate_with_code(
    directory: &str,
    recursive: bool,
    code: Option<CodeGateOptions<'_>>,
) -> Result<GateReport, MalformedConfig> {
    build_gate_internal(directory, recursive, code, None)
}

/// Unified gate over one verified effective composition.
pub fn build_gate_with_composed(
    directory: &str,
    recursive: bool,
    code: Option<CodeGateOptions<'_>>,
    corpus: &crate::composition::ComposedCorpus,
) -> Result<GateReport, MalformedConfig> {
    build_gate_internal(directory, recursive, code, Some(corpus))
}

fn build_gate_internal(
    directory: &str,
    recursive: bool,
    code: Option<CodeGateOptions<'_>>,
    composed: Option<&crate::composition::ComposedCorpus>,
) -> Result<GateReport, MalformedConfig> {
    // The oracle raises from load_enforcement_policy first, then
    // load_overrides — mirror that order so a doubly-malformed config
    // reports the enforcement error.
    let policy = load_enforcement_policy(directory)?;
    check_overrides(directory)?;

    let items: Vec<_> = composed
        .map(|corpus| corpus.effective().cloned().collect())
        .unwrap_or_else(|| corpus_items(directory, recursive));
    let validation = composed.map_or_else(
        || validate_directory(directory, recursive),
        |_| crate::commands::validate_directory_from_items(directory, recursive, &items),
    );
    let relationships: RelationshipValidation = composed.map_or_else(
        || validate_relationships(directory, recursive),
        |corpus| corpus.validate_relationships(directory, recursive),
    );
    // Parent review advisories remain parent-owned. Validation,
    // relationships, and code enforcement use the effective corpus, but a
    // child gate reviews only its writable local layer.
    let review_items: Vec<_> = composed
        .map(|corpus| corpus.local_items().cloned().collect())
        .unwrap_or_else(|| items.clone());
    let portfolio = portfolio_from_corpus(directory, &review_items, recursive);
    let review: ReviewReport = review_from_portfolio(directory, portfolio, recursive);

    let mut findings: Vec<GateFinding> = Vec::new();
    let mut code_coverage = None;
    let mut add = |source: &'static str,
                   code: String,
                   severity: String,
                   path: String,
                   line: Option<i64>,
                   message: String,
                   decision_id: Option<String>,
                   origin: Option<crate::corpus::ArtifactOrigin>,
                   default: &'static str| {
        if let Some(enforcement) = policy.classify(&code, default) {
            findings.push(GateFinding {
                source,
                code,
                severity,
                enforcement,
                path,
                line,
                message,
                decision_id,
                origin,
            });
        }
    };

    // Validate: an "error" fails validation, so it is blocking by default;
    // warnings and OKF info findings are advisory.
    for (code, severity, path, line, message) in validate_findings(&validation) {
        let default = if severity == "error" {
            ENFORCEMENT_BLOCKING
        } else {
            ENFORCEMENT_ADVISORY
        };
        add(
            SOURCE_VALIDATE,
            code,
            severity,
            path,
            line,
            message,
            None,
            None,
            default,
        );
    }

    // Relationships: every issue fails `--validate` today, so blocking by
    // default. Message and (percent-encoded) path come from the shared SARIF
    // result builder so the two surfaces can never drift.
    for issue in &relationships.issues {
        let (message, uri) = relationship_sarif_parts(issue);
        let severity = relationship_severity(&issue.code).to_string();
        add(
            SOURCE_RELATIONSHIPS,
            issue.code.clone(),
            severity,
            uri,
            None,
            message,
            None,
            None,
            ENFORCEMENT_BLOCKING,
        );
    }

    // Review: priority 1-2 findings fail review today, so they are blocking
    // by default; advisory priorities (3+) are advisory.
    for issue in &review.issues {
        let message = if issue.action.is_empty() {
            issue.message.clone()
        } else {
            format!("{} \u{2014} {}", issue.message, issue.action)
        };
        let default = if issue.priority <= PRIORITY_BROKEN_RELATIONSHIP {
            ENFORCEMENT_BLOCKING
        } else {
            ENFORCEMENT_ADVISORY
        };
        add(
            SOURCE_REVIEW,
            issue.code.clone(),
            issue.severity.clone(),
            issue.path.clone(),
            None,
            message,
            None,
            None,
            default,
        );
    }

    if let Some(options) = code {
        let sentry = if let Some(corpus) = composed {
            crate::sentry::analyze_with_items_excluding(
                directory,
                options.repository,
                options.base,
                options.full_tree,
                &items,
                true,
                corpus.read_only_roots(),
            )
        } else {
            crate::sentry::analyze(
                directory,
                options.repository,
                recursive,
                options.base,
                options.full_tree,
            )
        };
        match sentry {
            Ok(report) => {
                code_coverage = Some(CodeCoverage {
                    live_decisions: report.live_decisions,
                    classified_decisions: report.classified_decisions,
                    unclassified_decisions: report.unclassified_decisions(),
                    eligible_decisions: report.eligible_decisions,
                    constrained_decisions: report.constrained_decisions,
                    active_rules: report.active_rules,
                    corpus_adoption_percent: report.corpus_adoption_percent(),
                    eligible_coverage_percent: report.eligible_coverage_percent(),
                });
                for finding in report.findings {
                    let decision_id = finding.decision_id.clone();
                    let origin = finding.origin.clone();
                    let message = if composed.is_some() {
                        let context = [finding.decision_id.as_deref(), finding.rule_id.as_deref()]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(" ");
                        if context.is_empty() {
                            finding.message
                        } else {
                            format!("{context}: {}", finding.message)
                        }
                    } else {
                        finding.message
                    };
                    add(
                        SOURCE_SENTRY,
                        finding.code.to_string(),
                        "error".to_string(),
                        finding.path,
                        finding.line,
                        message,
                        decision_id,
                        origin,
                        ENFORCEMENT_BLOCKING,
                    );
                }
            }
            Err(message) => {
                add(
                    SOURCE_SENTRY,
                    crate::sentry::INVALID_CONSTRAINT.to_string(),
                    "error".to_string(),
                    directory.to_string(),
                    None,
                    message,
                    None,
                    None,
                    ENFORCEMENT_BLOCKING,
                );
            }
        }
    }

    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.unwrap_or(0).cmp(&b.line.unwrap_or(0)))
            .then(a.source.cmp(b.source))
            .then(a.code.cmp(&b.code))
            .then(a.message.cmp(&b.message))
    });
    Ok(GateReport {
        directory: directory.to_string(),
        recursive,
        findings,
        code_coverage,
    })
}
