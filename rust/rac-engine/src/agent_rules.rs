//! Agent-rules projection (`decided.services.agent_rules`) — `decided export
//! --agent-rules [--check]`, per PORT-CONTRACT.d/17 §3.
//!
//! Distils the live corpus (Accepted, non-retired decisions) into a
//! drift-guarded managed block spliced into per-client agent-context files.
//! The provenance digest is sha256 over the canonical JSON serialization of
//! the ordered entries (`sort_keys`, separators `(",", ":")`,
//! `ensure_ascii=False`) — any byte deviation cascades into `--check` drift,
//! so the canonical dump must match CPython exactly (`pyjson::dumps_canonical_sorted`).

use std::path::Path;

use serde_json::{json, Map, Value};

use crate::identity::artifact_identifier;
use crate::pycompat::{first_nonempty_line, py_casefold, read_text_universal};
use crate::spec::spec_for;

const DECISION_TYPE: &str = "decision";
const LIVE_STATUS: &str = "accepted";

const BEGIN_PREFIX: &str = "<!-- BEGIN RAC MANAGED BLOCK (digest: ";
const BEGIN_SUFFIX: &str = ") -->";
const END_MARKER: &str = "<!-- END RAC MANAGED BLOCK -->";

const GENERATED_HEADER: &str = "<!-- Managed by `decided export --agent-rules`. \
     Edit decisions in decisions/, not here; content outside this block is preserved. -->";

/// One per-client target file (selector + root-relative path), in the
/// oracle's fixed `TARGETS` order.
pub struct AgentRulesTarget {
    pub client: &'static str,
    pub path: &'static str,
}

pub const TARGETS: [AgentRulesTarget; 4] = [
    AgentRulesTarget { client: "agents", path: "AGENTS.md" },
    AgentRulesTarget { client: "claude", path: "CLAUDE.md" },
    AgentRulesTarget { client: "copilot", path: ".github/copilot-instructions.md" },
    AgentRulesTarget { client: "cursor", path: ".cursor/rules" },
];

/// `targets_for(clients)` — always `TARGETS` order regardless of selector
/// order (and duplicates collapse via set membership).
fn targets_for(clients: &[String]) -> Vec<&'static AgentRulesTarget> {
    if clients.is_empty() {
        return TARGETS.iter().collect();
    }
    TARGETS
        .iter()
        .filter(|t| clients.iter().any(|c| c == t.client))
        .collect()
}

/// One distilled live-decision pointer.
struct AgentRulesEntry {
    identifier: String,
    title: String,
    category: Option<String>,
}

// Per-file outcome states (stable JSON contract, ADR-007).
pub const STATE_WRITTEN: &str = "written";
pub const STATE_UPDATED: &str = "updated";
pub const STATE_IN_SYNC: &str = "in-sync";
pub const STATE_STALE: &str = "stale";
pub const STATE_MISSING: &str = "missing";

pub struct AgentRulesFileResult {
    pub client: &'static str,
    pub path: &'static str,
    pub state: &'static str,
}

/// The outcome of a generate or check run (mirrors `AgentRulesResult`).
pub struct AgentRulesResult {
    /// `"generate"` or `"check"`.
    pub mode: &'static str,
    pub digest: String,
    /// `str(root_path)` — the PurePosixPath-normalized output root.
    pub root: String,
    pub files: Vec<AgentRulesFileResult>,
}

impl AgentRulesResult {
    /// True when any checked file is stale or missing its block.
    pub fn drifted(&self) -> bool {
        self.files
            .iter()
            .any(|f| f.state == STATE_STALE || f.state == STATE_MISSING)
    }
}

