//! decided-mcp — the Guide MCP server over stdio, a Rust port of `decided mcp`
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

/// The pinned `tools/list` result — the captured ORACLE-NEXT bytes, embedded
/// verbatim (schemas, descriptions, pydantic-shaped titles incl. the
/// function-name leaks `find_decisions_toolArguments` /
/// `retrieve_grounding_toolArguments`; §4).
const TOOLS_LIST_RESULT: &str = include_str!("tools_list_result.json");

pub(crate) struct ServerState {
    tracker: Option<rac_engine::freshness::FreshnessTracker>,
    graph_cache: graph::GraphCache,
}

/// The SDK's logging notification for an unparseable input line (§1) —
/// note the field order: method, params, jsonrpc.
const PARSE_ERROR_NOTIFICATION: &str = "{\"method\":\"notifications/message\",\"params\":{\"level\":\"error\",\"logger\":\"mcp.server.exception_handler\",\"data\":\"Internal Server Error\"},\"jsonrpc\":\"2.0\"}";

fn usage_error(msg: &str) -> ! {
    eprintln!("decided-mcp: error: {msg}");
    std::process::exit(2);
}

fn main() {
    let mut argv = std::env::args().skip(1);
    let mut root = ".".to_string();
    let mut cache = true;
    // Transport (ADR-098). stdio is the default and byte-unchanged; http selects
    // the streamable-HTTP transport (mandatory audit-on, loopback by default).
    let mut transport = "stdio".to_string();
    let mut host = "127.0.0.1".to_string();
    let mut port: u16 = 8000;
    let mut path = "/mcp".to_string();
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
            // Cache flags are real since INDEX-PLAN B6 and remain
            // output-neutral (ADR-112: cache-on vs cache-off runs are
            // frame-for-frame byte-identical; native warm == cold holds
            // even for the duplicate-token class, PORT-CONTRACT.d/10 §0a).
            "--no-cache" => cache = false,
            "--cache" => cache = true,
            other => usage_error(&format!("unrecognized argument: {other}")),
        }
    }
    if !std::path::Path::new(&root).is_dir() {
        usage_error(&format!("not a directory: {root}"));
    }
    check_corpus(&root);
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
    let mut state = ServerState {
        tracker,
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
        let mut recorder = audit::build(&root, "http", &audit_config);
        http::serve_http(&root, &mut state, &mut recorder, &host, port, &path);
    }
    let mut recorder = audit::build(&root, "stdio", &audit_config);
    serve(&root, &mut state, &mut recorder);
}

/// Startup diagnostic (stderr only; declared-normalized in parity, §0).
fn check_corpus(root: &str) {
    let entries = rac_engine::resolve::build_index(root, true);
    if !entries.iter().any(|e| e.artifact_type != "unknown") {
        eprintln!(
            "decided-mcp: no AsDecided artifacts found under '{root}'. Point --root at a \
directory containing RAC Markdown artifacts, or run 'decided init' to initialize \
a new repository. The server is running; get_summary will report the empty state."
        );
    }
}

fn serve(
    root: &str,
    state: &mut ServerState,
    recorder: &mut Option<audit::Recorder>,
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
        let id = message.get("id");
        let Some(id) = id else {
            continue; // notification (e.g. notifications/initialized): no response
        };
        let id_json = serde_json::to_string(id).unwrap_or_else(|_| "null".to_string());
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
        let frame =
            process_request(root, state, era, &id_json, &message, recorder.as_mut(), None);
        writeln!(out, "{frame}").ok();
        out.flush().ok();
    }
}

