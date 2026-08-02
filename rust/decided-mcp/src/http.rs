//! HTTP transport for `decided-mcp` (ADR-098) — a minimal, dependency-free
//! HTTP/1.1 server over `std::net`.
//!
//! It fronts the very same [`crate::process_request`] frame processor as the
//! stdio transport, so every tool/response body is byte-identical to stdio —
//! the parity surface ADR-098 defines ("an HTTP response is payload-identical
//! to stdio for identical corpus bytes"). Serving is stateless per call
//! (ADR-032): one JSON response per POST, no session store. The transport
//! envelope (uvicorn's `Date`/`Server` header bytes on the Python side, and its
//! Python-specific error prose) is the SDK's incidental framing, not RAC's
//! contract, and is a declared non-parity surface — the same posture the stdio
//! port took toward argparse's usage-wrapping (PORT-CONTRACT.d/10 §9).
//!
//! HTTP is mandatory-audit-on (ADR-084, ADR-098): [`ensure_audit_sink`] proves
//! a working audit sink at startup or the server refuses to start. Attribution
//! rides the `X-AsDecided-Principal` request header (ADR-098) — recorded, never
//! verified, never an access-control input.
//!
//! Wire contract: `rust/PORT-CONTRACT.d/19-http-transport.md`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{atomic::{AtomicUsize, Ordering}, Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use crate::{audit, process_request, protocol, ServerState};

/// Keep a malformed or abandoned client from consuming the shared endpoint
/// indefinitely. These are transport limits, not MCP payload limits: the
/// response budget is enforced by the engine after dispatch.
const HTTP_IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_ACTIVE_CONNECTIONS: usize = 64;
const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
const MAX_HEADER_LINE_BYTES: usize = 8 * 1024;
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_HEADER_COUNT: usize = 64;
const MAX_BODY_BYTES: usize = 1024 * 1024;

/// Prove a working audit sink for HTTP serving (ADR-084 fail-loud). Audit must
/// be enabled in `.decided/config.yaml` and its resolved path writable, or the
/// shared endpoint refuses to start. stdio never calls this — audit stays
/// config-driven and default-absent there.
pub fn ensure_audit_sink(config: &audit::AuditConfig) -> Result<(), String> {
    if !config.enabled {
        return Err(
            "HTTP serving requires the read-access audit log, but it is not \
enabled. Add an `audit:` stanza with `enabled: true` to .decided/config.yaml \
before serving over HTTP (ADR-084)."
                .to_string(),
        );
    }
    let path = audit::resolve_audit_path(&config.path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| audit_unwritable(&path, &e.to_string()))?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| audit_unwritable(&path, &e.to_string()))?;
    Ok(())
}

fn audit_unwritable(path: &std::path::Path, reason: &str) -> String {
    format!(
        "HTTP serving requires a writable audit log, but {} could not be opened \
for append ({reason}). Fix the audit path or permissions before serving over \
HTTP (ADR-084).",
        path.display()
    )
}

/// Serve `root` over streamable HTTP (stateless JSON mode) until interrupted.
/// Connections are handled by a bounded set of workers. The mutable freshness
/// model and audit recorder remain serialised behind mutexes, while request
/// parsing happens outside those locks so a slow client cannot block the next
/// client from being accepted.
pub fn serve_http(
    root: &str,
    state: ServerState,
    recorder: Option<audit::Recorder>,
    host: &str,
    port: u16,
    path: &str,
    allowed_origins: &[String],
) -> ! {
    let listener = match TcpListener::bind((host, port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("decided-mcp: error: could not bind {host}:{port} ({e})");
            std::process::exit(1);
        }
    };
    eprintln!(
        "decided-mcp: serving over HTTP at http://{host}:{port}{path} (read-only, \
stateless per call; authentication belongs to the deployment proxy, ADR-085)."
    );
    eprintln!(
        "decided-mcp: HTTP limits — {MAX_ACTIVE_CONNECTIONS} active connections, \
{MAX_REQUEST_LINE_BYTES}B request line, {MAX_HEADER_BYTES}B headers, \
{MAX_BODY_BYTES}B body, {}s I/O timeout.",
        HTTP_IO_TIMEOUT.as_secs()
    );
    let state = Arc::new(Mutex::new(state));
    let recorder = Arc::new(Mutex::new(recorder));
    let root = Arc::new(root.to_string());
    let path = Arc::new(path.to_string());
    let allowed_origins = Arc::new(allowed_origins.to_vec());
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let admitted = active
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                        (current < MAX_ACTIVE_CONNECTIONS).then_some(current + 1)
                    })
                    .is_ok();
                if !admitted {
                    reject_connection(s);
                    continue;
                }
                let active = Arc::clone(&active);
                let state = Arc::clone(&state);
                let recorder = Arc::clone(&recorder);
                let root = Arc::clone(&root);
                let path = Arc::clone(&path);
                let allowed_origins = Arc::clone(&allowed_origins);
                std::thread::spawn(move || {
                    let _permit = ConnectionPermit { active };
                    handle_connection(&root, &state, &recorder, &path, &allowed_origins, s);
                });
            }
            Err(_) => continue,
        }
    }
    std::process::exit(0);
}

