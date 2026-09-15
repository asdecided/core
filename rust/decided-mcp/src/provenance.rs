//! Git-derived artifact provenance — a port of `artifact_provenance`,
//! `_commit_record` and `_status_history` in `src/asdecided/services/recency.py`
//! (WS5, ADR-045/ADR-065). Same narrow git touchpoint as
//! `rac_engine::gitinfo`: shell out to the real `git`, degrade every field to
//! `null` / `[]` when git cannot answer — no error crosses the boundary.

use rac_engine::gitinfo::{pathspec, run_git_text as run_git};
use rac_engine::pycompat::py_strip;
use rac_engine::resolve::artifact_status;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// Field separator in combined `git log --format` records (`\x1f`).
const FIELD_SEP: char = '\x1f';

/// Like `gitinfo::repository_root`, but over the universal-newline `run_git`
/// (Python `text=True`) this surface uses everywhere — the two differ only
/// for a toplevel path containing a bare `\r`.
fn repository_root(directory: &str) -> Option<PathBuf> {
    let out = run_git(&["rev-parse", "--show-toplevel"], Path::new(directory))?;
    let root = out.trim();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

/// `_parse_stamp` → isoformat, or `None` for a blank/unparseable stamp.
fn parse_stamp(stamp: &str) -> Option<String> {
    let stamp = py_strip(stamp);
    if stamp.is_empty() {
        return None;
    }
    // The oracle treats a `fromisoformat` failure as unknown; git `%cI` is
    // always parseable, so validate lightly through the epoch parser.
    rac_engine::gitinfo::parse_iso8601_epoch(stamp)?;
    Some(rac_engine::gitinfo::isoformat_roundtrip(stamp))
}

/// `(committed isoformat | None, author | None)` for one boundary commit.
fn commit_record(repo_root: &Path, spec: &str, earliest: bool) -> (Option<String>, Option<String>) {
    let fmt = format!("--format=%cI{FIELD_SEP}%an <%ae>");
    let args: Vec<&str> = if earliest {
        vec!["log", "--reverse", &fmt, "--", spec]
    } else {
        vec!["log", "-1", &fmt, "--", spec]
    };
    let Some(out) = run_git(&args, repo_root) else {
        return (None, None);
    };
    for line in out.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (stamp, author) = match line.find(FIELD_SEP) {
            Some(i) => (&line[..i], &line[i + 1..]),
            None => (line, ""),
        };
        let author = py_strip(author);
        return (
            parse_stamp(stamp),
            if author.is_empty() {
                None
            } else {
                Some(author.to_string())
            },
        );
    }
    (None, None)
}

/// `_status_history(repo_root, path)` — one entry per parsed `## Status`
/// change, oldest first.
fn status_history(repo_root: &Path, spec: &str) -> Vec<Value> {
    let fmt = format!("--format=%H{FIELD_SEP}%cI{FIELD_SEP}%an <%ae>");
    let Some(walk) = run_git(&["log", "--reverse", &fmt, "--", spec], repo_root) else {
        return Vec::new();
    };
    let mut history: Vec<Value> = Vec::new();
    let mut last_status = String::new();
    for line in walk.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, FIELD_SEP);
        let sha = parts.next().unwrap_or("");
        let stamp = parts.next().unwrap_or("");
        let author = parts.next().unwrap_or("");
        let Some(shown) = run_git(&["show", &format!("{sha}:{spec}")], repo_root) else {
            continue;
        };
        let status = artifact_status(&rac_engine::parse::parse_text(&shown, spec));
        if !status.is_empty() && status != last_status {
            let author = py_strip(author);
            let mut change = Map::new();
            change.insert("status".to_string(), json!(status));
            change.insert(
                "committed".to_string(),
                match parse_stamp(stamp) {
                    Some(s) => json!(s),
                    None => Value::Null,
                },
            );
            change.insert(
                "author".to_string(),
                if author.is_empty() {
                    Value::Null
                } else {
                    json!(author)
                },
            );
            last_status = status;
            history.push(Value::Object(change));
        }
    }
    history
}

/// `artifact_provenance(directory, path).to_dict()` — the git-only fields
/// (the caller prepends the parsed `status`).
pub fn artifact_provenance(directory: &str, path: &str) -> Map<String, Value> {
    let mut m = Map::new();
    let opt = |v: Option<String>| v.map(|s| json!(s)).unwrap_or(Value::Null);
    match repository_root(directory) {
        None => {
            m.insert("last_committed".to_string(), Value::Null);
            m.insert("last_author".to_string(), Value::Null);
            m.insert("first_committed".to_string(), Value::Null);
            m.insert("first_author".to_string(), Value::Null);
            m.insert("status_history".to_string(), json!([]));
        }
        Some(root) => {
            let spec = pathspec(&root, Path::new(path));
            let (last_committed, last_author) = commit_record(&root, &spec, false);
            let (first_committed, first_author) = commit_record(&root, &spec, true);
            m.insert("last_committed".to_string(), opt(last_committed));
            m.insert("last_author".to_string(), opt(last_author));
            m.insert("first_committed".to_string(), opt(first_committed));
            m.insert("first_author".to_string(), opt(first_author));
            m.insert(
                "status_history".to_string(),
                Value::Array(status_history(&root, &spec)),
            );
        }
    }
    m
}
