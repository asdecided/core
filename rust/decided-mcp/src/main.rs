//! decided-mcp — the AsDecided MCP server over stdio
//! (PORT-CONTRACT.d/10-mcp-surface.md is the binding wire contract).
//!
//! Framing: newline-delimited JSON-RPC, UTF-8, non-ASCII raw, no
//! Content-Length headers (§1). Envelopes are compact (§2); the inner tool
//! payload is `json.dumps(..., ensure_ascii=False)` with DEFAULT separators
//! (spaces after `:` and `,`). Six tools (the ORACLE-NEXT surface, a strict
//! superset of PRIMARY's five, §10). Stateless re-read per call (ADR-032).

mod args;
mod audit;
mod graph;
mod http;
mod protocol;
mod provenance;
mod sidecar;
mod tools;

use args::{Arg, Kind, Param};
use rac_engine::budget;
use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// The pinned `tools/list` result — the captured ORACLE-NEXT bytes, embedded
/// verbatim (schemas, descriptions, pydantic-shaped titles incl. the
/// function-name leaks `find_decisions_toolArguments` /
/// `retrieve_grounding_toolArguments`; §4).
const TOOLS_LIST_RESULT: &str = include_str!("tools_list_result.json");

pub(crate) struct ServerState {
    repository_root: PathBuf,
    root_corpus_relative: String,
    federation_mode: Option<FederationMode>,
    persistent_cache: bool,
    tracker: Option<rac_engine::freshness::FreshnessTracker>,
    federated_tracker: Option<
        rac_engine::derived_cache::FederatedCacheTracker<
            rac_engine::composition::ComposedCorpus,
        >,
    >,
    graph_tracker: rac_engine::derived_cache::GraphFederatedCacheTracker,
    graph_cache: graph::GraphCache,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FederationMode {
    Unknown,
    V1,
    V2,
}

enum RequestRead<'a> {
    Legacy {
        generation: Option<u64>,
        model: Option<&'a rac_engine::freshness::TrackerModel>,
    },
    FederatedCached(
        rac_engine::derived_cache::FederatedCacheRead<
            'a,
            rac_engine::composition::ComposedCorpus,
        >,
    ),
    FederatedFresh {
        generation: rac_engine::derived_cache::LogicalGeneration,
        composed: Box<rac_engine::composition::ComposedCorpus>,
    },
    Graph(rac_engine::derived_cache::GraphFederatedCacheRead<'a>),
}

impl RequestRead<'_> {
    fn legacy(&self) -> (Option<u64>, Option<&rac_engine::freshness::TrackerModel>) {
        match self {
            Self::Legacy { generation, model } => (*generation, *model),
            _ => (None, None),
        }
    }

    fn composed(&self) -> Option<&rac_engine::composition::ComposedCorpus> {
        match self {
            Self::FederatedCached(read) => Some(read.composed),
            Self::FederatedFresh { composed, .. } => Some(composed),
            Self::Legacy { .. } | Self::Graph(_) => None,
        }
    }

    fn cached_model(&self) -> Option<&rac_engine::derived_cache::ReadModel> {
        match self {
            Self::FederatedCached(read) => Some(read.model),
            _ => None,
        }
    }

    fn logical_generation(&self) -> Option<&rac_engine::derived_cache::LogicalGeneration> {
        match self {
            Self::FederatedCached(read) => Some(read.generation),
            Self::FederatedFresh { generation, .. } => Some(generation),
            Self::Legacy { .. } | Self::Graph(_) => None,
        }
    }

    fn graph(&self) -> Option<&rac_engine::derived_cache::GraphFederatedCacheRead<'_>> {
        match self {
            Self::Graph(read) => Some(read),
            _ => None,
        }
    }
}

/// The SDK's logging notification for an unparseable input line (§1) —
/// note the field order: method, params, jsonrpc.
const PARSE_ERROR_NOTIFICATION: &str = "{\"method\":\"notifications/message\",\"params\":{\"level\":\"error\",\"logger\":\"mcp.server.exception_handler\",\"data\":\"Internal Server Error\"},\"jsonrpc\":\"2.0\"}";

