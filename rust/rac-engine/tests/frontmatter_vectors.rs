//! Frontmatter conformance: replay the oracle-generated `frontmatter.json`
//! vectors (PORT-CONTRACT.d/02). Regenerate with
//! `rust/spec/gen_vectors_frontmatter.py` (oracle venv).
//!
//! Oracle-crash cases (`"crash"` entries) assert the port's
//! `internal-oracle-divergence` issue instead — PORT-CONTRACT decision 3.

use std::fs;
use std::path::{Path, PathBuf};

use rac_engine::frontmatter::{
    exceeds_byte_cap, file_cap, file_cap_from, is_valid_id, load_frontmatter_mapping,
    non_utf8_issue, normalize_id, oversize_parse_issue, parse_frontmatter, read_artifact_text,
    split_frontmatter, unterminated_issue, ArtifactMetadata, FileCap, Issue, Yaml,
};
use serde_json::{json, Value};

fn vectors() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/frontmatter.json");
    let text = fs::read_to_string(&path).expect("read frontmatter.json");
    serde_json::from_str(&text).expect("parse frontmatter.json")
}

/// Sorted-keys compact ASCII JSON — must order identically to the
/// generator's `json.dumps(e, sort_keys=True, separators=(",",":"))`.
fn canon(v: &Value) -> String {
    fn esc(s: &str, out: &mut String) {
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c if c.is_ascii() => out.push(c),
                c => {
                    let cp = c as u32;
                    if cp < 0x10000 {
                        out.push_str(&format!("\\u{cp:04x}"));
                    } else {
                        let v = cp - 0x10000;
                        out.push_str(&format!(
                            "\\u{:04x}\\u{:04x}",
                            0xd800 + (v >> 10),
                            0xdc00 + (v & 0x3ff)
                        ));
                    }
                }
            }
        }
        out.push('"');
    }
    let mut out = String::new();
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => esc(s, &mut out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&canon(item));
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                esc(k, &mut out);
                out.push(':');
                out.push_str(&canon(&map[k]));
            }
            out.push('}');
        }
    }
    out
}

/// Mirror of the generator's tagged value encoding.
fn enc_yaml(v: &Yaml) -> Value {
    match v {
        Yaml::Null => json!({"t": "none"}),
        Yaml::Bool(b) => json!({"t": "bool", "v": b}),
        Yaml::Int(i) => json!({"t": "int", "v": i.to_string()}),
        Yaml::BigInt(b) => json!({"t": "int", "v": b.to_string()}),
        Yaml::Float(f) => json!({"t": "float", "v": rac_engine::pycompat::py_float_repr(*f)}),
        Yaml::Str(s) => json!({"t": "str", "v": s}),
        Yaml::Bytes(b) => json!({"t": "bytes", "v": b}),
        Yaml::Date { year, month, day } => json!({"t": "date", "v": [year, month, day]}),
        Yaml::DateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
            micro,
            tz,
        } => {
            json!({"t": "datetime", "v": [year, month, day, hour, minute, second, micro], "tz": tz})
        }
        Yaml::Tuple(items) => {
            json!({"t": "tuple", "v": items.iter().map(enc_yaml).collect::<Vec<_>>()})
        }
        Yaml::List(items) => {
            json!({"t": "list", "v": items.iter().map(enc_yaml).collect::<Vec<_>>()})
        }
        Yaml::Map(pairs) => json!({
            "t": "map",
            "v": pairs
                .iter()
                .map(|(k, val)| json!([enc_yaml(k), enc_yaml(val)]))
                .collect::<Vec<_>>()
        }),
        Yaml::Set(items) => {
            let mut encoded: Vec<Value> = items.iter().map(enc_yaml).collect();
            encoded.sort_by_key(canon);
            json!({"t": "set", "v": encoded})
        }
    }
}

fn enc_issue(i: &Issue) -> Value {
    json!([i.severity, i.code, i.message, i.line])
}

fn enc_issues(issues: &[Issue]) -> Value {
    Value::Array(issues.iter().map(enc_issue).collect())
}

fn enc_meta(m: &Option<ArtifactMetadata>) -> Value {
    match m {
        None => Value::Null,
        Some(m) => json!({
            "schema_version": m.schema_version.to_string(),
            "id": m.id,
            "type": m.artifact_type,
            "relationships": m
                .relationships
                .iter()
                .map(|(k, v)| json!([k, v]))
                .collect::<Vec<_>>(),
            "tags": m.tags,
            "provenance": m.provenance,
        }),
    }
}

fn case_raw(case: &Value) -> String {
    if let Some(raw) = case["raw"].as_str() {
        return raw.to_string();
    }
    let mut out = String::new();
    for pair in case["raw_rle"].as_array().expect("raw or raw_rle") {
        let chunk = pair[0].as_str().unwrap();
        let count = pair[1].as_u64().unwrap();
        for _ in 0..count {
            out.push_str(chunk);
        }
    }
    out
}

