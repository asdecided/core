//! The Risk family over the MCP surface (ADR-151 decision 6): the existing
//! tools accept and report `risk` wherever they take or return an artifact
//! type, and `related_risks` is an ordinary edge in `get_related`.

mod common;

use common::{parse, run_stdio_with_budget};
use serde_json::Value;

const DECISION: &str = "---\nschema_version: 1\nid: TST-00000000000A\ntype: decision\n---\n# ADR-001: Use the vendor queue\n\n## Context\n\nC.\n\n## Decision\n\nD.\n\n## Consequences\n\nE.\n\n## Related Risks\n\n- TST-00000000000R\n";

const RISK: &str = "---\nschema_version: 1\nid: TST-00000000000R\ntype: risk\n---\n# Vendor lock-in on the managed queue\n\n## Status\n\nAccepted\n\n## Risk\n\nR.\n\n## Likelihood\n\nL.\n\n## Impact\n\nI.\n\n## Related Decisions\n\n- TST-00000000000A\n";

fn initialize() -> String {
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"risk-family","version":"1.0.0"}}}"#.to_string()
}

fn call(id: u32, name: &str, arguments: Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments}
    })
    .to_string()
}

fn text_payload(frame: &Value) -> Value {
    let text = frame["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    serde_json::from_str(text).expect("tool text is JSON")
}

#[test]
fn served_corpus_reports_searches_and_relates_risk_artifacts() {
    let files = [
        ("decisions/adr-001.md", DECISION),
        ("risks/vendor-lock-in.md", RISK),
    ];
    let frames = run_stdio_with_budget(
        "risk-family",
        16_384,
        &files,
        &[
            initialize(),
            call(2, "get_summary", serde_json::json!({})),
            call(
                3,
                "search_artifacts",
                serde_json::json!({"query": "vendor", "type": "risk"}),
            ),
            call(
                4,
                "get_related",
                serde_json::json!({"id": "TST-00000000000A"}),
            ),
        ],
    );

    let summary = text_payload(&parse(&frames[1]));
    let by_type = summary["artifacts"]["by_type"].as_object().unwrap();
    let keys: Vec<&str> = by_type.keys().map(String::as_str).collect();
    // The five fixed keys and `unknown` keep their places; a registry-driven
    // family follows them, present only when the corpus holds one.
    assert_eq!(
        keys,
        [
            "requirement",
            "decision",
            "roadmap",
            "prompt",
            "design",
            "unknown",
            "risk"
        ],
        "{summary}"
    );
    assert_eq!(by_type["risk"], 1);

    let search = text_payload(&parse(&frames[2]));
    assert_eq!(search["match_count"], 1, "{search}");
    assert_eq!(search["matches"][0]["id"], "TST-00000000000R");
    assert_eq!(search["matches"][0]["type"], "risk");

    let related = text_payload(&parse(&frames[3]));
    assert_eq!(
        related["outgoing"]["related_risks"],
        serde_json::json!(["TST-00000000000R"]),
        "{related}"
    );
    assert_eq!(related["incoming"][0]["type"], "risk");
    assert_eq!(related["incoming"][0]["section"], "related_decisions");
}
