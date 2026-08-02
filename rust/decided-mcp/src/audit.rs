//! Read-access audit recorder (ADR-084), ported for the HTTP transport
//! (ADR-098's mandatory-audit-on). Mirrors `src/asdecided/mcp/audit.py`: one JSON
//! line per read-tool call — who claimed to query, which tool, the query args
//! (never content), the body-free artifact references returned, the outcome —
//! appended to the configured `.decided/config.yaml` audit sink.
//!
//! Default-absent for stdio (no `audit:` stanza ⇒ no recorder ⇒ byte-unchanged,
//! ADR-084's strict superset). For HTTP the sink is mandatory and the recorder
//! is *shared*: its construction-time principal skips the host git identity
//! (per-call attribution rides `X-AsDecided-Principal`), and a write failure blocks
//! the call rather than serving un-recordable reads.
//!
//! Not a wire-parity surface (the log is a side file with non-deterministic
//! `ts`/`session`/`duration_ms`); the referee compares records field-by-field
//! excluding those three. The line format itself is byte-faithful:
//! `pyjson::dumps_compact` == `json.dumps(event, ensure_ascii=False)`.

use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rac_engine::pyjson::dumps_compact;
use serde_json::{json, Map, Value};

const SCHEMA_VERSION: &str = "1";
const CONFIG_DIR: &str = ".decided";
const CONFIG_FILE: &str = "config.yaml";
const AUDIT_FILENAME: &str = "audit.jsonl";
const PATH_ENV: &str = "DECIDED_AUDIT_PATH";
const PRINCIPAL_ENV: &str = "DECIDED_AUDIT_PRINCIPAL";
const UNATTRIBUTED: &str = "unattributed";
pub const CANONICAL_PRINCIPAL_HEADER: &str = "X-AsDecided-Principal";
const MAX_PRINCIPAL_BYTES: usize = 512;
const AUDIT_SCOPE: &str = "MCP read tools";

/// The `audit:` stanza (ADR-084): enabled, configured path, on-write-error mode.
pub struct AuditConfig {
    pub enabled: bool,
    pub path: String,
    pub on_write_error: String, // "warn" | "block"
}

/// Read the `audit` stanza from the nearest `.decided/config.yaml` at or above
/// `root`. No config or no `audit` section ⇒ disabled. Malformed ⇒ Err (the
/// audit posture is never silently misconfigured).
pub fn load_audit_config(root: &str) -> Result<AuditConfig, String> {
    use rac_engine::frontmatter::{yaml_load_config, Yaml};
    let disabled = AuditConfig { enabled: false, path: String::new(), on_write_error: "warn".into() };
    let Some(config_path) = find_config_file(root) else {
        return Ok(disabled);
    };
    let text = std::fs::read_to_string(&config_path).map_err(|e| format!("invalid YAML: {e}"))?;
    let Yaml::Map(pairs) = yaml_load_config(&text).map_err(|p| format!("invalid YAML: {p}"))? else {
        return Ok(disabled);
    };
    let get = |m: &[(Yaml, Yaml)], key: &str| -> Option<Yaml> {
        m.iter().find_map(|(k, v)| match k {
            Yaml::Str(s) if s == key => Some(v.clone()),
            _ => None,
        })
    };
    let Some(section) = get(&pairs, "audit") else {
        return Ok(disabled);
    };
    let Yaml::Map(audit) = section else {
        return Err("'audit' must be a mapping".to_string());
    };
    let enabled = match get(&audit, "enabled") {
        None => false,
        Some(Yaml::Bool(b)) => b,
        Some(_) => return Err("'audit.enabled' must be true or false".to_string()),
    };
    let path = match get(&audit, "path") {
        None => String::new(),
        Some(Yaml::Str(s)) => s,
        Some(_) => return Err("'audit.path' must be a string".to_string()),
    };
    let on_write_error = match get(&audit, "on_write_error") {
        None => "warn".to_string(),
        Some(Yaml::Str(s)) if s == "warn" || s == "block" => s,
        Some(_) => return Err("'audit.on_write_error' must be one of warn, block".to_string()),
    };
    Ok(AuditConfig { enabled, path, on_write_error })
}

/// `DECIDED_AUDIT_PATH` > config `path` > `$XDG_STATE_HOME/decided/audit.jsonl`.
pub fn resolve_audit_path(configured: &str) -> PathBuf {
    if let Some(env) = std::env::var_os(PATH_ENV) {
        if !env.is_empty() {
            return PathBuf::from(env);
        }
    }
    if !configured.is_empty() {
        return PathBuf::from(configured);
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join(".local").join("state")
        });
    base.join("decided").join(AUDIT_FILENAME)
}

