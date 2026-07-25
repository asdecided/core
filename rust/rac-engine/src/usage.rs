//! CLI usage telemetry (`src/asdecided/usage.py`) — ADR-046, content-free,
//! consent-gated, local-only.
//!
//! Two halves, both consent-shaped:
//! - the READ-BACK (`decided usage`): a unified summary over the CLI-usage log
//!   (`$XDG_STATE_HOME/decisions/decided-usage.jsonl`) and the Guide log (via
//!   `telemetry::summarize`), with no consent gate on reads;
//! - the RECORDER: one content-free event appended after every dispatched
//!   command, if and only if consent is recorded (`decided telemetry on`).
//!   Write-only observability: silent on every failure path, never alters
//!   output or exit codes, and skipped entirely for parse-level exits
//!   (argparse errors, `--version`/`-h`) exactly like the oracle's
//!   `cli.main`, which computes the command name only after `parse_args`
//!   returns.
//!
//! The recorder's bytes (wall-clock ts, per-process random session id,
//! measured duration) are nondeterministic by design and never
//! byte-refereed; the read-back over SEEDED logs is what parity pins.

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::consent::{load_consent, now_epoch, token_hex, utc_isoformat_micros, xdg_rac_file};
use crate::pycompat::{py_splitlines, py_strip, quote_plus_urlencode};
use crate::pyjson;
use crate::telemetry::{summary_value, LogNotUtf8, TelemetrySummary};

pub const SCHEMA_VERSION: &str = "1";
const USAGE_FILENAME: &str = "decided-usage.jsonl";
pub const OUTCOME_OK: &str = "ok";
pub const OUTCOME_ERROR: &str = "error";

/// `recent` keeps the last N distinct UTC dates; the oracle's default.
const RECENT_DAYS: usize = 7;

pub const SHARE_ISSUE_URL: &str = "https://github.com/asdecided/core/issues/new";
pub const SHARE_TEMPLATE: &str = "guide-usage-report.yml";
pub const SHARE_FIELD: &str = "report";

pub struct CommandUsage {
    pub command: String,
    pub calls: i64,
    pub errors: i64,
}

pub struct UsageSummary {
    pub total: i64,
    pub sessions: i64,
    pub commands: Vec<CommandUsage>,
    /// date (YYYY-MM-DD, UTC) -> event count, ascending, last N days.
    pub recent: Vec<(String, i64)>,
}

/// Location of the CLI-usage log (separate from the Guide log, ADR-046).
pub fn usage_path() -> String {
    xdg_rac_file("XDG_STATE_HOME", &[".local", "state"], USAGE_FILENAME)
}

/// Read usage events; a missing or malformed log yields what is parseable
/// (malformed lines are skipped WITHOUT counting, unlike the Guide log).
/// Non-UTF-8 mirrors the oracle's `UnicodeDecodeError` crash.
fn read_usage(path: &str) -> Result<Vec<Map<String, Value>>, LogNotUtf8> {
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(Vec::new());
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return Err(LogNotUtf8);
    };
    let mut events = Vec::new();
    for line in py_splitlines(&text) {
        let line = py_strip(line);
        if line.is_empty() {
            continue;
        }
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(line) {
            events.push(map);
        }
    }
    Ok(events)
}

/// Per-command counts, session count, and a recent-activity trend.
pub fn summarize_usage() -> Result<UsageSummary, LogNotUtf8> {
    let events = read_usage(&usage_path())?;
    let sessions: HashSet<&str> = events
        .iter()
        .filter_map(|ev| ev.get("session").and_then(Value::as_str))
        .collect();
    let mut by_command: std::collections::BTreeMap<&str, Vec<&Map<String, Value>>> =
        std::collections::BTreeMap::new();
    for ev in &events {
        if let Some(command) = ev.get("command").and_then(Value::as_str) {
            by_command.entry(command).or_default().push(ev);
        }
    }
    let commands = by_command
        .iter()
        .map(|(command, rows)| CommandUsage {
            command: (*command).to_string(),
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
        })
        .collect();
    // `ts[:10]` buckets by CODE POINTS with a `len(ts) >= 10` guard; the
    // Counter is then sorted by date and truncated to the trailing window.
    let mut day_counts: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    for ev in &events {
        if let Some(ts) = ev.get("ts").and_then(Value::as_str) {
            if ts.chars().count() >= 10 {
                let day: String = ts.chars().take(10).collect();
                *day_counts.entry(day).or_insert(0) += 1;
            }
        }
    }
    let skip = day_counts.len().saturating_sub(RECENT_DAYS);
    let recent = day_counts.into_iter().skip(skip).collect();
    Ok(UsageSummary {
        total: events.len() as i64,
        sessions: sessions.len() as i64,
        commands,
        recent,
    })
}