fn usage_error(msg: &str) -> ! {
    eprintln!("decided-mcp: error: {msg}");
    std::process::exit(2);
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn non_loopback_bind_error(transport: &str, host: &str, behind_proxy: bool) -> Option<String> {
    if transport == "http" && !behind_proxy && !is_loopback_host(host) {
        Some(format!(
            "non-loopback HTTP bind {host:?} requires explicit --behind-proxy acknowledgement; \
             put an authenticating TLS proxy in front of the server and review \
             docs/deployment-hardening.md before exposing it"
        ))
    } else {
        None
    }
}

fn main() {
    let mut argv = std::env::args().skip(1);
    let mut root = ".".to_string();
    let mut cache = true;
    // Transport (ADR-098). stdio is the default and byte-unchanged; http selects
    // the streamable-HTTP transport (mandatory audit-on, loopback by default).
    let mut transport = "stdio".to_string();
    let mut host = "127.0.0.1".to_string();
    let mut behind_proxy = false;
    let mut port: u16 = 8000;
    let mut path = "/mcp".to_string();
    let mut server_budget = budget::DEFAULT_BUDGET;
    let mut allowed_origins = Vec::new();
    while let Some(a) = argv.next() {
        match a.as_str() {
            "--root" => match argv.next() {
                Some(v) => root = v,
                None => usage_error("--root requires a value"),
            },
            "--transport" => match argv.next().as_deref() {
                Some(v @ ("stdio" | "http")) => transport = v.to_string(),
                Some(v) => usage_error(&format!(
                    "argument --transport: invalid choice: '{v}' (choose from 'stdio', 'http')"
                )),
                None => usage_error("--transport requires a value"),
            },
            "--host" => match argv.next() {
                Some(v) => host = v,
                None => usage_error("--host requires a value"),
            },
            "--behind-proxy" => behind_proxy = true,
            "--port" => match argv.next() {
                Some(v) => match v.parse::<u16>() {
                    Ok(p) => port = p,
                    Err(_) => usage_error(&format!("argument --port: invalid int value: '{v}'")),
                },
                None => usage_error("--port requires a value"),
            },
            "--path" => match argv.next() {
                Some(v) => path = v,
                None => usage_error("--path requires a value"),
            },
            "--budget" => match argv.next() {
                Some(v) => match v.parse::<i64>() {
                    Ok(value) if budget::valid_configured_budget(value) => server_budget = value,
                    Ok(value) => usage_error(&format!(
                        "argument --budget: value must be at least {} (got {value})",
                        budget::MIN_BUDGET
                    )),
                    Err(_) => usage_error(&format!("argument --budget: invalid int value: '{v}'")),
                },
                None => usage_error("--budget requires a value"),
            },
            "--allowed-origin" => match argv.next() {
                Some(v) if !v.trim().is_empty() => allowed_origins.push(v),
                Some(_) => usage_error("--allowed-origin requires a non-empty origin"),
                None => usage_error("--allowed-origin requires a value"),
            },
            // Cache flags are real since INDEX-PLAN B6 and remain
            // output-neutral (ADR-112: cache-on vs cache-off runs are
            // frame-for-frame byte-identical; native warm == cold holds
            // even for the duplicate-token class, PORT-CONTRACT.d/10 §0a).
            "--no-cache" => cache = false,
            "--cache" => cache = true,
            other => usage_error(&format!("unrecognized argument: {other}")),
        }
    }
    if let Some(message) = non_loopback_bind_error(&transport, &host, behind_proxy) {
        usage_error(&message);
    }
    if !std::path::Path::new(&root).is_dir() {
        usage_error(&format!("not a directory: {root}"));
    }
    let mut federation_mode = None;
    let topology = repository_topology(&root, None, &mut federation_mode)
        .unwrap_or_else(|error| usage_error(&error));
    check_corpus(&root, &topology);
    // Server-lifetime freshness (ADR-105/118): one tracker per server keeps
    // the derived read-model current through Linux inotify-clean detection or
    // the authoritative stat fallback, re-deriving only where files changed.
    let tracker = if rac_engine::derived_cache::cache_enabled(cache) {
        Some(rac_engine::freshness::FreshnessTracker::new(
            rac_engine::derived_cache::default_cache_dir(),
            &root,
            None,
        ))
    } else {
        None
    };
    let federated_tracker = if rac_engine::derived_cache::cache_enabled(cache) {
        Some(rac_engine::derived_cache::FederatedCacheTracker::new(
            rac_engine::derived_cache::default_cache_dir(),
        ))
    } else {
        None
    };
    let mut state = ServerState {
        repository_root: topology.repository_root,
        root_corpus_relative: topology.root_corpus_relative,
        federation_mode,
        persistent_cache: rac_engine::derived_cache::cache_enabled(cache),
        tracker,
        federated_tracker,
        graph_tracker: rac_engine::derived_cache::GraphFederatedCacheTracker::new(
            rac_engine::derived_cache::default_cache_dir(),
        ),
        graph_cache: graph::GraphCache::default(),
    };
    // Audit recorder (ADR-084): built from the `.decided/config.yaml` audit stanza,
    // default-absent for stdio (byte-unchanged when off), mandatory for HTTP.
    let audit_config = match audit::load_audit_config(&root) {
        Ok(c) => c,
        Err(reason) => usage_error(&format!("malformed audit config: {reason}")),
    };
    if transport == "http" {
        // Mandatory audit-on (ADR-098): refuse to start without a working sink.
        if let Err(msg) = http::ensure_audit_sink(&audit_config) {
            usage_error(&msg);
        }
        let recorder = audit::build(&root, "http", &audit_config);
        if let Some(recorder) = recorder.as_ref() {
            audit::announce(recorder);
        }
        http::serve_http(
            &root,
            state,
            recorder,
            &host,
            port,
            &path,
            &allowed_origins,
            server_budget,
        );
    }
    let mut recorder = audit::build(&root, "stdio", &audit_config);
    if let Some(recorder) = recorder.as_ref() {
        audit::announce(recorder);
    }
    serve(&root, &mut state, &mut recorder, server_budget);
}

/// Startup diagnostic (stderr only; declared-normalized in parity, §0).
fn check_corpus(root: &str, topology: &RepositoryTopology) {
    let has_artifacts = if topology.federation_mode == Some(FederationMode::V2) {
        let verified = rac_engine::federation::verify_federation(
            &topology.repository_root,
            &topology.root_corpus_relative,
        )
        .unwrap_or_else(|error| usage_error(&error.to_string()))
        .unwrap_or_else(|| usage_error("version-2 federation manifest disappeared"));
        rac_engine::graph_federated_corpus::compose_verified_federation(verified)
            .unwrap_or_else(|error| usage_error(&error.to_string()))
            .composition
            .effective()
            .any(|item| item.spec.is_some())
    } else if topology.federation_mode == Some(FederationMode::V1) {
        let generation = rac_engine::derived_cache::capture_logical_generation(
            &topology.repository_root,
            root,
            true,
        )
        .unwrap_or_else(|error| usage_error(&error.to_string()));
        let composed = rac_engine::derived_cache::compose_logical_generation(root, &generation)
            .unwrap_or_else(|error| usage_error(&error.to_string()));
        let has_artifacts = composed
            .effective()
            .any(|item| item.spec.is_some());
        has_artifacts
    } else {
        rac_engine::resolve::build_index(root, true)
            .iter()
            .any(|entry| entry.artifact_type != "unknown")
    };
    if !has_artifacts {
        eprintln!(
            "decided-mcp: no AsDecided artifacts found under '{root}'. Point --root at a \
directory containing RAC Markdown artifacts, or run 'decided init' to initialize \
a new repository. The server is running; get_summary will report the empty state."
        );
    }
}

struct RepositoryTopology {
    repository_root: PathBuf,
    root_corpus_relative: String,
    federation_mode: Option<FederationMode>,
}

fn marker_present(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "parent-corpus-malformed-manifest: cannot inspect repository topology {}: {error}",
            path.display()
        )),
    }
}