#[test]
fn split_matches_oracle() {
    let v = vectors();
    let cases = v["split"].as_array().unwrap();
    assert!(cases.len() >= 25);
    for case in cases {
        let text = case["text"].as_str().unwrap();
        let s = split_frontmatter(text);
        assert_eq!(s.raw.as_deref(), case["raw"].as_str(), "raw for {text:?}");
        assert_eq!(s.body, case["body"].as_str().unwrap(), "body for {text:?}");
        assert_eq!(
            s.line_offset as u64,
            case["line_offset"].as_u64().unwrap(),
            "offset for {text:?}"
        );
        assert_eq!(
            s.unterminated,
            case["unterminated"].as_bool().unwrap(),
            "unterminated for {text:?}"
        );
    }
}

#[test]
fn parse_matches_oracle() {
    let v = vectors();
    let cases = v["parse"].as_array().unwrap();
    assert!(cases.len() >= 300, "expected 300+ parse vectors");
    for case in cases {
        let raw = case_raw(case);
        let label: String = raw.chars().take(60).collect();
        if let Some(crash) = case["crash"].as_str() {
            // Oracle crashes; the port returns internal-oracle-divergence
            // with the mirrored exception string (PORT-CONTRACT decision 3).
            // crash_stage "validate" = the load succeeded and the crash came
            // from a field validator (int->str limit on a bignum message),
            // so only parse_frontmatter carries the marker.
            let validate_stage = case["crash_stage"].as_str() == Some("validate");
            let (data, issues) = load_frontmatter_mapping(&raw);
            if validate_stage {
                assert!(data.is_some(), "validate-crash case must load: {label:?}");
                assert!(issues.is_empty(), "validate-crash load issues: {label:?}");
            } else {
                assert!(data.is_none(), "crash case loaded data: {label:?}");
                assert_eq!(issues.len(), 1, "crash case issues: {label:?}");
                assert_eq!(
                    issues[0].code, "internal-oracle-divergence",
                    "crash case code: {label:?} -> {}",
                    issues[0].message
                );
                assert_eq!(issues[0].message, crash, "crash message for {label:?}");
            }
            let (meta, issues) = parse_frontmatter(&raw);
            assert!(meta.is_none(), "crash case metadata: {label:?}");
            assert_eq!(issues.len(), 1, "crash case parse issues: {label:?}");
            assert_eq!(
                issues[0].code, "internal-oracle-divergence",
                "crash case parse code: {label:?} -> {}",
                issues[0].message
            );
            assert_eq!(issues[0].message, crash, "crash message for {label:?}");
            continue;
        }
        let (data, load_issues) = load_frontmatter_mapping(&raw);
        let got_data = match &data {
            None => Value::Null,
            Some(pairs) => enc_yaml(&Yaml::Map(pairs.clone())),
        };
        if case["data"] != json!({"t": "omitted"}) {
            assert_eq!(got_data, case["data"], "data for {label:?}");
        } else {
            assert!(data.is_some(), "omitted-data case must load: {label:?}");
        }
        assert_eq!(
            enc_issues(&load_issues),
            case["load_issues"],
            "load_issues for {label:?}"
        );
        let (meta, issues) = parse_frontmatter(&raw);
        assert_eq!(enc_meta(&meta), case["metadata"], "metadata for {label:?}");
        assert_eq!(enc_issues(&issues), case["issues"], "issues for {label:?}");
    }
}

#[test]
fn ids_match_oracle() {
    let v = vectors();
    for case in v["ids"].as_array().unwrap() {
        let value = case["value"].as_str().unwrap();
        assert_eq!(
            is_valid_id(value),
            case["valid"].as_bool().unwrap(),
            "is_valid_id({value:?})"
        );
        assert_eq!(
            normalize_id(value),
            case["normalized"].as_str().unwrap(),
            "normalize_id({value:?})"
        );
    }
}

/// The read stage of `parse_file`, mirrored over the frontmatter helpers
/// exactly as `markdown.parse_file` sequences them.
fn run_file_pipeline(path: &str) -> (Vec<Issue>, Option<ArtifactMetadata>, Vec<Issue>) {
    let read = read_artifact_text(path);
    let mut read_issues: Vec<Issue> = Vec::new();
    let mut metadata = None;
    let mut metadata_issues: Vec<Issue> = Vec::new();
    if let Some(issue) = read.issue {
        read_issues.push(issue);
        return (read_issues, metadata, metadata_issues);
    }
    let text = read.text.unwrap();
    let cap = match file_cap() {
        FileCap::Cap(cap) => cap,
        FileCap::OracleCrash(_) => unreachable!("file cases never set a crash-zone cap"),
    };
    if exceeds_byte_cap(&text, cap as usize) {
        read_issues.push(oversize_parse_issue(cap));
    } else {
        let split = split_frontmatter(&text);
        match split.raw {
            Some(raw) => {
                let (m, iss) = parse_frontmatter(&raw);
                metadata = m;
                metadata_issues = iss;
            }
            None => {
                if split.unterminated {
                    metadata_issues.push(unterminated_issue());
                }
            }
        }
    }
    if read.lossy {
        read_issues.push(non_utf8_issue());
    }
    (read_issues, metadata, metadata_issues)
}

