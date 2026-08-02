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
        Self::start_with_origins(tag, &[])
    }

    fn start_with_origins(tag: &str, allowed_origins: &[&str]) -> Self {
        Self::start_with_origins_and_files(tag, allowed_origins, &[])
    }

    fn start_with_files(tag: &str, files: &[(&str, &str)]) -> Self {
        Self::start_with_origins_and_files(tag, &[], files)
    }

    fn start_with_origins_and_files(
        tag: &str,
        allowed_origins: &[&str],
        files: &[(&str, &str)],
    ) -> Self {
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
        for (name, contents) in files {
            let path = corpus.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }
        let port = TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut args = vec![
            "--root".to_string(),
            corpus.to_str().unwrap().to_string(),
            "--no-cache".to_string(),
            "--transport".to_string(),
            "http".to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
            "--port".to_string(),
            port.to_string(),
        ];
        for origin in allowed_origins {
            args.push("--allowed-origin".to_string());
            args.push((*origin).to_string());
        }
        let child = Command::new(env!("CARGO_BIN_EXE_decided-mcp"))
            .args(&args)
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
        self.post_with_version_and_origin(body, method_header, name, CURRENT_VERSION, None)
    }

    fn post_with_version(
        &self,
        body: &Value,
        method_header: &str,
        name: Option<&str>,
        version: &str,
    ) -> (String, Value) {
        self.post_with_version_and_origin(body, method_header, name, version, None)
    }

    fn post_with_origin(
        &self,
        body: &Value,
        method_header: &str,
        name: Option<&str>,
        origin: Option<&str>,
    ) -> (String, Value) {
        self.post_with_version_and_origin(body, method_header, name, CURRENT_VERSION, origin)
    }

    fn post_with_version_and_origin(
        &self,
        body: &Value,
        method_header: &str,
        name: Option<&str>,
        version: &str,
        origin: Option<&str>,
    ) -> (String, Value) {
        self.post_with_version_origin_headers(
            body,
            method_header,
            name,
            version,
            origin,
            &[],
        )
    }

    fn post_with_principal_headers(
        &self,
        body: &Value,
        method_header: &str,
        name: Option<&str>,
        headers: &[(&str, &str)],
    ) -> (String, Value) {
        self.post_with_version_origin_headers(
            body,
            method_header,
            name,
            CURRENT_VERSION,
            None,
            headers,
        )
    }

    fn post_with_version_origin_headers(
        &self,
        body: &Value,
        method_header: &str,
        name: Option<&str>,
        version: &str,
        origin: Option<&str>,
        extra_headers: &[(&str, &str)],
    ) -> (String, Value) {
        let body = body.to_string();
        let mut request = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: application/json\r\n\
Content-Type: application/json\r\nMCP-Protocol-Version: {version}\r\n\
Mcp-Method: {method_header}\r\n"
        );
        if let Some(name) = name {
            request.push_str(&format!("Mcp-Name: {name}\r\n"));
        }
        if let Some(origin) = origin {
            request.push_str(&format!("Origin: {origin}\r\n"));
        }
        for (key, value) in extra_headers {
            request.push_str(&format!("{key}: {value}\r\n"));
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

    fn audit_events(&self) -> Vec<Value> {
        std::fs::read_to_string(self.corpus.join("audit.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(parse)
            .collect()
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
fn current_http_rejects_legacy_header_with_current_body_metadata() {
    let _guard = serial_http_test();
    let server = Server::start("http-version-mismatch");
    let request = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/list",
        "params": {"_meta": current_meta()}
    });
    let (status, response) = server.post_with_version(
        &request,
        "tools/list",
        None,
        "2025-11-25",
    );
    assert_eq!(status, "HTTP/1.1 400 Bad Request");
    assert_eq!(response.pointer("/error/code"), Some(&json!(-32020)));
}

#[test]
fn current_http_rejects_invalid_jsonrpc_envelope() {
    let _guard = serial_http_test();
    let server = Server::start("http-envelope");
    let request = json!({
        "jsonrpc": "1.0",
        "id": {"object": true},
        "method": "server/discover",
        "params": {"_meta": current_meta()}
    });
    let (status, response) = server.post(&request, "server/discover", None);
    assert_eq!(status, "HTTP/1.1 400 Bad Request");
    assert_eq!(response.pointer("/error/code"), Some(&json!(-32600)));
    assert_eq!(response.pointer("/id"), Some(&json!(null)));
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

#[test]
fn current_http_rejects_unconfigured_origin_before_body_read() {
    let _guard = serial_http_test();
    let server = Server::start("http-origin-rejected");
    let body = json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "server/discover",
        "params": {"_meta": current_meta()}
    });
    let body = body.to_string();
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: https://attacker.example\r\n\
Accept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        1024 * 1024 + 1,
        body
    );
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert_eq!(response.lines().next(), Some("HTTP/1.1 403 Forbidden"));
}

#[test]
fn current_http_allows_explicit_origin() {
    let _guard = serial_http_test();
    let server = Server::start_with_origins("http-origin-allowed", &["https://agent.example"]);
    let request = json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "server/discover",
        "params": {"_meta": current_meta()}
    });
    let (status, response) = server.post_with_origin(
        &request,
        "server/discover",
        None,
        Some("https://agent.example"),
    );
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert_eq!(
        response.pointer("/result/supportedVersions/0"),
        Some(&json!(CURRENT_VERSION))
    );
}

