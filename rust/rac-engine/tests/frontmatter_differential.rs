//! Differential driver for oracle-vs-port frontmatter fuzzing (Phase 3
//! support). Ignored by default; the fuzz harness runs it with:
//!
//!   DECIDED_FM_DIFF_IN=<inputs.json> DECIDED_FM_DIFF_OUT=<out.json> \
//!       cargo test -p asdecided-core --test frontmatter_differential -- --ignored
//!
//! `inputs.json` is a JSON array of raw frontmatter strings; the output is a
//! JSON array of {data, load_issues, metadata, issues} objects in the same
//! tagged encoding as `frontmatter.json` (crash-mirroring entries carry
//! {"internal": message} instead).

use rac_engine::frontmatter::{
    load_frontmatter_mapping, parse_frontmatter, ArtifactMetadata, Issue, Yaml,
};
use serde_json::{json, Value};

fn canon(v: &Value) -> String {
    // Order-stable sort key (ASCII-escaped compact JSON, sorted keys).
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

#[test]
#[ignore = "differential driver; run explicitly with DECIDED_FM_DIFF_IN/OUT"]
fn differential_batch() {
    let input = std::env::var("DECIDED_FM_DIFF_IN").expect("DECIDED_FM_DIFF_IN");
    let output = std::env::var("DECIDED_FM_DIFF_OUT").expect("DECIDED_FM_DIFF_OUT");
    let text = std::fs::read_to_string(&input).expect("read diff input");
    let raws: Vec<String> = serde_json::from_str(&text).expect("parse diff input");
    let mut results: Vec<Value> = Vec::new();
    for raw in &raws {
        let (data, load_issues) = load_frontmatter_mapping(raw);
        if load_issues
            .first()
            .is_some_and(|i| i.code == "internal-oracle-divergence")
        {
            results.push(json!({"internal": load_issues[0].message}));
            continue;
        }
        let (meta, issues) = parse_frontmatter(raw);
        if meta.is_none()
            && issues
                .first()
                .is_some_and(|i| i.code == "internal-oracle-divergence")
        {
            // Validator-stage oracle crash (int->str limit): load succeeded,
            // the marker surfaces from parse_frontmatter.
            results.push(json!({"internal": issues[0].message}));
            continue;
        }
        results.push(json!({
            "data": match &data {
                None => Value::Null,
                Some(pairs) => enc_yaml(&Yaml::Map(pairs.clone())),
            },
            "load_issues": load_issues.iter().map(enc_issue).collect::<Vec<_>>(),
            "metadata": enc_meta(&meta),
            "issues": issues.iter().map(enc_issue).collect::<Vec<_>>(),
        }));
    }
    std::fs::write(
        &output,
        serde_json::to_string(&results).expect("serialize diff output"),
    )
    .expect("write diff output");
}