pub fn find_config_file(root: &str) -> Option<PathBuf> {
    let start = std::fs::canonicalize(root).unwrap_or_else(|_| PathBuf::from(root));
    std::iter::successors(Some(start.as_path()), |p| p.parent())
        .map(|dir| dir.join(CONFIG_DIR).join(CONFIG_FILE))
        .find(|c| c.is_file())
}

/// The audit principal: `DECIDED_AUDIT_PRINCIPAL` > (git identity, if allowed) >
/// `unattributed`. `allow_git` is false on the shared HTTP server (ADR-098):
/// the checkout's git identity is the host's, never a caller's.
fn resolve_principal(root: &str, allow_git: bool) -> String {
    if let Ok(v) = std::env::var(PRINCIPAL_ENV) {
        if !v.trim().is_empty() {
            return v.trim().to_string();
        }
    }
    if allow_git {
        if let Some(id) = git_identity(root) {
            return id;
        }
    }
    UNATTRIBUTED.to_string()
}

fn git_identity(root: &str) -> Option<String> {
    let cfg = |key: &str| -> Option<String> {
        let out = std::process::Command::new("git")
            .args(["config", key])
            .current_dir(root)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!s.is_empty()).then_some(s)
    };
    let name = cfg("user.name");
    let email = cfg("user.email");
    match (name, email) {
        (Some(n), Some(e)) => Some(format!("{n} <{e}>")),
        (n, e) => e.or(n),
    }
}

/// An append-only, content-bearing audit writer (ADR-084).
pub struct Recorder {
    path: PathBuf,
    principal: String,
    on_write_error: String,
    transport: &'static str,
    session: String,
    warned: bool,
}

/// Resolve the shared HTTP attribution carrier.
///
/// `X-AsDecided-Principal` is the only accepted carrier. Duplicate
/// occurrences are rejected because accepting them would make intermediary
/// header coalescing ambiguous.
pub fn resolve_http_principal(headers: &[(String, String)]) -> Result<Option<String>, &'static str> {
    principal_values(headers, CANONICAL_PRINCIPAL_HEADER)
}

fn principal_values(
    headers: &[(String, String)],
    name: &str,
) -> Result<Option<String>, &'static str> {
    let values: Vec<&str> = headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
        .collect();
    if values.len() > 1 {
        return Err("duplicate principal header");
    }
    let Some(value) = values.first().copied() else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > MAX_PRINCIPAL_BYTES || value.chars().any(char::is_control) {
        return Err("malformed principal header");
    }
    Ok(Some(value.to_string()))
}

/// Build a recorder when audit is enabled, else None (default-absent path).
/// `transport` shapes the shared-server posture: on `"http"` the recorder skips
/// the host git identity and blocks on write failure (ADR-098).
pub fn build(root: &str, transport: &'static str, config: &AuditConfig) -> Option<Recorder> {
    if !config.enabled {
        return None;
    }
    let shared = transport == "http";
    let path = resolve_audit_path(&config.path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent); // first write fails loud if not
    }
    Some(Recorder {
        path,
        principal: resolve_principal(root, !shared),
        on_write_error: if shared { "block".to_string() } else { config.on_write_error.clone() },
        transport,
        session: token_hex(8),
        warned: false,
    })
}

impl Recorder {
    fn record(&mut self, event: &Value) -> bool {
        use std::io::Write;
        let line = dumps_compact(event) + "\n";
        match std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            Ok(mut f) => f.write_all(line.as_bytes()).is_ok(),
            Err(e) => {
                if !self.warned {
                    let action =
                        if self.on_write_error == "block" { "refusing tool calls" } else { "continuing" };
                    eprintln!(
                        "decided-mcp: audit write failed ({e}); {action} \
(audit.on_write_error={}, path={}).",
                        self.on_write_error,
                        self.path.display()
                    );
                    self.warned = true;
                }
                false
            }
        }
    }
}

/// Announce an enabled recorder on stderr. Audit is deliberately explicit at
/// startup: operators should see the sink, covered scope, and failure posture
/// without having to infer them from a later JSONL line (ADR-084).
pub fn announce(recorder: &Recorder) {
    eprintln!("{}", activation_message(recorder));
}

