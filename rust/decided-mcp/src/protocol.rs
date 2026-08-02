//! MCP wire-revision handling.
//!
//! Tool semantics deliberately do not live here. This module owns only the
//! dual-era lifecycle, revision metadata, cache hints, and protocol errors
//! recorded by ADR-121.

use serde_json::{json, Value};

pub const CURRENT_PROTOCOL_VERSION: &str = "2026-07-28";
pub const LATEST_LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";
pub const LEGACY_PROTOCOL_VERSIONS: [&str; 4] = [
    "2024-11-05",
    "2025-03-26",
    "2025-06-18",
    LATEST_LEGACY_PROTOCOL_VERSION,
];
pub const PROTOCOL_VERSION_META_KEY: &str = "io.modelcontextprotocol/protocolVersion";
pub const CLIENT_CAPABILITIES_META_KEY: &str =
    "io.modelcontextprotocol/clientCapabilities";

const SERVER_NAME: &str = "decided-mcp";
const DAY_MS: u64 = 86_400_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Era {
    Legacy,
    Current,
}

pub fn requested_version(message: &Value) -> Option<&str> {
    message
        .pointer("/params/_meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get(PROTOCOL_VERSION_META_KEY))
        .and_then(Value::as_str)
}

/// Return a request ID only when it is safe to echo under JSON-RPC. Invalid
/// IDs are represented as `null` in the invalid-request response rather than
/// reflecting an object, array, boolean, or null value back to the client.
pub fn request_id_json(message: &Value) -> String {
    message
        .get("id")
        .filter(|id| valid_request_id(id))
        .and_then(|id| serde_json::to_string(id).ok())
        .unwrap_or_else(|| "null".to_string())
}

/// Validate the JSON-RPC request envelope before either protocol era dispatches
/// it. Both transports share this gate so current and legacy tools cannot
/// accidentally accept a non-2.0 envelope or a non-scalar request ID.
pub fn validate_request_envelope(message: &Value, id_json: &str) -> Result<(), String> {
    let Some(object) = message.as_object() else {
        return Err(invalid_request_frame(id_json));
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(invalid_request_frame(id_json));
    }
    if object.get("method").and_then(Value::as_str).is_none() {
        return Err(invalid_request_frame(id_json));
    }
    if let Some(id) = object.get("id") {
        if !valid_request_id(id) {
            return Err(invalid_request_frame("null"));
        }
    }
    if let Some(params) = object.get("params") {
        if !params.is_object() && !params.is_array() {
            return Err(invalid_request_frame(id_json));
        }
    }
    Ok(())
}

fn valid_request_id(id: &Value) -> bool {
    id.is_string() || id.as_i64().is_some() || id.as_u64().is_some()
}

pub fn era_for_stdio(method: &str, message: &Value) -> Result<Era, String> {
    if method == "initialize" && requested_version(message) != Some(CURRENT_PROTOCOL_VERSION) {
        return Ok(Era::Legacy);
    }
    match requested_version(message) {
        Some(CURRENT_PROTOCOL_VERSION) => Ok(Era::Current),
        Some(version) if LEGACY_PROTOCOL_VERSIONS.contains(&version) => Ok(Era::Legacy),
        Some(version) => Err(unsupported_protocol_frame(
            message.get("id"),
            Some(version),
        )),
        None if method == "server/discover" => Ok(Era::Current),
        None => Ok(Era::Legacy),
    }
}

pub fn legacy_initialize_frame(id_json: &str, message: &Value) -> String {
    let requested = message
        .pointer("/params/protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(LATEST_LEGACY_PROTOCOL_VERSION);
    let version = if LEGACY_PROTOCOL_VERSIONS.contains(&requested) {
        requested
    } else {
        LATEST_LEGACY_PROTOCOL_VERSION
    };
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id_json},\"result\":{{\"protocolVersion\":\"{version}\",\
\"capabilities\":{{\"experimental\":{{}},\"prompts\":{{\"listChanged\":false}},\
\"resources\":{{\"subscribe\":false,\"listChanged\":false}},\
\"tools\":{{\"listChanged\":false}}}},\
\"serverInfo\":{{\"name\":\"lore\",\"version\":\"1.28.1\"}}}}}}"
    )
}

pub fn discover_frame(id_json: &str) -> String {
    let result = json!({
        "resultType": "complete",
        "ttlMs": DAY_MS,
        "cacheScope": "public",
        "supportedVersions": [CURRENT_PROTOCOL_VERSION],
        "capabilities": {
            "tools": {
                "listChanged": false,
            },
            "prompts": {
                "listChanged": false,
            },
            "resources": {
                "subscribe": false,
                "listChanged": false,
            },
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Read-only AsDecided decision grounding server",
        },
        "instructions": "Use the read-only tools to ground engineering work in the repository decision corpus.",
    });
    result_frame(id_json, result)
}

pub fn current_tools_list_frame(id_json: &str, legacy_result: &str) -> String {
    let mut result: Value =
        serde_json::from_str(legacy_result).expect("embedded tools/list result must be valid JSON");
    let object = result
        .as_object_mut()
        .expect("embedded tools/list result must be an object");
    object.insert("ttlMs".to_string(), json!(DAY_MS));
    object.insert("cacheScope".to_string(), json!("public"));
    object.insert("resultType".to_string(), json!("complete"));
    result_frame(id_json, result)
}