struct ConnectionPermit {
    active: Arc<AtomicUsize>,
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    /// The request target's path, without any `?query`.
    fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or(&self.target)
    }
}

fn handle_connection(
    root: &str,
    state: &Mutex<ServerState>,
    recorder: &Mutex<Option<audit::Recorder>>,
    path: &str,
    allowed_origins: &[String],
    mut stream: TcpStream,
) {
    if stream.set_read_timeout(Some(HTTP_IO_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(HTTP_IO_TIMEOUT)).is_err()
    {
        return;
    }
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let req = match read_request(&mut reader, allowed_origins) {
        Ok(Some(req)) => req,
        Ok(None) => return,
        Err(error) => {
            respond(
                &mut stream,
                &Response { status: error.status(), body: None },
            );
            return;
        }
    };
    let response = {
        let mut state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut recorder = recorder.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        route(root, &mut state, &mut recorder, path, &req)
    };
    respond(&mut stream, &response);
}

/// Parse one HTTP/1.1 request: request line, headers, and a Content-Length body.
fn read_request(
    reader: &mut BufReader<TcpStream>,
    allowed_origins: &[String],
) -> Result<Option<Request>, ReadError> {
    let Some(line) = read_line_limited(reader, MAX_REQUEST_LINE_BYTES)? else {
        return Ok(None);
    };
    let line = std::str::from_utf8(&line).map_err(|_| ReadError::BadRequest)?;
    let mut parts = line.trim_end_matches(['\r', '\n']).split_whitespace();
    let method = parts.next().ok_or(ReadError::BadRequest)?.to_string();
    let target = parts.next().ok_or(ReadError::BadRequest)?.to_string();
    let version = parts.next().ok_or(ReadError::BadRequest)?;
    if parts.next().is_some() || version != "HTTP/1.1" {
        return Err(ReadError::BadRequest);
    }

    let mut headers = Vec::new();
    let mut header_bytes = 0usize;
    loop {
        let Some(line) = read_line_limited(reader, MAX_HEADER_LINE_BYTES)? else {
            return Err(ReadError::BadRequest);
        };
        header_bytes = header_bytes
            .checked_add(line.len())
            .ok_or(ReadError::TooLarge)?;
        if header_bytes > MAX_HEADER_BYTES {
            return Err(ReadError::TooLarge);
        }
        let trimmed = std::str::from_utf8(&line)
            .map_err(|_| ReadError::BadRequest)?
            .trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        let (k, v) = trimmed.split_once(':').ok_or(ReadError::BadRequest)?;
        if headers.len() >= MAX_HEADER_COUNT {
            return Err(ReadError::TooLarge);
        }
        headers.push((k.trim().to_string(), v.trim().to_string()));
    }

    if !origin_allowed(&headers, allowed_origins) {
        return Err(ReadError::OriginDenied);
    }
    if headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("transfer-encoding")) {
        return Err(ReadError::BadRequest);
    }
    let mut len = None;
    for (_, value) in headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
    {
        let parsed = value.trim().parse::<usize>().map_err(|_| ReadError::BadRequest)?;
        if len.is_some_and(|previous| previous != parsed) {
            return Err(ReadError::BadRequest);
        }
        len = Some(parsed);
    }
    let len = len.unwrap_or(0);
    if len > MAX_BODY_BYTES {
        return Err(ReadError::TooLarge);
    }
    let mut body = vec![0u8; len];
    if len > 0 && reader.read_exact(&mut body).is_err() {
        return Err(ReadError::BadRequest);
    }
    Ok(Some(Request { method, target, headers, body }))
}

