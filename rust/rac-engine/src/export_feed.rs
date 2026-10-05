//! Incremental export change feeds (`decided export --since <rev>`).
//!
//! A feed is the difference between two rendered export payloads: the corpus
//! at the base revision and the corpus at the head (a second revision, or the
//! working tree). Both sides are produced by the released documents or graph
//! exporter, so a feed record carries exactly the bytes a consumer would have
//! read from a full export. Change identity is the record-owning source plus
//! the canonical id (ADR-026), so a move that keeps the id is `modified`.
//!
//! The replay law: applying a documents feed to the base documents export
//! reproduces the head documents export byte-for-byte ([`apply_documents_feed`]).
//! A graph feed reproduces the head node and edge sets ([`apply_graph_feed`]);
//! graph nodes carry no path, so node order is not recoverable from the graph
//! alone. Nothing here persists state: the consumer owns the cursor (ADR-080).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use crate::pyjson::{dumps_compact, dumps_indent2};

/// A record's change identity: its owning source and canonical id.
pub type RecordKey = (String, String);

/// The `head` cursor value when the head side is the working tree.
pub const WORKING_TREE: &str = "working-tree";

/// Resolved feed endpoints: `base` is a full commit SHA; `head` is a full
/// commit SHA or [`WORKING_TREE`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedCursor {
    pub base: String,
    pub head: String,
}

/// Compare two corpus-relative paths component by component, matching the
/// corpus walker's order (`walk::find_markdown_files`).
fn compare_paths(left: &str, right: &str) -> Ordering {
    left.split('/').cmp(right.split('/'))
}

struct DocumentLine {
    source: String,
    id: String,
    path: String,
    value: Value,
    bytes: String,
}

fn string_at<'a>(value: &'a Value, pointer: &str, what: &str) -> Result<&'a str, String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("documents export record is missing {what}"))
}

fn parse_documents(text: &str, side: &str) -> Result<Vec<DocumentLine>, String> {
    let mut lines = Vec::new();
    let mut seen = BTreeSet::new();
    for line in text.split('\n').filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line)
            .map_err(|error| format!("{side} documents export is not JSON Lines: {error}"))?;
        let id = string_at(&value, "/id", "id")?.to_string();
        let source = string_at(&value, "/metadata/source", "metadata.source")?.to_string();
        let path = string_at(&value, "/metadata/path", "metadata.path")?.to_string();
        if !seen.insert((source.clone(), id.clone())) {
            return Err(format!(
                "{side} documents export has more than one record for {source}::{id}"
            ));
        }
        lines.push(DocumentLine {
            source,
            id,
            path,
            bytes: dumps_compact(&value),
            value,
        });
    }
    Ok(lines)
}

fn change_rank(change: &str) -> u8 {
    match change {
        "removed" => 0,
        "modified" => 1,
        _ => 2,
    }
}

/// The documents feed: a cursor line, then one line per changed record.
///
/// Change lines are ordered by the path they refer to (the head path for
/// `added` and `modified`, the last-known path for `removed`), then source,
/// id, and change kind.
pub fn documents_feed(base: &str, head: &str, cursor: &FeedCursor) -> Result<String, String> {
    let base_lines = parse_documents(base, "base")?;
    let head_lines = parse_documents(head, "head")?;
    let base_by_key: BTreeMap<(&str, &str), &DocumentLine> = base_lines
        .iter()
        .map(|line| ((line.source.as_str(), line.id.as_str()), line))
        .collect();
    let head_keys: BTreeSet<(&str, &str)> = head_lines
        .iter()
        .map(|line| (line.source.as_str(), line.id.as_str()))
        .collect();

    // (path, source, id, change, record)
    let mut changes: Vec<(&str, &str, &str, &str, Value)> = Vec::new();
    for line in &head_lines {
        let change = match base_by_key.get(&(line.source.as_str(), line.id.as_str())) {
            None => "added",
            Some(previous) if previous.bytes != line.bytes => "modified",
            Some(_) => continue,
        };
        let mut record = Map::new();
        record.insert("schema_version".into(), json!("1"));
        record.insert("change".into(), json!(change));
        record.insert("document".into(), line.value.clone());
        changes.push((
            &line.path,
            &line.source,
            &line.id,
            change,
            Value::Object(record),
        ));
    }
    for line in &base_lines {
        if head_keys.contains(&(line.source.as_str(), line.id.as_str())) {
            continue;
        }
        let mut record = Map::new();
        record.insert("schema_version".into(), json!("1"));
        record.insert("change".into(), json!("removed"));
        record.insert("id".into(), json!(line.id));
        record.insert("source".into(), json!(line.source));
        record.insert("path".into(), json!(line.path));
        changes.push((
            &line.path,
            &line.source,
            &line.id,
            "removed",
            Value::Object(record),
        ));
    }
    changes.sort_by(|left, right| {
        compare_paths(left.0, right.0)
            .then_with(|| left.1.cmp(right.1))
            .then_with(|| left.2.cmp(right.2))
            .then_with(|| change_rank(left.3).cmp(&change_rank(right.3)))
    });

    let mut header = Map::new();
    header.insert("schema_version".into(), json!("1"));
    header.insert("feed".into(), json!("documents"));
    header.insert("base".into(), json!(cursor.base));
    header.insert("head".into(), json!(cursor.head));
    let mut output = vec![dumps_compact(&Value::Object(header))];
    output.extend(changes.into_iter().map(|change| dumps_compact(&change.4)));
    Ok(output.join("\n"))
}