/// Discover and then pin the repository topology used by one server.
///
/// Startup searches for either governing config or federation manifest. Each
/// request supplies the pinned root, so removing config cannot make discovery
/// jump to another ancestor and silently return to the single-corpus model.
/// Manifest presence is inspected with `symlink_metadata`, then parsed by the
/// strict loader; directories, symlinks (including dangling ones), and races
/// therefore fail closed instead of masquerading as absence.
fn repository_topology(
    root: &str,
    pinned_root: Option<&Path>,
    federation_mode: &mut Option<FederationMode>,
) -> Result<RepositoryTopology, String> {
    let resolved_root = std::fs::canonicalize(root).map_err(|error| {
        format!("cannot resolve MCP corpus root {}: {error}", Path::new(root).display())
    })?;
    let repository_root = if let Some(pinned) = pinned_root {
        pinned.to_path_buf()
    } else {
        let mut selected = None;
        for ancestor in resolved_root.ancestors() {
            let config = ancestor.join(rac_engine::federation::CONFIG_RELATIVE_PATH);
            let manifest = ancestor.join(rac_engine::federation::MANIFEST_RELATIVE_PATH);
            if marker_present(&config)? || marker_present(&manifest)? {
                selected = Some(ancestor.to_path_buf());
                break;
            }
        }
        selected.unwrap_or_else(|| resolved_root.clone())
    };
    let relative = resolved_root.strip_prefix(&repository_root).map_err(|_| {
        format!(
            "parent-corpus-path-escape: MCP corpus root {} escaped pinned repository root {}",
            resolved_root.display(),
            repository_root.display()
        )
    })?;
    let root_corpus_relative = if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    };

    let manifest_path = repository_root.join(rac_engine::federation::MANIFEST_RELATIVE_PATH);
    let present = marker_present(&manifest_path)?;
    if present && federation_mode.is_none() {
        // Presence becomes sticky before parsing. A malformed live addition
        // therefore cannot be removed to regain a local-only fallback.
        *federation_mode = Some(FederationMode::Unknown);
    }
    if !present && federation_mode.is_some() {
        return Err(format!(
            "parent-corpus-malformed-manifest: federation manifest disappeared after this server observed federation: {}",
            manifest_path.display()
        ));
    }
    let observed = if present {
        if rac_engine::federation::load_graph_manifest(&repository_root)
            .map_err(|error| error.to_string())?
            .is_some()
        {
            Some(FederationMode::V2)
        } else if rac_engine::federation::load_manifest(&repository_root)
            .map_err(|error| error.to_string())?
            .is_some()
        {
            Some(FederationMode::V1)
        } else {
            return Err(format!(
                "parent-corpus-malformed-manifest: federation manifest has no supported version: {}",
                manifest_path.display()
            ));
        }
    } else {
        None
    };
    match (*federation_mode, observed) {
        (Some(FederationMode::Unknown), Some(actual)) => *federation_mode = Some(actual),
        (Some(expected), Some(actual)) if expected != actual => {
            return Err(format!(
                "parent-corpus-malformed-manifest: federation manifest version changed after this server pinned {expected:?}: {}",
                manifest_path.display()
            ));
        }
        (None, Some(actual)) => *federation_mode = Some(actual),
        _ => {}
    }
    if federation_mode.is_some() {
        let config_path = repository_root.join(rac_engine::federation::CONFIG_RELATIVE_PATH);
        let metadata = std::fs::symlink_metadata(&config_path).map_err(|error| {
            format!(
                "parent-corpus-child-config-missing: child config is unavailable after this server observed federation: {}: {error}",
                config_path.display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "parent-corpus-symlink-traversal: child config must not be a symlink: {}",
                config_path.display()
            ));
        }
        if !metadata.is_file() {
            return Err(format!(
                "parent-corpus-child-config-missing: child config is not a regular file: {}",
                config_path.display()
            ));
        }
    }
    Ok(RepositoryTopology {
        repository_root,
        root_corpus_relative,
        federation_mode: *federation_mode,
    })
}

