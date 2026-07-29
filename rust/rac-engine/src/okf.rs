//! OKF bundle export (`decided.output.okf` + the recency join) — `decided export
//! --okf`, per PORT-CONTRACT.d/17 §4.
//!
//! A derived tree of Markdown files: one per typed artifact at its path
//! relative to the exported corpus root, plus generated `index.md` and
//! `log.md`. OKF v0.2 `generated.at` derives from the last git commit time
//! with the committer's stored offset preserved (`%cI`, ADR-045); when git
//! cannot answer, `generated` is omitted and `log.md` degrades to a placeholder.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;

use crate::export::{CorpusExport, ExportArtifact};
use crate::gitinfo;
use crate::markdown::split_frontmatter;
use crate::pycompat::{py_relpath, py_strip, read_text_universal};

/// RAC `type` → OKF `type` (`decided.core.okf.OKF_TYPE`, ADR-048).
fn okf_type(rac_type: &str) -> &'static str {
    match rac_type {
        "requirement" => "Requirement",
        "decision" => "ADR",
        "design" => "Design",
        "roadmap" => "Roadmap",
        "prompt" => "Prompt",
        // Unknown-type files are excluded from the export, so every
        // exported artifact's type resolves (a KeyError would be an
        // engine bug, not an input condition).
        other => panic!("no OKF type mapping for {other:?}"),
    }
}

/// Human plural headings for the index, in the fixed disclosure order.
const INDEX_SECTIONS: [(&str, &str); 5] = [
    ("requirement", "Requirements"),
    ("decision", "Decisions"),
    ("design", "Designs"),
    ("roadmap", "Roadmaps"),
    ("prompt", "Prompts"),
];

const INDEX_PATH: &str = "index.md";
const LOG_PATH: &str = "log.md";

/// One artifact's git-derived authored times as verbatim-offset ISO strings
/// (already `fromisoformat().isoformat()` round-tripped), or `None` when git
/// does not know. Mirrors `ArtifactRecency` with `with_creation=True`.
pub struct ArtifactRecency {
    pub path: String,
    pub first_committed: Option<String>,
    pub last_committed: Option<String>,
}

/// `_parse_stamp` fidelity gate: the oracle turns an unparseable stamp into
/// `None`; mirror by validating before round-tripping.
fn parsed(stamp: Option<String>) -> Option<String> {
    let s = stamp?;
    gitinfo::parse_iso8601_epoch(&s)?;
    Some(gitinfo::isoformat_roundtrip(&s))
}

/// `artifact_recency(directory, with_creation=True)`, restricted to the
/// export's artifact set (identical to the oracle's recognised-walk set).
/// Outside a repository every value is `None` — no error crosses the
/// boundary (ADR-045).
pub fn artifact_recency(directory: &str, export: &CorpusExport) -> Vec<ArtifactRecency> {
    let repo_root = gitinfo::repository_root(Path::new(directory));
    export
        .artifacts
        .iter()
        .map(|art| {
            let (first, last) = match &repo_root {
                None => (None, None),
                Some(root) => (
                    parsed(gitinfo::first_committed(root, Path::new(&art.path))),
                    parsed(gitinfo::last_committed(root, Path::new(&art.path))),
                ),
            };
            ArtifactRecency {
                path: art.path.clone(),
                first_committed: first,
                last_committed: last,
            }
        })
        .collect()
}

/// `_body(path)` — the Markdown body after the frontmatter envelope,
/// re-read in text mode, stripped. The oracle's strict-utf8 read would
/// crash on invalid bytes; this port degrades to an empty body
/// (PORT-CONTRACT decision 3, same posture as the export body reader).
fn body(path: &str) -> String {
    let text = read_text_universal(path).unwrap_or_default();
    py_strip(&split_frontmatter(&text).body).to_string()
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a Rust string cannot fail")
}

fn okf_status(status: &str) -> Option<&'static str> {
    match status.trim().to_ascii_lowercase().as_str() {
        "" | "unknown" => None,
        "proposed" | "draft" => Some("draft"),
        "retired" | "superseded" | "deprecated" | "obsolete" => Some("deprecated"),
        _ => Some("stable"),
    }
}