fn activation_message(recorder: &Recorder) -> String {
    format!(
        "decided-mcp: audit enabled (path={}, scope={}, on_write_error={}, transport={}).",
        recorder.path.display(),
        AUDIT_SCOPE,
        recorder.on_write_error,
        recorder.transport,
    )
}

/// Run `call`, record one audit event, and return the payload unchanged
/// (ADR-084: audit is observability outside the response contract). With no
/// recorder this is exactly `call()`. Under `on_write_error: block` a failed
/// write refuses the call with a structured `audit-unavailable` error.
pub fn observe(
    recorder: Option<&mut Recorder>,
    request_principal: Option<&str>,
    tool: &str,
    args: Value,
    call: impl FnOnce() -> String,
) -> String {
    let Some(recorder) = recorder else {
        return call();
    };
    let stripped = request_principal.map(str::trim).filter(|s| !s.is_empty());
    let asserted = stripped.is_some();
    let principal = stripped.map(str::to_string).unwrap_or_else(|| recorder.principal.clone());
    let started = Instant::now();
    let payload = call();
    let event = build_event(
        recorder,
        tool,
        args,
        returned_records(&payload),
        outcome(&payload),
        started,
        &principal,
        asserted,
    );
    if !recorder.record(&event) && recorder.on_write_error == "block" {
        let mut err = Map::new();
        err.insert("schema_version".into(), Value::String(SCHEMA_VERSION.into()));
        err.insert("error".into(), Value::String("audit-unavailable".into()));
        err.insert("tool".into(), Value::String(tool.into()));
        return dumps_compact(&Value::Object(err));
    }
    payload
}

#[allow(clippy::too_many_arguments)]
fn build_event(
    recorder: &Recorder,
    tool: &str,
    args: Value,
    returned: Vec<Value>,
    outcome: &str,
    started: Instant,
    principal: &str,
    asserted: bool,
) -> Value {
    let mut e = Map::new();
    e.insert("schema_version".into(), Value::String(SCHEMA_VERSION.into()));
    e.insert("ts".into(), Value::String(iso_now()));
    e.insert("session".into(), Value::String(recorder.session.clone()));
    e.insert("principal".into(), Value::String(principal.into()));
    e.insert("transport".into(), Value::String(recorder.transport.into()));
    e.insert(
        "attribution".into(),
        Value::String(if asserted { "asserted" } else { "local" }.into()),
    );
    e.insert("tool".into(), Value::String(tool.into()));
    e.insert("query".into(), args);
    e.insert("returned".into(), Value::Array(returned));
    e.insert("outcome".into(), Value::String(outcome.into()));
    e.insert(
        "duration_ms".into(),
        Value::Number((started.elapsed().as_millis() as u64).into()),
    );
    Value::Object(e)
}

/// `"error"` when the payload is a structured error, else `"ok"` (ADR-034).
fn outcome(payload: &str) -> &'static str {
    match serde_json::from_str::<Value>(payload) {
        Ok(Value::Object(m)) if m.contains_key("error") => "error",
        _ => "ok",
    }
}

