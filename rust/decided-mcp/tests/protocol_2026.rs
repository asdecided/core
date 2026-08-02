mod common;

use common::{current_meta, parse, run_stdio, CURRENT_VERSION};
use serde_json::json;

fn assert_schema_subset(schema: &serde_json::Value) {
    let object = schema.as_object().expect("schema is an object");
    if let Some(kind) = object.get("type") {
        let kind = kind.as_str().expect("type is a string");
        assert!(
            matches!(
                kind,
                "null"
                    | "boolean"
                    | "object"
                    | "array"
                    | "number"
                    | "string"
                    | "integer"
            ),
            "valid JSON Schema type: {kind}"
        );
    }
    if let Some(properties) = object.get("properties") {
        for property in properties
            .as_object()
            .expect("properties is an object")
            .values()
        {
            assert_schema_subset(property);
        }
    }
    if let Some(items) = object.get("items") {
        assert_schema_subset(items);
    }
    if let Some(any_of) = object.get("anyOf") {
        let alternatives = any_of.as_array().expect("anyOf is an array");
        assert!(!alternatives.is_empty(), "anyOf has an alternative");
        alternatives.iter().for_each(assert_schema_subset);
    }
    if let Some(required) = object.get("required") {
        assert!(
            required
                .as_array()
                .expect("required is an array")
                .iter()
                .all(serde_json::Value::is_string),
            "required contains only property names"
        );
    }
    if let Some(reference) = object.get("$ref").and_then(serde_json::Value::as_str) {
        assert!(
            reference.starts_with('#'),
            "external references are not allowed: {reference}"
        );
    }
}

#[test]
fn current_client_discovers_native_server_without_initialize() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": "discover",
        "method": "server/discover",
        "params": {"_meta": current_meta()}
    });
    let frames = run_stdio("current-discover", &[request.to_string()]);
    let response = parse(&frames[0]);
    assert_eq!(
        response.pointer("/result/supportedVersions/0"),
        Some(&json!(CURRENT_VERSION))
    );
    assert_eq!(
        response.pointer("/result/serverInfo/name"),
        Some(&json!("decided-mcp"))
    );
    assert_eq!(
        response.pointer("/result/serverInfo/version"),
        Some(&json!(env!("CARGO_PKG_VERSION")))
    );
    assert_eq!(
        response.pointer("/result/resultType"),
        Some(&json!("complete"))
    );
    assert_eq!(
        response.pointer("/result/cacheScope"),
        Some(&json!("public"))
    );
    assert!(
        response
            .pointer("/result/ttlMs")
            .and_then(|value| value.as_u64())
            .is_some()
    );
}

#[test]
fn current_lists_include_required_cache_hints() {
    let requests = ["tools/list", "prompts/list", "resources/list"]
        .iter()
        .enumerate()
        .map(|(index, method)| {
            json!({
                "jsonrpc": "2.0",
                "id": index + 1,
                "method": method,
                "params": {"_meta": current_meta()}
            })
            .to_string()
        })
        .collect::<Vec<_>>();
    let frames = run_stdio("current-cache", &requests);
    for frame in frames {
        let response = parse(&frame);
        assert!(
            response
                .pointer("/result/ttlMs")
                .and_then(|value| value.as_u64())
                .is_some()
        );
        assert!(matches!(
            response
                .pointer("/result/cacheScope")
                .and_then(|value| value.as_str()),
            Some("public" | "private")
        ));
    }
}

#[test]
fn current_unknown_method_and_version_return_conforming_errors() {
    let unknown = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "unknown/current",
        "params": {"_meta": current_meta()}
    });
    let unsupported = json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "tools/list",
        "params": {
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2099-01-01"
            }
        }
    });
    let frames = run_stdio(
        "current-errors",
        &[unknown.to_string(), unsupported.to_string()],
    );
    assert_eq!(parse(&frames[0]).pointer("/error/code"), Some(&json!(-32601)));
    assert_eq!(parse(&frames[1]).pointer("/error/code"), Some(&json!(-32022)));
}

#[test]
fn current_tool_results_include_result_type() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "tools/call",
        "params": {
            "name": "get_summary",
            "arguments": {},
            "_meta": current_meta()
        }
    });
    let frames = run_stdio("current-call", &[request.to_string()]);
    let response = parse(&frames[0]);
    assert_eq!(
        response.pointer("/result/resultType"),
        Some(&json!("complete"))
    );
    assert!(response.pointer("/result/content").is_some());
}

#[test]
fn current_requests_require_client_capabilities_metadata() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/list",
        "params": {
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": CURRENT_VERSION
            }
        }
    });
    let frames = run_stdio("current-metadata", &[request.to_string()]);
    assert_eq!(parse(&frames[0]).pointer("/error/code"), Some(&json!(-32602)));
}

#[test]
fn current_initialize_and_invalid_envelopes_use_current_validation() {
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": "initialize",
        "method": "initialize",
        "params": {"_meta": current_meta()}
    });
    let invalid = json!({
        "jsonrpc": "1.0",
        "id": {"object": true},
        "method": "server/discover",
        "params": {"_meta": current_meta()}
    });
    let frames = run_stdio(
        "current-envelope",
        &[initialize.to_string(), invalid.to_string()],
    );
    assert_eq!(parse(&frames[0]).pointer("/error/code"), Some(&json!(-32601)));
    assert_eq!(parse(&frames[1]).pointer("/error/code"), Some(&json!(-32600)));
    assert_eq!(parse(&frames[1]).pointer("/id"), Some(&json!(null)));
}

#[test]
fn tool_schemas_are_json_schema_2020_12_compatible_and_local() {
    let list: serde_json::Value =
        serde_json::from_str(include_str!("../src/tools_list_result.json")).unwrap();
    for tool in list["tools"].as_array().unwrap() {
        let input = &tool["inputSchema"];
        assert_eq!(input["type"], "object", "tool input root stays an object");
        assert_schema_subset(input);
        assert_schema_subset(&tool["outputSchema"]);
    }
}