/// `str(PurePosixPath(p))`: duplicate slashes and `.` components collapse,
/// a trailing slash drops, `""` becomes `"."`; a leading `//` (exactly two
/// slashes) is preserved, `/`+ otherwise collapses to one.
pub fn py_path_str(p: &str) -> String {
    let double_root = p.starts_with("//") && !p.starts_with("///");
    let absolute = p.starts_with('/');
    let comps: Vec<&str> = p.split('/').filter(|c| !c.is_empty() && *c != ".").collect();
    let body = comps.join("/");
    if absolute {
        let root = if double_root { "//" } else { "/" };
        format!("{root}{body}")
    } else if body.is_empty() {
        ".".to_string()
    } else {
        body
    }
}

/// `PurePosixPath(a) / b` for a relative `b`, rendered as `str(...)`.
fn py_path_join(a: &str, b: &str) -> String {
    if a == "." {
        py_path_str(b)
    } else {
        py_path_str(&format!("{a}/{b}"))
    }
}

/// `_agent_rules_root(directory, out)` — explicit `--out` wins; else the
/// parent of a `decisions/`-named directory (or `.` when that parent is `.`),
/// else the directory itself.
pub fn agent_rules_root(directory: &str, out: Option<&str>) -> String {
    if let Some(o) = out {
        return py_path_str(o);
    }
    let path = py_path_str(directory.trim_end_matches('/'));
    let name = path.rsplit('/').next().unwrap_or("");
    if name == "decisions" {
        let parent = match path.rfind('/') {
            Some(idx) if idx > 0 => &path[..idx],
            Some(_) => "/",
            None => ".",
        };
        if parent.is_empty() || parent == "." {
            ".".to_string()
        } else {
            parent.to_string()
        }
    } else {
        path
    }
}

/// `artifact_status(product)` — first non-empty line of `## Status`.
fn artifact_status(artifact: &crate::parse::Artifact) -> String {
    artifact
        .section("status")
        .map(first_nonempty_line)
        .unwrap_or("")
        .to_string()
}