fn serve(
    root: &str,
    state: &mut ServerState,
    recorder: &mut Option<audit::Recorder>,
    server_budget: i64,
) {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            writeln!(out, "{PARSE_ERROR_NOTIFICATION}").ok();
            out.flush().ok();
            continue;
        };
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            writeln!(out, "{PARSE_ERROR_NOTIFICATION}").ok();
            out.flush().ok();
            continue;
        };
        let id_present = message.get("id").is_some();
        let id_json = protocol::request_id_json(&message);
        if let Err(frame) = protocol::validate_request_envelope(&message, &id_json) {
            if id_present {
                writeln!(out, "{frame}").ok();
                out.flush().ok();
            }
            continue;
        }
        if !id_present {
            continue; // notification (e.g. notifications/initialized): no response
        }
        // stdio has no per-request principal; attribution stays the recorder's
        // locally resolved identity (ADR-098).
        let era = match protocol::era_for_stdio(method, &message) {
            Ok(era) => era,
            Err(frame) => {
                writeln!(out, "{frame}").ok();
                out.flush().ok();
                continue;
            }
        };
        if era == protocol::Era::Current {
            if let Err(frame) = protocol::validate_current_metadata(&message, &id_json) {
                writeln!(out, "{frame}").ok();
                out.flush().ok();
                continue;
            }
        }
        let frame = process_request(
            root,
            state,
            era,
            &id_json,
            &message,
            recorder.as_mut(),
            None,
            server_budget,
        );
        writeln!(out, "{frame}").ok();
        out.flush().ok();
    }
}

/// Produce the JSON-RPC response frame for one request `method` (transport-
/// agnostic, so stdio and HTTP share exactly one code path — the byte-parity
/// surface, PORT-CONTRACT.d/10 §2/§4/§5). Callers extract `method`/`id` and the
/// per-transport envelope; this owns only the payload.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_request(
    root: &str,
    state: &mut ServerState,
    era: protocol::Era,
    id_json: &str,
    message: &Value,
    recorder: Option<&mut audit::Recorder>,
    principal: Option<&str>,
    server_budget: i64,
) -> String {
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .expect("transport validates method before dispatch");
    match (era, method) {
        (protocol::Era::Legacy, "initialize") => {
            protocol::legacy_initialize_frame(id_json, message)
        }
        (protocol::Era::Current, "server/discover") => protocol::discover_frame(id_json),
        (protocol::Era::Current, "initialize") => {
            protocol::method_not_found_frame(id_json, method)
        }
        (protocol::Era::Legacy, "ping") => {
            format!("{{\"jsonrpc\":\"2.0\",\"id\":{id_json},\"result\":{{}}}}")
        }
        (protocol::Era::Current, "tools/list") => {
            protocol::current_tools_list_frame(id_json, TOOLS_LIST_RESULT)
        }
        (protocol::Era::Legacy, "tools/list") => {
            format!("{{\"jsonrpc\":\"2.0\",\"id\":{id_json},\"result\":{TOOLS_LIST_RESULT}}}")
        }
        (protocol::Era::Current, "prompts/list") => {
            protocol::current_empty_list_frame(id_json, "prompts", true)
        }
        (protocol::Era::Legacy, "prompts/list") => {
            format!("{{\"jsonrpc\":\"2.0\",\"id\":{id_json},\"result\":{{\"prompts\":[]}}}}")
        }
        (protocol::Era::Current, "resources/list") => {
            protocol::current_empty_list_frame(id_json, "resources", false)
        }
        (protocol::Era::Legacy, "resources/list") => {
            format!("{{\"jsonrpc\":\"2.0\",\"id\":{id_json},\"result\":{{\"resources\":[]}}}}")
        }
        (_, "tools/call") => tools_call_frame(
            root,
            state,
            era,
            id_json,
            message,
            recorder,
            principal,
            server_budget,
        ),
        (protocol::Era::Current, _) => protocol::method_not_found_frame(id_json, method),
        (protocol::Era::Legacy, _) => format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id_json},\"error\":{{\"code\":-32602,\"message\":\"Invalid request parameters\",\"data\":\"\"}}}}"
        ),
    }
}

/// Serialize the tools/call result envelope (§5). Success duplicates the
/// payload under `structuredContent.result` (the handlers return `str` and an
/// outputSchema exists — landmine 1); SDK-text errors ride `isError:true`
/// with no `structuredContent`.
fn call_result_frame(
    era: protocol::Era,
    id_json: &str,
    text: &str,
    is_error: bool,
) -> String {
    let mut content_item = Map::new();
    content_item.insert("type".to_string(), json!("text"));
    content_item.insert("text".to_string(), json!(text));
    let mut result = Map::new();
    result.insert(
        "content".to_string(),
        Value::Array(vec![Value::Object(content_item)]),
    );
    if !is_error {
        let mut structured = Map::new();
        structured.insert("result".to_string(), json!(text));
        result.insert("structuredContent".to_string(), Value::Object(structured));
    }
    result.insert("isError".to_string(), json!(is_error));
    if era == protocol::Era::Current {
        result.insert("resultType".to_string(), json!("complete"));
    }
    let result_json = serde_json::to_string(&Value::Object(result)).expect("serializable");
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id_json},\"result\":{result_json}}}")
}

