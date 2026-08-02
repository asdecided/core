mod common;

use common::{current_meta, parse, run_stdio_with_budget, scratch};
use serde_json::{json, Value};
use std::process::Command;

#[test]
fn stdio_caps_large_artifact_payload() {
    let artifact = format!(
        "---\nschema_version: 1\nid: RAC-111111111111\ntype: decision\n---\n# Budget fixture\n\n## Status\n\nAccepted\n\n## Context\n\n{}\n\n## Decision\n\nKeep the response bounded.\n\n## Consequences\n\nThe caller can request the source file for the remainder.\n",
        "large-context ".repeat(2_000)
    );
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "get_artifact",
            "arguments": {"id": "RAC-111111111111"},
            "_meta": current_meta()
        }
    });
    let frames = run_stdio_with_budget(
        "stdio-budget",
        512,
        &[("large.md", artifact.as_str())],
        &[request.to_string()],
    );
    let response = parse(&frames[0]);
    let text = response
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .expect("tool text");
    assert!(text.chars().count() <= 512, "{} characters", text.chars().count());
    let payload: Value = serde_json::from_str(text).expect("serialized payload");
    assert_eq!(payload["truncated"], json!(true));
    assert!(payload["omitted"].as_i64().unwrap_or(0) > 0);
}

#[test]
fn per_call_budget_below_minimum_is_a_tool_error() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "get_artifact",
            "arguments": {"id": "FIX-MCP20260728", "budget": 64},
            "_meta": current_meta()
        }
    });
    let frames = run_stdio_with_budget("stdio-minimum", 512, &[], &[request.to_string()]);
    let response = parse(&frames[0]);
    assert_eq!(response.pointer("/result/isError"), Some(&json!(true)));
    assert!(response
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("minimum supported budget")));
}

#[test]
fn startup_budget_below_minimum_fails_before_serving() {
    let corpus = scratch("startup-minimum");
    let output = Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
        .args([
            "--root",
            corpus.to_str().expect("UTF-8 corpus path"),
            "--budget",
            "127",
        ])
        .output()
        .expect("run invalid startup budget");
    let _ = std::fs::remove_dir_all(corpus);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("must be at least 128"));
}