/// One v0.2 concept file. Structural relationships are navigation links, not
/// provenance, so they deliberately do not populate `sources`.
fn artifact_file(
    art: &ExportArtifact,
    related: &[(String, String)],
    updated: Option<&str>,
) -> String {
    let mut lines = vec![
        "---".to_string(),
        format!("type: {}", okf_type(&art.artifact_type)),
        format!("id: {}", yaml_string(&art.id)),
        format!("title: {}", yaml_string(&art.title)),
    ];
    if let Some(status) = okf_status(&art.status) {
        lines.push(format!("status: {status}"));
    }
    if let Some(updated) = updated {
        lines.push("generated:".to_string());
        lines.push(format!("  by: asdecided/{}", env!("CARGO_PKG_VERSION")));
        lines.push(format!("  at: {updated}"));
    }
    if !art.tags.is_empty() {
        lines.push(format!(
            "tags: {}",
            serde_json::to_string(&art.tags).expect("serializing tags cannot fail")
        ));
    }
    lines.push("---".to_string());
    lines.push(String::new());
    lines.push(body(&art.path));
    if !related.is_empty() {
        lines.push(String::new());
        lines.push("# Related concepts".to_string());
        lines.push(String::new());
        for (title, path) in related {
            lines.push(format!("- [{title}]({path})"));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Resolved outgoing relationships
/// as `(title, bundle path)` pairs, in relationship order.
fn related_concepts(
    art: &ExportArtifact,
    export: &CorpusExport,
    by_id: &HashMap<&str, &ExportArtifact>,
    rel: &HashMap<&str, String>,
) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for edge in &export.relationships {
        if edge.from != art.id {
            continue;
        }
        if let Some(target) = by_id.get(edge.to.as_str()) {
            pairs.push((target.title.clone(), rel[target.path.as_str()].clone()));
        }
    }
    pairs
}

/// `_index(export, rel)` — overview line, then artifacts by type in the
/// fixed section order (artifact order preserved within a section).
fn index(export: &CorpusExport, rel: &HashMap<&str, String>) -> String {
    let count = export.artifact_count();
    let noun = if count == 1 { "artifact" } else { "artifacts" };
    let mut lines = vec![
        "---".to_string(),
        "okf_version: \"0.2\"".to_string(),
        "---".to_string(),
        String::new(),
        format!("# {} \u{2014} Knowledge Index", export.corpus_name),
        String::new(),
        format!(
            "A derived OKF bundle of {count} {noun}. The AsDecided corpus is authoritative; \
             this index is a generated entry point."
        ),
    ];
    for (type_name, heading) in INDEX_SECTIONS {
        let members: Vec<&ExportArtifact> = export
            .artifacts
            .iter()
            .filter(|a| a.artifact_type == type_name)
            .collect();
        if members.is_empty() {
            continue;
        }
        lines.push(String::new());
        lines.push(format!("## {heading}"));
        lines.push(String::new());
        for art in members {
            lines.push(format!("- [{}]({})", art.title, rel[art.path.as_str()]));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// `_log(export, recency, rel)` — corpus history grouped by commit date
/// (the `%cI` civil date, offset preserved), newest first; within a day,
/// path order. No git history → the placeholder.
fn log(
    export: &CorpusExport,
    recency: &[ArtifactRecency],
    rel: &HashMap<&str, String>,
) -> String {
    let title_by_path: HashMap<&str, &str> = export
        .artifacts
        .iter()
        .map(|a| (a.path.as_str(), a.title.as_str()))
        .collect();
    let mut dated: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for a in recency {
        let Some(committed) = &a.last_committed else { continue };
        if !title_by_path.contains_key(a.path.as_str()) {
            continue;
        }
        // `committed.date().isoformat()` — the stamp's stored civil date.
        let day = committed.chars().take(10).collect::<String>();
        dated.entry(day).or_default().push(a.path.as_str());
    }
    if dated.is_empty() {
        return "# Log\n\n_No commit history available._\n".to_string();
    }
    let mut lines = vec!["# Log".to_string()];
    for (day, paths) in dated.iter().rev() {
        lines.push(String::new());
        lines.push(format!("## {day}"));
        lines.push(String::new());
        let mut paths = paths.clone();
        paths.sort_unstable();
        for path in paths {
            lines.push(format!("- [{}]({})", title_by_path[path], rel[path]));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// `render_okf_bundle(export, recency, root)` — `{relative path: contents}`
/// in a sorted map (the CLI writes `sorted(bundle.items())`).
///
/// `Err` mirrors the oracle's uncaught `ValueError` on an `index.md` /
/// `log.md` filename collision (a Python traceback, exit 1 — normally
/// prevented by the okf-reserved-filename validate gate).
pub fn render_okf_bundle(
    export: &CorpusExport,
    recency: &[ArtifactRecency],
    root: &str,
) -> Result<BTreeMap<String, String>, String> {
    let rel: HashMap<&str, String> = export
        .artifacts
        .iter()
        .map(|a| (a.path.as_str(), py_relpath(&a.path, root)))
        .collect();
    // Dict-comprehension semantics: a duplicated id keeps the LAST artifact.
    let mut by_id: HashMap<&str, &ExportArtifact> = HashMap::new();
    for art in &export.artifacts {
        by_id.insert(&art.id, art);
    }
    let recency_by_path: HashMap<&str, &ArtifactRecency> =
        recency.iter().map(|a| (a.path.as_str(), a)).collect();

    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for art in &export.artifacts {
        let key = rel[art.path.as_str()].clone();
        if key == INDEX_PATH || key == LOG_PATH {
            return Err(format!(
                "artifact path '{key}' collides with a generated bundle file"
            ));
        }
        let record = recency_by_path.get(art.path.as_str());
        let updated = record.and_then(|r| r.last_committed.as_deref());
        files.insert(
            key,
            artifact_file(art, &related_concepts(art, export, &by_id, &rel), updated),
        );
    }
    files.insert(INDEX_PATH.to_string(), index(export, &rel));
    files.insert(LOG_PATH.to_string(), log(export, recency, &rel));
    Ok(files)
}
