mod common;

use common::{parse, run_stdio};
use serde_json::json;

#[test]
fn legacy_2025_06_initialize_bytes_stay_pinned() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "legacy-test", "version": "1.0.0"}
        }
    });
    let frames = run_stdio("legacy-init", &[request.to_string()]);
    assert_eq!(
        frames,
        [concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-06-18\",",
            "\"capabilities\":{\"experimental\":{},\"prompts\":{\"listChanged\":false},",
            "\"resources\":{\"subscribe\":false,\"listChanged\":false},",
            "\"tools\":{\"listChanged\":false}},",
            "\"serverInfo\":{\"name\":\"lore\",\"version\":\"1.28.1\"}}}"
        )]
    );
}

#[test]
fn legacy_clients_can_negotiate_2025_11_25() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "legacy-test", "version": "1.0.0"}
        }
    });
    let frames = run_stdio("legacy-latest", &[request.to_string()]);
    assert_eq!(
        parse(&frames[0]).pointer("/result/protocolVersion"),
        Some(&json!("2025-11-25"))
    );
}

#[test]
fn legacy_list_result_does_not_gain_current_cache_fields() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/list",
        "params": {}
    });
    let frames = run_stdio("legacy-list", &[request.to_string()]);
    let response = parse(&frames[0]);
    assert!(response.pointer("/result/tools").is_some());
    assert!(response.pointer("/result/ttlMs").is_none());
    assert!(response.pointer("/result/cacheScope").is_none());
}