/// The artifact identities a call surfaced, without artifact bodies (ADR-084).
///
/// The audit schema deliberately keeps a small reference rather than copying
/// result records: `id` is the stable identity, `resolved` records the
/// resolution state, and `provenance.path` points back into the served corpus.
/// The extractor covers every result collection emitted by the MCP tools. Raw
/// outgoing relationship text is intentionally excluded because it can be an
/// unresolved declaration rather than a returned artifact.
fn returned_records(payload: &str) -> Vec<Value> {
    let Ok(Value::Object(data)) = serde_json::from_str::<Value>(payload) else {
        return Vec::new();
    };
    if data.contains_key("error") {
        return Vec::new();
    }
    let mut records = Vec::new();
    if let Some(record) = returned_record(&data) {
        records.push(record);
    }
    for key in ["matches", "items", "decisions", "incoming", "neighborhood"] {
        if let Some(Value::Array(items)) = data.get(key) {
            for item in items {
                if let Some(object) = item.as_object() {
                    if let Some(record) = returned_record(object) {
                        records.push(record);
                    }
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    records.retain(|record| {
        record
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| seen.insert(id.to_string()))
    });
    records
}

fn returned_record(object: &Map<String, Value>) -> Option<Value> {
    let id = object.get("id").and_then(Value::as_str)?;
    let resolved = object
        .get("resolved")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let provenance = object
        .get("path")
        .and_then(Value::as_str)
        .map(|path| json!({ "path": path }))
        .unwrap_or(Value::Null);
    Some(json!({
        "id": id,
        "resolved": resolved,
        "provenance": provenance,
    }))
}

fn token_hex(nbytes: usize) -> String {
    use std::io::Read;
    let mut buf = vec![0u8; nbytes];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_err()
    {
        // Fallback mix (never byte-refereed); good enough for a session tag.
        let mut state = std::process::id() as u64 ^ 0x9E37_79B9_7F4A_7C15;
        if let Ok(d) = SystemTime::now().duration_since(UNIX_EPOCH) {
            state ^= d.as_nanos() as u64;
        }
        for b in &mut buf {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *b = (state >> 33) as u8;
        }
    }
    let mut out = String::with_capacity(nbytes * 2);
    for b in buf {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// `datetime.now(UTC).isoformat(timespec="milliseconds")` with `+00:00`→`Z`.
fn iso_now() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (h, mi, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{millis:03}Z")
}

/// Proleptic-Gregorian civil date from days since 1970-01-01 (Hinnant).
fn civil_from_days(z0: i64) -> (i64, u32, u32) {
    let z = z0 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_principal_is_resolved_and_trimmed() {
        let headers = vec![
            (CANONICAL_PRINCIPAL_HEADER.to_string(), "alice".to_string()),
        ];
        assert_eq!(resolve_http_principal(&headers).unwrap().as_deref(), Some("alice"));
    }

    #[test]
    fn duplicate_principals_are_rejected() {
        let duplicate = vec![
            (CANONICAL_PRINCIPAL_HEADER.to_string(), "alice".to_string()),
            (CANONICAL_PRINCIPAL_HEADER.to_string(), "alice".to_string()),
        ];
        assert_eq!(resolve_http_principal(&duplicate), Err("duplicate principal header"));
    }

    #[test]
    fn empty_and_malformed_principals_have_explicit_behaviour() {
        let empty = vec![(CANONICAL_PRINCIPAL_HEADER.to_string(), "  ".to_string())];
        assert_eq!(resolve_http_principal(&empty).unwrap(), None);

        let malformed = vec![(CANONICAL_PRINCIPAL_HEADER.to_string(), "alice\nproxy".to_string())];
        assert_eq!(resolve_http_principal(&malformed), Err("malformed principal header"));

        let oversized = vec![(CANONICAL_PRINCIPAL_HEADER.to_string(), "a".repeat(513))];
        assert_eq!(resolve_http_principal(&oversized), Err("malformed principal header"));
    }

    #[test]
    fn returned_records_cover_every_result_collection_without_bodies() {
        let payload = json!({
            "id": "A",
            "path": "a.md",
            "content": "secret body",
            "matches": [{"id": "B", "path": "b.md"}, {"id": "A", "path": "a.md"}],
            "items": [{"id": "C", "path": "c.md", "resolved": false}],
            "decisions": [{"id": "D", "path": "d.md"}],
            "incoming": [{"id": "E", "path": "e.md"}],
            "neighborhood": [{"id": "F", "path": "f.md"}],
        })
        .to_string();
        assert_eq!(
            returned_records(&payload),
            vec![
                json!({"id": "A", "resolved": true, "provenance": {"path": "a.md"}}),
                json!({"id": "B", "resolved": true, "provenance": {"path": "b.md"}}),
                json!({"id": "C", "resolved": false, "provenance": {"path": "c.md"}}),
                json!({"id": "D", "resolved": true, "provenance": {"path": "d.md"}}),
                json!({"id": "E", "resolved": true, "provenance": {"path": "e.md"}}),
                json!({"id": "F", "resolved": true, "provenance": {"path": "f.md"}}),
            ]
        );
        assert!(!returned_records(&payload).iter().any(|item| item.to_string().contains("secret")));
        assert!(returned_records(r#"{"error":{"code":-1},"id":"A"}"#).is_empty());
    }

    #[test]
    fn activation_message_declares_path_scope_and_failure_mode() {
        let recorder = Recorder {
            path: PathBuf::from("/tmp/audit.jsonl"),
            principal: UNATTRIBUTED.to_string(),
            on_write_error: "block".to_string(),
            transport: "http",
            session: "session".to_string(),
            warned: false,
        };
        let message = activation_message(&recorder);
        assert!(message.contains("path=/tmp/audit.jsonl"));
        assert!(message.contains("scope=MCP read tools"));
        assert!(message.contains("on_write_error=block"));
    }
}