fn tool_call(id: u64, name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": arguments,
            "_meta": current_meta()
        }
    })
}

#[test]
fn http_audit_records_every_result_collection() {
    let _guard = serial_http_test();
    let server = Server::start_with_files(
        "http-audit-collections",
        &[(
            "src/valid.md",
            "---\nschema_version: 1\nid: RAC-111111111111\ntype: decision\n---\n\
# Valid fixture\n\n## Status\n\nAccepted\n\n## Category\n\nTechnical\n\n## Context\n\nA valid fixture.\n\n\
## Decision\n\nKeep scope explicit.\n\n\
## Consequences\n\nThe audit is testable.\n\n\
## Applies To\n\n- src/valid.md\n",
        )],
    );

    let calls = [
        ("get_artifact", json!({"id": "RAC-111111111111"})),
        ("search_artifacts", json!({"query": "valid fixture"})),
        ("retrieve_grounding", json!({"task": "valid fixture"})),
        ("find_decisions", json!({"topic": "valid"})),
        (
            "find_decisions",
            json!({"topic": "", "path": "src/valid.md"}),
        ),
        ("get_related", json!({"id": "RAC-111111111111"})),
        ("get_summary", json!({})),
    ];
    for (index, (name, arguments)) in calls.iter().enumerate() {
        let (status, _) = server.post_with_principal_headers(
            &tool_call(index as u64 + 1, name, arguments.clone()),
            "tools/call",
            Some(name),
            &[("X-Lore-Principal", "alice@example.com")],
        );
        assert_eq!(status, "HTTP/1.1 200 OK", "{name} request failed");
    }

    let events = server.audit_events();
    assert_eq!(events.len(), calls.len());
    for event in &events {
        assert_eq!(event["principal"], json!("alice@example.com"));
        assert_eq!(event["attribution"], json!("asserted"));
        for record in event["returned"].as_array().unwrap() {
            assert!(record["id"].is_string());
            assert!(record["resolved"].is_boolean());
            assert!(record["provenance"].is_object());
            assert!(record["provenance"]["path"].is_string());
            assert!(record.get("content").is_none());
        }
    }

    let returned: Vec<Vec<String>> = events
        .iter()
        .map(|event| {
            event["returned"]
                .as_array()
                .unwrap()
                .iter()
                .map(|record| record["id"].as_str().unwrap().to_string())
                .collect()
        })
        .collect();
    assert!(returned[0].contains(&"RAC-111111111111".to_string()));
    assert!(returned[1].contains(&"RAC-111111111111".to_string()));
    assert!(returned[2].contains(&"RAC-111111111111".to_string()));
    assert!(returned[3].contains(&"RAC-111111111111".to_string()));
    assert!(returned[4].contains(&"RAC-111111111111".to_string()));
    assert!(returned[5].contains(&"RAC-111111111111".to_string()));
    assert!(returned[6].is_empty());
}

#[test]
fn http_principal_migration_is_explicit_and_response_independent() {
    let _guard = serial_http_test();
    let server = Server::start("http-principal-migration");
    let request = tool_call(20, "get_summary", json!({}));

    let (canonical_status, canonical) = server.post_with_principal_headers(
        &request,
        "tools/call",
        Some("get_summary"),
        &[("X-Lore-Principal", "alice@example.com")],
    );
    let (legacy_status, legacy) = server.post_with_principal_headers(
        &request,
        "tools/call",
        Some("get_summary"),
        &[("X-AsDecided-Principal", "bob@example.com")],
    );
    assert_eq!(canonical_status, "HTTP/1.1 200 OK");
    assert_eq!(legacy_status, "HTTP/1.1 200 OK");
    assert_eq!(canonical, legacy, "principal attribution must not authorize");

    let (equal_status, _) = server.post_with_principal_headers(
        &request,
        "tools/call",
        Some("get_summary"),
        &[
            ("X-Lore-Principal", "same@example.com"),
            ("X-AsDecided-Principal", "same@example.com"),
        ],
    );
    assert_eq!(equal_status, "HTTP/1.1 200 OK");

    let (conflict_status, conflict) = server.post_with_principal_headers(
        &request,
        "tools/call",
        Some("get_summary"),
        &[
            ("X-Lore-Principal", "alice@example.com"),
            ("X-AsDecided-Principal", "bob@example.com"),
        ],
    );
    assert_eq!(conflict_status, "HTTP/1.1 400 Bad Request");
    assert_eq!(conflict["error"]["code"], json!(-32023));

    let (duplicate_status, duplicate) = server.post_with_principal_headers(
        &request,
        "tools/call",
        Some("get_summary"),
        &[
            ("X-Lore-Principal", "alice@example.com"),
            ("X-Lore-Principal", "alice@example.com"),
        ],
    );
    assert_eq!(duplicate_status, "HTTP/1.1 400 Bad Request");
    assert_eq!(duplicate["error"]["code"], json!(-32023));
}
