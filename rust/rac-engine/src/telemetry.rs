//! Guide telemetry read-back (`src/asdecided/mcp/telemetry.py`) — ADR-040.
//!
//! Read side only: `decided mcp-stats` (and the guide half of `decided usage`)
//! summarizes the append-only JSONL log under
//! `$XDG_STATE_HOME/decisions/guide-telemetry.jsonl`. The recorder itself lives
//! in the MCP serving path and stays a documented no-op seam in the Rust
//! sidecar (`rust/decided-mcp/src/sidecar.rs`).
//!
//! Corruption posture is pinned: a missing log is an empty log; a garbled
//! line (non-JSON, or JSON that is not an object) is skipped and COUNTED;
//! a blank line is skipped silently. A log that is not valid UTF-8 makes
//! the oracle crash (`read_text` raises `UnicodeDecodeError`, which its
//! `except OSError` does not catch): traceback to stderr, empty stdout,
//! exit 1 — mirrored here as [`LogNotUtf8`].

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::consent::xdg_rac_file;
use crate::pycompat::{py_round, py_splitlines, py_strip, quote_plus_urlencode};
use crate::pyjson;

pub const SCHEMA_VERSION: &str = "1";
const TELEMETRY_FILENAME: &str = "guide-telemetry.jsonl";

pub const SHARE_ISSUE_URL: &str = "https://github.com/asdecided/core/issues/new";
pub const SHARE_TEMPLATE: &str = "guide-usage-report.yml";
pub const SHARE_FIELD: &str = "report";

/// The oracle's `UnicodeDecodeError` crash on a non-UTF-8 log: empty
/// stdout, exit 1 (the reader catches only `OSError`).
pub struct LogNotUtf8;

/// Aggregated usage for one tool, ordered by tool name in the summary.
pub struct ToolUsage {
    pub tool: String,
    pub calls: i64,
    pub errors: i64,
    pub truncated: i64,
    pub avg_duration_ms: i64,
}

/// What the local log says about Guide usage (the `mcp-stats` payload).
pub struct TelemetrySummary {
    pub path: String,
    pub event_count: i64,
    pub session_count: i64,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
    pub skipped_lines: i64,
    pub tools: Vec<ToolUsage>,
}

/// The local telemetry log path under the XDG state directory.
pub fn telemetry_path() -> String {
    xdg_rac_file("XDG_STATE_HOME", &[".local", "state"], TELEMETRY_FILENAME)
}

/// Events plus the count of skipped unreadable lines. Missing file ->
/// `([], 0)`; non-UTF-8 -> the oracle-crash mirror.
pub(crate) fn read_events(path: &str) -> Result<(Vec<Map<String, Value>>, i64), LogNotUtf8> {
    let Ok(bytes) = std::fs::read(path) else {
        return Ok((Vec::new(), 0));
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return Err(LogNotUtf8);
    };
    let mut events = Vec::new();
    let mut skipped = 0i64;
    for line in py_splitlines(&text) {
        if py_strip(line).is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(map)) => events.push(map),
            Ok(_) => skipped += 1,
            Err(_) => skipped += 1,
        }
    }
    Ok((events, skipped))
}

/// `isinstance(value, int)` for the duration average — CPython counts
/// bools as ints (`True` averages as 1), floats never.
fn py_int_like(v: &Value) -> Option<i128> {
    match v {
        Value::Bool(b) => Some(i128::from(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i128::from(i))
            } else {
                n.as_u64().map(i128::from)
            }
        }
        _ => None,
    }
}

/// `round(mean)` over the int durations; empty -> 0. `round()` is CPython
/// half-to-even over the exact double, via `pycompat::py_round`.
fn average_duration(rows: &[&Map<String, Value>]) -> i64 {
    let durations: Vec<i128> = rows
        .iter()
        .filter_map(|ev| ev.get("duration_ms").and_then(py_int_like))
        .collect();
    if durations.is_empty() {
        return 0;
    }
    let sum: i128 = durations.iter().sum();
    py_round(sum as f64 / durations.len() as f64, 0) as i64
}

