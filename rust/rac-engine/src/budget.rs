//! Per-response character budget (ADR-033) — a port of `src/asdecided/mcp/budget.py`
//! (ORACLE-NEXT revision, which adds the `items` rule). Shared by the CLI
//! retrieve surface (`commands::cmd_retrieve`) and the decided-mcp server.
//!
//! The budget unit is CHARACTERS (Python `len` of the serialized string —
//! Unicode code points, not bytes) of the payload serialized as
//! `json.dumps(payload, ensure_ascii=False)` with DEFAULT separators — i.e.
//! `", "` / `": "` WITH spaces (`pyjson::dumps_compact`). The docstring in the
//! oracle says "no spaces"; the code does not pass `separators`, so the wire
//! truth is *with* spaces (PORT-CONTRACT.d/10 §2) — port the code, not the
//! comment.
//!
//! Truncation is deterministic and whole-item wherever a repeated collection is
//! involved. Every successful payload is either reduced to the configured
//! character budget or replaced with a small explicit budget error; an
//! over-budget success is never returned.

use crate::pyjson::dumps_compact;
use serde_json::{json, Map, Value};

pub const DEFAULT_BUDGET: i64 = 10_000;
/// Smallest supported configured budget. This leaves room for a structured
/// budget error when fixed response fields alone cannot fit.
pub const MIN_BUDGET: i64 = 128;

pub const MARKER_TRUNCATED: &str = "truncated";
pub const MARKER_OMITTED: &str = "omitted";
pub const MARKER_HINT: &str = "hint";

pub const HINT_SEARCH: &str = "Narrow the query or request a specific artifact ID.";
pub const HINT_RELATED: &str = "Request the artifact directly, or narrow what you are changing.";
pub const HINT_CONTENT: &str =
    "Request a more specific artifact, or read the file directly for the full content.";
pub const HINT_SUMMARY: &str = "The repository summary exceeds the response budget; raise the \
server budget to see the full overview.";
pub const HINT_RETRIEVE: &str = "Lower top_k, raise the budget, or narrow the task.";
pub const BUDGET_ERROR: &str = "response_budget_exceeded";
pub const BUDGET_ERROR_HINT: &str = "Raise the response budget or narrow the request.";

type TruncationStrategy = fn(&Value, i64) -> (Value, bool);

pub fn valid_configured_budget(budget: i64) -> bool {
    budget >= MIN_BUDGET
}

pub fn validate_call_budget(budget: i64) -> Result<(), String> {
    if budget > 0 && budget < MIN_BUDGET {
        return Err(format!(
            "Requested response budget {budget} is below the minimum supported budget of {MIN_BUDGET} characters."
        ));
    }
    Ok(())
}

/// `len(text)` in Python — code points, not bytes.
pub fn char_len(s: &str) -> i64 {
    s.chars().count() as i64
}

/// `text[:stop]` with Python slice semantics (negative stop trims the tail;
/// the truncators only pass non-negative stops, but the retrieve excerpt
/// share can go negative).
pub fn py_slice_to(s: &str, stop: i64) -> String {
    let n = char_len(s);
    let stop = if stop < 0 { (n + stop).max(0) } else { stop.min(n) };
    s.chars().take(stop as usize).collect()
}

fn length(payload: &Value) -> i64 {
    char_len(&dumps_compact(payload))
}

/// `budget.serialize(payload, budget)`.
pub fn serialize(payload: &Value, budget: i64) -> String {
    let text = dumps_compact(payload);
    if char_len(&text) <= budget {
        return text;
    }
    let truncated = truncate(payload, budget);
    let text = dumps_compact(&truncated);
    if char_len(&text) <= budget {
        return text;
    }
    budget_error(budget)
}

fn truncate(payload: &Value, budget: i64) -> Value {
    let Some(obj) = payload.as_object() else {
        return json!({"error": BUDGET_ERROR, "hint": BUDGET_ERROR_HINT});
    };
    let mut candidate = Value::Object(obj.clone());

    // A shape can require more than one reduction (for example, a deep
    // relationship response has both incoming and neighborhood collections).
    // Apply strategies in a stable order, then remove optional fixed fields as
    // a last resort before returning the explicit error from `serialize`.
    let strategies: [TruncationStrategy; 9] = [
        |value, limit| truncate_list_strategy(value, "matches", limit, HINT_SEARCH),
        |value, limit| truncate_items_strategy(value, limit),
        |value, limit| truncate_content_strategy(value, limit),
        |value, limit| truncate_related_strategy(value, limit),
        |value, limit| truncate_list_strategy(value, "decisions", limit, HINT_SEARCH),
        |value, limit| truncate_list_strategy(value, "attention", limit, HINT_SUMMARY),
        |value, limit| truncate_list_strategy(value, "neighborhood", limit, HINT_RELATED),
        |value, limit| truncate_outgoing_strategy(value, limit),
        truncate_optional_strategy,
    ];
    for strategy in strategies {
        if char_len(&dumps_compact(&candidate)) <= budget {
            break;
        }
        let (next, changed) = strategy(&candidate, budget);
        candidate = next;
        if !changed {
            continue;
        }
    }
    candidate
}