fn read_line_limited(
    reader: &mut impl BufRead,
    limit: usize,
) -> Result<Option<Vec<u8>>, ReadError> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(|_| ReadError::BadRequest)?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(ReadError::BadRequest)
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |position| position + 1);
        if line.len().saturating_add(take) > limit {
            return Err(ReadError::TooLarge);
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            return Ok(Some(line));
        }
    }
}

#[derive(Clone, Copy)]
enum ReadError {
    BadRequest,
    TooLarge,
    OriginDenied,
}

impl ReadError {
    fn status(self) -> &'static str {
        match self {
            Self::BadRequest => "400 Bad Request",
            Self::TooLarge => "413 Payload Too Large",
            Self::OriginDenied => "403 Forbidden",
        }
    }
}

fn reject_connection(mut stream: TcpStream) {
    let _ = stream.set_write_timeout(Some(HTTP_IO_TIMEOUT));
    respond(
        &mut stream,
        &Response { status: "503 Service Unavailable", body: None },
    );
}

fn origin_allowed(headers: &[(String, String)], allowed_origins: &[String]) -> bool {
    let origins: Vec<&str> = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("origin"))
        .map(|(_, value)| value.trim())
        .collect();
    match origins.as_slice() {
        [] => true,
        [origin] => allowed_origins.iter().any(|allowed| allowed == origin),
        _ => false,
    }
}

struct Response {
    status: &'static str, // e.g. "200 OK"
    body: Option<String>, // None => empty body, no content-type
}

fn json_response(status: &'static str, body: String) -> Response {
    Response { status, body: Some(body) }
}

/// Apply the streamable-HTTP status-code semantics captured from the SDK, then
/// hand valid requests to the shared frame processor.
fn route(
    root: &str,
    state: &mut ServerState,
    recorder: &mut Option<audit::Recorder>,
    path: &str,
    req: &Request,
) -> Response {
    if req.path() != path {
        return Response { status: "404 Not Found", body: None };
    }
    match req.method.as_str() {
        // The server offers no server-initiated SSE stream (stateless, read-only,
        // emits no notifications), so it declines GET — spec-permitted, and the
        // covered POST-only clients are unaffected (declared divergence from the
        // SDK, which opens an idle stream).
        "GET" => Response { status: "405 Method Not Allowed", body: None },
        "DELETE" => Response { status: "405 Method Not Allowed", body: None },
        "POST" => route_post(root, state, recorder, req),
        _ => Response { status: "405 Method Not Allowed", body: None },
    }
}

