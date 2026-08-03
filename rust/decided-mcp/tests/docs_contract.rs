//! Keep the public MCP guide tied to the shipped Rust binary.

use std::fs;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn guide_documents_the_native_surface_and_starts_the_example_server() {
    let root = repo_root();
    let guide = fs::read_to_string(root.join("docs/mcp.md")).expect("read MCP guide");
    for tool in [
        "get_artifact",
        "search_artifacts",
        "retrieve_grounding",
        "find_decisions",
        "get_related",
        "get_summary",
    ] {
        assert!(guide.contains(&format!("`{tool}`")), "guide omits {tool}");
    }
    assert!(
        guide.contains("--budget N"),
        "guide omits the response budget flag"
    );
    assert!(
        !guide.contains("--telemetry"),
        "guide still advertises the retired MCP telemetry flag"
    );

    let example_root = root.join("examples/guide");
    let mut child = Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
        .args([
            "--root",
            example_root.to_str().expect("example path is UTF-8"),
            "--no-cache",
            "--budget",
            "128",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start documented stdio command");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{}}\n")
        .expect("send tools/list");
    let output = child
        .wait_with_output()
        .expect("wait for documented command");
    assert!(
        output.status.success(),
        "documented command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let frame = String::from_utf8(output.stdout).expect("MCP stdout is UTF-8");
    let response: Value = serde_json::from_str(frame.trim()).expect("tools/list response is JSON");
    let tools = response
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .expect("tools/list response contains tools");
    assert_eq!(tools.len(), 6, "the guide and binary must expose six tools");

    // The documented HTTP command has a mandatory audit-on contract. Start it
    // with the smallest valid config and prove that the process binds before
    // killing it; the detailed request/response matrix remains in http_transport.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let http_root = std::env::temp_dir().join(format!("asdecided-mcp-docs-{nonce}"));
    fs::create_dir_all(http_root.join(".decided")).expect("create HTTP smoke corpus");
    fs::write(
        http_root.join(".decided/config.yaml"),
        "audit:\n  enabled: true\n",
    )
    .expect("write HTTP audit config");
    let audit_path = http_root.join("audit.jsonl");
    let probe = TcpListener::bind(("127.0.0.1", 0)).expect("reserve HTTP smoke port");
    let port = probe.local_addr().expect("read HTTP smoke port").port();
    drop(probe);
    let mut http = Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
        .args([
            "--root",
            http_root.to_str().expect("HTTP root is UTF-8"),
            "--transport",
            "http",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--path",
            "/mcp",
            "--budget",
            "10000",
        ])
        .env("DECIDED_AUDIT_PATH", &audit_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start documented HTTP command");
    let mut ready = false;
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !ready {
        let _ = http.kill();
        let _ = http.wait();
        let _ = fs::remove_dir_all(&http_root);
        panic!("documented HTTP command did not bind");
    }
    http.kill().expect("stop HTTP smoke server");
    http.wait().expect("reap HTTP smoke server");
    fs::remove_dir_all(http_root).expect("remove HTTP smoke corpus");
}