/// Produce the JSON-RPC response frame for one request `method` (transport-
/// agnostic, so stdio and HTTP share exactly one code path — the byte-parity
/// surface, PORT-CONTRACT.d/10 §2/§4/§5). Callers extract `method`/`id` and the
/// per-transport envelope; this owns only the payload.
pub(crate) fn process_request(
    root: &str,
    state: &mut ServerState,
    era: protocol::Era,
    id_json: &str,
    message: &Value,
    recorder: Option<&mut audit::Recorder>,
    principal: Option<&str>,
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
        (_, "tools/call") => {
            tools_call_frame(root, state, era, id_json, message, recorder, principal)
        }
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

fn tools_call_frame(
    root: &str,
    state: &mut ServerState,
    era: protocol::Era,
    id_json: &str,
    message: &Value,
    recorder: Option<&mut audit::Recorder>,
    principal: Option<&str>,
) -> String {
    let name = message
        .pointer("/params/name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let empty = json!({});
    let arguments = message.pointer("/params/arguments").unwrap_or(&empty);
    let dispatch_started = rac_engine::timing::start();
    let dispatched = dispatch(root, state, name, arguments, recorder, principal);
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

fn dispatch(
    root: &str,
    state: &mut ServerState,
    name: &str,
    arguments: &Value,
    recorder: Option<&mut audit::Recorder>,
    principal: Option<&str>,
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
    // ADR-033: the server budget is fixed at construction; the stdio CLI has
    // no flag, so it is always the default.
    let server_budget = budget::DEFAULT_BUDGET;
    // Freshen the read-model once per call (the corpus-change check every
    // tool answer rides, ADR-105); without the tracker every arm re-walks.
    let (generation, model) = match state.tracker.as_mut() {
        Some(tracker) => {
            let (generation, model) = tracker.read_model_with_generation(false);
            (Some(generation), Some(model))
        }
        None => (None, None),
    };
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
            let audit_args = json!({ "id": a_str(&a, 0, "") });
            Ok(sidecar::observe(name, || {
                audit::observe(recorder, principal, name, audit_args, || {
                    tools::get_artifact(root, model, &a_str(&a, 0, ""), effective)
                })
            }))
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
            Ok(sidecar::observe(name, || {
                audit::observe(recorder, principal, name, audit_args, || {
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
            }))
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
            Ok(sidecar::observe(name, || {
                audit::observe(recorder, principal, name, audit_args, || {
                    tools::retrieve_grounding(
                        root, model, &task, &scope, top_k, effective, live_only,
                    )
                })
            }))
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
            Ok(sidecar::observe(name, || {
                audit::observe(recorder, principal, name, audit_args, || {
                    tools::find_decisions_tool(root, model, &topic, path.as_deref(), server_budget)
                })
            }))
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
            let fresh_graph;
            let graph_view = match (generation, model) {
                (Some(generation), Some(model)) => state.graph_cache.view_for(generation, model),
                _ => {
                    fresh_graph = graph::GraphView::fresh(root);
                    &fresh_graph
                }
            };
            Ok(sidecar::observe(name, || {
                audit::observe(recorder, principal, name, audit_args, || {
                    tools::get_related(graph_view, &id, depth, server_budget)
                })
            }))
        }
        "get_summary" => {
            let params: [Param; 0] = [];
            args::validate(name, "get_summaryArguments", &params, arguments)?;
            let audit_args = json!({});
            Ok(sidecar::observe(name, || {
                audit::observe(recorder, principal, name, audit_args, || {
                    tools::get_summary(root, model, server_budget)
                })
            }))
        }
        _ => unreachable!("known tool guard and dispatch arms must stay aligned"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            tracker: Some(rac_engine::freshness::FreshnessTracker::new(
                std::path::PathBuf::from("/definitely-not-a-decided-cache"),
                "/definitely-not-a-decided-corpus",
                None,
            )),
            graph_cache: graph::GraphCache::default(),
        };
        let result = dispatch(
            "/definitely-not-a-decided-corpus",
            &mut state,
            "not_a_tool",
            &json!({}),
            None,
            None,
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
            tracker: Some(rac_engine::freshness::FreshnessTracker::new(
                cache.clone(),
                &root,
                Some(10),
            )),
            graph_cache: graph::GraphCache::default(),
        };
        let arguments = json!({"id": "FIX-0DEC1GRAPH00", "depth": 2});

        let first = dispatch(&root, &mut state, "get_related", &arguments, None, None).unwrap();
        assert!(first.contains("FIX-0REQ1GRAPH00"), "{first}");
        assert_eq!(state.graph_cache.builds(), 1);
        let first_generation = state.tracker.as_ref().unwrap().serving_generation();

        let second = dispatch(&root, &mut state, "get_related", &arguments, None, None).unwrap();
        assert_eq!(second, first);
        assert_eq!(state.graph_cache.builds(), 1);
        assert_eq!(
            state.tracker.as_ref().unwrap().serving_generation(),
            first_generation
        );

        std::fs::write(corpus.join("requirement-2.md"), requirement("FIX-0REQ2GRAPH00")).unwrap();
        let changed = dispatch(&root, &mut state, "get_related", &arguments, None, None).unwrap();
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