fn route_post(
    root: &str,
    state: &mut ServerState,
    recorder: &mut Option<audit::Recorder>,
    req: &Request,
) -> Response {
    // Accept must be present and admit JSON (json_response mode): absent -> 406.
    let accepts_json = req.header("accept").is_some_and(|a| {
        a.contains("application/json") || a.contains("text/event-stream") || a.contains("*/*")
    });
    if !accepts_json {
        return Response { status: "406 Not Acceptable", body: None };
    }
    // Content-Type must be JSON.
    let json_ct = req
        .header("content-type")
        .is_some_and(|c| c.contains("application/json"));
    if !json_ct {
        return Response { status: "400 Bad Request", body: None };
    }
    let message: Value = match serde_json::from_slice(&req.body) {
        Ok(v) => v,
        Err(_) => {
            return json_response(
                "400 Bad Request",
                "{\"jsonrpc\":\"2.0\",\"id\":\"server-error\",\"error\":{\"code\":-32700,\
\"message\":\"Parse error\"}}"
                    .to_string(),
            );
        }
    };
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return json_response(
            "400 Bad Request",
            "{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32600,\
\"message\":\"Invalid Request\"}}"
                .to_string(),
        );
    };
    let id_json = message
        .get("id")
        .and_then(|id| serde_json::to_string(id).ok())
        .unwrap_or_else(|| "null".to_string());
    let era = match http_era(req, &message, &id_json) {
        Ok(era) => era,
        Err(response) => return response,
    };
    if era == protocol::Era::Current {
        if let Err(response) = validate_current_headers(req, &message, method, &id_json) {
            return response;
        }
        if let Err(frame) = protocol::validate_current_metadata(&message, &id_json) {
            return json_response("400 Bad Request", frame);
        }
    }
    // A notification (no id) is acknowledged with 202 and no body (ADR-032:
    // nothing to return; the read-only server holds no session to advance).
    if message.get("id").is_none() {
        return Response { status: "202 Accepted", body: None };
    }
    // Attribution rides X-AsDecided-Principal (ADR-098): recorded by audit, never an
    // access-control input — the response is identical whatever it says.
    let principal = req.header("x-asdecided-principal");
    let frame = process_request(
        root,
        state,
        era,
        &id_json,
        &message,
        recorder.as_mut(),
        principal,
    );
    let status = if era == protocol::Era::Current
        && !protocol::current_method_supported(method)
    {
        "404 Not Found"
    } else {
        "200 OK"
    };
    json_response(status, frame)
}

fn http_era(req: &Request, message: &Value, id_json: &str) -> Result<protocol::Era, Response> {
    let header_version = req.header("mcp-protocol-version");
    let metadata_version = protocol::requested_version(message);
    match header_version {
        Some(protocol::CURRENT_PROTOCOL_VERSION) => Ok(protocol::Era::Current),
        Some(version) if protocol::LEGACY_PROTOCOL_VERSIONS.contains(&version) => {
            Ok(protocol::Era::Legacy)
        }
        Some(version) => Err(json_response(
            "400 Bad Request",
            protocol::unsupported_protocol_frame(message.get("id"), Some(version)),
        )),
        None if metadata_version == Some(protocol::CURRENT_PROTOCOL_VERSION) => Err(json_response(
            "400 Bad Request",
            protocol::header_mismatch_frame(
                id_json,
                "MCP-Protocol-Version",
                Some(protocol::CURRENT_PROTOCOL_VERSION),
                None,
            ),
        )),
        None => Ok(protocol::Era::Legacy),
    }
}

fn validate_current_headers(
    req: &Request,
    message: &Value,
    method: &str,
    id_json: &str,
) -> Result<(), Response> {
    let metadata_version = protocol::requested_version(message);
    if metadata_version != Some(protocol::CURRENT_PROTOCOL_VERSION) {
        return Err(json_response(
            "400 Bad Request",
            protocol::header_mismatch_frame(
                id_json,
                "MCP-Protocol-Version",
                Some(protocol::CURRENT_PROTOCOL_VERSION),
                metadata_version,
            ),
        ));
    }
    let method_header = req.header("mcp-method");
    if method_header != Some(method) {
        return Err(json_response(
            "400 Bad Request",
            protocol::header_mismatch_frame(id_json, "Mcp-Method", Some(method), method_header),
        ));
    }
    if method == "tools/call" {
        let expected_name = message.pointer("/params/name").and_then(Value::as_str);
        let actual_name = req.header("mcp-name");
        if expected_name.is_none() || actual_name != expected_name {
            return Err(json_response(
                "400 Bad Request",
                protocol::header_mismatch_frame(
                    id_json,
                    "Mcp-Name",
                    expected_name,
                    actual_name,
                ),
            ));
        }
    }
    Ok(())
}

fn respond(writer: &mut TcpStream, resp: &Response) {
    let mut out = format!("HTTP/1.1 {}\r\n", resp.status);
    match &resp.body {
        Some(body) => {
            out.push_str("content-type: application/json\r\n");
            out.push_str(&format!("content-length: {}\r\n", body.len()));
            out.push_str("connection: close\r\n\r\n");
            out.push_str(body);
        }
        None => {
            out.push_str("content-length: 0\r\n");
            out.push_str("connection: close\r\n\r\n");
        }
    }
    let _ = writer.write_all(out.as_bytes());
    let _ = writer.flush();
}
