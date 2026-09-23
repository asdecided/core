//! Pinned spec bundles over the MCP surface (ADR-083, revised): a served
//! corpus that pins a bundle answers with its declared types, a pin the
//! server cannot honour is a tool error, and a corpus without the stanza is
//! untouched.

mod common;

use common::{parse, run_stdio_with_budget};
use serde_json::Value;

const BUNDLE: &str = r#"{"artifact_specs":[{"name":"runbook","display":"Runbook","required":["purpose","steps"],"recommended":["rollback"],"optional":["related decisions"],"metadata":{"status":["Draft","Active","Retired"]},"retired_status":["Retired"],"descriptions":{"purpose":"Why"},"guidance":{"purpose":["What?"],"steps":["How?"],"rollback":["Undo?"]},"synonyms":{},"id_field":null,"starter_bodies":{"purpose":"TODO"},"okf_type":"Runbook"}]}"#;

const RUNBOOK: &str = "---\nschema_version: 1\nid: MB-000000000001\ntype: runbook\n---\n# Deploy the Search Service\n\n## Status\n\nActive\n\n## Purpose\n\nRoll a build.\n\n## Steps\n\n1. Drain.\n2. Deploy.\n\n## Rollback\n\nShift back.\n";

fn config(digest: &str) -> String {
    format!(
        "repository_key: MB\ncorpus:\n  source: acme/mcp-bundle\nartifact_types:\n  version: 1\n  bundle:\n    path: .decided/artifact-specs.json\n    digest: sha256:{digest}\n"
    )
}

fn initialize() -> String {
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"spec-bundle","version":"1.0.0"}}}"#.to_string()
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
fn served_corpus_answers_with_bundle_types() {
    let digest = rac_engine::sha256::hexdigest(BUNDLE.as_bytes());
    let cfg = config(&digest);
    let files = [
        (".decided/config.yaml", cfg.as_str()),
        (".decided/artifact-specs.json", BUNDLE),
        ("runbooks/deploy.md", RUNBOOK),
    ];
    let frames = run_stdio_with_budget(
        "spec-bundle-served",
        16_384,
        &files,
        &[
            initialize(),
            call(2, "get_summary", serde_json::json!({})),
            call(
                3,
                "search_artifacts",
                serde_json::json!({"query": "deploy", "artifact_type": "runbook"}),
            ),
            call(
                4,
                "get_artifact",
                serde_json::json!({"id": "MB-000000000001"}),
            ),
        ],
    );
    assert_eq!(frames.len(), 4, "{frames:?}");
    let summary = text_payload(&parse(&frames[1]));
    assert_eq!(summary["artifacts"]["by_type"]["runbook"], 1);
    let search = text_payload(&parse(&frames[2]));
    assert_eq!(search["match_count"], 1);
    assert_eq!(search["matches"][0]["type"], "runbook");
    assert_eq!(search["matches"][0]["id"], "MB-000000000001");
    let artifact = text_payload(&parse(&frames[3]));
    assert_eq!(artifact["type"], "runbook");
}

#[test]
fn digest_mismatch_is_a_tool_error() {
    let cfg = config(&"0".repeat(64));
    let files = [
        (".decided/config.yaml", cfg.as_str()),
        (".decided/artifact-specs.json", BUNDLE),
        ("runbooks/deploy.md", RUNBOOK),
    ];
    let frames = run_stdio_with_budget(
        "spec-bundle-mismatch",
        16_384,
        &files,
        &[initialize(), call(2, "get_summary", serde_json::json!({}))],
    );
    assert_eq!(frames.len(), 2, "{frames:?}");
    let frame = parse(&frames[1]);
    assert_eq!(frame["result"]["isError"], true, "{frame}");
    let text = frame["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("artifact-spec-bundle-digest-mismatch: "),
        "{text}"
    );
}

#[test]
fn corpus_without_the_stanza_sees_only_the_builtins() {
    let files = [
        (
            ".decided/config.yaml",
            "repository_key: MB\ncorpus:\n  source: acme/plain\n",
        ),
        ("runbooks/deploy.md", RUNBOOK),
    ];
    let frames = run_stdio_with_budget(
        "spec-bundle-absent",
        16_384,
        &files,
        &[initialize(), call(2, "get_summary", serde_json::json!({}))],
    );
    let summary = text_payload(&parse(&frames[1]));
    let by_type = summary["artifacts"]["by_type"].as_object().unwrap();
    let keys: Vec<&str> = by_type.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "requirement",
            "decision",
            "roadmap",
            "prompt",
            "design",
            "unknown"
        ]
    );
    // The runbook frontmatter names an unregistered type here, so the file is
    // an unknown document — exactly as before bundles existed.
    assert_eq!(by_type["unknown"], 1);
}

#[test]
fn startup_probe_counts_bundle_type_artifacts() {
    let root = common::scratch("spec-bundle-startup");
    let digest = rac_engine::sha256::hexdigest(BUNDLE.as_bytes());
    for (name, contents) in [
        (".decided/config.yaml", config(&digest)),
        (".decided/artifact-specs.json", BUNDLE.to_string()),
        ("runbooks/deploy.md", RUNBOOK.to_string()),
    ] {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
        .arg("--root")
        .arg(&root)
        .arg("--no-cache")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            writeln!(child.stdin.take().unwrap(), "{}", initialize()).unwrap();
            child.wait_with_output()
        })
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("no AsDecided artifacts found"),
        "a corpus holding only bundle types is not empty: {stderr}"
    );
}

#[test]
fn get_summary_lists_bundle_types_in_registry_order() {
    // The bundle declares runbook before policy; the walk meets the policy
    // first (`a-policies/` sorts before `runbooks/`).
    let policy_element = r#"{"name":"policy","display":"Policy","required":["scope","rules"],"recommended":[],"optional":[],"metadata":{},"retired_status":[],"descriptions":{},"guidance":{},"synonyms":{},"id_field":null,"starter_bodies":{}}"#;
    let elements = BUNDLE
        .strip_suffix("]}")
        .expect("the bundle ends its element array");
    let bundle = format!("{elements},{policy_element}]}}");
    let digest = rac_engine::sha256::hexdigest(bundle.as_bytes());
    let cfg = config(&digest);
    let policy = "---\nschema_version: 1\nid: MB-000000000002\ntype: policy\n---\n# Data Retention\n\n## Status\n\nActive\n\n## Scope\n\nAll data.\n\n## Rules\n\nKeep 30 days.\n";
    let files = [
        (".decided/config.yaml", cfg.as_str()),
        (".decided/artifact-specs.json", bundle.as_str()),
        ("a-policies/retention.md", policy),
        ("runbooks/deploy.md", RUNBOOK),
    ];
    let frames = run_stdio_with_budget(
        "spec-bundle-order",
        16_384,
        &files,
        &[initialize(), call(2, "get_summary", serde_json::json!({}))],
    );
    let summary = text_payload(&parse(&frames[1]));
    let keys: Vec<&str> = summary["artifacts"]["by_type"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "requirement",
            "decision",
            "roadmap",
            "prompt",
            "design",
            "unknown",
            "runbook",
            "policy"
        ],
        "{summary}"
    );
}