fn budget_error(budget: i64) -> String {
    let full = dumps_compact(&json!({
        "error": BUDGET_ERROR,
        "hint": BUDGET_ERROR_HINT,
    }));
    if char_len(&full) <= budget {
        return full;
    }
    let short = dumps_compact(&json!({"error": BUDGET_ERROR}));
    if char_len(&short) <= budget {
        return short;
    }
    if budget >= 2 {
        return "{}".to_string();
    }
    if budget == 1 {
        return "0".to_string();
    }
    String::new()
}

/// A copy of `payload` with `key` replaced by `kept` and the marker added.
/// `IndexMap::insert` keeps an existing key's position, matching Python
/// dict-update semantics (a `truncated` key already present — the
/// `get_related` edge-overflow marker — is overwritten in place).
fn with_marker(payload: &Value, key: &str, kept: Vec<Value>, omitted: i64, hint: &str) -> Value {
    let mut marked: Map<String, Value> = payload.as_object().expect("object").clone();
    marked.insert(key.to_string(), Value::Array(kept));
    marked.insert(MARKER_TRUNCATED.to_string(), json!(true));
    marked.insert(MARKER_OMITTED.to_string(), json!(omitted));
    marked.insert(MARKER_HINT.to_string(), json!(hint));
    Value::Object(marked)
}