/// Apply a documents feed to a base documents export, producing the head
/// export: drop `removed` and `modified` records, add `added` and `modified`
/// records, and re-sort by component-wise `metadata.path`, then source and id.
pub fn apply_documents_feed(base: &str, feed: &str) -> Result<String, String> {
    let mut records: BTreeMap<(String, String), (String, String)> = BTreeMap::new();
    for line in parse_documents(base, "base")? {
        records.insert((line.source, line.id), (line.path, line.bytes));
    }
    for (index, line) in feed
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        let value: Value = serde_json::from_str(line)
            .map_err(|error| format!("feed is not JSON Lines: {error}"))?;
        if index == 0 {
            if value.get("feed").and_then(Value::as_str) != Some("documents") {
                return Err("feed does not start with a documents cursor line".to_string());
            }
            continue;
        }
        match value.get("change").and_then(Value::as_str) {
            Some("removed") => {
                let id = string_at(&value, "/id", "id")?.to_string();
                let source = string_at(&value, "/source", "source")?.to_string();
                records.remove(&(source, id));
            }
            Some("added") | Some("modified") => {
                let document = value
                    .get("document")
                    .ok_or_else(|| "feed record is missing document".to_string())?;
                let id = string_at(document, "/id", "id")?.to_string();
                let source =
                    string_at(document, "/metadata/source", "metadata.source")?.to_string();
                let path = string_at(document, "/metadata/path", "metadata.path")?.to_string();
                records.insert((source, id), (path, dumps_compact(document)));
            }
            _ => return Err("feed record has no recognised change".to_string()),
        }
    }
    let mut ordered: Vec<((String, String), (String, String))> = records.into_iter().collect();
    ordered.sort_by(|left, right| {
        compare_paths(&left.1 .0, &right.1 .0)
            .then_with(|| left.0 .0.cmp(&right.0 .0))
            .then_with(|| left.0 .1.cmp(&right.0 .1))
    });
    Ok(ordered
        .into_iter()
        .map(|(_, (_, bytes))| bytes)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn parse_graph(text: &str, side: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|error| format!("{side} graph export is not JSON: {error}"))?;
    for field in ["nodes", "edges"] {
        if !value.get(field).is_some_and(Value::is_array) {
            return Err(format!("{side} graph export has no {field} array"));
        }
    }
    Ok(value)
}

fn node_key(node: &Value, default_source: &str) -> Result<RecordKey, String> {
    let id = node
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "graph node is missing id".to_string())?;
    let source = node
        .pointer("/provenance/source")
        .and_then(Value::as_str)
        .unwrap_or(default_source);
    Ok((source.to_string(), id.to_string()))
}

fn keyed_nodes(graph: &Value) -> Result<Vec<(RecordKey, &Value)>, String> {
    let default_source = graph.get("source").and_then(Value::as_str).unwrap_or("");
    let mut seen = BTreeSet::new();
    let mut nodes = Vec::new();
    for node in graph["nodes"].as_array().into_iter().flatten() {
        let key = node_key(node, default_source)?;
        if !seen.insert(key.clone()) {
            return Err(format!(
                "graph export has more than one node for {}::{}",
                key.0, key.1
            ));
        }
        nodes.push((key, node));
    }
    Ok(nodes)
}

