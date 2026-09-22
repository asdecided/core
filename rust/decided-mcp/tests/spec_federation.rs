//! Inherited spec bundles over the MCP surface (ADR-150): a served child
//! answers with its parent's declared types, and a collision introduced while
//! the server runs blocks the next request with the composition finding.

mod common;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use common::scratch;
use serde_json::{json, Value};

const STANDARDS: &str = "asdecided/fixtures-spec-standards";
const RUNBOOK_ALT: &str = r#"{"name":"runbook","display":"Run Book","required":["purpose"],"recommended":[],"optional":[],"metadata":{"status":["Active"]},"retired_status":[],"descriptions":{},"guidance":{},"synonyms":{},"id_field":null,"starter_bodies":{}}"#;

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create fixture directory");
    for entry in fs::read_dir(from).expect("read fixture directory") {
        let entry = entry.expect("fixture entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("entry type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy fixture file");
        }
    }
}

fn fixture_copy(tag: &str) -> PathBuf {
    let target = scratch(tag);
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/spec-federation");
    copy_tree(&source, &target);
    target
}

fn initialize() -> String {
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"spec-federation","version":"1.0.0"}}}"#.to_string()
}

fn call(id: u32, name: &str, arguments: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments}
    })
    .to_string()
}

fn spawn(root: &Path, cached: bool) -> (Child, ChildStdin, BufReader<ChildStdout>) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_decided-mcp"));
    command.arg("--root").arg(root.join("decisions"));
    if cached {
        command
            .env("DECIDED_CACHE_DIR", root.join(".decided/cache"))
            .env("XDG_CACHE_HOME", root.join(".xdg/cache"));
    } else {
        command.arg("--no-cache");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn decided-mcp");
    let stdin = child.stdin.take().expect("server stdin");
    let stdout = BufReader::new(child.stdout.take().expect("server stdout"));
    (child, stdin, stdout)
}

fn send(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>, request: &str) -> Value {
    writeln!(stdin, "{request}").expect("write MCP request");
    stdin.flush().expect("flush MCP request");
    let mut line = String::new();
    stdout.read_line(&mut line).expect("read MCP response");
    serde_json::from_str(line.trim()).expect("MCP response JSON")
}

fn finish(child: Child, stdin: ChildStdin, stdout: BufReader<ChildStdout>) {
    drop(stdin);
    drop(stdout);
    let output = child.wait_with_output().expect("wait for decided-mcp");
    assert!(
        output.status.success(),
        "server failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn payload(frame: &Value) -> Value {
    let text = frame["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tool text in {frame}"));
    serde_json::from_str(text).expect("tool text is JSON")
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[test]
fn a_served_child_answers_with_inherited_types_and_blocks_a_live_collision() {
    live_collision("spec-federation-live", false);
}

/// The cached path reports the same finding; both name the code once.
#[test]
fn a_cached_server_blocks_a_live_collision_with_one_code_prefix() {
    live_collision("spec-federation-live-cached", true);
}

fn live_collision(tag: &str, cached: bool) {
    let root = fixture_copy(tag);
    let (child, mut stdin, mut stdout) = spawn(&root, cached);
    send(&mut stdin, &mut stdout, &initialize());

    let summary = payload(&send(
        &mut stdin,
        &mut stdout,
        &call(2, "get_summary", json!({})),
    ));
    assert_eq!(summary["artifacts"]["by_type"]["runbook"], 1);
    assert_eq!(summary["artifacts"]["by_type"]["policy"], 1);

    let search = payload(&send(
        &mut stdin,
        &mut stdout,
        &call(
            3,
            "search_artifacts",
            json!({"query": "deploy", "type": "runbook"}),
        ),
    ));
    assert_eq!(search["match_count"], 1, "{search}");
    assert_eq!(search["matches"][0]["id"], "SPS-000000000001");
    assert_eq!(search["matches"][0]["type"], "runbook");
    assert_eq!(search["matches"][0]["provenance"]["source"], STANDARDS);

    let artifact = payload(&send(
        &mut stdin,
        &mut stdout,
        &call(4, "get_artifact", json!({"id": "SPS-000000000001"})),
    ));
    assert_eq!(artifact["type"], "runbook");

    // A different `runbook` declaration lands in the child and is re-pinned
    // while the server runs: the next request recomposes and fails closed.
    let bundle = format!("{{\"artifact_specs\":[{RUNBOOK_ALT}]}}\n");
    fs::write(root.join(".decided/artifact-specs.json"), &bundle).unwrap();
    let config_path = root.join(".decided/config.yaml");
    let config = fs::read_to_string(&config_path).unwrap();
    let start = config.find("digest: sha256:").unwrap();
    let end = start + "digest: sha256:".len() + 64;
    fs::write(
        &config_path,
        format!(
            "{}digest: sha256:{}{}",
            &config[..start],
            sha256(bundle.as_bytes()),
            &config[end..]
        ),
    )
    .unwrap();
    let frame = send(&mut stdin, &mut stdout, &call(5, "get_summary", json!({})));
    assert_eq!(frame["result"]["isError"], true, "{frame}");
    let text = frame["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("corpus-federation-artifact-type-conflict: source "),
        "{text}"
    );
    assert!(text.contains("'runbook'"), "{text}");
    finish(child, stdin, stdout);
}

#[test]
fn a_closure_without_bundles_keeps_the_builtin_shape() {
    let root = scratch("spec-federation-inert");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/eval/federation");
    copy_tree(&source, &root);
    let mut child = Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
        .arg("--root")
        .arg(root.join("graph-decisions"))
        .arg("--no-cache")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn decided-mcp");
    {
        let stdin = child.stdin.as_mut().expect("server stdin");
        writeln!(stdin, "{}", initialize()).unwrap();
        writeln!(stdin, "{}", call(2, "get_summary", json!({}))).unwrap();
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("wait for decided-mcp");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let frames: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let summary = payload(&frames[1]);
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
            "unknown"
        ]
    );
}