/// `UsageSummary.to_dict()` — pinned key order.
pub fn cli_value(summary: &UsageSummary) -> Value {
    let mut m = Map::new();
    m.insert("schema_version".into(), Value::String(SCHEMA_VERSION.into()));
    m.insert("total".into(), Value::from(summary.total));
    m.insert("sessions".into(), Value::from(summary.sessions));
    m.insert(
        "commands".into(),
        Value::Array(
            summary
                .commands
                .iter()
                .map(|c| {
                    let mut cm = Map::new();
                    cm.insert("command".into(), Value::String(c.command.clone()));
                    cm.insert("calls".into(), Value::from(c.calls));
                    cm.insert("errors".into(), Value::from(c.errors));
                    Value::Object(cm)
                })
                .collect(),
        ),
    );
    let mut recent = Map::new();
    for (day, count) in &summary.recent {
        recent.insert(day.clone(), Value::from(*count));
    }
    m.insert("recent".into(), Value::Object(recent));
    Value::Object(m)
}

/// `_combined(summary, guide)` — the `usage --json`/`--share` payload.
/// Unlike mcp-stats' share report, the guide dict keeps its `path`.
pub fn combined_value(cli: &UsageSummary, guide: &TelemetrySummary) -> Value {
    let mut m = Map::new();
    m.insert("schema_version".into(), Value::String(SCHEMA_VERSION.into()));
    m.insert("cli".into(), cli_value(cli));
    m.insert("guide".into(), summary_value(guide));
    Value::Object(m)
}

/// The prefilled GitHub issue URL — the FULL combined report, including
/// `guide.path` (usage does not strip the path; mcp-stats does).
pub fn share_url(cli: &UsageSummary, guide: &TelemetrySummary) -> String {
    let report = pyjson::dumps_indent2_no_ascii(&combined_value(cli, guide));
    let query = quote_plus_urlencode(&[("template", SHARE_TEMPLATE), (SHARE_FIELD, &report)]);
    format!("{SHARE_ISSUE_URL}?{query}")
}

// ---------------------------------------------------------------------------
// Recorder (write side)
// ---------------------------------------------------------------------------

/// One random session id per process (`secrets.token_hex(8)`), never
/// persisted to config.
fn session_id() -> &'static str {
    static SESSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SESSION.get_or_init(|| token_hex(8))
}

/// Append one content-free usage event, if consent is recorded (ADR-046).
/// Silent on every failure path — no consent, no command name, or an
/// unwritable log all mean "record nothing".
pub fn record_command(command: &str, outcome: &str, duration_ms: i64) {
    if command.is_empty() {
        return;
    }
    if !load_consent().share_usage {
        return;
    }
    let path = usage_path();
    if let Some(parent) = std::path::Path::new(&path).parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let (secs, micros) = now_epoch();
    let mut event = Map::new();
    event.insert("schema_version".into(), Value::String(SCHEMA_VERSION.into()));
    event.insert(
        "ts".into(),
        Value::String(utc_isoformat_micros(secs, micros)),
    );
    event.insert("session".into(), Value::String(session_id().to_string()));
    event.insert("command".into(), Value::String(command.to_string()));
    event.insert("outcome".into(), Value::String(outcome.to_string()));
    event.insert("duration_ms".into(), Value::from(duration_ms));
    let line = pyjson::dumps_compact(&Value::Object(event)) + "\n";
    use std::io::Write as _;
    if let Ok(mut handle) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
    {
        let _ = handle.write_all(line.as_bytes());
    }
}
