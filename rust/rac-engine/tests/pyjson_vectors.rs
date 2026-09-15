//! Oracle-vector conformance tests for `rac_engine::pyjson`.
//!
//! Vectors: tests/vectors/pyjson.json, generated from the CPython 3.11
//! oracle by rust/spec/gen_vectors_pyjson.py. Each row carries the doc plus
//! the two dialect encodings (`json.dumps(indent=2)` / compact
//! ensure_ascii=False). The int-vs-float distinction survives transport:
//! serde_json (preserve_order) parses `2` integral and `2.0` as f64 and
//! keeps key insertion order.

use rac_engine::pyjson::{dumps_compact, dumps_indent2};
use serde_json::Value;

#[test]
fn dumps_matches_oracle() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/pyjson.json");
    let text = std::fs::read_to_string(path).expect("vector file readable");
    let v: Value = serde_json::from_str(&text).expect("vector file parses");
    let rows = v["rows"].as_array().expect("rows present");
    assert!(
        rows.len() >= 50,
        "expected >=50 pyjson rows, got {}",
        rows.len()
    );
    for (i, row) in rows.iter().enumerate() {
        let doc = &row["doc"];
        let indent2 = row["indent2"].as_str().unwrap();
        let compact = row["compact"].as_str().unwrap();
        assert_eq!(dumps_indent2(doc), indent2, "row {i} indent2: doc={doc}");
        assert_eq!(dumps_compact(doc), compact, "row {i} compact: doc={doc}");
    }
}