#[allow(clippy::too_many_arguments)]
fn tools_call_frame(
    root: &str,
    state: &mut ServerState,
    era: protocol::Era,
    id_json: &str,
    message: &Value,
    recorder: Option<&mut audit::Recorder>,
    principal: Option<&str>,
    server_budget: i64,
) -> String {
    let name = message
        .pointer("/params/name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let empty = json!({});
    let arguments = message.pointer("/params/arguments").unwrap_or(&empty);
    let dispatch_started = rac_engine::timing::start();
    let dispatched = dispatch(
        root,
        state,
        name,
        arguments,
        recorder,
        principal,
        server_budget,
    );
    rac_engine::timing::emit_since(
        "mcp.dispatch",
        dispatch_started,
        &[("success", u64::from(dispatched.is_ok()))],
    );
    let serialize_started = rac_engine::timing::start();
    let frame = match dispatched {
        Ok(payload) => call_result_frame(era, id_json, &payload, false),
        Err(text) => call_result_frame(era, id_json, &text, true),
    };
    rac_engine::timing::emit_since(
        "mcp.response_serialize",
        serialize_started,
        &[("bytes", frame.len() as u64)],
    );
    frame
}

// Argument accessors over the coerced vector (defaults applied here, matching
// the Python signatures).
fn a_str(args: &[Arg], i: usize, default: &str) -> String {
    match &args[i] {
        Arg::Str(s) => s.clone(),
        _ => default.to_string(),
    }
}
fn a_opt_str(args: &[Arg], i: usize) -> Option<String> {
    match &args[i] {
        Arg::OptStr(v) => v.clone(),
        _ => None,
    }
}
fn a_opt_list_str(args: &[Arg], i: usize) -> Option<Vec<String>> {
    match &args[i] {
        Arg::OptListStr(v) => v.clone(),
        _ => None,
    }
}
fn a_int(args: &[Arg], i: usize, default: i64) -> i64 {
    match &args[i] {
        Arg::Int(v) => *v,
        _ => default,
    }
}
fn a_bool(args: &[Arg], i: usize, default: bool) -> bool {
    match &args[i] {
        Arg::Bool(v) => *v,
        _ => default,
    }
}

#[allow(clippy::too_many_arguments)]
fn read_request<'a>(
    root: &str,
    repository_root: &Path,
    root_corpus_relative: &str,
    federation_mode: &mut Option<FederationMode>,
    persistent_cache: bool,
    tracker: &'a mut Option<rac_engine::freshness::FreshnessTracker>,
    federated_tracker: &'a mut Option<
        rac_engine::derived_cache::FederatedCacheTracker<
            rac_engine::composition::ComposedCorpus,
        >,
    >,
    graph_tracker: &'a mut rac_engine::derived_cache::GraphFederatedCacheTracker,
) -> Result<RequestRead<'a>, String> {
    // Every recognized tool enters the same strict topology boundary after
    // its allowlisted arguments have been normalized for audit. Cache-off
    // skips persistence only; it never skips topology, parent verification,
    // or exact-byte composition.
    let topology = repository_topology(root, Some(repository_root), federation_mode)?;
    if topology.root_corpus_relative != root_corpus_relative {
        return Err(format!(
            "parent-corpus-path-escape: MCP corpus root changed after topology was pinned (expected {root_corpus_relative}, got {})",
            topology.root_corpus_relative
        ));
    }
    if topology.federation_mode == Some(FederationMode::V2) {
        Ok(RequestRead::Graph(
            graph_tracker
                .read_graph(
                    &topology.repository_root,
                    root_corpus_relative,
                    true,
                    persistent_cache,
                )
                .map_err(|error| error.to_string())?,
        ))
    } else if topology.federation_mode == Some(FederationMode::V1) {
        match federated_tracker.as_mut() {
            Some(tracker) => Ok(RequestRead::FederatedCached(
                tracker
                    .read_composed(&topology.repository_root, root, true)
                    .map_err(|error| error.to_string())?,
            )),
            None => {
                let generation = rac_engine::derived_cache::capture_logical_generation(
                    &topology.repository_root,
                    root,
                    true,
                )
                .map_err(|error| error.to_string())?;
                let composed = rac_engine::derived_cache::compose_logical_generation(
                    root,
                    &generation,
                )
                .map_err(|error| error.to_string())?;
                Ok(RequestRead::FederatedFresh {
                    generation,
                    composed: Box::new(composed),
                })
            }
        }
    } else {
        Ok(match tracker.as_mut() {
            Some(tracker) => {
                let (generation, model) = tracker.read_model_with_generation(false);
                RequestRead::Legacy {
                    generation: Some(generation),
                    model: Some(model),
                }
            }
            None => RequestRead::Legacy {
                generation: None,
                model: None,
            },
        })
    }
}

