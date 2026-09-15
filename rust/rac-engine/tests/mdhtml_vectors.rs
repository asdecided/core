//! Oracle-vector conformance tests for `rac_engine::mdhtml`.
//!
//! Vectors: tests/vectors/mdhtml.json, generated from the markdown-it-py
//! 4.2.0 oracle (`MarkdownIt("commonmark", {"html": False})`, the exact
//! `export` body_html configuration) by rust/spec/gen_vectors_mdhtml.py.
//! The grid sweeps every C0 control char U+0001-U+001F through the block
//! positions where markdown-it-py's Python `str.strip()` inline trimming
//! (whitespace set includes U+001C-U+001F) diverges from ASCII trimming —
//! fuzz campaign 2, findings 009/036/039.

use rac_engine::mdhtml::render;
use serde_json::Value;

#[test]
fn render_matches_oracle() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/mdhtml.json");
    let text = std::fs::read_to_string(path).expect("vector file readable");
    let v: Value = serde_json::from_str(&text).expect("vector file parses");
    let cases = v["cases"].as_array().expect("cases present");
    assert!(
        cases.len() >= 500,
        "expected >=500 mdhtml cases, got {}",
        cases.len()
    );
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let body = case["text"].as_str().unwrap();
        let expected = case["html"].as_str().unwrap();
        assert_eq!(render(body), expected, "case {name}: body={body:?}");
    }
}