/// Summarize the telemetry log; an empty or missing log is a valid answer.
pub fn summarize() -> Result<TelemetrySummary, LogNotUtf8> {
    let log = telemetry_path();
    let (events, skipped) = read_events(&log)?;
    let sessions: HashSet<&str> = events
        .iter()
        .filter_map(|ev| ev.get("session").and_then(Value::as_str))
        .collect();
    let mut stamps: Vec<&str> = events
        .iter()
        .filter_map(|ev| ev.get("ts").and_then(Value::as_str))
        .collect();
    stamps.sort_unstable();
    let mut by_tool: std::collections::BTreeMap<&str, Vec<&Map<String, Value>>> =
        std::collections::BTreeMap::new();
    for ev in &events {
        if let Some(tool) = ev.get("tool").and_then(Value::as_str) {
            by_tool.entry(tool).or_default().push(ev);
        }
    }
    let tools = by_tool
        .iter()
        .map(|(tool, rows)| ToolUsage {
            tool: (*tool).to_string(),
            calls: rows.len() as i64,
            errors: rows
                .iter()
                .filter(|ev| {
                    matches!(
                        ev.get("outcome").and_then(Value::as_str),
                        Some("error") | Some("exception")
                    )
                })
                .count() as i64,
            truncated: rows
                .iter()
                .filter(|ev| ev.get("truncated") == Some(&Value::Bool(true)))
                .count() as i64,
            avg_duration_ms: average_duration(rows),
        })
        .collect();
    Ok(TelemetrySummary {
        path: log,
        event_count: events.len() as i64,
        session_count: sessions.len() as i64,
        first_ts: stamps.first().map(|s| s.to_string()),
        last_ts: stamps.last().map(|s| s.to_string()),
        skipped_lines: skipped,
        tools,
    })
}

/// `TelemetrySummary.to_dict()` — pinned key order.
pub fn summary_value(summary: &TelemetrySummary) -> Value {
    let mut m = Map::new();
    m.insert("schema_version".into(), Value::String(SCHEMA_VERSION.into()));
    m.insert("path".into(), Value::String(summary.path.clone()));
    m.insert("event_count".into(), Value::from(summary.event_count));
    m.insert("session_count".into(), Value::from(summary.session_count));
    m.insert(
        "first_ts".into(),
        summary
            .first_ts
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    m.insert(
        "last_ts".into(),
        summary
            .last_ts
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    m.insert("skipped_lines".into(), Value::from(summary.skipped_lines));
    m.insert(
        "tools".into(),
        Value::Array(summary.tools.iter().map(tool_value).collect()),
    );
    Value::Object(m)
}

fn tool_value(tool: &ToolUsage) -> Value {
    let mut m = Map::new();
    m.insert("tool".into(), Value::String(tool.tool.clone()));
    m.insert("calls".into(), Value::from(tool.calls));
    m.insert("errors".into(), Value::from(tool.errors));
    m.insert("truncated".into(), Value::from(tool.truncated));
    m.insert("avg_duration_ms".into(), Value::from(tool.avg_duration_ms));
    Value::Object(m)
}

/// The prefilled usage-report issue URL. The local log path is DELETED
/// from the shared report (counts and timestamps only); the JSON is
/// `json.dumps(..., ensure_ascii=False, indent=2)` and the query is
/// `urllib.parse.urlencode` (quote_plus per value).
pub fn share_url(summary: &TelemetrySummary) -> String {
    let mut report_data = summary_value(summary);
    if let Value::Object(map) = &mut report_data {
        map.shift_remove("path");
    }
    let report = pyjson::dumps_indent2_no_ascii(&report_data);
    let query = quote_plus_urlencode(&[("template", SHARE_TEMPLATE), (SHARE_FIELD, &report)]);
    format!("{SHARE_ISSUE_URL}?{query}")
}
