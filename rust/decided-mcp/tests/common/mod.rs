#![allow(dead_code)]

use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub const CURRENT_VERSION: &str = "2026-07-28";
pub const VERSION_META_KEY: &str = "io.modelcontextprotocol/protocolVersion";

pub fn scratch(tag: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "decided-mcp-protocol-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch corpus");
    std::fs::write(
        directory.join("decision.md"),
        "---\nschema_version: 1\nid: FIX-MCP20260728\ntype: decision\n---\n\
# Protocol fixture\n\n## Status\n\nAccepted\n\n## Category\n\nTechnical\n\n\
## Context\n\nA protocol fixture is required.\n\n## Decision\n\nKeep it deterministic.\n\n\
## Consequences\n\nProtocol tests stay local.\n",
    )
    .expect("write fixture artifact");
    directory
}

pub fn run_stdio(tag: &str, requests: &[String]) -> Vec<String> {
    let corpus = scratch(tag);
    let mut child = Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
        .arg("--root")
        .arg(&corpus)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn decided-mcp");
    {
        let stdin = child.stdin.as_mut().expect("child stdin");
        for request in requests {
            writeln!(stdin, "{request}").expect("write request");
        }
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("wait for decided-mcp");
    let _ = std::fs::remove_dir_all(corpus);
    assert!(
        output.status.success(),
        "decided-mcp failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF-8 stdout")
        .lines()
        .map(str::to_string)
        .collect()
}

pub fn parse(frame: &str) -> Value {
    serde_json::from_str(frame).expect("valid JSON-RPC frame")
}

pub fn current_meta() -> Value {
    serde_json::json!({
        VERSION_META_KEY: CURRENT_VERSION,
        "io.modelcontextprotocol/clientInfo": {
            "name": "asdecided-conformance",
            "version": "1.0.0"
        },
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}