pub fn current_empty_list_frame(id_json: &str, field: &str, public: bool) -> String {
    let mut result = serde_json::Map::new();
    result.insert(field.to_string(), json!([]));
    result.insert("ttlMs".to_string(), json!(if public { DAY_MS } else { 0 }));
    result.insert(
        "cacheScope".to_string(),
        json!(if public { "public" } else { "private" }),
    );
    result.insert("resultType".to_string(), json!("complete"));
    result_frame(id_json, Value::Object(result))
}

pub fn method_not_found_frame(id_json: &str, method: &str) -> String {
    error_frame(
        id_json,
        -32601,
        "Method not found",
        json!({ "method": method }),
    )
}

pub fn header_mismatch_frame(
    id_json: &str,
    header: &str,
    expected: Option<&str>,
    actual: Option<&str>,
) -> String {
    error_frame(
        id_json,
        -32020,
        "Header mismatch",
        json!({
            "header": header,
            "expected": expected,
            "actual": actual,
        }),
    )
}

pub fn unsupported_protocol_frame(id: Option<&Value>, requested: Option<&str>) -> String {
    let id_json = id
        .and_then(|id| serde_json::to_string(id).ok())
        .unwrap_or_else(|| "null".to_string());
    error_frame(
        &id_json,
        -32022,
        "Unsupported protocol version",
        json!({
            "requested": requested.unwrap_or(""),
            "supported": [CURRENT_PROTOCOL_VERSION],
        }),
    )
}

pub fn validate_current_metadata(message: &Value, id_json: &str) -> Result<(), String> {
    let Some(meta) = message
        .pointer("/params/_meta")
        .and_then(Value::as_object)
    else {
        return Err(invalid_params_frame(
            id_json,
            "Current protocol requests require params._meta",
        ));
    };
    if meta
        .get(PROTOCOL_VERSION_META_KEY)
        .and_then(Value::as_str)
        != Some(CURRENT_PROTOCOL_VERSION)
    {
        return Err(invalid_params_frame(
            id_json,
            "Current protocol requests require protocolVersion metadata",
        ));
    }
    if !meta
        .get(CLIENT_CAPABILITIES_META_KEY)
        .is_some_and(Value::is_object)
    {
        return Err(invalid_params_frame(
            id_json,
            "Current protocol requests require clientCapabilities metadata",
        ));
    }
    Ok(())
}

pub fn current_method_supported(method: &str) -> bool {
    matches!(
        method,
        "server/discover"
            | "tools/list"
            | "prompts/list"
            | "resources/list"
            | "tools/call"
    )
}

fn result_frame(id_json: &str, result: Value) -> String {
    let result_json = serde_json::to_string(&result).expect("protocol result must serialize");
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id_json},\"result\":{result_json}}}")
}

fn error_frame(id_json: &str, code: i64, message: &str, data: Value) -> String {
    let error = json!({
        "code": code,
        "message": message,
        "data": data,
    });
    let error_json = serde_json::to_string(&error).expect("protocol error must serialize");
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id_json},\"error\":{error_json}}}")
}

fn invalid_params_frame(id_json: &str, detail: &str) -> String {
    error_frame(id_json, -32602, "Invalid params", json!({ "detail": detail }))
}

pub fn invalid_request_frame(id_json: &str) -> String {
    error_frame(id_json, -32600, "Invalid Request", Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_metadata_selects_current_era() {
        let mut meta = serde_json::Map::new();
        meta.insert(
            PROTOCOL_VERSION_META_KEY.to_string(),
            json!(CURRENT_PROTOCOL_VERSION),
        );
        let request = json!({
            "id": 1,
            "method": "tools/list",
            "params": {
                "_meta": Value::Object(meta),
            }
        });
        assert_eq!(era_for_stdio("tools/list", &request), Ok(Era::Current));
    }

    #[test]
    fn current_initialize_is_not_treated_as_legacy() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "_meta": {
                    PROTOCOL_VERSION_META_KEY: CURRENT_PROTOCOL_VERSION,
                    CLIENT_CAPABILITIES_META_KEY: {}
                }
            }
        });
        assert_eq!(era_for_stdio("initialize", &request), Ok(Era::Current));
    }

    #[test]
    fn discovery_identifies_native_server() {
        let frame: Value = serde_json::from_str(&discover_frame("1")).unwrap();
        assert_eq!(
            frame.pointer("/result/supportedVersions/0"),
            Some(&json!(CURRENT_PROTOCOL_VERSION))
        );
        assert_eq!(
            frame.pointer("/result/serverInfo/name"),
            Some(&json!("decided-mcp"))
        );
    }

    #[test]
    fn cache_hints_do_not_mutate_legacy_input() {
        let legacy = r#"{"tools":[]}"#;
        let frame: Value =
            serde_json::from_str(&current_tools_list_frame("1", legacy)).unwrap();
        assert_eq!(frame.pointer("/result/ttlMs"), Some(&json!(DAY_MS)));
        assert_eq!(
            frame.pointer("/result/cacheScope"),
            Some(&json!("public"))
        );
        assert_eq!(legacy, r#"{"tools":[]}"#);
    }

    #[test]
    fn unsupported_version_lists_both_eras() {
        let request = json!({"id": 7});
        let frame: Value =
            serde_json::from_str(&unsupported_protocol_frame(request.get("id"), Some("2099-01-01")))
                .unwrap();
        assert_eq!(frame.pointer("/error/code"), Some(&json!(-32022)));
        assert_eq!(
            frame.pointer("/error/data/supported/0"),
            Some(&json!(CURRENT_PROTOCOL_VERSION))
        );
    }
}
