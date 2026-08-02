mod common;

use common::{current_meta, parse, scratch, CURRENT_VERSION};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

static HTTP_TEST_LOCK: Mutex<()> = Mutex::new(());

fn serial_http_test() -> MutexGuard<'static, ()> {
    HTTP_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Server {
    child: Child,
    corpus: std::path::PathBuf,
    port: u16,
}

impl Server {
    fn start(tag: &str) -> Self {
        let corpus = scratch(tag);
        let audit_path = corpus.join("audit.jsonl");
        std::fs::create_dir_all(corpus.join(".decided")).unwrap();
        std::fs::write(
            corpus.join(".decided/config.yaml"),
            format!(
                "audit:\n  enabled: true\n  path: {}\n",
                audit_path.display()
            ),
        )
        .unwrap();
        let port = TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let child = Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
            .args([
                "--root",
                corpus.to_str().unwrap(),
                "--transport",
                "http",
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn HTTP server");
        let server = Self {
            child,
            corpus,
            port,
        };
        for _ in 0..100 {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return server;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("HTTP server did not start");
    }

    fn post(&self, body: &Value, method_header: &str, name: Option<&str>) -> (String, Value) {
        let body = body.to_string();
        let mut request = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: application/json\r\n\
Content-Type: application/json\r\nMCP-Protocol-Version: {CURRENT_VERSION}\r\n\
Mcp-Method: {method_header}\r\n"
        );
        if let Some(name) = name {
            request.push_str(&format!("Mcp-Name: {name}\r\n"));
        }
        request.push_str(&format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ));
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        let status = headers.lines().next().unwrap().to_string();
        (status, parse(body))
    }

    fn raw(&self, request: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.corpus);
    }
}

#[test]
fn current_http_discovery_is_sessionless() {
    let _guard = serial_http_test();
    let server = Server::start("http-discover");
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "server/discover",
        "params": {"_meta": current_meta()}
    });
    let (status, response) = server.post(&request, "server/discover", None);
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert_eq!(
        response.pointer("/result/supportedVersions/0"),
        Some(&json!(CURRENT_VERSION))
    );
}

#[test]
fn current_http_rejects_header_body_mismatch() {
    let _guard = serial_http_test();
    let server = Server::start("http-mismatch");
    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {"_meta": current_meta()}
    });
    let (status, response) = server.post(&request, "resources/list", None);
    assert_eq!(status, "HTTP/1.1 400 Bad Request");
    assert_eq!(response.pointer("/error/code"), Some(&json!(-32020)));
}

#[test]
fn current_http_unknown_rpc_uses_404_and_method_not_found() {
    let _guard = serial_http_test();
    let server = Server::start("http-unknown");
    let request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "unknown/current",
        "params": {"_meta": current_meta()}
    });
    let (status, response) = server.post(&request, "unknown/current", None);
    assert_eq!(status, "HTTP/1.1 404 Not Found");
    assert_eq!(response.pointer("/error/code"), Some(&json!(-32601)));
}

#[test]
fn current_http_tool_call_requires_name_and_returns_current_result() {
    let _guard = serial_http_test();
    let server = Server::start("http-tool-call");
    let request = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "get_summary",
            "arguments": {},
            "_meta": current_meta()
        }
    });
    let (missing_status, missing_response) = server.post(&request, "tools/call", None);
    assert_eq!(missing_status, "HTTP/1.1 400 Bad Request");
    assert_eq!(
        missing_response.pointer("/error/code"),
        Some(&json!(-32020))
    );

    let (status, response) = server.post(&request, "tools/call", Some("get_summary"));
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert_eq!(
        response.pointer("/result/resultType"),
        Some(&json!("complete"))
    );
}

#[test]
fn stalled_client_does_not_block_a_later_request() {
    let _guard = serial_http_test();
    let server = Server::start("http-stalled-client");
    let mut stalled = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stalled
        .write_all(b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\n")
        .unwrap();

    let request = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "server/discover",
        "params": {"_meta": current_meta()}
    });
    let (status, response) = server.post(&request, "server/discover", None);
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert_eq!(
        response.pointer("/result/supportedVersions/0"),
        Some(&json!(CURRENT_VERSION))
    );
}

#[test]
fn oversized_content_length_is_rejected_before_allocation() {
    let _guard = serial_http_test();
    let server = Server::start("http-oversized-body");
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: application/json\r\n\
Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        1024 * 1024 + 1
    );
    let response = server.raw(&request);
    assert_eq!(
        response.lines().next(),
        Some("HTTP/1.1 413 Payload Too Large")
    );
}