fn existing_omitted(payload: &Value) -> i64 {
    payload
        .get(MARKER_OMITTED)
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn truncate_list_strategy(
    payload: &Value,
    key: &str,
    budget: i64,
    hint: &str,
) -> (Value, bool) {
    let Some(items) = payload.get(key).and_then(Value::as_array) else {
        return (payload.clone(), false);
    };
    if items.is_empty() {
        return (payload.clone(), false);
    }
    let items: Vec<Value> = payload[key].as_array().cloned().unwrap_or_default();
    let total = items.len() as i64;
    let mut kept = items;
    while !kept.is_empty() {
        let candidate = with_marker(payload, key, kept.clone(), total - kept.len() as i64, hint);
        if length(&candidate) <= budget {
            return (candidate, true);
        }
        kept.pop();
    }
    (with_marker(payload, key, Vec::new(), total, hint), true)
}

fn truncate_items_strategy(payload: &Value, budget: i64) -> (Value, bool) {
    if payload
        .get("items")
        .and_then(Value::as_array)
        .is_none_or(|items| items.is_empty())
    {
        return (payload.clone(), false);
    }
    (truncate_items(payload, budget), true)
}

fn truncate_content_strategy(payload: &Value, budget: i64) -> (Value, bool) {
    let Some(content) = payload.get("content").and_then(Value::as_str) else {
        return (payload.clone(), false);
    };
    if content.is_empty() {
        return (payload.clone(), false);
    }
    (truncate_content(payload, budget), true)
}

fn truncate_content(payload: &Value, budget: i64) -> Value {
    let content = payload["content"].as_str().unwrap_or("").to_string();
    let total = char_len(&content);
    let with_content = |kept: String, omitted: i64| -> Value {
        let mut marked: Map<String, Value> = payload.as_object().expect("object").clone();
        marked.insert("content".to_string(), json!(kept));
        marked.insert(MARKER_TRUNCATED.to_string(), json!(true));
        marked.insert(MARKER_OMITTED.to_string(), json!(omitted));
        marked.insert(MARKER_HINT.to_string(), json!(HINT_CONTENT));
        Value::Object(marked)
    };
    let (mut lo, mut hi) = (0i64, total);
    let mut best = 0i64;
    while lo <= hi {
        let mid = (lo + hi).div_euclid(2);
        let candidate = with_content(py_slice_to(&content, mid), total - mid);
        if length(&candidate) <= budget {
            best = mid;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    with_content(py_slice_to(&content, best), total - best)
}

/// The retrieve `items` rule (ADR-113): excerpt-first, then whole-item.
fn truncate_items(payload: &Value, budget: i64) -> Value {
    let items: Vec<Value> = payload["items"].as_array().cloned().unwrap_or_default();
    let total = items.len() as i64;
    let mut kept = items;
    while !kept.is_empty() {
        let omitted = total - kept.len() as i64;
        let candidate = with_marker(payload, "items", kept.clone(), omitted, HINT_RETRIEVE);
        if length(&candidate) <= budget {
            return candidate;
        }
        // Trim the last kept item's excerpt before dropping it entirely.
        let mut last = kept
            .last()
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let excerpt: String = last
            .get("excerpt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let (mut lo, mut hi) = (0i64, char_len(&excerpt));
        let mut best: Option<i64> = None;
        while lo <= hi {
            let mid = (lo + hi).div_euclid(2);
            last.insert("excerpt".to_string(), json!(py_slice_to(&excerpt, mid)));
            let mut trial_items: Vec<Value> = kept[..kept.len() - 1].to_vec();
            trial_items.push(Value::Object(last.clone()));
            let trial = with_marker(payload, "items", trial_items, omitted, HINT_RETRIEVE);
            if length(&trial) <= budget {
                best = Some(mid);
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        if let Some(best) = best {
            last.insert("excerpt".to_string(), json!(py_slice_to(&excerpt, best)));
            let mut final_items: Vec<Value> = kept[..kept.len() - 1].to_vec();
            final_items.push(Value::Object(last));
            return with_marker(payload, "items", final_items, omitted, HINT_RETRIEVE);
        }
        kept.pop();
    }
    with_marker(payload, "items", Vec::new(), total, HINT_RETRIEVE)
}

fn truncate_related_strategy(payload: &Value, budget: i64) -> (Value, bool) {
    let has_incoming = payload.get("incoming").is_some_and(Value::is_array);
    let has_neighborhood = payload.get("neighborhood").is_some_and(Value::is_array);
    if !has_incoming && !has_neighborhood {
        return (payload.clone(), false);
    }
    let mut candidate = payload.clone();
    let mut omitted = existing_omitted(payload);
    let mut changed = false;
    for key in ["incoming", "neighborhood"] {
        let Some(items) = candidate.get(key).and_then(Value::as_array).cloned() else {
            continue;
        };
        let mut kept = items;
        while length(&candidate) > budget && !kept.is_empty() {
            kept.pop();
            omitted += 1;
            changed = true;
            let mut marked = candidate.as_object().expect("object").clone();
            marked.insert(key.to_string(), Value::Array(kept.clone()));
            marked.insert(MARKER_TRUNCATED.to_string(), json!(true));
            marked.insert(MARKER_OMITTED.to_string(), json!(omitted));
            marked.insert(MARKER_HINT.to_string(), json!(HINT_RELATED));
            candidate = Value::Object(marked);
        }
        if length(&candidate) <= budget {
            return (candidate, changed);
        }
    }
    (candidate, changed)
}

fn truncate_outgoing_strategy(payload: &Value, budget: i64) -> (Value, bool) {
    let Some(outgoing) = payload.get("outgoing").and_then(Value::as_object) else {
        return (payload.clone(), false);
    };
    if !outgoing.values().any(Value::is_array) {
        return (payload.clone(), false);
    }
    let mut candidate = payload.clone();
    let mut omitted = existing_omitted(payload);
    let mut changed = false;
    while length(&candidate) > budget {
        let Some((section, targets)) = candidate
            .get("outgoing")
            .and_then(Value::as_object)
            .and_then(|map| {
                map.iter()
                    .rev()
                    .find(|(_, value)| value.as_array().is_some_and(|items| !items.is_empty()))
            })
        else {
            break;
        };
        let mut updated = candidate.as_object().expect("object").clone();
        let mut outgoing = updated
            .get("outgoing")
            .and_then(Value::as_object)
            .cloned()
            .expect("outgoing object");
        let mut kept = targets.as_array().cloned().expect("outgoing targets");
        kept.pop();
        if kept.is_empty() {
            outgoing.remove(section);
        } else {
            outgoing.insert(section.clone(), Value::Array(kept));
        }
        omitted += 1;
        changed = true;
        updated.insert("outgoing".to_string(), Value::Object(outgoing));
        updated.insert(MARKER_TRUNCATED.to_string(), json!(true));
        updated.insert(MARKER_OMITTED.to_string(), json!(omitted));
        updated.insert(MARKER_HINT.to_string(), json!(HINT_RELATED));
        candidate = Value::Object(updated);
    }
    (candidate, changed)
}

fn truncate_optional_strategy(payload: &Value, budget: i64) -> (Value, bool) {
    let Some(object) = payload.as_object() else {
        return (payload.clone(), false);
    };
    // These fields are derived context, not the artifact identity itself. Drop
    // them in a fixed order only after whole-item collection truncation has
    // been exhausted. The marker tells the caller that context was omitted.
    const OPTIONAL: [&str; 10] = [
        "provenance",
        "evidence",
        "outgoing",
        "incoming",
        "neighborhood",
        "attention",
        "completeness",
        "relationships",
        "health",
        "validation_status",
    ];
    let mut candidate = object.clone();
    let mut changed = false;
    for key in OPTIONAL {
        if candidate.contains_key(key) && length(&Value::Object(candidate.clone())) > budget {
            candidate.remove(key);
            candidate.insert(MARKER_TRUNCATED.to_string(), json!(true));
            candidate.insert(MARKER_OMITTED.to_string(), json!(existing_omitted(payload)));
            candidate.insert(MARKER_HINT.to_string(), json!(HINT_RELATED));
            changed = true;
        }
        if length(&Value::Object(candidate.clone())) <= budget {
            break;
        }
    }
    (Value::Object(candidate), changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repeated(prefix: &str, count: usize) -> Vec<Value> {
        (0..count)
            .map(|index| json!({"id": format!("{prefix}-{index:04}"), "path": format!("{prefix}/{index}.md")}))
            .collect()
    }

    #[test]
    fn summary_attention_is_truncated_to_the_budget() {
        let payload = json!({
            "schema_version": "1",
            "directory": "decisions",
            "recursive": true,
            "attention": repeated("attention", 256),
            "health": {"score": 1.0}
        });
        let text = serialize(&payload, 512);
        assert!(char_len(&text) <= 512, "{} characters", char_len(&text));
        let value: Value = serde_json::from_str(&text).expect("budget result is JSON");
        assert_eq!(value[MARKER_TRUNCATED], json!(true));
        assert!(value[MARKER_OMITTED].as_i64().unwrap_or(0) > 0);
        assert!(value["attention"].as_array().unwrap().len() < 256);
    }

    #[test]
    fn related_incoming_and_neighborhood_are_truncated_deterministically() {
        let payload = json!({
            "schema_version": "1",
            "id": "ADR-001",
            "depth": 3,
            "incoming": repeated("incoming", 128),
            "neighborhood": repeated("neighbor", 128)
        });
        let first = serialize(&payload, 512);
        let second = serialize(&payload, 512);
        assert_eq!(first, second);
        assert!(char_len(&first) <= 512, "{} characters", char_len(&first));
        let value: Value = serde_json::from_str(&first).expect("budget result is JSON");
        assert_eq!(value[MARKER_TRUNCATED], json!(true));
        assert!(value[MARKER_OMITTED].as_i64().unwrap_or(0) > 0);
    }

    #[test]
    fn outgoing_relationship_targets_are_truncated_deterministically() {
        let payload = json!({
            "schema_version": "1",
            "id": "ADR-001",
            "depth": 3,
            "outgoing": {
                "related decisions": repeated("decision", 96),
                "related requirements": repeated("requirement", 96)
            }
        });
        let first = serialize(&payload, 512);
        let second = serialize(&payload, 512);
        assert_eq!(first, second);
        assert!(char_len(&first) <= 512, "{} characters", char_len(&first));
        let value: Value = serde_json::from_str(&first).expect("budget result is JSON");
        assert_eq!(value[MARKER_TRUNCATED], json!(true));
        assert!(value[MARKER_OMITTED].as_i64().unwrap_or(0) > 0);
    }

    #[test]
    fn fixed_fields_return_an_explicit_error_instead_of_an_oversized_success() {
        let payload = json!({"query": "x".repeat(20_000)});
        let text = serialize(&payload, MIN_BUDGET);
        assert!(char_len(&text) <= MIN_BUDGET);
        let value: Value = serde_json::from_str(&text).expect("budget error is JSON");
        assert_eq!(value["error"], json!(BUDGET_ERROR));
    }

    #[test]
    fn configured_and_per_call_minimums_are_explicit() {
        assert!(valid_configured_budget(MIN_BUDGET));
        assert!(!valid_configured_budget(MIN_BUDGET - 1));
        assert!(validate_call_budget(0).is_ok());
        assert!(validate_call_budget(MIN_BUDGET).is_ok());
        assert!(validate_call_budget(MIN_BUDGET - 1).is_err());
    }
}