fn edge_order(left: &Value, right: &Value) -> Ordering {
    let field =
        |edge: &Value, name: &str| edge.get(name).and_then(Value::as_str).map(str::to_owned);
    field(left, "source")
        .cmp(&field(right, "source"))
        .then_with(|| field(left, "type").cmp(&field(right, "type")))
        .then_with(|| field(left, "target").cmp(&field(right, "target")))
        .then_with(|| dumps_compact(left).cmp(&dumps_compact(right)))
}

/// The graph feed: one JSON object with the cursor, the head `source`, and
/// node and edge delta arrays. Nodes keep export order (removed nodes their
/// base order); edges keep the export's (source, type, target) order.
pub fn graph_feed(base: &str, head: &str, cursor: &FeedCursor) -> Result<String, String> {
    let base_graph = parse_graph(base, "base")?;
    let head_graph = parse_graph(head, "head")?;
    let base_nodes = keyed_nodes(&base_graph)?;
    let head_nodes = keyed_nodes(&head_graph)?;
    let base_by_key: BTreeMap<&(String, String), &Value> =
        base_nodes.iter().map(|(key, node)| (key, *node)).collect();
    let head_keys: BTreeSet<&(String, String)> = head_nodes.iter().map(|(key, _)| key).collect();

    let mut nodes_added = Vec::new();
    let mut nodes_modified = Vec::new();
    for (key, node) in &head_nodes {
        match base_by_key.get(key) {
            None => nodes_added.push((*node).clone()),
            Some(previous) if dumps_compact(previous) != dumps_compact(node) => {
                nodes_modified.push((*node).clone())
            }
            Some(_) => {}
        }
    }
    let nodes_removed: Vec<Value> = base_nodes
        .iter()
        .filter(|(key, _)| !head_keys.contains(key))
        .map(|(_, node)| (*node).clone())
        .collect();

    let edge_set = |graph: &Value| -> BTreeMap<String, Value> {
        graph["edges"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|edge| (dumps_compact(edge), edge.clone()))
            .collect()
    };
    let base_edges = edge_set(&base_graph);
    let head_edges = edge_set(&head_graph);
    let mut edges_added: Vec<Value> = head_edges
        .iter()
        .filter(|(key, _)| !base_edges.contains_key(*key))
        .map(|(_, edge)| edge.clone())
        .collect();
    let mut edges_removed: Vec<Value> = base_edges
        .iter()
        .filter(|(key, _)| !head_edges.contains_key(*key))
        .map(|(_, edge)| edge.clone())
        .collect();
    edges_added.sort_by(edge_order);
    edges_removed.sort_by(edge_order);

    let mut payload = Map::new();
    payload.insert("schema_version".into(), json!("1"));
    payload.insert("feed".into(), json!("graph"));
    payload.insert("base".into(), json!(cursor.base));
    payload.insert("head".into(), json!(cursor.head));
    payload.insert(
        "source".into(),
        head_graph.get("source").cloned().unwrap_or(Value::Null),
    );
    payload.insert("nodes_added".into(), Value::Array(nodes_added));
    payload.insert("nodes_modified".into(), Value::Array(nodes_modified));
    payload.insert("nodes_removed".into(), Value::Array(nodes_removed));
    payload.insert("edges_added".into(), Value::Array(edges_added));
    payload.insert("edges_removed".into(), Value::Array(edges_removed));
    Ok(dumps_indent2(&Value::Object(payload)))
}

/// A graph feed applied to its base: the head `source`, the node set keyed by
/// (source, id) with canonical node bytes, and the edge set as canonical edge
/// bytes. These equal the head export's sets.
#[derive(Debug, PartialEq, Eq)]
pub struct ReplayedGraph {
    pub source: Value,
    pub nodes: BTreeMap<RecordKey, String>,
    pub edges: BTreeSet<String>,
}

