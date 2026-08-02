//! Consume the language-neutral MCP vectors published by asdecided/spec.
//!
//! The fixture manifest is intentionally outside this repository. When
//! `ASDECIDED_MCP_SPEC_DIR` is unset the test skips, which keeps local focused
//! MCP development self-contained; the native CI job checks out the
//! authoritative spec and runs this test with the variable set.

mod common;

use common::{parse, run_stdio, scratch};
use serde_json::Value;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const CURRENT_VERSION: &str = "2026-07-28";

struct HttpServer {
    child: Child,
    corpus: PathBuf,
    port: u16,
}

impl HttpServer {
    fn start(tag: &str, allowed_origins: &[&str]) -> Self {
        let corpus = scratch(&format!("spec-http-{tag}"));
        let audit_path = corpus.join("audit.jsonl");
        std::fs::create_dir_all(corpus.join(".decided")).expect("create audit config directory");
        std::fs::write(
            corpus.join(".decided/config.yaml"),
            format!(
                "audit:\n  enabled: true\n  path: {}\n",
                audit_path.display()
            ),
        )
        .expect("write audit config");

        let port = TcpListener::bind(("127.0.0.1", 0))
            .expect("reserve HTTP test port")
            .local_addr()
            .expect("read HTTP test port")
            .port();
        let mut args = vec![
            "--root".to_string(),
            corpus.to_string_lossy().into_owned(),
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
            .expect("spawn decided-mcp HTTP server");
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
        panic!("HTTP server did not start for vector {tag}");
    }

    fn request(&self, vector: &Value) -> (u16, Option<Value>) {
        let http = vector
            .get("http")
            .and_then(Value::as_object)
            .expect("HTTP vector has an http object");
        let path = http.get("path").and_then(Value::as_str).unwrap_or("/mcp");
        let method = http.get("method").and_then(Value::as_str).unwrap_or("POST");
        let body = vector
            .get("request")
            .expect("vector has a request")
            .to_string();
        let declared_length = http
            .get("content_length")
            .and_then(Value::as_u64)
            .unwrap_or(body.len() as u64);
        let mut wire =
            format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
        if let Some(headers) = http.get("headers").and_then(Value::as_object) {
            for (name, value) in headers {
                wire.push_str(name);
                wire.push_str(": ");
                wire.push_str(value.as_str().expect("HTTP header values are strings"));
                wire.push_str("\r\n");
            }
        }
        wire.push_str(&format!("Content-Length: {declared_length}\r\n\r\n{body}"));

        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connect HTTP vector");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("set HTTP vector timeout");
        stream
            .write_all(wire.as_bytes())
            .expect("write HTTP vector");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("read HTTP vector response");
        let (header_bytes, body) = response
            .split_once("\r\n\r\n")
            .expect("HTTP response has a header/body separator");
        let status = header_bytes
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .expect("HTTP response has a numeric status");
        let json = (!body.trim().is_empty()).then(|| parse(body));
        (status, json)
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.corpus);
    }
}

fn assert_json_assertions(id: &str, value: &Value, expected: &Value) {
    let assertions = expected
        .get("assert")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    for assertion in assertions {
        let pointer = assertion
            .get("pointer")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("vector {id} assertion has no JSON Pointer"));
        let actual = value.pointer(pointer);
        if assertion.get("exists").and_then(Value::as_bool) == Some(true) {
            assert!(actual.is_some(), "vector {id}: {pointer} should exist");
        } else if assertion.get("absent").and_then(Value::as_bool) == Some(true) {
            assert!(actual.is_none(), "vector {id}: {pointer} should be absent");
        } else {
            let wanted = assertion
                .get("equals")
                .unwrap_or_else(|| panic!("vector {id}: assertion {pointer} has no operator"));
            assert_eq!(actual, Some(wanted), "vector {id}: mismatch at {pointer}");
        }
    }
}

fn run_stdio_vector(id: &str, vector: &Value, expected: &Value) {
    let request = vector.get("request").expect("stdio vector has a request");
    let frames = run_stdio(&format!("spec-{id}"), &[request.to_string()]);
    if let Some(count) = expected.get("frame_count").and_then(Value::as_u64) {
        assert_eq!(frames.len() as u64, count, "vector {id}: frame count");
    }
    if let Some(exact) = expected.get("exact_frames") {
        let expected_frames: Vec<&str> = exact
            .as_array()
            .expect("exact_frames is an array")
            .iter()
            .map(|frame| frame.as_str().expect("exact frame is a string"))
            .collect();
        assert_eq!(
            frames, expected_frames,
            "vector {id}: frozen legacy bytes changed"
        );
    }
    if let Some(first) = frames.first() {
        assert_json_assertions(id, &parse(first), expected);
    }
}

fn run_http_vector(id: &str, vector: &Value, expected: &Value) {
    let http = vector.get("http").expect("HTTP vector has an http object");
    let origins = http
        .get("allowed_origins")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|origin| origin.as_str().expect("allowed origin is a string"))
        .collect::<Vec<_>>();
    let server = HttpServer::start(id, &origins);
    let (status, body) = server.request(vector);
    let expected_status = expected
        .get("status")
        .and_then(Value::as_u64)
        .expect("HTTP vector expects a status");
    assert_eq!(status as u64, expected_status, "vector {id}: HTTP status");
    if let Some(body) = body {
        assert_json_assertions(id, &body, expected);
    } else {
        assert!(
            expected.get("assert").is_none(),
            "vector {id}: JSON assertions require a response body"
        );
    }
}

#[test]
fn shared_mcp_vectors_match_asdecided_spec() {
    let Some(spec_dir) = std::env::var_os("ASDECIDED_MCP_SPEC_DIR") else {
        eprintln!("SKIP: ASDECIDED_MCP_SPEC_DIR is unset");
        return;
    };
    let manifest_path = PathBuf::from(spec_dir).join("mcp/conformance/vectors.json");
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(&manifest_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", manifest_path.display())),
    )
    .unwrap_or_else(|error| panic!("parse {}: {error}", manifest_path.display()));
    assert_eq!(
        manifest.pointer("/_meta/format").and_then(Value::as_str),
        Some("asdecided-mcp-conformance-v1")
    );
    assert_eq!(
        manifest
            .pointer("/_meta/protocol_version")
            .and_then(Value::as_str),
        Some(CURRENT_VERSION)
    );
    let vectors = manifest
        .get("vectors")
        .and_then(Value::as_array)
        .expect("MCP fixture manifest has a vectors array");
    assert!(!vectors.is_empty(), "MCP fixture manifest is not empty");
    let mut ids = HashSet::new();
    for vector in vectors {
        let id = vector
            .get("id")
            .and_then(Value::as_str)
            .expect("MCP vector has a stable id");
        assert!(ids.insert(id), "duplicate MCP vector id: {id}");
        let transport = vector
            .get("transport")
            .and_then(Value::as_str)
            .expect("MCP vector has a transport");
        let expected = vector.get("expect").expect("MCP vector has expectations");
        match transport {
            "stdio" => run_stdio_vector(id, vector, expected),
            "http" => run_http_vector(id, vector, expected),
            other => panic!("vector {id}: unsupported transport {other:?}"),
        }
    }
    eprintln!("OK: consumed {} shared MCP vectors", vectors.len());
}
