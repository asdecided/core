//! Usage-sharing consent (`src/asdecided/consent.py`) — ADR-041, ADR-086.
//!
//! The record is JSON under `$XDG_CONFIG_HOME/decisions/telemetry.json` with the
//! Explorer-preferences posture: a missing/corrupt/non-dict file means no
//! consent, loading never raises, and saving tolerates filesystem trouble
//! silently. The install id is random (`secrets.token_hex(16)`), minted at
//! opt-in and preserved across off-and-on toggles; the enterprise hard-lock
//! (ADR-086) records a forced-off choice and refuses opt-in until unlocked.
//! The native engine has no sender (ADR-131), so this state is local
//! compatibility data only.
//!
//! Loading mirrors CPython's coercions field-by-field: `bool(value)` truth
//! semantics for the flags and `str(value)` (including `str(None) == "None"`
//! and container repr) for the id fields — a present-but-null install_id
//! really does read back as the string `None`, exactly like the oracle.
//!
//! This module also carries the shared XDG path builder, the UTC timestamp
//! formatters, and the `/dev/urandom` token minting that `usage.rs` reuses
//! for the recorder — the Rust analogue of consent.py sitting outside
//! `decided.mcp` so everything here stays SDK-free.

use serde_json::{Map, Value};

use crate::pycompat::py_float_repr;
use crate::pyjson;
use crate::walk::normalize_root;

/// ADR-131 retires the native PostHog sender. Keep the symbol as an explicit
/// empty kill switch for compatibility with the consent/status contract; no
/// network client or sender exists in this build.
pub const POSTHOG_API_KEY: &str = "";

const CONSENT_FILENAME: &str = "telemetry.json";

/// The recorded sharing choice; the default is no consent.
#[derive(Debug, Clone, Default)]
pub struct Consent {
    pub share_usage: bool,
    pub install_id: String,
    pub salt: String,
    pub consented_at: String,
    pub enterprise_locked: bool,
}

/// What `decided telemetry status` reports.
pub struct ConsentStatus {
    pub sharing: bool,
    pub install_id: String,
    pub consented_at: String,
    pub path: String,
    pub endpoint_configured: bool,
    pub enterprise_locked: bool,
}

// ---------------------------------------------------------------------------
// XDG paths — `os.environ.get(VAR) or str(Path.home() / …)`, then
// `str(Path(base) / "decided" / name)`.
// ---------------------------------------------------------------------------

/// `str(Path(base) / "decided" / name)` where `base` is `$VAR` when set and
/// NON-EMPTY (Python's `or` treats `""` as unset), else `$HOME/<fallback…>`.
/// The base goes through PurePosixPath normalization, and a base that
/// normalizes to `.` joins as a bare relative path (`Path(".") / "decided"` is
/// `decided`, not `./decided`) — relative XDG values resolve against the process
/// cwd on both engines, byte-for-byte.
pub(crate) fn xdg_rac_file(var: &str, home_fallback: &[&str], name: &str) -> String {
    let base = match std::env::var(var) {
        Ok(v) if !v.is_empty() => v,
        _ => {
            let mut p = std::env::var("HOME").unwrap_or_default();
            for seg in home_fallback {
                p.push('/');
                p.push_str(seg);
            }
            p
        }
    };
    let norm = normalize_root(&base);
    match norm.as_str() {
        "." => format!("decisions/{name}"),
        "/" => format!("/decisions/{name}"),
        "//" => format!("//decisions/{name}"),
        _ => format!("{norm}/decisions/{name}"),
    }
}

pub fn consent_path() -> String {
    xdg_rac_file("XDG_CONFIG_HOME", &[".config"], CONSENT_FILENAME)
}

/// `consent_recorded()` — true once ANY answer (including a decline) has
/// been persisted; the ask-at-most-once gate of the init/quickstart prompt.
pub fn consent_recorded() -> bool {
    std::path::Path::new(&consent_path()).is_file()
}

// ---------------------------------------------------------------------------
// CPython value coercions (load_consent applies bool()/str() field-wise)
// ---------------------------------------------------------------------------

/// Python `bool(value)` over a JSON value.
fn py_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python `repr(value)` over a JSON value (containers in `str()` position).
fn py_repr_json(v: &Value) -> String {
    match v {
        Value::String(s) => crate::pycompat::py_repr_str(s),
        other => py_str_json(other),
    }
}