/// Apply a graph feed to a base graph export.
pub fn apply_graph_feed(base: &str, feed: &str) -> Result<ReplayedGraph, String> {
    let base_graph = parse_graph(base, "base")?;
    let feed: Value =
        serde_json::from_str(feed).map_err(|error| format!("graph feed is not JSON: {error}"))?;
    let source = feed.get("source").cloned().unwrap_or(Value::Null);
    let head_source = source.as_str().unwrap_or("");
    let base_source = base_graph
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut nodes: BTreeMap<RecordKey, String> = keyed_nodes(&base_graph)?
        .into_iter()
        .map(|(key, node)| (key, dumps_compact(node)))
        .collect();
    let array = |name: &str| {
        feed.get(name)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    for node in array("nodes_removed") {
        nodes.remove(&node_key(&node, base_source)?);
    }
    for node in array("nodes_added")
        .into_iter()
        .chain(array("nodes_modified"))
    {
        nodes.insert(node_key(&node, head_source)?, dumps_compact(&node));
    }
    let mut edges: BTreeSet<String> = base_graph["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .map(dumps_compact)
        .collect();
    for edge in array("edges_removed") {
        edges.remove(&dumps_compact(&edge));
    }
    for edge in array("edges_added") {
        edges.insert(dumps_compact(&edge));
    }
    Ok(ReplayedGraph {
        source,
        nodes,
        edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(source: &str, id: &str, path: &str, text: &str) -> String {
        dumps_compact(&json!({
            "schema_version": "1", "id": id, "type": "decision", "status": "Accepted",
            "title": id, "text": text,
            "metadata": {"path": path, "aliases": [], "tags": [], "source": source}
        }))
    }

    fn cursor() -> FeedCursor {
        FeedCursor {
            base: "a".repeat(40),
            head: WORKING_TREE.to_string(),
        }
    }

    #[test]
    fn path_order_is_component_wise() {
        assert_eq!(compare_paths("a/b.md", "a-c.md"), Ordering::Less);
        assert_eq!(compare_paths("a-c.md", "a/b.md"), Ordering::Greater);
    }

    #[test]
    fn documents_feed_classifies_and_replays() {
        let base = [
            record("s", "A", "d/a.md", "one"),
            record("s", "B", "d/b.md", "two"),
            record("s", "C", "d/c.md", "three"),
        ]
        .join("\n");
        let head = [
            record("s", "A", "d/a.md", "one"),
            record("s", "D", "d/d.md", "four"),
            record("s", "B", "e/moved.md", "two"),
        ]
        .join("\n");
        let feed = documents_feed(&base, &head, &cursor()).unwrap();
        let changes: Vec<Value> = feed
            .lines()
            .skip(1)
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let kinds: Vec<&str> = changes
            .iter()
            .map(|change| change["change"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["removed", "added", "modified"]);
        assert_eq!(changes[2]["document"]["metadata"]["path"], "e/moved.md");
        assert_eq!(apply_documents_feed(&base, &feed).unwrap(), head);
    }

    #[test]
    fn identical_payloads_produce_only_the_cursor() {
        let base = record("s", "A", "d/a.md", "one");
        let feed = documents_feed(&base, &base, &cursor()).unwrap();
        assert_eq!(feed.lines().count(), 1);
        let header: Value = serde_json::from_str(&feed).unwrap();
        assert_eq!(header["head"], WORKING_TREE);
        assert_eq!(apply_documents_feed(&base, &feed).unwrap(), base);
    }

    #[test]
    fn graph_feed_replays_node_and_edge_sets() {
        let base = json!({"schema_version": "1", "source": "s",
            "nodes": [{"id": "A", "type": "decision", "status": "Accepted", "title": "A"},
                      {"id": "B", "type": "decision", "status": "Accepted", "title": "B"}],
            "edges": [{"source": "A", "target": "B", "type": "related_decisions"}]});
        let head = json!({"schema_version": "1", "source": "s",
            "nodes": [{"id": "A", "type": "decision", "status": "Superseded", "title": "A"},
                      {"id": "C", "type": "decision", "status": "Accepted", "title": "C"}],
            "edges": [{"source": "A", "target": "C", "type": "related_decisions"}]});
        let (base, head) = (dumps_indent2(&base), dumps_indent2(&head));
        let feed = graph_feed(&base, &head, &cursor()).unwrap();
        let parsed: Value = serde_json::from_str(&feed).unwrap();
        for (field, count) in [
            ("nodes_added", 1),
            ("nodes_modified", 1),
            ("nodes_removed", 1),
            ("edges_added", 1),
            ("edges_removed", 1),
        ] {
            assert_eq!(parsed[field].as_array().unwrap().len(), count, "{field}");
        }
        let ReplayedGraph {
            source,
            nodes,
            edges,
        } = apply_graph_feed(&base, &feed).unwrap();
        let head_graph: Value = serde_json::from_str(&head).unwrap();
        assert_eq!(source, head_graph["source"]);
        let expected_nodes: BTreeMap<(String, String), String> = keyed_nodes(&head_graph)
            .unwrap()
            .into_iter()
            .map(|(key, node)| (key, dumps_compact(node)))
            .collect();
        assert_eq!(nodes, expected_nodes);
        let expected_edges: BTreeSet<String> = head_graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(dumps_compact)
            .collect();
        assert_eq!(edges, expected_edges);
    }
}