/// Files + env-cap cases share the DECIDED_MAX_FILE_BYTES env var, so they run
/// inside a single test (tests are threads of one process).
#[test]
fn files_and_env_cap_match_oracle() {
    let v = vectors();
    let base = std::env::var("CARGO_TARGET_TMPDIR").unwrap_or_else(|_| "/tmp".into());
    let root = PathBuf::from(base).join(format!("frontmatter_files_{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();

    for (n, case) in v["files"].as_array().unwrap().iter().enumerate() {
        let name = case["name"].as_str().unwrap();
        std::env::remove_var("DECIDED_MAX_FILE_BYTES");
        if let Some(env) = case["env"].as_str() {
            std::env::set_var("DECIDED_MAX_FILE_BYTES", env);
        }
        let dir = root.join(format!("case_{n}"));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("artifact.md");
        let path_str = path.to_str().unwrap().to_string();
        if case["dir"].as_bool() == Some(true) {
            fs::create_dir_all(&path).unwrap();
        } else if let Some(repeat) = case["repeat"].as_array() {
            let ch = repeat[0].as_str().unwrap();
            let count = repeat[1].as_u64().unwrap() as usize;
            fs::write(&path, ch.repeat(count)).unwrap();
        } else if let Some(b64) = case["bytes"].as_str() {
            fs::write(&path, b64_decode(b64)).unwrap();
        } // else: missing file

        let (read_issues, metadata, metadata_issues) = run_file_pipeline(&path_str);
        let subst = |issues: &[Issue]| {
            Value::Array(
                issues
                    .iter()
                    .map(|i| {
                        let mut i = i.clone();
                        i.message = i.message.replace(&path_str, "{PATH}");
                        enc_issue(&i)
                    })
                    .collect(),
            )
        };
        assert_eq!(
            subst(&read_issues),
            case["read_issues"],
            "read_issues for {name}"
        );
        assert_eq!(enc_meta(&metadata), case["metadata"], "metadata for {name}");
        assert_eq!(
            enc_issues(&metadata_issues),
            case["metadata_issues"],
            "metadata_issues for {name}"
        );
    }

    for case in v["env_cap"].as_array().unwrap() {
        std::env::remove_var("DECIDED_MAX_FILE_BYTES");
        if let Some(val) = case["value"].as_str() {
            std::env::set_var("DECIDED_MAX_FILE_BYTES", val);
        }
        assert_eq!(
            file_cap(),
            FileCap::Cap(case["expected"].as_u64().unwrap()),
            "file_cap for {:?}",
            case["value"]
        );
    }
    std::env::remove_var("DECIDED_MAX_FILE_BYTES");

    // Oracle read-crash zone (empirical, CPython 3.11; see FileCap docs):
    // fh.read(cap + 1) raises uncaught for caps at or above 2^63 - 34.
    let toolarge = FileCap::OracleCrash("OverflowError: byte string is too large");
    let overflow =
        FileCap::OracleCrash("OverflowError: cannot fit 'int' into an index-sized integer");
    for (value, expected) in [
        ("9223372036854775773", FileCap::Cap(9223372036854775773)), // 2^63 - 35
        ("9223372036854775774", toolarge),                          // 2^63 - 34
        ("9223372036854775806", toolarge),                          // 2^63 - 2
        ("9223372036854775807", overflow),                          // sys.maxsize
        ("9223372036854775808", overflow),                          // 2^63
        ("18446744073709551617", overflow),                         // 2^64 + 1
        ("99999999999999999999", overflow),
        // beyond i128 (saturating parse still lands in the crash zone)
        ("199999999999999999999999999999999999999999", overflow),
        // huge negatives are non-positive: default cap, no crash
        ("-99999999999999999999", FileCap::Cap(1 << 20)),
    ] {
        assert_eq!(
            file_cap_from(Some(value)),
            expected,
            "file_cap_from({value:?})"
        );
    }

    fs::remove_dir_all(&root).ok();
}

/// Minimal base64 decoder for the vector payloads (test-only; no new deps).
fn b64_decode(s: &str) -> Vec<u8> {
    fn val(b: u8) -> u32 {
        match b {
            b'A'..=b'Z' => (b - b'A') as u32,
            b'a'..=b'z' => (b - b'a' + 26) as u32,
            b'0'..=b'9' => (b - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }
    }
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'=' && b != b'\n').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for &b in chunk {
            acc = (acc << 6) | val(b);
        }
        match chunk.len() {
            4 => {
                out.push((acc >> 16) as u8);
                out.push((acc >> 8) as u8);
                out.push(acc as u8);
            }
            3 => {
                acc <<= 6;
                out.push((acc >> 16) as u8);
                out.push((acc >> 8) as u8);
            }
            2 => {
                acc <<= 12;
                out.push((acc >> 16) as u8);
            }
            _ => {}
        }
    }
    out
}