/// Python `str(value)` over a JSON value: `None`/`True`/`False`, int
/// digits, float repr, the string itself, and container repr.
fn py_str_json(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else {
                py_float_repr(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr_json).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    format!("{}: {}", crate::pycompat::py_repr_str(k), py_repr_json(v))
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

// ---------------------------------------------------------------------------
// Load / save
// ---------------------------------------------------------------------------

/// Read the consent record; any problem means no consent (never raises).
/// A non-UTF-8 file is a `UnicodeDecodeError` in the oracle — a `ValueError`
/// subclass, so it lands in the same tolerant default (unlike the state
/// LOGS, whose readers catch only `OSError` and crash).
pub fn load_consent() -> Consent {
    let Ok(bytes) = std::fs::read(consent_path()) else {
        return Consent::default();
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return Consent::default();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Consent::default();
    };
    let Value::Object(map) = value else {
        return Consent::default();
    };
    Consent {
        share_usage: map.get("share_usage").map(py_truthy).unwrap_or(false),
        install_id: map
            .get("install_id")
            .map(py_str_json)
            .unwrap_or_default(),
        salt: map.get("salt").map(py_str_json).unwrap_or_default(),
        consented_at: map
            .get("consented_at")
            .map(py_str_json)
            .unwrap_or_default(),
        enterprise_locked: map
            .get("enterprise_locked")
            .map(py_truthy)
            .unwrap_or(false),
    }
}

/// Persist the record: `json.dumps(asdict(consent), indent=2) + "\n"` in
/// dataclass field order; tolerates filesystem trouble silently.
pub fn save_consent(consent: &Consent) {
    let mut m = Map::new();
    m.insert("share_usage".into(), Value::Bool(consent.share_usage));
    m.insert("install_id".into(), Value::String(consent.install_id.clone()));
    m.insert("salt".into(), Value::String(consent.salt.clone()));
    m.insert(
        "consented_at".into(),
        Value::String(consent.consented_at.clone()),
    );
    m.insert(
        "enterprise_locked".into(),
        Value::Bool(consent.enterprise_locked),
    );
    let text = pyjson::dumps_indent2(&Value::Object(m)) + "\n";
    let path = consent_path();
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, text);
}

/// Record consent, minting ids only where none exist yet; the enterprise
/// lock is preserved, never cleared here (ADR-086).
pub fn opt_in() -> Consent {
    let existing = load_consent();
    let consent = Consent {
        share_usage: true,
        install_id: if existing.install_id.is_empty() {
            token_hex(16)
        } else {
            existing.install_id
        },
        salt: if existing.salt.is_empty() {
            token_hex(16)
        } else {
            existing.salt
        },
        consented_at: utc_now_seconds_z(),
        enterprise_locked: existing.enterprise_locked,
    };
    save_consent(&consent);
    consent
}

/// `decline()` — persist the default no-consent record, making ask-once
/// true (unlike `opt_out`, nothing from an existing record is kept).
pub fn decline() -> Consent {
    let consent = Consent::default();
    save_consent(&consent);
    consent
}

/// Withdraw consent; the ids are kept so a later opt-in stays continuous.
pub fn opt_out() -> Consent {
    let existing = load_consent();
    let consent = Consent {
        share_usage: false,
        ..existing
    };
    save_consent(&consent);
    consent
}

/// Force the ping off and hard-lock it (ADR-086); ids kept.
pub fn enterprise_lock() -> Consent {
    let existing = load_consent();
    let consent = Consent {
        share_usage: false,
        enterprise_locked: true,
        ..existing
    };
    save_consent(&consent);
    consent
}

/// Remove the enterprise hard-lock (ADR-086); sharing stays as recorded.
pub fn enterprise_unlock() -> Consent {
    let existing = load_consent();
    let consent = Consent {
        enterprise_locked: false,
        ..existing
    };
    save_consent(&consent);
    consent
}

pub fn consent_status() -> ConsentStatus {
    let consent = load_consent();
    ConsentStatus {
        sharing: consent.share_usage,
        install_id: consent.install_id,
        consented_at: consent.consented_at,
        path: consent_path(),
        // Keep the status field deterministic for existing JSON consumers;
        // ADR-131 makes the native build explicitly unconfigured.
        #[allow(clippy::const_is_empty)]
        endpoint_configured: !POSTHOG_API_KEY.is_empty(),
        enterprise_locked: consent.enterprise_locked,
    }
}

// ---------------------------------------------------------------------------
// Shared seams: CSPRNG token minting and UTC timestamp formatting
// (`secrets.token_hex`, `datetime.now(UTC).isoformat(...)`)
// ---------------------------------------------------------------------------

/// `secrets.token_hex(nbytes)` — lowercase hex over CSPRNG bytes. Falls
/// back to a time/pid hash mix if `/dev/urandom` is unreadable (the oracle
/// would fail hard there; this channel is never byte-refereed).
pub(crate) fn token_hex(nbytes: usize) -> String {
    let mut buf = vec![0u8; nbytes];
    let read_ok = (|| -> std::io::Result<()> {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)
    })()
    .is_ok();
    if !read_ok {
        use std::hash::{Hash, Hasher};
        let mut seed = std::collections::hash_map::DefaultHasher::new();
        std::process::id().hash(&mut seed);
        if let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            d.subsec_nanos().hash(&mut seed);
            d.as_secs().hash(&mut seed);
        }
        let mut state = seed.finish();
        for chunk in buf.chunks_mut(8) {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            for (i, b) in chunk.iter_mut().enumerate() {
                *b = (state >> (8 * i)) as u8;
            }
        }
    }
    let mut out = String::with_capacity(nbytes * 2);
    for b in buf {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Proleptic-Gregorian civil date from days since 1970-01-01 (Howard
/// Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (y + i64::from(m <= 2), m, d)
}

fn utc_fields(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (
        y,
        m,
        d,
        (sod / 3600) as u32,
        ((sod % 3600) / 60) as u32,
        (sod % 60) as u32,
    )
}

pub(crate) fn now_epoch() -> (i64, u32) {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => (d.as_secs() as i64, d.subsec_micros()),
        Err(_) => (0, 0),
    }
}