/// `_category(product)` — first non-empty line of `## Category`, or `None`.
fn category(artifact: &crate::parse::Artifact) -> Option<String> {
    let line = artifact
        .section("category")
        .map(first_nonempty_line)
        .unwrap_or("");
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

/// `_is_live_decision` — Accepted and not spec-retired (ADR-067, ADR-051).
fn is_live_decision(artifact: &crate::parse::Artifact) -> bool {
    let status = py_casefold(&artifact_status(artifact));
    if status != LIVE_STATUS {
        return false;
    }
    let retired: Vec<String> = spec_for(DECISION_TYPE)
        .map(|s| s.retired_status.iter().map(|r| py_casefold(r)).collect())
        .unwrap_or_default();
    !retired.contains(&status)
}

/// `build_agent_rules_block(directory)` → ordered entries + digest.
fn build_projection(directory: &str) -> Result<(Vec<AgentRulesEntry>, String), String> {
    let mut entries: Vec<AgentRulesEntry> = Vec::new();
    for item in crate::federated_corpus::local_writable_items(directory, true)
        .map_err(|error| error.to_string())?
    {
        let Some(spec) = item.spec else { continue };
        if spec.name != DECISION_TYPE || !is_live_decision(&item.artifact) {
            continue;
        }
        let identifier = artifact_identifier(&item.artifact, item.spec, &item.path);
        let title = match &item.artifact.product.title {
            Some(t) if !t.is_empty() => t.clone(),
            _ => identifier.clone(),
        };
        entries.push(AgentRulesEntry {
            identifier,
            title,
            category: category(&item.artifact),
        });
    }
    // Deterministic order: casefolded identifier, exact identifier tiebreak.
    entries.sort_by(|a, b| {
        py_casefold(&a.identifier)
            .cmp(&py_casefold(&b.identifier))
            .then_with(|| a.identifier.cmp(&b.identifier))
    });
    let payload: Vec<Value> = entries
        .iter()
        .map(|e| {
            let mut m = Map::new();
            m.insert("identifier".into(), json!(e.identifier));
            m.insert("title".into(), json!(e.title));
            m.insert("category".into(), json!(e.category));
            Value::Object(m)
        })
        .collect();
    let canonical = crate::pyjson::dumps_canonical_sorted(&Value::Array(payload));
    let digest = crate::sha256::hexdigest(canonical.as_bytes());
    Ok((entries, digest))
}

/// `render_managed_block(projection)` — markers + distilled pointers, no
/// trailing newline (the merge adds it).
fn render_managed_block(entries: &[AgentRulesEntry], digest: &str) -> String {
    let mut lines = vec![
        format!("{BEGIN_PREFIX}{digest}{BEGIN_SUFFIX}"),
        GENERATED_HEADER.to_string(),
        "## Settled decisions (AsDecided)".to_string(),
        String::new(),
        "These decisions are already accepted. Do not re-open or contradict them; \
         ask the AsDecided MCP tools (`get_artifact`, `search_artifacts`) for the \
         full text before proposing a change that touches one."
            .to_string(),
        String::new(),
    ];
    if entries.is_empty() {
        lines.push("_No live decisions recorded yet._".to_string());
    } else {
        for entry in entries {
            let suffix = match &entry.category {
                Some(c) => format!(" _({c})_"),
                None => String::new(),
            };
            lines.push(format!(
                "- **{}** \u{2014} {}{suffix}",
                entry.identifier, entry.title
            ));
        }
    }
    lines.push(END_MARKER.to_string());
    lines.join("\n")
}

/// `embedded_digest(file_text)` — the digest in the BEGIN marker, or `None`.
fn embedded_digest(file_text: &str) -> Option<String> {
    let start = file_text.find(BEGIN_PREFIX)?;
    let after = start + BEGIN_PREFIX.len();
    let end = after + file_text[after..].find(BEGIN_SUFFIX)?;
    let digest = crate::pycompat::py_strip(&file_text[after..end]);
    if digest.is_empty() {
        None
    } else {
        Some(digest.to_string())
    }
}

/// `merge_managed_block(existing, block)` — splice, preserving everything
/// outside the markers; always ends with a single trailing newline.
fn merge_managed_block(existing: Option<&str>, block: &str) -> String {
    let existing = match existing {
        None => return format!("{block}\n"),
        Some(e) if crate::pycompat::py_strip(e).is_empty() => return format!("{block}\n"),
        Some(e) => e,
    };
    let begin = existing.find(BEGIN_PREFIX);
    let end = existing.find(END_MARKER);
    if let (Some(begin), Some(end)) = (begin, end) {
        if end > begin {
            let end = end + END_MARKER.len();
            let mut merged =
                format!("{}{}{}", &existing[..begin], block, &existing[end..]);
            if !merged.ends_with('\n') {
                merged.push('\n');
            }
            return merged;
        }
    }
    // No managed block yet: append one, separated by a blank line.
    let body = existing.trim_end_matches('\n');
    format!("{body}\n\n{block}\n")
}

/// `generate_agent_rules(directory, root, clients)` — write/update the
/// managed block in each target under `root`. Writes are skipped when the
/// embedded digest already matches (idempotent). An io error surfaces as
/// `Err` for the caller's `cannot write under {root}` usage error.
pub fn generate_agent_rules(
    directory: &str,
    root: &str,
    clients: &[String],
) -> Result<AgentRulesResult, String> {
    let (entries, digest) = build_projection(directory)?;
    let block = render_managed_block(&entries, &digest);

    let mut files: Vec<AgentRulesFileResult> = Vec::new();
    for target in targets_for(clients) {
        let dest = py_path_join(root, target.path);
        let dest_path = Path::new(&dest);
        let existing = if dest_path.exists() {
            // The oracle's strict-utf8 `read_text` would crash on invalid
            // bytes; a healthy target file always decodes. An unreadable
            // file degrades to the OSError path via the write below.
            read_text_universal(&dest)
        } else {
            None
        };

        if let Some(text) = &existing {
            if embedded_digest(text).as_deref() == Some(digest.as_str()) {
                files.push(AgentRulesFileResult {
                    client: target.client,
                    path: target.path,
                    state: STATE_IN_SYNC,
                });
                continue;
            }
        }

        let merged = merge_managed_block(existing.as_deref(), &block);
        if let Some(parent) = dest_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        std::fs::write(dest_path, merged).map_err(|e| e.to_string())?;
        files.push(AgentRulesFileResult {
            client: target.client,
            path: target.path,
            state: if existing.is_none() { STATE_WRITTEN } else { STATE_UPDATED },
        });
    }

    Ok(AgentRulesResult {
        mode: "generate",
        digest,
        root: root.to_string(),
        files,
    })
}

/// `check_agent_rules(directory, root, clients)` — never writes; compares
/// each present target's embedded digest to the live projection.
pub fn check_agent_rules(
    directory: &str,
    root: &str,
    clients: &[String],
) -> Result<AgentRulesResult, String> {
    let (_, digest) = build_projection(directory)?;

    let mut files: Vec<AgentRulesFileResult> = Vec::new();
    for target in targets_for(clients) {
        let dest = py_path_join(root, target.path);
        let state = if !Path::new(&dest).exists() {
            STATE_MISSING
        } else {
            match read_text_universal(&dest).as_deref().and_then(embedded_digest) {
                None => STATE_MISSING,
                Some(d) if d == digest => STATE_IN_SYNC,
                Some(_) => STATE_STALE,
            }
        };
        files.push(AgentRulesFileResult {
            client: target.client,
            path: target.path,
            state,
        });
    }

    Ok(AgentRulesResult {
        mode: "check",
        digest,
        root: root.to_string(),
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_str_normalization() {
        assert_eq!(py_path_str(""), ".");
        assert_eq!(py_path_str("."), ".");
        assert_eq!(py_path_str("./x"), "x");
        assert_eq!(py_path_str("a//b/./c/"), "a/b/c");
        assert_eq!(py_path_str("/a/b"), "/a/b");
        assert_eq!(py_path_str("//a"), "//a");
        assert_eq!(py_path_str("///a"), "/a");
    }

    #[test]
    fn root_resolution() {
        assert_eq!(agent_rules_root("decisions", None), ".");
        assert_eq!(agent_rules_root("decisions/", None), ".");
        assert_eq!(agent_rules_root("./decisions", None), ".");
        assert_eq!(agent_rules_root("proj/decisions", None), "proj");
        assert_eq!(agent_rules_root("proj/sub/decisions", None), "proj/sub");
        assert_eq!(agent_rules_root("/abs/decisions", None), "/abs");
        assert_eq!(agent_rules_root("corpus", None), "corpus");
        assert_eq!(agent_rules_root("proj/decisions", Some("custom")), "custom");
    }

    #[test]
    fn merge_rules() {
        let block = "<!-- BEGIN RAC MANAGED BLOCK (digest: d) -->\nB\n<!-- END RAC MANAGED BLOCK -->";
        assert_eq!(merge_managed_block(None, block), format!("{block}\n"));
        assert_eq!(merge_managed_block(Some(""), block), format!("{block}\n"));
        assert_eq!(
            merge_managed_block(Some("prose\n"), block),
            format!("prose\n\n{block}\n")
        );
        assert_eq!(
            merge_managed_block(Some("prose without newline"), block),
            format!("prose without newline\n\n{block}\n")
        );
        let seeded = format!("above\n\n{block}\nafter\n");
        let updated = merge_managed_block(Some(&seeded), block);
        assert_eq!(updated, seeded);
    }

    #[test]
    fn embedded_digest_extraction() {
        assert_eq!(embedded_digest("no block"), None);
        assert_eq!(
            embedded_digest("<!-- BEGIN RAC MANAGED BLOCK (digest: abc123) -->"),
            Some("abc123".to_string())
        );
        assert_eq!(
            embedded_digest("<!-- BEGIN RAC MANAGED BLOCK (digest:  ) -->"),
            None
        );
    }
}