fn dispatch(
    root: &str,
    state: &mut ServerState,
    name: &str,
    arguments: &Value,
    recorder: Option<&mut audit::Recorder>,
    principal: Option<&str>,
    server_budget: i64,
) -> Result<String, String> {
    if !matches!(
        name,
        "get_artifact"
            | "search_artifacts"
            | "retrieve_grounding"
            | "find_decisions"
            | "get_related"
            | "get_summary"
    ) {
        return Err(format!("Unknown tool: {name}"));
    }
    let ServerState {
        repository_root,
        root_corpus_relative,
        federation_mode,
        persistent_cache,
        tracker,
        federated_tracker,
        graph_tracker,
        graph_cache,
    } = state;
    // Audit args mirror server.py's per-tool `observed(...)` shapes exactly
    // (insertion order = recorded key order): non-default arguments ride the
    // record only when supplied. `sidecar::observe` keeps the telemetry seam
    // (ADR-040), nesting audit inside as the oracle's
    // `telemetry.observe(audit.observe(...))` does.
    match name {
        "get_artifact" => {
            let params = [
                Param { name: "id", kind: Kind::Str, required: true },
                Param { name: "budget", kind: Kind::Int, required: false },
            ];
            let a = args::validate(name, "get_artifactArguments", &params, arguments)?;
            let effective = tools::effective_budget(server_budget, a_int(&a, 1, 0));
            budget::validate_call_budget(effective)?;
            let audit_args = json!({ "id": a_str(&a, 0, "") });
            sidecar::observe(name, || {
                audit::observe_result(recorder, principal, name, audit_args, || {
                    let request = read_request(
                        root,
                        repository_root,
                        root_corpus_relative,
                        federation_mode,
                        *persistent_cache,
                        tracker,
                        federated_tracker,
                        graph_tracker,
                    )?;
                    let (_, model) = request.legacy();
                    Ok(if let Some(read) = request.graph() {
                        tools::get_artifact_graph(
                            root,
                            read.corpus,
                            &a_str(&a, 0, ""),
                            effective,
                        )
                    } else if let Some(corpus) = request.composed() {
                        tools::get_artifact_composed(
                            root,
                            corpus,
                            &a_str(&a, 0, ""),
                            effective,
                        )
                    } else {
                        tools::get_artifact(root, model, &a_str(&a, 0, ""), effective)
                    })
                })
            })
        }
        "search_artifacts" => {
            let params = [
                Param { name: "query", kind: Kind::Str, required: true },
                Param { name: "type", kind: Kind::OptStr, required: false },
                Param { name: "tags", kind: Kind::OptListStr, required: false },
                Param { name: "live_only", kind: Kind::Bool, required: false },
            ];
            let a = args::validate(name, "search_artifactsArguments", &params, arguments)?;
            let query = a_str(&a, 0, "");
            let artifact_type = a_opt_str(&a, 1);
            let tags = a_opt_list_str(&a, 2).unwrap_or_default();
            let live_only = a_bool(&a, 3, false);
            let mut m = Map::new();
            m.insert("query".into(), Value::String(query.clone()));
            m.insert("type".into(), artifact_type.clone().map_or(Value::Null, Value::String));
            if !tags.is_empty() {
                m.insert("tags".into(), Value::Array(tags.iter().cloned().map(Value::String).collect()));
            }
            if live_only {
                m.insert("live_only".into(), Value::Bool(true));
            }
            let audit_args = Value::Object(m);
            sidecar::observe(name, || {
                audit::observe_result(recorder, principal, name, audit_args, || {
                    let request = read_request(
                        root,
                        repository_root,
                        root_corpus_relative,
                        federation_mode,
                        *persistent_cache,
                        tracker,
                        federated_tracker,
                        graph_tracker,
                    )?;
                    let (_, model) = request.legacy();
                    Ok(if let Some(read) = request.graph() {
                        tools::search_artifacts_graph(
                            root,
                            read.model,
                            read.corpus,
                            &query,
                            artifact_type.as_deref(),
                            &tags,
                            live_only,
                            server_budget,
                        )
                    } else if let Some(corpus) = request.composed() {
                        tools::search_artifacts_composed(
                            root,
                            request.cached_model(),
                            corpus,
                            &query,
                            artifact_type.as_deref(),
                            &tags,
                            live_only,
                            server_budget,
                        )
                    } else {
                        tools::search_artifacts(
                            root,
                            model,
                            &query,
                            artifact_type.as_deref(),
                            &tags,
                            live_only,
                            server_budget,
                        )
                    })
                })
            })
        }
        "retrieve_grounding" => {
            let params = [
                Param { name: "task", kind: Kind::Str, required: true },
                Param { name: "scope", kind: Kind::Str, required: false },
                Param { name: "top_k", kind: Kind::Int, required: false },
                Param { name: "budget", kind: Kind::Int, required: false },
                Param { name: "live_only", kind: Kind::Bool, required: false },
            ];
            let a = args::validate(name, "retrieve_grounding_toolArguments", &params, arguments)?;
            let task = a_str(&a, 0, "");
            let scope = a_str(&a, 1, "");
            let top_k = a_int(&a, 2, 5);
            let raw_budget = a_int(&a, 3, 0);
            let live_only = a_bool(&a, 4, true);
            let effective = tools::effective_budget(server_budget, raw_budget);
            budget::validate_call_budget(effective)?;
            let mut m = Map::new();
            m.insert("task".into(), Value::String(task.clone()));
            if !scope.is_empty() {
                m.insert("scope".into(), Value::String(scope.clone()));
            }
            if top_k != 5 {
                m.insert("top_k".into(), json!(top_k));
            }
            if raw_budget > 0 {
                m.insert("budget".into(), json!(raw_budget));
            }
            if !live_only {
                m.insert("live_only".into(), Value::Bool(false));
            }
            let audit_args = Value::Object(m);
            sidecar::observe(name, || {
                audit::observe_result(recorder, principal, name, audit_args, || {
                    let request = read_request(
                        root,
                        repository_root,
                        root_corpus_relative,
                        federation_mode,
                        *persistent_cache,
                        tracker,
                        federated_tracker,
                        graph_tracker,
                    )?;
                    let (_, model) = request.legacy();
                    Ok(if let Some(read) = request.graph() {
                        tools::retrieve_grounding_graph(
                            root,
                            read.corpus,
                            &task,
                            &scope,
                            top_k,
                            effective,
                            live_only,
                        )
                    } else if let Some(corpus) = request.composed() {
                        tools::retrieve_grounding_composed(
                            root, corpus, &task, &scope, top_k, effective, live_only,
                        )
                    } else {
                        tools::retrieve_grounding(
                            root, model, &task, &scope, top_k, effective, live_only,
                        )
                    })
                })
            })
        }
        "find_decisions" => {
            let params = [
                Param { name: "topic", kind: Kind::Str, required: false },
                Param { name: "path", kind: Kind::OptStr, required: false },
            ];
            let a = args::validate(name, "find_decisions_toolArguments", &params, arguments)?;
            let topic = a_str(&a, 0, "");
            let path = a_opt_str(&a, 1);
            let mut m = Map::new();
            m.insert("topic".into(), Value::String(topic.clone()));
            if let Some(p) = &path {
                m.insert("path".into(), Value::String(p.clone()));
            }
            let audit_args = Value::Object(m);
            sidecar::observe(name, || {
                audit::observe_result(recorder, principal, name, audit_args, || {
                    let request = read_request(
                        root,
                        repository_root,
                        root_corpus_relative,
                        federation_mode,
                        *persistent_cache,
                        tracker,
                        federated_tracker,
                        graph_tracker,
                    )?;
                    let (_, model) = request.legacy();
                    Ok(if let Some(read) = request.graph() {
                        tools::find_decisions_tool_graph(
                            root,
                            read.model,
                            read.corpus,
                            &topic,
                            path.as_deref(),
                            server_budget,
                        )
                    } else if let Some(corpus) = request.composed() {
                        tools::find_decisions_tool_composed(
                            root,
                            corpus,
                            &topic,
                            path.as_deref(),
                            server_budget,
                        )
                    } else {
                        tools::find_decisions_tool(
                            root,
                            model,
                            &topic,
                            path.as_deref(),
                            server_budget,
                        )
                    })
                })
            })
        }
        "get_related" => {
            let params = [
                Param { name: "id", kind: Kind::Str, required: true },
                Param { name: "depth", kind: Kind::Int, required: false },
            ];
            let a = args::validate(name, "get_relatedArguments", &params, arguments)?;
            let id = a_str(&a, 0, "");
            let depth = a_int(&a, 1, 1);
            let audit_args = json!({ "id": id.clone(), "depth": depth });
            sidecar::observe(name, || {
                audit::observe_result(recorder, principal, name, audit_args, || {
                    let request = read_request(
                        root,
                        repository_root,
                        root_corpus_relative,
                        federation_mode,
                        *persistent_cache,
                        tracker,
                        federated_tracker,
                        graph_tracker,
                    )?;
                    let (generation, model) = request.legacy();
                    let fresh_graph;
                    let graph_view = if let Some(read) = request.graph() {
                        graph_cache.view_for_graph(read.generation, &read.corpus.composition)
                    } else if let (Some(corpus), Some(logical)) =
                        (request.composed(), request.logical_generation())
                    {
                        graph_cache.view_for_composed(logical.cache_key(), corpus)
                    } else {
                        match (generation, model) {
                            (Some(generation), Some(model)) => {
                                graph_cache.view_for(generation, model)
                            }
                            _ => {
                                fresh_graph = graph::GraphView::fresh(root);
                                &fresh_graph
                            }
                        }
                    };
                    Ok(if let Some(read) = request.graph() {
                        tools::get_related_graph(
                            graph_view,
                            read.corpus,
                            &id,
                            depth,
                            server_budget,
                        )
                    } else if let Some(corpus) = request.composed() {
                        tools::get_related_composed(
                            graph_view,
                            corpus,
                            &id,
                            depth,
                            server_budget,
                        )
                    } else {
                        tools::get_related(graph_view, &id, depth, server_budget)
                    })
                })
            })
        }
        "get_summary" => {
            let params: [Param; 0] = [];
            args::validate(name, "get_summaryArguments", &params, arguments)?;
            let audit_args = json!({});
            sidecar::observe(name, || {
                audit::observe_result(recorder, principal, name, audit_args, || {
                    let request = read_request(
                        root,
                        repository_root,
                        root_corpus_relative,
                        federation_mode,
                        *persistent_cache,
                        tracker,
                        federated_tracker,
                        graph_tracker,
                    )?;
                    let (_, model) = request.legacy();
                    Ok(if let Some(read) = request.graph() {
                        tools::get_summary_graph(read.model, server_budget)
                    } else if let (Some(corpus), Some(generation)) =
                        (request.composed(), request.logical_generation())
                    {
                        tools::get_summary_composed(
                            root,
                            generation,
                            corpus,
                            server_budget,
                        )
                    } else {
                        tools::get_summary(root, model, server_budget)
                    })
                })
            })
        }
        _ => unreachable!("known tool guard and dispatch arms must stay aligned"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_host_detection_is_conservative() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("::"));
        assert!(!is_loopback_host("knowledge.internal"));
    }

    #[test]
    fn non_loopback_http_requires_explicit_proxy_acknowledgement() {
        assert!(non_loopback_bind_error("http", "0.0.0.0", false).is_some());
        assert!(non_loopback_bind_error("http", "::", false).is_some());
        assert!(non_loopback_bind_error("http", "0.0.0.0", true).is_none());
        assert!(non_loopback_bind_error("http", "127.0.0.1", false).is_none());
        assert!(non_loopback_bind_error("stdio", "0.0.0.0", false).is_none());
    }

    const DECISION: &str = "---\nschema_version: 1\nid: FIX-0DEC1GRAPH00\ntype: decision\n---\n# Graph Decision\n\n## Context\n\nGraph context.\n\n## Decision\n\nKeep the graph indexed.\n\n## Consequences\n\nFast reads.\n\n## Status\n\nAccepted\n";

    fn requirement(id: &str) -> String {
        format!(
            "---\nschema_version: 1\nid: {id}\ntype: requirement\n---\n# Graph Requirement\n\n## Status\n\nAccepted\n\n## Problem\n\nGraph reads scale with corpus size.\n\n## Requirements\n\n- [REQ-001] Graph reads are indexed.\n\n## Related Decisions\n\n- FIX-0DEC1GRAPH00\n"
        )
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "decided-mcp-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("scratch");
        directory
    }

    #[test]
    fn unknown_tool_does_not_freshen_tracker() {
        let mut state = ServerState {
            repository_root: PathBuf::from("/definitely-not-a-decided-corpus"),
            root_corpus_relative: ".".to_string(),
            federation_mode: None,
            persistent_cache: true,
            tracker: Some(rac_engine::freshness::FreshnessTracker::new(
                std::path::PathBuf::from("/definitely-not-a-decided-cache"),
                "/definitely-not-a-decided-corpus",
                None,
            )),
            federated_tracker: None,
            graph_tracker: rac_engine::derived_cache::GraphFederatedCacheTracker::new(
                PathBuf::from("/definitely-not-a-decided-cache"),
            ),
            graph_cache: graph::GraphCache::default(),
        };
        let result = dispatch(
            "/definitely-not-a-decided-corpus",
            &mut state,
            "not_a_tool",
            &json!({}),
            None,
            None,
            budget::DEFAULT_BUDGET,
        );
        assert_eq!(result, Err("Unknown tool: not_a_tool".to_string()));
        assert_eq!(state.tracker.as_ref().and_then(|t| t.corpus_hash()), None);
    }

    #[test]
    fn graph_view_reuses_generation_and_rebuilds_after_mutation() {
        let corpus = scratch("graph-corpus");
        let cache = scratch("graph-cache");
        std::fs::write(corpus.join("decision.md"), DECISION).unwrap();
        std::fs::write(corpus.join("requirement-1.md"), requirement("FIX-0REQ1GRAPH00")).unwrap();
        let root = corpus.to_string_lossy().into_owned();
        let mut state = ServerState {
            repository_root: corpus.clone(),
            root_corpus_relative: ".".to_string(),
            federation_mode: None,
            persistent_cache: true,
            tracker: Some(rac_engine::freshness::FreshnessTracker::new(
                cache.clone(),
                &root,
                Some(10),
            )),
            federated_tracker: None,
            graph_tracker: rac_engine::derived_cache::GraphFederatedCacheTracker::new(
                cache.clone(),
            ),
            graph_cache: graph::GraphCache::default(),
        };
        let arguments = json!({"id": "FIX-0DEC1GRAPH00", "depth": 2});

        let first = dispatch(
            &root,
            &mut state,
            "get_related",
            &arguments,
            None,
            None,
            budget::DEFAULT_BUDGET,
        )
        .unwrap();
        assert!(first.contains("FIX-0REQ1GRAPH00"), "{first}");
        assert_eq!(state.graph_cache.builds(), 1);
        let first_generation = state.tracker.as_ref().unwrap().serving_generation();

        let second = dispatch(
            &root,
            &mut state,
            "get_related",
            &arguments,
            None,
            None,
            budget::DEFAULT_BUDGET,
        )
        .unwrap();
        assert_eq!(second, first);
        assert_eq!(state.graph_cache.builds(), 1);
        assert_eq!(
            state.tracker.as_ref().unwrap().serving_generation(),
            first_generation
        );

        std::fs::write(corpus.join("requirement-2.md"), requirement("FIX-0REQ2GRAPH00")).unwrap();
        let changed = dispatch(
            &root,
            &mut state,
            "get_related",
            &arguments,
            None,
            None,
            budget::DEFAULT_BUDGET,
        )
        .unwrap();
        assert!(changed.contains("FIX-0REQ1GRAPH00"));
        assert!(changed.contains("FIX-0REQ2GRAPH00"));
        assert_eq!(state.graph_cache.builds(), 2);
        assert_eq!(
            state.tracker.as_ref().unwrap().serving_generation(),
            first_generation + 1
        );

        let _ = std::fs::remove_dir_all(&corpus);
        let _ = std::fs::remove_dir_all(&cache);
    }
}