/// `datetime.now(UTC).isoformat(timespec="seconds").replace("+00:00", "Z")`.
fn utc_now_seconds_z() -> String {
    let (secs, _) = now_epoch();
    let (y, mo, d, h, mi, s) = utc_fields(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// `datetime.now(UTC).isoformat()` — microseconds included, but omitted
/// entirely when the microsecond field is zero (CPython isoformat).
pub(crate) fn utc_isoformat_micros(secs: i64, micros: u32) -> String {
    let (y, mo, d, h, mi, s) = utc_fields(secs);
    if micros == 0 {
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}+00:00")
    } else {
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{micros:06}+00:00")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn truthy_matches_python_bool() {
        assert!(py_truthy(&json!("no"))); // bool("no") is True
        assert!(!py_truthy(&json!("")));
        assert!(!py_truthy(&json!(0)));
        assert!(!py_truthy(&json!(0.0)));
        assert!(py_truthy(&json!(2)));
        assert!(!py_truthy(&json!(null)));
        assert!(!py_truthy(&json!([])));
        assert!(py_truthy(&json!([0])));
    }

    #[test]
    fn str_matches_python_str() {
        assert_eq!(py_str_json(&json!(null)), "None");
        assert_eq!(py_str_json(&json!(42)), "42");
        assert_eq!(py_str_json(&json!(true)), "True");
        assert_eq!(py_str_json(&json!(3.5)), "3.5");
        assert_eq!(py_str_json(&json!([1, "a"])), "[1, 'a']");
        assert_eq!(py_str_json(&json!({"a": 1})), "{'a': 1}");
    }

    #[test]
    fn civil_dates_round_trip() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // leap year
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        // datetime(2026, 7, 12, 22, 6, 59, tzinfo=UTC).timestamp()
        assert_eq!(utc_fields(1_783_894_019), (2026, 7, 12, 22, 6, 59));
    }

    #[test]
    fn isoformat_micro_omission() {
        assert_eq!(
            utc_isoformat_micros(1_783_894_019, 0),
            "2026-07-12T22:06:59+00:00"
        );
        assert_eq!(
            utc_isoformat_micros(1_783_894_019, 547_399),
            "2026-07-12T22:06:59.547399+00:00"
        );
    }
}
